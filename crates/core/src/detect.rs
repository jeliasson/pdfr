//! Field detection on flat and scanned pages. One pipeline handles both.
//!
//! Input is a grayscale render of the page (display orientation). All
//! thresholds are given in points and converted with the render scale.

use crate::field::{Field, FieldKind, Origin, TextMode};
use crate::geom::Rect;

/// Render scale used for detection (pixels per point).
pub const DETECT_SCALE: f32 = 3.0;
/// Pixels darker than this are ink.
pub const INK_THRESHOLD: u8 = 215;

/// Minimum ruled-line length (pt).
const MIN_LINE: f32 = 3.0;
/// Maximum perpendicular thickness of a ruled line (pt).
const MAX_THICK: f32 = 3.5;
/// Collinear pieces closer than this are merged (pt).
const MERGE_GAP: f32 = 3.0;
/// Lines shorter than this do not take part in box finding (pt).
const MIN_GRID_LINE: f32 = 5.0;
/// Line positions closer than this are one grid line (pt).
const CLUSTER_TOL: f32 = 0.8;
/// Slack when checking whether a line covers a grid edge (pt).
const COVER_TOL: f32 = 3.0;
/// Total gap allowed along a box's left or right side (pt).
const SIDE_GAP: f32 = 2.5;
/// Closed cells less tall than this inside a box are letters of its label,
/// not boxes (pt). Strokes of neighbouring letters can join into wide cells.
const HOLE_MAX: f32 = 9.0;
/// Dotted blanks must be at least this long (pt); dots mark a blank
/// unambiguously, so short inline ones count.
const MIN_DOTTED: f32 = 10.0;
/// Solid ink pixels a box may hold and still count as empty: scanner dust.
const SPECKS: f32 = 14.0;
/// Write-on lines must be at least this long (pt).
const MIN_WRITE_LINE: f32 = 30.0;
/// Comb boxes are at most this many times as wide as tall.
const COMB_MAX_ASPECT: f32 = 1.4;
/// Share of solid ink just under a line above which it is the tops of a
/// row of letters, not a line to write on.
const TEXT_UNDER: f32 = 0.3;

pub struct Gray<'a> {
    pub w: usize,
    pub h: usize,
    pub px: &'a [u8],
    /// Pixels per point.
    pub scale: f32,
}

/// A ruled line in points: `pos` is the perpendicular coordinate (y for
/// horizontal lines), `a0..a1` the extent along the line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seg {
    pub pos: f32,
    pub a0: f32,
    pub a1: f32,
    pub thick: f32,
    /// d(pos)/d(along), from a least-squares fit (skew).
    pub slope: f32,
}

impl Seg {
    pub fn len(&self) -> f32 {
        self.a1 - self.a0
    }
}

struct Masks {
    w: usize,
    h: usize,
    ink: Vec<bool>,
    /// Ink with at least 6 ink pixels in its 3×3 neighbourhood (no specks).
    solid: Vec<bool>,
    /// Prefix sums of `solid` for O(1) rectangle counts.
    solid_sum: Vec<u32>,
}

impl Masks {
    fn new(g: &Gray) -> Masks {
        Self::with_threshold(g, INK_THRESHOLD, true)
    }

    fn with_threshold(g: &Gray, threshold: u8, with_solid: bool) -> Masks {
        let (w, h) = (g.w, g.h);
        let ink: Vec<bool> = g.px.iter().map(|&p| p < threshold).collect();
        if !with_solid {
            return Masks {
                w,
                h,
                ink,
                solid: vec![],
                solid_sum: vec![],
            };
        }
        // 3×3 ink counts, separably: horizontal triples, then vertical sums.
        let mut row3 = vec![0u8; w * h];
        for y in 0..h {
            let r = &ink[y * w..(y + 1) * w];
            let o = &mut row3[y * w..(y + 1) * w];
            for x in 1..w.saturating_sub(1) {
                o[x] = r[x - 1] as u8 + r[x] as u8 + r[x + 1] as u8;
            }
        }
        let mut solid = vec![false; w * h];
        for y in 1..h.saturating_sub(1) {
            let (a, b, c) = (
                &row3[(y - 1) * w..y * w],
                &row3[y * w..(y + 1) * w],
                &row3[(y + 1) * w..(y + 2) * w],
            );
            let r = &ink[y * w..(y + 1) * w];
            let o = &mut solid[y * w..(y + 1) * w];
            for x in 0..w {
                o[x] = r[x] && a[x] + b[x] + c[x] >= 6;
            }
        }
        let mut solid_sum = vec![0u32; (w + 1) * (h + 1)];
        for y in 0..h {
            let mut row = 0u32;
            for x in 0..w {
                row += solid[y * w + x] as u32;
                solid_sum[(y + 1) * (w + 1) + x + 1] = solid_sum[y * (w + 1) + x + 1] + row;
            }
        }
        Masks {
            w,
            h,
            ink,
            solid,
            solid_sum,
        }
    }

    /// Solid ink pixels in the pixel rectangle `[x0,x1) × [y0,y1)`.
    fn count(&self, x0: i64, y0: i64, x1: i64, y1: i64) -> u32 {
        let c = |v: i64, m: usize| v.clamp(0, m as i64) as usize;
        let (x0, x1, y0, y1) = (c(x0, self.w), c(x1, self.w), c(y0, self.h), c(y1, self.h));
        if x1 <= x0 || y1 <= y0 {
            return 0;
        }
        let s = &self.solid_sum;
        let w = self.w + 1;
        s[y1 * w + x1] + s[y0 * w + x0] - s[y0 * w + x1] - s[y1 * w + x0]
    }
}

/// Union-find.
struct Dsu(Vec<usize>);
impl Dsu {
    fn new(n: usize) -> Self {
        Dsu((0..n).collect())
    }
    fn find(&mut self, mut a: usize) -> usize {
        while self.0[a] != a {
            self.0[a] = self.0[self.0[a]];
            a = self.0[a];
        }
        a
    }
    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.0[b] = a;
        }
    }
}

/// Extracts ruled lines along one axis. `horizontal` scans rows.
/// Returns thin lines, plus strokes 3.5–7 pt thick: on blurred scans two
/// neighbouring box edges fuse into one such stroke. Those are used only as
/// box dividers, never as write-on lines.
fn extract_lines(m: &Masks, scale: f32, horizontal: bool) -> (Vec<Seg>, Vec<Seg>) {
    let (w, h) = (m.w, m.h);
    let (_n_lines, _n_along) = if horizontal { (h, w) } else { (w, h) };
    let _at = |line: usize, a: usize| -> usize {
        if horizontal {
            line * w + a
        } else {
            a * w + line
        }
    };
    let max_thick_px = (MAX_THICK * scale).ceil() as usize;
    // Perpendicular run length for every ink pixel (fused strokes up to 2×).
    let mut perp = vec![0u16; w * h];
    if horizontal {
        // Vertical run length per pixel, with two row-major passes.
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                if m.ink[i] {
                    perp[i] = if y > 0 {
                        perp[i - w].saturating_add(1)
                    } else {
                        1
                    };
                }
            }
        }
        for y in (0..h.saturating_sub(1)).rev() {
            for x in 0..w {
                let i = y * w + x;
                if m.ink[i] && m.ink[i + w] {
                    perp[i] = perp[i + w];
                }
            }
        }
    } else {
        // Horizontal run length per pixel (already row-major).
        for y in 0..h {
            let mut x = 0;
            while x < w {
                if !m.ink[y * w + x] {
                    x += 1;
                    continue;
                }
                let s0 = x;
                while x < w && m.ink[y * w + x] {
                    x += 1;
                }
                let len = (x - s0).min(u16::MAX as usize) as u16;
                perp[y * w + s0..y * w + x].fill(len);
            }
        }
    }
    // The mask admits slightly thicker ink than a line may average, so that
    // noisy lines near the limit do not break into fragments.
    let (thin, _) = runs_to_segs(m, &perp, scale, horizontal, (4.5 * scale).ceil() as usize);
    let (_, mut fused) = runs_to_segs(m, &perp, scale, horizontal, 2 * max_thick_px);
    fused.retain(|f| {
        !thin
            .iter()
            .any(|t| (t.pos - f.pos).abs() <= 1.5 && t.a0 < f.a1 && f.a0 < t.a1)
    });
    (thin, fused)
}

/// Builds line segments from runs of ink whose perpendicular run length is at
/// most `limit` pixels. Returns `(thin, fused)` split at `MAX_THICK`.
#[allow(clippy::needless_range_loop)]
fn runs_to_segs(
    m: &Masks,
    perp: &[u16],
    scale: f32,
    horizontal: bool,
    limit: usize,
) -> (Vec<Seg>, Vec<Seg>) {
    let (w, h) = (m.w, m.h);
    let (n_lines, n_along) = if horizontal { (h, w) } else { (w, h) };
    let at = |line: usize, a: usize| -> usize {
        if horizontal {
            line * w + a
        } else {
            a * w + line
        }
    };
    let max_thick_px = (MAX_THICK * scale).ceil() as usize;
    // Runs along the axis in the thin-ink mask.
    let min_run = (MIN_LINE * scale) as usize;
    struct Run {
        line: usize,
        a0: usize,
        a1: usize,
    }
    let mut runs: Vec<Run> = Vec::new();
    let mut row_start = vec![0usize; n_lines + 1];
    for l in 0..n_lines {
        row_start[l] = runs.len();
        let mut a = 0;
        while a < n_along {
            let thin = |a: usize| {
                let i = at(l, a);
                m.ink[i] && (perp[i] as usize) <= limit
            };
            if !thin(a) {
                a += 1;
                continue;
            }
            let s = a;
            while a < n_along && thin(a) {
                a += 1;
            }
            if a - s >= min_run {
                runs.push(Run {
                    line: l,
                    a0: s,
                    a1: a,
                });
            }
        }
    }
    row_start[n_lines] = runs.len();
    // Connect runs in adjacent rows that overlap.
    let mut dsu = Dsu::new(runs.len());
    for l in 0..n_lines.saturating_sub(1) {
        let (mut i, ie) = (row_start[l], row_start[l + 1]);
        let (mut j, je) = (row_start[l + 1], row_start[l + 2]);
        while i < ie && j < je {
            if runs[i].a1 > runs[j].a0 && runs[j].a1 > runs[i].a0 {
                dsu.union(i, j);
            }
            if runs[i].a1 < runs[j].a1 {
                i += 1
            } else {
                j += 1
            }
        }
    }
    // Aggregate components.
    // Per component: extent, pixel count and sums for a least-squares fit
    // of perpendicular position against position along the line.
    #[derive(Default)]
    struct Acc {
        a0: usize,
        a1: usize,
        n: f64,
        sp: f64,
        sa: f64,
        saa: f64,
        sap: f64,
        lmin: usize,
        lmax: usize,
    }
    let mut comp: std::collections::HashMap<usize, Acc> = Default::default();
    for (k, r) in runs.iter().enumerate() {
        let root = dsu.find(k);
        let e = comp.entry(root).or_insert(Acc {
            a0: usize::MAX,
            lmin: usize::MAX,
            ..Default::default()
        });
        let n = (r.a1 - r.a0) as f64;
        let p = r.line as f64 + 0.5;
        // Sums over a = a0+0.5 .. a1-0.5.
        let (lo, hi) = (r.a0 as f64, r.a1 as f64);
        let sa = (hi * hi - lo * lo) / 2.0;
        let saa = (hi.powi(3) - lo.powi(3)) / 3.0;
        e.a0 = e.a0.min(r.a0);
        e.a1 = e.a1.max(r.a1);
        e.lmin = e.lmin.min(r.line);
        e.lmax = e.lmax.max(r.line);
        e.n += n;
        e.sp += n * p;
        e.sa += sa;
        e.saa += saa;
        e.sap += sa * p;
    }
    let mut fused: Vec<Seg> = Vec::new();
    let mut segs: Vec<Seg> = comp
        .into_values()
        .filter_map(|c| {
            let extent = (c.a1 - c.a0) as f64;
            let thick = c.n / extent;
            if thick > limit as f64 {
                return None;
            }
            let var = c.saa - c.sa * c.sa / c.n;
            let slope = if var > 1e-6 {
                (c.sap - c.sa * c.sp / c.n) / var
            } else {
                0.0
            };
            let seg = Seg {
                pos: (c.sp / c.n) as f32 / scale,
                a0: c.a0 as f32 / scale,
                a1: c.a1 as f32 / scale,
                thick: thick as f32 / scale,
                slope: slope as f32,
            };
            // Fused candidates: solid bands at least 2 pt thick (words have holes).
            let fill = c.n / (extent * (c.lmax - c.lmin + 1) as f64);
            if thick >= 2.0 * scale as f64 && fill >= 0.8 && seg.len() >= MIN_GRID_LINE {
                fused.push(seg);
            }
            if thick > max_thick_px as f64 {
                return None;
            }
            Some(seg)
        })
        .collect();
    merge_collinear(&mut segs);
    segs.retain(|s| s.len() >= MIN_LINE);
    merge_collinear(&mut fused);
    (segs, fused)
}

/// Top and bottom edges of solid bars (filled header bands thicker than any
/// line). Cells directly under a section header are closed by the bar's
/// bottom edge rather than a drawn line. Used only for box finding.
fn bar_edges(m: &Masks, scale: f32) -> Vec<Seg> {
    let (w, h) = (m.w, m.h);
    let min_thick = (2.0 * MAX_THICK * scale).ceil() as usize;
    let min_len = (20.0 * scale) as usize;
    // Vertical ink run length per pixel.
    let mut run = vec![0u16; w * h];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if m.ink[i] {
                run[i] = if y > 0 {
                    run[i - w].saturating_add(1)
                } else {
                    1
                };
            }
        }
    }
    for y in (0..h.saturating_sub(1)).rev() {
        for x in 0..w {
            let i = y * w + x;
            if m.ink[i] && m.ink[i + w] {
                run[i] = run[i + w];
            }
        }
    }
    let bar = |x: usize, y: usize| run[y * w + x] as usize > min_thick;
    let mut out = Vec::new();
    for y in 0..h {
        // Bottom edge: bar here, no ink below. Top edge: bar here, no ink above.
        for (below, edge_y) in [(true, y + 1), (false, y)] {
            let mut x = 0;
            while x < w {
                let is_edge = |x: usize| {
                    bar(x, y)
                        && if below {
                            y + 1 >= h || !m.ink[(y + 1) * w + x]
                        } else {
                            y == 0 || !m.ink[(y - 1) * w + x]
                        }
                };
                if !is_edge(x) {
                    x += 1;
                    continue;
                }
                let s0 = x;
                while x < w && is_edge(x) {
                    x += 1;
                }
                out.push(Seg {
                    pos: edge_y as f32 / scale,
                    a0: s0 as f32 / scale,
                    a1: x as f32 / scale,
                    thick: 0.0,
                    slope: 0.0,
                });
            }
        }
    }
    // White heading text inside a bar breaks its edge into pieces between
    // letters; join pieces on the same row across gaps of up to 15 pt.
    out.sort_by(|a, b| a.pos.total_cmp(&b.pos).then(a.a0.total_cmp(&b.a0)));
    let mut joined: Vec<Seg> = Vec::new();
    for s in out {
        match joined.last_mut() {
            Some(o) if o.pos == s.pos && s.a0 - o.a1 <= 15.0 => {
                o.thick += s.len(); // covered length, temporarily
                o.a1 = s.a1;
            }
            _ => joined.push(Seg {
                thick: s.len(),
                ..s
            }),
        }
    }
    joined.retain(|s| s.len() >= min_len as f32 / scale && s.thick >= 0.5 * s.len());
    for s in &mut joined {
        s.thick = 0.0;
    }
    joined
}

/// Dotted or dashed write-on lines: rows of small marks (each at most
/// 1.5 pt tall and 4 pt long) with gaps of at most 3 pt, 30 pt or longer.
/// Only used as write-on lines, never as box edges.
fn dotted_lines(m: &Masks, scale: f32) -> Vec<Seg> {
    let (w, h) = (m.w, m.h);
    let max_h = (1.5 * scale).ceil() as usize;
    let max_mark = (4.0 * scale).ceil() as usize;
    let max_gap = (3.0 * scale).ceil() as usize;
    let min_len = MIN_DOTTED * scale;
    // Vertical ink run length per pixel.
    let mut run = vec![0u16; w * h];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if m.ink[i] {
                run[i] = if y > 0 {
                    run[i - w].saturating_add(1)
                } else {
                    1
                };
            }
        }
    }
    for y in (0..h.saturating_sub(1)).rev() {
        for x in 0..w {
            let i = y * w + x;
            if m.ink[i] && m.ink[i + w] {
                run[i] = run[i + w];
            }
        }
    }
    let mut out = Vec::new();
    for y in 0..h {
        let row = &run[y * w..(y + 1) * w];
        let mut x = 0;
        // Current chain of marks: start, end, mark count, inked pixels, smallest gap.
        let mut chain: Option<(usize, usize, usize, usize, usize)> = None;
        let flush = |c: Option<(usize, usize, usize, usize, usize)>, out: &mut Vec<Seg>| {
            if let Some((a0, a1, n, ink, _)) = c {
                let len = (a1 - a0) as f32;
                // Real dots stand alone: nothing just above or below them
                // (the tops and bottoms of letters fail this).
                let band_ink = |y0: usize, y1: usize| {
                    (y0..y1.min(h))
                        .map(|yy| {
                            m.ink[yy * w + a0..yy * w + a1]
                                .iter()
                                .filter(|&&i| i)
                                .count()
                        })
                        .sum::<usize>()
                };
                let clear = (max_h + 1)..(max_h + 1 + (1.5 * scale) as usize);
                let below = band_ink(y + clear.start, y + clear.end);
                let above = if y >= clear.end {
                    band_ink(y - clear.end + 1, y - clear.start + 1)
                } else {
                    0
                };
                if len >= min_len && n >= 5 && (ink as f32) < 0.85 * len && below + above <= n / 2 {
                    out.push(Seg {
                        pos: (y as f32 + 0.5) / scale,
                        a0: a0 as f32 / scale,
                        a1: a1 as f32 / scale,
                        thick: 1.0 / scale,
                        slope: 0.0,
                    });
                }
            }
        };
        while x < w {
            if row[x] == 0 {
                x += 1;
                continue;
            }
            let s0 = x;
            let mut tall = false;
            while x < w && row[x] > 0 {
                tall |= row[x] as usize > max_h;
                x += 1;
            }
            // A dot is compact: the pixels beside it, at every height it
            // could reach, are blank (specks on the edge of a line are not).
            let side_clear = |xx: usize| {
                (y.saturating_sub(max_h)..(y + max_h + 1).min(h)).all(|yy| !m.ink[yy * w + xx])
            };
            let good = !tall
                && x - s0 <= max_mark
                && (s0 == 0 || side_clear(s0 - 1))
                && (x >= w || side_clear(x));
            chain = match (chain, good) {
                // A gap much wider than the dot spacing separates two blanks
                // (table columns drawn as dotted lines).
                (Some((a0, a1, n, ink, g)), true)
                    if s0 - a1 <= max_gap && (n < 3 || s0 - a1 <= (2 * g).max(g + 2)) =>
                {
                    Some((a0, x, n + 1, ink + x - s0, g.min(s0 - a1)))
                }
                (c, true) => {
                    flush(c, &mut out);
                    Some((s0, x, 1, x - s0, usize::MAX))
                }
                (c, false) => {
                    flush(c, &mut out);
                    None
                }
            };
        }
        flush(chain, &mut out);
    }
    // One dotted line covers a few pixel rows, and on a skewed scan it
    // steps from row to row in pieces: join pieces on the same line that
    // overlap or nearly touch, into one segment per line.
    out.sort_by(|a, b| a.a0.total_cmp(&b.a0));
    let mut kept: Vec<Seg> = Vec::new();
    for s in out {
        if let Some(o) = kept
            .iter_mut()
            .find(|o| (s.pos - o.pos).abs() <= 1.5 && s.a0 <= o.a1 + 3.0 && o.a0 <= s.a1 + 3.0)
        {
            o.a0 = o.a0.min(s.a0);
            o.a1 = o.a1.max(s.a1);
            continue;
        }
        kept.push(s);
    }
    kept
}

/// Merges collinear pieces across small gaps (crossings, dashes).
fn merge_collinear(segs: &mut Vec<Seg>) {
    segs.sort_by(|a, b| a.pos.total_cmp(&b.pos).then(a.a0.total_cmp(&b.a0)));
    loop {
        let mut merged = false;
        let mut out: Vec<Seg> = Vec::with_capacity(segs.len());
        'next: for s in segs.drain(..) {
            for o in out.iter_mut().rev() {
                if s.pos - o.pos > 1.5 {
                    break;
                }
                if (s.pos - o.pos).abs() <= 1.5
                    && s.a0 <= o.a1 + MERGE_GAP
                    && o.a0 <= s.a1 + MERGE_GAP
                {
                    let (l1, l2) = (o.len().max(0.1), s.len().max(0.1));
                    o.pos = (o.pos * l1 + s.pos * l2) / (l1 + l2);
                    o.thick = (o.thick * l1 + s.thick * l2) / (l1 + l2);
                    o.slope = (o.slope * l1 + s.slope * l2) / (l1 + l2);
                    o.a0 = o.a0.min(s.a0);
                    o.a1 = o.a1.max(s.a1);
                    merged = true;
                    continue 'next;
                }
            }
            out.push(s);
        }
        out.sort_by(|a, b| a.pos.total_cmp(&b.pos).then(a.a0.total_cmp(&b.a0)));
        *segs = out;
        if !merged {
            break;
        }
    }
}

/// Clusters line positions, seeding clusters with the longest lines.
/// Returns sorted cluster positions and, per segment, its cluster index.
fn cluster(segs: &[Seg]) -> (Vec<f32>, Vec<usize>) {
    let mut order: Vec<usize> = (0..segs.len()).collect();
    order.sort_by(|&a, &b| segs[b].len().total_cmp(&segs[a].len()));
    let mut centers: Vec<(f32, f32)> = Vec::new(); // (pos, weight)
    let mut assign = vec![0usize; segs.len()];
    for i in order {
        let s = segs[i];
        if let Some((k, _)) = centers
            .iter()
            .enumerate()
            .filter(|(_, c)| (c.0 - s.pos).abs() <= CLUSTER_TOL)
            .min_by(|a, b| (a.1.0 - s.pos).abs().total_cmp(&(b.1.0 - s.pos).abs()))
        {
            assign[i] = k;
        } else {
            assign[i] = centers.len();
            centers.push((s.pos, s.len()));
        }
    }
    // Sort clusters by position and remap.
    let mut idx: Vec<usize> = (0..centers.len()).collect();
    idx.sort_by(|&a, &b| centers[a].0.total_cmp(&centers[b].0));
    let mut remap = vec![0; centers.len()];
    for (new, &old) in idx.iter().enumerate() {
        remap[old] = new;
    }
    let pos = idx.iter().map(|&i| centers[i].0).collect();
    (pos, assign.into_iter().map(|a| remap[a]).collect())
}

/// Closed rectangular regions formed by ruled lines.
fn find_boxes(h: &[Seg], v: &[Seg]) -> Vec<Rect> {
    let h: Vec<Seg> = h
        .iter()
        .copied()
        .filter(|s| s.len() >= MIN_GRID_LINE)
        .collect();
    let v: Vec<Seg> = v
        .iter()
        .copied()
        .filter(|s| s.len() >= MIN_GRID_LINE)
        .collect();
    if h.len() < 2 || v.len() < 2 {
        return vec![];
    }
    let (ys, hc) = cluster(&h);
    let (xs, vc) = cluster(&v);
    let (nx, ny) = (xs.len(), ys.len());
    if nx < 2 || ny < 2 {
        return vec![];
    }
    // hedge[j][i]: a horizontal line at ys[j] spans xs[i]..xs[i+1].
    let mut hedge = vec![vec![false; nx - 1]; ny];
    for (s, &j) in h.iter().zip(&hc) {
        for i in 0..nx - 1 {
            if s.a0 <= xs[i] + COVER_TOL && s.a1 >= xs[i + 1] - COVER_TOL {
                hedge[j][i] = true;
            }
        }
    }
    let mut vedge = vec![vec![false; ny - 1]; nx];
    for (s, &i) in v.iter().zip(&vc) {
        for j in 0..ny - 1 {
            if s.a0 <= ys[j] + COVER_TOL && s.a1 >= ys[j + 1] - COVER_TOL {
                vedge[i][j] = true;
            }
        }
    }
    let cw = nx - 1;
    let cell = |i: usize, j: usize| j * cw + i;
    let mut dsu = Dsu::new(cw * (ny - 1));
    for j in 0..ny - 1 {
        for i in 0..cw {
            if i + 1 < cw && !vedge[i + 1][j] {
                dsu.union(cell(i, j), cell(i + 1, j));
            }
            if j + 1 < ny - 1 && !hedge[j + 1][i] {
                dsu.union(cell(i, j), cell(i, j + 1));
            }
        }
    }
    let mut regions: std::collections::HashMap<usize, (usize, usize, usize, usize, usize)> =
        Default::default();
    for j in 0..ny - 1 {
        for i in 0..cw {
            let r = dsu.find(cell(i, j));
            let e = regions.entry(r).or_insert((i, j, i, j, 0));
            e.0 = e.0.min(i);
            e.1 = e.1.min(j);
            e.2 = e.2.max(i);
            e.3 = e.3.max(j);
            e.4 += 1;
        }
    }
    // A region is a box when its bounding rectangle is closed on all four
    // sides and every cell inside it belongs to the region or to a smaller
    // region wholly inside it (letters of a label form tiny closed cells).
    let regions: Vec<_> = regions.into_iter().collect();
    let bbox_of: std::collections::HashMap<usize, (usize, usize, usize, usize)> = regions
        .iter()
        .map(|(r, e)| (*r, (e.0, e.1, e.2, e.3)))
        .collect();
    let mut out = Vec::new();
    for &(root, (i0, j0, i1, j1, n)) in &regions {
        if n != (i1 - i0 + 1) * (j1 - j0 + 1) {
            let mut holes_ok = true;
            'scan: for j in j0..=j1 {
                for i in i0..=i1 {
                    let r = dsu.find(cell(i, j));
                    if r == root {
                        continue;
                    }
                    let b = bbox_of[&r];
                    let inside = b.0 >= i0 && b.2 <= i1 && b.1 >= j0 && b.3 <= j1;
                    // Strictly inside, or small enough to be a letter of the label.
                    let strict = b.0 > i0 && b.2 < i1 && b.1 > j0 && b.3 < j1;
                    let small = ys[b.3 + 1] - ys[b.1] < HOLE_MAX;
                    if !(inside && (strict || small)) {
                        holes_ok = false;
                        break 'scan;
                    }
                }
            }
            if !holes_ok {
                continue;
            }
        }
        let closed = (i0..=i1).all(|i| hedge[j0][i] && hedge[j1 + 1][i])
            && (j0..=j1).all(|j| vedge[i0][j] && vedge[i1 + 1][j]);
        if !closed {
            continue;
        }
        let r = Rect::new(xs[i0], ys[j0], xs[i1 + 1], ys[j1 + 1]);
        // Each side must be ruled nearly end to end; a glyph such as ")"
        // passes the per-cell check but stops short of the lines.
        let covered = |segs: &[Seg], clus: &[usize], k: usize, a0: f32, a1: f32| {
            segs.iter()
                .zip(clus)
                .filter(|(_, c)| **c == k)
                .map(|(s, _)| (s.a1.min(a1) - s.a0.max(a0)).max(0.0))
                .sum::<f32>()
        };
        let need_v = r.height() - SIDE_GAP;
        if covered(&v, &vc, i0, r.y0, r.y1) < need_v
            || covered(&v, &vc, i1 + 1, r.y0, r.y1) < need_v
        {
            continue;
        }
        if r.width() >= 5.0 && r.height() >= 5.0 {
            out.push(r);
        }
    }
    out.sort_by(|a, b| a.y0.total_cmp(&b.y0).then(a.x0.total_cmp(&b.x0)));
    out
}

/// Hand-drawn boxes: wobbly, slanted outlines that never line up into ruled
/// lines. Any closed outline encloses paper the page background cannot reach,
/// so this looks for such enclosed regions (4-connected, so an 8-connected
/// stroke seals them) that are box-sized and mostly fill their bounding box.
/// Letter holes (o, a, e) are far too small. Returns the largest clear
/// rectangle inside each region.
fn drawn_boxes(m: &Masks, scale: f32) -> Vec<Rect> {
    let (w, h) = (m.w, m.h);
    // Runs of paper per row, joined into regions where they touch the row
    // above (much faster than flooding pixel by pixel).
    let mut runs: Vec<(usize, usize, usize)> = Vec::new(); // y, x0, x1 (excl)
    let mut row_start = Vec::with_capacity(h + 1);
    for y in 0..h {
        row_start.push(runs.len());
        let r = &m.ink[y * w..(y + 1) * w];
        let mut x = 0;
        while x < w {
            if r[x] {
                x += 1;
                continue;
            }
            let x0 = x;
            while x < w && !r[x] {
                x += 1;
            }
            runs.push((y, x0, x));
        }
    }
    row_start.push(runs.len());
    let mut dsu = Dsu::new(runs.len());
    for y in 1..h {
        let (mut i, ie) = (row_start[y - 1], row_start[y]);
        let (mut j, je) = (row_start[y], row_start[y + 1]);
        while i < ie && j < je {
            let (_, a0, a1) = runs[i];
            let (_, b0, b1) = runs[j];
            if a0 < b1 && b0 < a1 {
                dsu.union(i, j);
            }
            if a1 < b1 {
                i += 1;
            } else {
                j += 1;
            }
        }
    }
    struct Comp {
        x0: usize,
        y0: usize,
        x1: usize,
        y1: usize,
        n: usize,
        border: bool,
    }
    let mut comps: std::collections::HashMap<usize, Comp> = Default::default();
    let mut root = vec![0usize; runs.len()];
    for (k, &(y, x0, x1)) in runs.iter().enumerate() {
        let r = dsu.find(k);
        root[k] = r;
        let c = comps.entry(r).or_insert(Comp {
            x0,
            y0: y,
            x1: x1 - 1,
            y1: y,
            n: 0,
            border: false,
        });
        c.x0 = c.x0.min(x0);
        c.x1 = c.x1.max(x1 - 1);
        c.y0 = c.y0.min(y);
        c.y1 = c.y1.max(y);
        c.n += x1 - x0;
        c.border |= y == 0 || y == h - 1 || x0 == 0 || x1 == w;
    }
    let mut out = Vec::new();
    for (&id, c) in &comps {
        let (bw, bh) = (c.x1 + 1 - c.x0, c.y1 + 1 - c.y0);
        let (wpt, hpt) = (bw as f32 / scale, bh as f32 / scale);
        if c.border || wpt < MIN_WRITE_LINE || !(8.0..=150.0).contains(&hpt) {
            continue;
        }
        if (c.n as f32) < 0.5 * (bw * bh) as f32 {
            continue;
        }
        // The region's pixels within its bounding box. Specks (ink that is
        // not solid) count as paper.
        let mut free = vec![false; bw * bh];
        for y in c.y0..=c.y1 {
            for k in row_start[y]..row_start[y + 1] {
                let (_, x0, x1) = runs[k];
                if root[k] == id {
                    let o = (y - c.y0) * bw;
                    free[o + x0 - c.x0..o + x1 - c.x0].fill(true);
                }
            }
            for x in c.x0..=c.x1 {
                let p = y * w + x;
                if m.ink[p] && !m.solid[p] {
                    free[(y - c.y0) * bw + x - c.x0] = true;
                }
            }
        }
        // Ink the region surrounds is a label: a table cell whose grid line
        // the scan broke, so the ruled-box finder could not close it, with
        // its text inside. The outline, however wobbly, reaches the edge of
        // the bounding box; letters inside the paper do not.
        let mut reach = vec![false; bw * bh];
        let mut stack: Vec<usize> = Vec::new();
        for y in 0..bh {
            for x in 0..bw {
                let i = y * bw + x;
                if (y == 0 || y == bh - 1 || x == 0 || x == bw - 1) && !free[i] {
                    reach[i] = true;
                    stack.push(i);
                }
            }
        }
        while let Some(i) = stack.pop() {
            let (x, y) = ((i % bw) as i64, (i / bw) as i64);
            for (dx, dy) in [
                (-1, -1),
                (0, -1),
                (1, -1),
                (-1, 0),
                (1, 0),
                (-1, 1),
                (0, 1),
                (1, 1),
            ] {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= bw as i64 || ny >= bh as i64 {
                    continue;
                }
                let j = ny as usize * bw + nx as usize;
                if !free[j] && !reach[j] {
                    reach[j] = true;
                    stack.push(j);
                }
            }
        }
        let enclosed = (0..bw * bh)
            .filter(|&i| !free[i] && !reach[i] && m.solid[(c.y0 + i / bw) * w + c.x0 + i % bw])
            .count();
        if enclosed as f32 > SPECKS {
            continue;
        }
        // The largest free rectangle (histogram method): wobbly, slanted
        // edges eat into the bounding box from every side.
        let mut heights = vec![0usize; bw];
        let mut best = (0usize, 0, 0, 0, 0); // area, x0, x1, y0, y1 (exclusive ends)
        let mut st: Vec<usize> = Vec::new();
        for y in 0..bh {
            for (i, hgt) in heights.iter_mut().enumerate() {
                *hgt = if free[y * bw + i] { *hgt + 1 } else { 0 };
            }
            st.clear();
            for i in 0..=bw {
                let cur = heights.get(i).copied().unwrap_or(0);
                while let Some(&top) = st.last() {
                    if heights[top] < cur {
                        break;
                    }
                    st.pop();
                    let hgt = heights[top];
                    let left = st.last().map_or(0, |&l| l + 1);
                    if hgt * (i - left) > best.0 {
                        best = (hgt * (i - left), left, i, y + 1 - hgt, y + 1);
                    }
                }
                st.push(i);
            }
        }
        let (_, x0, x1, y0, y1) = best;
        // Most of the region must be usable: a ring around a word or a cell
        // with a label at its edge is not a box.
        if ((y1 - y0) as f32) < 0.3 * bh as f32 || ((x1 - x0) as f32) < 0.6 * bw as f32 {
            continue;
        }
        let r = Rect::new(
            (c.x0 + x0) as f32 / scale,
            (c.y0 + y0) as f32 / scale,
            (c.x0 + x1) as f32 / scale,
            (c.y0 + y1) as f32 / scale,
        );
        if r.width() >= MIN_WRITE_LINE && r.height() >= 8.0 {
            out.push(r);
        }
    }
    out.sort_by(|a, b| a.y0.total_cmp(&b.y0).then(a.x0.total_cmp(&b.x0)));
    out
}

/// Light grey shading that marks a fillable panel on digital forms (no
/// outline, a caption inside): darker than paper, far lighter than ink.
const PANEL_LO: u8 = 228;
const PANEL_HI: u8 = 250;

/// Shaded rectangles at least 40 pt wide and 12 pt tall: rows of panel
/// grey (with text on it) whose left and right ends line up.
fn panels(g: &Gray) -> Vec<Rect> {
    let s = g.scale;
    let (w, h) = (g.w, g.h);
    let min_w = (40.0 * s) as usize;
    struct Open {
        x0: usize,
        x1: usize,
        y0: usize,
        y1: usize,
    }
    let mut open: Vec<Open> = Vec::new();
    let mut out = Vec::new();
    let close = |o: &Open, out: &mut Vec<Rect>| {
        if (o.y1 + 1 - o.y0) as f32 >= 12.0 * s {
            out.push(Rect::new(
                o.x0 as f32 / s,
                o.y0 as f32 / s,
                o.x1 as f32 / s,
                (o.y1 + 1) as f32 / s,
            ));
        }
    };
    for y in 0..h {
        let row = &g.px[y * w..(y + 1) * w];
        let mut next: Vec<Open> = Vec::new();
        let mut x = 0;
        while x < w {
            if row[x] > PANEL_HI {
                x += 1;
                continue;
            }
            // A run of non-white pixels, mostly panel grey (text on it is
            // darker, but never lighter than the panel).
            let x0 = x;
            let mut shades = [0usize; (PANEL_HI - PANEL_LO + 1) as usize];
            while x < w && row[x] <= PANEL_HI {
                if row[x] >= PANEL_LO {
                    shades[(row[x] - PANEL_LO) as usize] += 1;
                }
                x += 1;
            }
            let grey: usize = shades.iter().sum();
            // A digital fill is one exact shade; scanned paper never is.
            let even = (0..shades.len())
                .map(|i| {
                    shades[i.saturating_sub(1)..(i + 2).min(shades.len())]
                        .iter()
                        .sum::<usize>()
                })
                .max()
                .unwrap_or(0);
            if x - x0 < min_w || grey * 2 < x - x0 || even * 100 < grey * 85 {
                continue;
            }
            match open
                .iter()
                .position(|o| o.x0.abs_diff(x0) <= 2 && o.x1.abs_diff(x) <= 2 && o.y1 + 1 == y)
            {
                Some(i) => {
                    let mut o = open.swap_remove(i);
                    o.y1 = y;
                    next.push(o);
                }
                None => next.push(Open {
                    x0,
                    x1: x,
                    y0: y,
                    y1: y,
                }),
            }
        }
        for o in &open {
            close(o, &mut out);
        }
        open = next;
    }
    for o in &open {
        close(o, &mut out);
    }
    out
}

/// The panel's grey under `r`: the most common shade in the panel range.
fn panel_grey(g: &Gray, r: &Rect) -> u8 {
    let s = g.scale;
    let mut hist = [0u32; 256];
    let (x0, x1) = ((r.x0 * s) as usize, ((r.x1 * s) as usize).min(g.w));
    let (y0, y1) = ((r.y0 * s) as usize, ((r.y1 * s) as usize).min(g.h));
    for y in y0..y1 {
        for &v in &g.px[y * g.w + x0..y * g.w + x1] {
            hist[v as usize] += 1;
        }
    }
    (PANEL_LO..=PANEL_HI)
        .max_by_key(|&v| hist[v as usize])
        .unwrap_or(242)
}

/// Detection output before classification into fields (exposed for tests).
pub struct Analysis {
    pub hlines: Vec<Seg>,
    pub vlines: Vec<Seg>,
    pub boxes: Vec<Rect>,
}

/// Detects fillable spots on a page image. Returned fields have `page == 0`.
/// Scans rarely have white, evenly lit paper: it is grey or warm, the light
/// falls off towards an edge, and the binding casts a shadow. Measured
/// against the fixed ink threshold, such paper turns into "ink" wherever it
/// is darkest, and lines there are lost in the noise. This divides every
/// pixel by the local paper brightness (a high percentile per 24 pt block,
/// widened over neighbouring blocks so filled bars and dense text don't
/// count as paper, and interpolated smoothly). Pages whose paper is already
/// white everywhere (every vector render) are left alone: `None`.
fn flatten_background(g: &Gray) -> Option<Vec<u8>> {
    let (w, h) = (g.w, g.h);
    let block = ((24.0 * g.scale) as usize).max(8);
    let (bw, bh) = (w.div_ceil(block), h.div_ceil(block));
    if w == 0 || h == 0 || g.px.len() < w * h {
        return None;
    }
    // 90th percentile of each block (every other pixel is plenty).
    let mut level = vec![255u8; bw * bh];
    for by in 0..bh {
        for bx in 0..bw {
            let mut hist = [0u32; 256];
            let mut n = 0u32;
            for y in (by * block..((by + 1) * block).min(h)).step_by(2) {
                let row = &g.px[y * w..(y + 1) * w];
                for &p in row[bx * block..((bx + 1) * block).min(w)].iter().step_by(2) {
                    hist[p as usize] += 1;
                    n += 1;
                }
            }
            let mut need = n / 10;
            let mut v = 255usize;
            while v > 0 {
                if hist[v] > need {
                    break;
                }
                need -= hist[v];
                v -= 1;
            }
            level[by * bw + bx] = v as u8;
        }
    }
    // Neighbourhood maximum: a block full of a filled bar or heavy print
    // takes its neighbours' paper.
    let mut paper = vec![0u8; bw * bh];
    for by in 0..bh {
        for bx in 0..bw {
            let mut m = 0u8;
            for y in by.saturating_sub(1)..(by + 2).min(bh) {
                for x in bx.saturating_sub(1)..(bx + 2).min(bw) {
                    m = m.max(level[y * bw + x]);
                }
            }
            paper[by * bw + bx] = m.max(96);
        }
    }
    if paper.iter().all(|&p| p >= 250) {
        return None;
    }
    // Bilinear between block centres.
    let at = |x: usize, y: usize| paper[y.min(bh - 1) * bw + x.min(bw - 1)] as f32;
    let mut out = vec![0u8; w * h];
    let half = block as f32 / 2.0;
    for y in 0..h {
        let fy = ((y as f32 - half) / block as f32).max(0.0);
        let (y0, ty) = (fy as usize, fy.fract());
        for x in 0..w {
            let fx = ((x as f32 - half) / block as f32).max(0.0);
            let (x0, tx) = (fx as usize, fx.fract());
            let top = at(x0, y0) * (1.0 - tx) + at(x0 + 1, y0) * tx;
            let bot = at(x0, y0 + 1) * (1.0 - tx) + at(x0 + 1, y0 + 1) * tx;
            let bg = top * (1.0 - ty) + bot * ty;
            let v = g.px[y * w + x] as f32 * 255.0 / bg;
            out[y * w + x] = v.min(255.0) as u8;
        }
    }
    Some(out)
}

pub fn detect(g: &Gray) -> Vec<Field> {
    detect_with_analysis(g).0
}

pub fn detect_with_analysis(g: &Gray) -> (Vec<Field>, Analysis) {
    // Scans: even out the paper first, so one ink threshold fits the page.
    let flat = flatten_background(g);
    // Shaded panels, on the page as rendered (before evening out a scan).
    let panel_rects = panels(g);
    let flat_g;
    let g = match &flat {
        Some(px) => {
            flat_g = Gray {
                w: g.w,
                h: g.h,
                px,
                scale: g.scale,
            };
            &flat_g
        }
        None => g,
    };
    let s = g.scale;
    let prof = std::env::var_os("PDFR_PROFILE").is_some();
    let t0 = std::time::Instant::now();
    let m = Masks::new(g);
    let t1 = t0.elapsed();
    let (hlines, hfused) = extract_lines(&m, s, true);
    let t2 = t0.elapsed();
    let (vlines, vfused) = extract_lines(&m, s, false);
    let t3 = t0.elapsed();
    if prof {
        eprintln!(
            "detect: masks {:?} hlines {:?} vlines {:?}",
            t1,
            t2 - t1,
            t3 - t2
        );
    }
    let bars = bar_edges(&m, s);
    let hall: Vec<Seg> = hlines.iter().chain(&hfused).chain(&bars).copied().collect();
    let vall: Vec<Seg> = vlines.iter().chain(&vfused).copied().collect();
    let boxes = find_boxes(&hall, &vall);
    // Half-thickness of the thickest line forming each side of a box.
    let edge = |lines: &[Seg], pos: f32, a0: f32, a1: f32| {
        lines
            .iter()
            .filter(|l| (l.pos - pos).abs() <= 1.5 && l.a0 < a1 && l.a1 > a0)
            .map(|l| l.thick / 2.0)
            .fold(0.0f32, f32::max)
    };
    let px = |v: f32| (v * s).round() as i64;
    let solid_in = |r: &Rect| m.count(px(r.x0), px(r.y0), px(r.x1), px(r.y1));

    let mut fields: Vec<Field> = Vec::new();
    // --- Closed boxes: empty ones, and label-on-top ones.
    struct Empty {
        r: Rect,
    }
    let mut empties: Vec<Empty> = Vec::new();
    // Page skew, from the long horizontal lines.
    let mut slopes: Vec<f32> = hlines
        .iter()
        .filter(|l| l.len() > 100.0)
        .map(|l| l.slope)
        .collect();
    slopes.sort_by(f32::total_cmp);
    let skew = slopes
        .get(slopes.len() / 2)
        .copied()
        .unwrap_or(0.0)
        .abs()
        .min(0.05);
    for b in &boxes {
        // Keep clear of the edges: line half-width, clustering error, blur and
        // the drift of a skewed edge across the box.
        let base = 1.4 + skew * b.width().max(b.height()) / 2.0;
        let lim = |v: f32, size: f32| v.min(size / 4.0);
        let (bw, bh) = (b.width(), b.height());
        let inner = Rect::new(
            b.x0 + lim(base + edge(&vall, b.x0, b.y0, b.y1).max(0.8), bw),
            b.y0 + lim(base + edge(&hall, b.y0, b.x0, b.x1).max(0.8), bh),
            b.x1 - lim(base + edge(&vall, b.x1, b.y0, b.y1).max(0.8), bw),
            b.y1 - lim(base + edge(&hall, b.y1, b.x0, b.x1).max(0.8), bh),
        );
        let area = (inner.width() * inner.height() * s * s).max(1.0);
        let ink = solid_in(&inner);
        // A few specks (scanner dust) still count as empty.
        if (ink as f32) <= (area * 0.002).max(SPECKS) {
            empties.push(Empty { r: *b });
            continue;
        }
        // Label-on-top: find the ink's vertical extent inside the box.
        if b.height() < 18.0 {
            continue;
        }
        // Ink that is itself a box (a checkbox in a table cell) is no label.
        if boxes.iter().any(|o| {
            o != b
                && o.x0 >= b.x0
                && o.x1 <= b.x1
                && o.y0 >= b.y0
                && o.y1 <= b.y1
                && o.width() >= 6.0
                && o.height() >= 6.0
                && (0.75..=1.33).contains(&(o.width() / o.height()))
        }) {
            continue;
        }
        // The label is the first band of ink rows from the top (gaps of up
        // to 3 pt between its lines). Below it there may only be specks.
        let (x0, x1) = (px(inner.x0), px(inner.x1));
        let y_start = px(inner.y0);
        let rows: Vec<u32> = (y_start..px(inner.y1))
            .map(|y| m.count(x0, y, x1, y + 1))
            .collect();
        let Some(first) = rows.iter().position(|&c| c > 0) else {
            continue;
        };
        let gap = (3.0 * s) as usize;
        let mut last = first;
        for (i, &c) in rows.iter().enumerate().skip(first) {
            if c > 0 {
                last = i;
            } else if i - last > gap {
                break;
            }
        }
        let below: u32 = rows.get(last + 1..).map_or(0, |r| r.iter().sum());
        if below as f32 > (area * 0.002).max(SPECKS) {
            continue;
        }
        let (t, bm) = (
            (y_start + first as i64) as f32 / s,
            (y_start + last as i64 + 1) as f32 / s,
        );
        let h = b.height();
        let free = b.y1 - bm;
        if t - b.y0 < 0.4 * h && bm - b.y0 < 0.6 * h && free >= (0.4 * h).max(9.0) {
            let r = Rect::new(b.x0, bm + 1.0, b.x1, b.y1);
            let mode = if r.height() > 60.0 {
                TextMode::Multi
            } else {
                TextMode::Single
            };
            fields.push(Field::new_text(0, r, mode, Origin::Detected));
        }
    }
    // Classify empty boxes: combs (rows of similar small boxes), checkboxes, text.
    empties.sort_by(|a, b| a.r.y0.total_cmp(&b.r.y0).then(a.r.x0.total_cmp(&b.r.x0)));
    let small =
        |r: &Rect| r.width() <= 25.0 && r.height() <= 30.0 && r.width() >= 4.0 && r.height() >= 7.0;
    let mut used = vec![false; empties.len()];
    for i in 0..empties.len() {
        if used[i] {
            continue;
        }
        let r = empties[i].r;
        // One character per box: a comb's boxes are about square or taller.
        // A row of wider boxes, each with its own label ("Portal",
        // "Escalera"), are separate fields that happen to sit side by side.
        let char_cell = |r: &Rect| r.width() <= COMB_MAX_ASPECT * r.height();
        if small(&r) && char_cell(&r) {
            // Grow a row of similar boxes to the right.
            let mut row = vec![i];
            let mut last = r;
            loop {
                let next = (0..empties.len()).find(|&k| {
                    let o = empties[k].r;
                    !used[k]
                        && !row.contains(&k)
                        && small(&o)
                        && char_cell(&o)
                        && (o.y0 - last.y0).abs() <= 2.0
                        && (o.y1 - last.y1).abs() <= 2.0
                        && (o.width() - last.width()).abs() <= 0.3 * last.width().max(o.width())
                        && o.x0 >= last.x1 - 1.0
                        && o.x0 - last.x1 < 0.5 * last.width()
                });
                match next {
                    Some(k) => {
                        row.push(k);
                        last = empties[k].r;
                    }
                    None => break,
                }
            }
            if row.len() >= 2 {
                for &k in &row {
                    used[k] = true;
                }
                let cells: Vec<(f32, f32)> = row
                    .iter()
                    .map(|&k| (empties[k].r.x0, empties[k].r.x1))
                    .collect();
                let y0 = row
                    .iter()
                    .map(|&k| empties[k].r.y0)
                    .fold(f32::MAX, f32::min);
                let y1 = row
                    .iter()
                    .map(|&k| empties[k].r.y1)
                    .fold(f32::MIN, f32::max);
                let rect = Rect::new(cells[0].0, y0, cells.last().unwrap().1, y1);
                fields.push(Field::new_text(
                    0,
                    rect,
                    TextMode::Comb(cells),
                    Origin::Detected,
                ));
                continue;
            }
            let aspect = r.width() / r.height();
            if (7.0..=20.0).contains(&r.width())
                && (7.0..=20.0).contains(&r.height())
                && (0.75..=1.33).contains(&aspect)
            {
                used[i] = true;
                fields.push(Field::new_checkbox(0, r, Origin::Detected));
                continue;
            }
        }
        used[i] = true;
        // Too small to write in: the loop of a handwritten letter.
        if r.width() < 8.0 || r.height() < 6.0 || (r.width() < 14.0 && r.height() < 10.0) {
            continue;
        }
        let mode = if r.height() > 60.0 {
            TextMode::Multi
        } else {
            TextMode::Single
        };
        fields.push(Field::new_text(0, r, mode, Origin::Detected));
    }

    // --- Hand-drawn boxes the ruled-line finder could not close.
    let mut drawn = drawn_boxes(&m, s);
    drawn.retain(|r| !boxes.iter().any(|b| b.overlap_ratio(r) > 0.5));
    for &r in &drawn {
        let mode = if r.height() > 60.0 {
            TextMode::Multi
        } else {
            TextMode::Single
        };
        fields.push(Field::new_text(0, r, mode, Origin::Detected));
    }

    let dotted_early = dotted_lines(&m, s);
    // --- Shaded panels: the writing space beside their caption. A panel
    // holding boxes or lines (a group of checkboxes) is no field itself.
    for p in &panel_rects {
        let inside = |r: &Rect| {
            let (cx, cy) = r.center();
            p.x0 <= cx && cx <= p.x1 && p.y0 - 1.0 <= cy && cy <= p.y1 + 1.0
        };
        let holds_line = hlines
            .iter()
            .chain(&dotted_early)
            .any(|l| l.len() >= MIN_WRITE_LINE && inside(&Rect::new(l.a0, l.pos, l.a1, l.pos)));
        // Checkbox-sized or larger: letters of printed text close into tiny boxes.
        let holds_box = boxes
            .iter()
            .any(|b| b.width() >= 7.0 && b.height() >= 7.0 && inside(b));
        if holds_box || fields.iter().any(|f| inside(&f.rect)) || holds_line {
            continue;
        }
        let inner = Rect::new(p.x0 + 1.0, p.y0 + 1.0, p.x1 - 1.0, p.y1 - 1.0);
        let (x0, x1) = (px(inner.x0), px(inner.x1));
        // Bands of text rows (gaps up to 1.5 pt join a band).
        let mut bands: Vec<(f32, f32)> = Vec::new();
        let gap = (1.5 * s) as i64;
        let mut cur: Option<(i64, i64)> = None;
        for y in px(inner.y0)..px(inner.y1) {
            if m.count(x0, y, x1, y + 1) > 0 {
                cur = Some(match cur {
                    Some((a, b)) if y - b <= gap => (a, y),
                    Some((a, b)) => {
                        bands.push((a as f32 / s, (b + 1) as f32 / s));
                        (y, y)
                    }
                    None => (y, y),
                });
            }
        }
        if let Some((a, b)) = cur {
            bands.push((a as f32 / s, (b + 1) as f32 / s));
        }
        // The caption is a short band against the top or bottom edge.
        let write = match (bands.first(), bands.last()) {
            (None, _) => Some((p.y0, p.y1)),
            (_, Some(&(t, b))) if p.y1 - b <= 4.0 && b - t <= 10.0 => Some((p.y0, t - 1.0)),
            (Some(&(t, b)), _) if t - p.y0 <= 4.0 && b - t <= 10.0 => Some((b + 1.0, p.y1)),
            _ => None,
        };
        let Some((y0, y1)) = write else { continue };
        if y1 - y0 < 9.0 {
            continue;
        }
        let r = Rect::new(p.x0 + 1.0, y0, p.x1 - 1.0, y1);
        let mode = if r.height() > 60.0 {
            TextMode::Multi
        } else {
            TextMode::Single
        };
        let mut f = Field::new_text(0, r, mode, Origin::Detected);
        let v = panel_grey(g, &r);
        f.backdrop = Some(crate::field::Backdrop {
            color: [v, v, v],
            printed: String::new(),
        });
        fields.push(f);
    }

    // --- Horizontal lines that are not box edges: tick rulers and write-on lines.
    let is_box_edge = |l: &Seg| {
        boxes.iter().any(|b| {
            ((l.pos - b.y0).abs() <= 2.0 || (l.pos - b.y1).abs() <= 2.0)
                && (l.a1.min(b.x1) - l.a0.max(b.x0)) > 0.5 * l.len().min(b.width())
        })
    };
    let hangs_down = |x: f32, y: f32| {
        vlines
            .iter()
            .any(|v| (v.pos - x).abs() <= 3.0 && (v.a0 - y).abs() <= 2.0 && v.len() >= 6.0)
    };
    let mut dotted = dotted_lines(&m, s);
    // Broken-up solid lines on scans look dotted; drop dotted lines lying on one.
    let on_solid = |d: &Seg, ls: &[Seg]| {
        ls.iter()
            .any(|l| (l.pos - d.pos).abs() <= 1.0 && l.a0 < d.a1 && d.a0 < l.a1)
    };
    dotted.retain(|d| !on_solid(d, &hlines) && !on_solid(d, &hfused) && !on_solid(d, &bars));
    let dotted_start = hlines.len();
    let write_lines: Vec<Seg> = hlines.iter().chain(&dotted).copied().collect();
    for (li, l) in write_lines.iter().enumerate() {
        let is_dotted = li >= dotted_start;
        // The bottom edge of a hand-drawn box belongs to the box.
        let under_drawn = drawn.iter().any(|r| {
            (r.y1 - 1.0..=r.y1 + 6.0).contains(&l.pos)
                && (l.a1.min(r.x1) - l.a0.max(r.x0)) > 0.5 * r.width()
        });
        if under_drawn || (!is_dotted && (l.len() < 15.0 || is_box_edge(l))) {
            continue;
        }
        // A rule running to the page's edge separates a header or footer;
        // no line to write on reaches the edge of the paper.
        let page_w = g.w as f32 / s;
        if l.a0 < 3.0 || l.a1 > page_w - 3.0 {
            continue;
        }
        if hangs_down(l.a0, l.pos) && hangs_down(l.a1, l.pos) {
            continue; // the top of a table
        }
        let y = l.pos - l.thick / 2.0;
        // On blurred scans the tops of a run of lowercase letters merge into
        // one thin stroke. Real lines have paper under them (a label sits a
        // little lower); letter tops have the letters' bodies hanging from
        // them. Dotted lines are never text.
        if !is_dotted {
            let yb = l.pos + l.thick / 2.0;
            let under = Rect::new(l.a0, yb + 0.3, l.a1, yb + 2.0);
            let area = (under.width() * under.height() * s * s).max(1.0);
            if solid_in(&under) as f32 > TEXT_UNDER * area {
                continue;
            }
        }
        // Tick marks: short ink columns rising from the line.
        let band_top = px(y - 3.0);
        let band_bot = px(y - 0.9);
        let mut ticks: Vec<f32> = Vec::new();
        let mut xx = px(l.a0 - 1.0);
        let end = px(l.a1 + 1.0);
        while xx < end {
            if m.count(xx, band_top, xx + 1, band_bot) as i64 >= (band_bot - band_top) - 1 {
                let s0 = xx;
                while xx < end
                    && m.count(xx, band_top, xx + 1, band_bot) as i64 >= (band_bot - band_top) - 1
                {
                    xx += 1;
                }
                if (xx - s0) as f32 <= 2.5 * s {
                    ticks.push((s0 + xx) as f32 / 2.0 / s);
                }
            }
            xx += 1;
        }
        if ticks.len() >= 3 && !is_dotted {
            let gaps: Vec<f32> = ticks.windows(2).map(|w| w[1] - w[0]).collect();
            let mean = gaps.iter().sum::<f32>() / gaps.len() as f32;
            let even = gaps.iter().all(|g| (g - mean).abs() <= 0.15 * mean) && mean >= 6.0;
            // Ticks must be short: nothing tall rises from the line.
            let tall = ticks
                .iter()
                .any(|&t| m.count(px(t) - 1, px(y - 14.0), px(t) + 2, px(y - 11.0)) > 0);
            // Between the ticks is room to write: letters standing on a
            // baseline (their stems look like ticks) fill it.
            let blank_cells = ticks.windows(2).all(|w| {
                let cell = Rect::new(w[0] + 1.5, y - 10.0, w[1] - 1.5, y - 1.0);
                cell.width() <= 0.0 || solid_in(&cell) as f32 <= SPECKS
            });
            if even && !tall && blank_cells {
                let cells: Vec<(f32, f32)> = ticks.windows(2).map(|w| (w[0], w[1])).collect();
                let rect = Rect::new(ticks[0], y - 12.0, *ticks.last().unwrap(), y);
                fields.push(Field::new_text(
                    0,
                    rect,
                    TextMode::Comb(cells),
                    Origin::Detected,
                ));
                continue;
            }
        }
        if l.len() < MIN_WRITE_LINE && !is_dotted {
            continue;
        }
        // Blank space above the line (less for dotted blanks inside a sentence).
        let need = if is_dotted { 8.0 } else { 12.0 };
        let above = Rect::new(l.a0 + 2.0, y - need, l.a1 - 2.0, y - 1.5);
        let area = above.width() * above.height() * s * s;
        if solid_in(&above) as f32 > (area * 0.001).max(3.0) {
            continue;
        }
        // How much free space is there? Up to 18pt, and never past another
        // write-on line above (dots are too small to count as ink).
        let ceiling = write_lines
            .iter()
            .filter(|o| o.pos < l.pos - 1.5 && o.a0 < l.a1 && o.a1 > l.a0)
            .map(|o| o.pos + 1.0)
            .fold(f32::MIN, f32::max);
        let mut free = need;
        while free < 18.0 && y - free - 1.0 > ceiling {
            let band = Rect::new(l.a0 + 2.0, y - free - 1.0, l.a1 - 2.0, y - free);
            if solid_in(&band) > 0 {
                break;
            }
            free += 1.0;
        }
        let free = free.min(y - ceiling).max(6.0);
        let rect = Rect::new(l.a0, y - free, l.a1, y);
        fields.push(Field::new_text(0, rect, TextMode::OnLine, Origin::Detected));
    }
    // Drop fields that sit inside or on top of another detected field.
    let mut keep = vec![true; fields.len()];
    for i in 0..fields.len() {
        for j in 0..fields.len() {
            if i != j
                && keep[j]
                && fields[i].rect.overlap_ratio(&fields[j].rect) > 0.6
                && fields[i].rect.area() < fields[j].rect.area()
            {
                keep[i] = false;
            }
        }
    }
    let mut out: Vec<Field> = fields
        .into_iter()
        .zip(keep)
        .filter(|(_, k)| *k)
        .map(|(f, _)| f)
        .collect();
    out.sort_by(|a, b| {
        a.rect
            .y0
            .total_cmp(&b.rect.y0)
            .then(a.rect.x0.total_cmp(&b.rect.x0))
    });
    (
        out,
        Analysis {
            hlines,
            vlines,
            boxes,
        },
    )
}

/// Sorts fields into reading order: per page, rows top to bottom (fields
/// whose vertical centres are close share a row), then left to right.
pub fn reading_order(fields: &mut Vec<Field>) {
    reading_order_by(fields, |f| (f.page, f.rect));
}

/// [`reading_order`] for any items, given each item's page and rectangle.
pub fn reading_order_by<T>(items: &mut Vec<T>, key: impl Fn(&T) -> (usize, Rect)) {
    crate::geom::line_order(
        items,
        |a, b| {
            let (pa, ra) = key(a);
            let (pb, rb) = key(b);
            pa.cmp(&pb).then(ra.center().1.total_cmp(&rb.center().1))
        },
        |first, f| {
            let (p0, r0) = key(first);
            let (p1, r1) = key(f);
            let tol = 0.5 * r0.height().min(r1.height()).min(12.0);
            p0 == p1 && (r1.center().1 - r0.center().1).abs() <= tol
        },
        |f| key(f).1.x0,
    );
}

/// Renders page `index` of `pdf` for detection and detects its fields.
pub fn detect_page<'a>(
    page: &'a hayro::hayro_syntax::page::Page<'a>,
    cache: &crate::render::RenderCache<'a>,
    index: usize,
) -> Vec<Field> {
    detect_page_gray(page, cache, index).0
}

/// A grey page render at [`DETECT_SCALE`], one byte per pixel.
pub struct PageGray {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

/// The largest page side (points) the PDF format allows (200 inches).
pub const MAX_PAGE_SIDE: f32 = 14_400.0;

/// Whether `page` can be rendered at [`DETECT_SCALE`] in full: no side
/// over [`MAX_PAGE_SIDE`], and no render side over
/// [`crate::render::MAX_RENDER_SIDE`] (which would lower the scale that
/// detection and scanned-text reading count on). Larger pages get no field
/// detection or text reading.
pub fn detectable(page: &hayro::hayro_syntax::page::Page<'_>) -> bool {
    let (w, h) = page.render_dimensions();
    let side = w.abs().max(h.abs());
    side.is_finite()
        && side <= MAX_PAGE_SIDE
        && side * DETECT_SCALE <= crate::render::MAX_RENDER_SIDE
}

/// Like [`detect_page`], also handing back the grey render it used (for
/// reading scanned text without rendering the page again). A page too
/// large to detect on ([`detectable`]) gives no fields and an empty render.
pub fn detect_page_gray<'a>(
    page: &'a hayro::hayro_syntax::page::Page<'a>,
    cache: &crate::render::RenderCache<'a>,
    index: usize,
) -> (Vec<Field>, PageGray) {
    if !detectable(page) {
        let empty = PageGray {
            w: 0,
            h: 0,
            px: vec![],
        };
        return (vec![], empty);
    }
    let img = crate::render::render_page(page, cache, DETECT_SCALE);
    let gray = img.to_gray();
    let g = Gray {
        w: img.width as usize,
        h: img.height as usize,
        px: &gray,
        scale: DETECT_SCALE,
    };
    let mut f = detect(&g);
    for x in &mut f {
        x.page = index;
        // The panel's colour as printed (detection only sees grey).
        if let Some(b) = &mut x.backdrop {
            b.color = mean_colour(&img, &x.rect, b.color[0]);
        }
    }
    let page_gray = PageGray {
        w: img.width as usize,
        h: img.height as usize,
        px: gray,
    };
    (f, page_gray)
}

/// Whether a page has (almost) no text layer: a scan, whose text can only
/// be read from the picture. A page number or stamp alone doesn't count.
pub fn lacks_text(runs: &[crate::text::TextRun]) -> bool {
    let chars: usize = runs
        .iter()
        .map(|r| r.text.chars().filter(|c| c.is_alphanumeric()).count())
        .sum();
    chars < 40
}

/// Drops found boxes that are printed text, judged by the words read from
/// a scan: a box mostly filled by words inside it (not a label above or
/// beside it) is a table cell or a framed paragraph, not a place to write.
/// Checkboxes and combs are kept; a big box with a heading in it is kept.
pub fn drop_text_boxes(fields: &mut Vec<Field>, words: &[crate::text::TextRun]) {
    fields.retain(|f| !(f.origin == Origin::Detected && holds_text(f, words)));
}

/// Fits found boxes and words read from a scan together: boxes that are
/// printed text go ([`drop_text_boxes`]), and checkbox outlines read as a
/// letter ("[ Other", "L Male") are cut from the words.
pub fn fit_scan_words(fields: &mut Vec<Field>, words: &mut Vec<crate::text::TextRun>) {
    drop_text_boxes(fields, words);
    drop_letter_boxes(fields, words);
    let boxes: Vec<Rect> = fields
        .iter()
        .filter(|f| f.kind.is_toggle())
        .map(|f| f.rect.inset(-1.5))
        .collect();
    for w in words.iter_mut() {
        trim_marks(w, &boxes);
    }
    words.retain(|w| !w.text.is_empty());
}

/// Drops found checkboxes that are a letter of a printed word: a "C" or
/// "D" whose curve closes into a box in a scan ("the CORRECT details").
/// The word read there must run on past the box (at least four letters or
/// digits), the box must be about letter-sized, and what was read on it
/// must not be one of the shapes an empty box is often read as ("L Male",
/// "[ Other", "O Yes"), so real checkboxes stay.
pub fn drop_letter_boxes(fields: &mut Vec<Field>, words: &[crate::text::TextRun]) {
    const BOX_LIKE: &[char] = &['O', 'o', 'L', 'l', 'I', 'i', 'D', '0', 'Q'];
    fields.retain(|f| {
        if !matches!(f.kind, FieldKind::Checkbox { .. }) || f.origin != Origin::Detected {
            return true;
        }
        let r = f.rect;
        let (cx, cy) = r.center();
        let Some(w) = words.iter().find(|w| w.rect.inset(-1.0).contains(cx, cy)) else {
            return true;
        };
        let chars: Vec<char> = w.text.chars().collect();
        if chars.len() != w.xs.len() || w.size <= 0.0 || r.height() > 1.4 * w.size {
            return true;
        }
        let on: Vec<usize> = (0..chars.len())
            .filter(|&k| {
                let c = w.xs[k] + w.ws[k] / 2.0;
                r.x0 - 1.0 <= c && c <= r.x1 + 1.0
            })
            .collect();
        let (Some(&first), Some(&last)) = (on.first(), on.last()) else {
            return true;
        };
        if on.len() > 2
            || on
                .iter()
                .any(|&k| !chars[k].is_alphanumeric() || BOX_LIKE.contains(&chars[k]))
        {
            return true;
        }
        // The word around the box: from the space before to the space after.
        let start = chars[..first]
            .iter()
            .rposition(|c| c.is_whitespace())
            .map_or(0, |i| i + 1);
        let end = chars[last..]
            .iter()
            .position(|c| c.is_whitespace())
            .map_or(chars.len(), |i| last + i);
        let letters = chars[start..end]
            .iter()
            .filter(|c| c.is_alphanumeric())
            .count();
        letters < 4
    });
}

/// Cuts the first and last word of a phrase when they lie on a checkbox.
fn trim_marks(w: &mut crate::text::TextRun, boxes: &[Rect]) {
    let chars: Vec<char> = w.text.chars().collect();
    if chars.len() != w.xs.len() || boxes.is_empty() {
        return;
    }
    let cy = w.rect.center().1;
    let on_box = |i: usize| {
        let cx = w.xs[i] + w.ws[i] / 2.0;
        boxes.iter().any(|b| b.contains(cx, cy))
    };
    let (mut a, mut b) = (0, chars.len());
    if let Some(sp) = chars.iter().position(|c| *c == ' ')
        && (0..sp).all(on_box)
    {
        a = sp + 1;
    }
    if let Some(sp) = chars.iter().rposition(|c| *c == ' ')
        && sp >= a
        && (sp + 1..chars.len()).all(on_box)
    {
        b = sp;
    }
    if (a, b) == (0, chars.len()) {
        return;
    }
    if (a..b).all(on_box) {
        b = a;
    }
    w.text = chars[a..b].iter().collect();
    w.xs = w.xs[a..b].to_vec();
    w.ws = w.ws[a..b].to_vec();
    if let (Some(x0), Some(x1)) = (w.xs.first(), w.xs.last().zip(w.ws.last())) {
        w.rect.x0 = *x0;
        w.rect.x1 = x1.0 + x1.1;
    }
}

fn holds_text(f: &Field, words: &[crate::text::TextRun]) -> bool {
    if !matches!(
        f.kind.text_mode(),
        Some(TextMode::Single | TextMode::Multi | TextMode::OnLine)
    ) {
        return false;
    }
    let r = f.rect;
    if r.width() <= 0.0 || r.height() <= 0.0 {
        return false;
    }
    let inside: Vec<&Rect> = words
        .iter()
        .map(|w| &w.rect)
        .filter(|w| r.contains(w.center().0, w.center().1) && w.overlap_ratio(&r) > 0.7)
        .collect();
    let Some(line_h) = inside.iter().map(|w| w.height()).reduce(f32::max) else {
        return false;
    };
    let area: f32 = inside
        .iter()
        .filter_map(|w| w.intersect(&r))
        .map(|w| w.area())
        .sum();
    if r.height() < 2.5 * line_h {
        // One line: words across most of its width.
        let mut spans: Vec<(f32, f32)> = inside
            .iter()
            .map(|w| (w.x0.max(r.x0), w.x1.min(r.x1)))
            .collect();
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (mut covered, mut end) = (0.0, r.x0);
        for (a, b) in spans {
            covered += (b - a.max(end)).max(0.0);
            end = end.max(b);
        }
        covered > 0.6 * r.width()
    } else {
        area > 0.35 * r.area()
    }
}

/// The average colour of the pixels under `r` whose grey is about `grey`
/// (the panel, not the text on it).
fn mean_colour(img: &crate::render::Image, r: &Rect, grey: u8) -> [u8; 3] {
    let s = DETECT_SCALE;
    let (w, h) = (img.width as usize, img.height as usize);
    let (x0, x1) = ((r.x0 * s) as usize, ((r.x1 * s) as usize).min(w));
    let (y0, y1) = ((r.y0 * s) as usize, ((r.y1 * s) as usize).min(h));
    let (mut sum, mut n) = ([0u64; 3], 0u64);
    for y in y0..y1 {
        for x in x0..x1 {
            let p = &img.rgba[(y * w + x) * 4..(y * w + x) * 4 + 3];
            let luma = (p[0] as u32 * 299 + p[1] as u32 * 587 + p[2] as u32 * 114) / 1000;
            if luma.abs_diff(grey as u32) <= 2 {
                for k in 0..3 {
                    sum[k] += p[k] as u64;
                }
                n += 1;
            }
        }
    }
    if n == 0 {
        return [grey; 3];
    }
    [(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8]
}

/// Gives each panel field the text already printed in it (by another
/// editor that flattened its answers into the page): the field starts with
/// that text, and covers it only once its value changes.
pub fn prefill_printed(fields: &mut [Field], runs: &[crate::text::TextRun]) {
    for f in fields.iter_mut() {
        let Some(b) = &mut f.backdrop else { continue };
        let r = f.rect;
        let mut inside: Vec<&crate::text::TextRun> = runs
            .iter()
            .filter(|t| {
                let (cx, cy) = t.rect.center();
                r.x0 <= cx && cx <= r.x1 && r.y0 <= cy && cy <= r.y1
            })
            .collect();
        if inside.is_empty() {
            continue;
        }
        // Lines top to bottom, runs left to right.
        crate::geom::line_order(
            &mut inside,
            |a, b| a.rect.center().1.total_cmp(&b.rect.center().1),
            |a, b| (a.rect.center().1 - b.rect.center().1).abs() < a.size.max(b.size) * 0.5,
            |t| t.rect.x0,
        );
        let text = inside
            .iter()
            .map(|t| t.text.trim())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        b.printed = text.clone();
        f.value = text;
    }
}
