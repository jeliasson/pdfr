//! Text with positions from a page's text layer, via hayro-interpret's
//! `Device` interface (pure Rust; no separate parser). Used for field labels.

use crate::geom::Rect;
use hayro::hayro_interpret::font::GlyphRun;
use hayro::hayro_interpret::util::TransformExt;
use hayro::hayro_interpret::{
    BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, Image, ImageDrawProps,
    InterpreterCache, InterpreterSettings, RenderContent, SoftMask, interpret_page,
};
use hayro::hayro_syntax::page::Page;
use hayro::kurbo::{Affine, BezPath, Point};

/// A phrase on one line, in display space.
#[derive(Clone, Debug, PartialEq)]
pub struct TextRun {
    pub rect: Rect,
    pub text: String,
    /// Font size (pt).
    pub size: f32,
    /// Left x of each char of `text` (same count as `text.chars()`), or
    /// empty when unknown.
    pub xs: Vec<f32>,
    /// Width of each char (parallel to `xs`): up to where the next glyph
    /// starts, or an estimate for the last one on a stretch.
    pub ws: Vec<f32>,
}

impl TextRun {
    /// The box of character `i`, in display space.
    pub fn char_rect(&self, i: usize) -> Rect {
        match (self.xs.get(i), self.ws.get(i)) {
            (Some(&x), Some(&w)) => Rect::new(x, self.rect.y0, x + w, self.rect.y1),
            _ => self.rect,
        }
    }

    /// The character nearest to x (by its middle), and whether `x` lies
    /// past that character's middle (a caret would go after it).
    pub fn char_at(&self, x: f32) -> (usize, bool) {
        let n = self.xs.len();
        if n == 0 {
            return (0, false);
        }
        let mut best = (0, f32::MAX);
        for i in 0..n {
            let r = self.char_rect(i);
            let d = (r.x0 - x).max(x - r.x1).max(0.0);
            if d < best.1 {
                best = (i, d);
            }
        }
        let r = self.char_rect(best.0);
        (best.0, x > (r.x0 + r.x1) / 2.0)
    }

    /// The part of the text whose characters start within `x0..x1`, trimmed.
    pub fn clip(&self, x0: f32, x1: f32) -> String {
        if self.xs.len() != self.text.chars().count() {
            return self.text.clone();
        }
        let chars: Vec<char> = self.text.chars().collect();
        let inside: Vec<usize> = (0..chars.len())
            .filter(|&i| self.xs[i] >= x0 && self.xs[i] < x1)
            .collect();
        let (Some(&first), Some(&last)) = (inside.first(), inside.last()) else {
            return String::new();
        };
        let (mut a, mut b) = (first, last + 1);
        // Never keep part of a word cut by the edge.
        if a > 0 && chars[a - 1].is_alphanumeric() {
            while a < b && chars[a].is_alphanumeric() {
                a += 1;
            }
        }
        if b < chars.len() && chars[b].is_alphanumeric() {
            while b > a && chars[b - 1].is_alphanumeric() {
                b -= 1;
            }
        }
        chars[a..b].iter().collect::<String>().trim().to_string()
    }

    /// Left x of the first non-space character starting within `x0..x1`.
    pub fn clip_start(&self, x0: f32, x1: f32) -> Option<f32> {
        if self.xs.len() != self.text.chars().count() {
            return Some(self.rect.x0);
        }
        self.text
            .chars()
            .zip(&self.xs)
            .find(|(c, x)| !c.is_whitespace() && **x >= x0 && **x < x1)
            .map(|(_, x)| *x)
    }
}

struct G {
    s: String,
    x: f32,
    y: f32,
    size: f32,
}

#[derive(Default)]
struct Collector {
    glyphs: Vec<G>,
}

impl Device<'_> for Collector {
    fn draw_path(&mut self, _: &BezPath, _: DrawProps<'_>, _: &DrawMode) {}
    fn push_clip_path(&mut self, _: &ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'_>>, _: BlendMode) {}
    fn draw_glyph_run(&mut self, run: &GlyphRun<'_, '_>, props: DrawProps<'_>, _: &DrawMode) {
        for g in run.glyphs() {
            let t: Affine = props.transform * g.transform();
            let o = t * Point::ZERO;
            let up = t * Point::new(0.0, 1000.0) - o;
            let right = t * Point::new(1000.0, 0.0) - o;
            // Horizontal, upright text only (display y grows downward).
            if right.x <= 0.0 || right.y.abs() > right.x * 0.2 || up.y >= 0.0 {
                continue;
            }
            let Some(u) = g.as_unicode() else { continue };
            let s = match u {
                hayro::hayro_interpret::hayro_cmap::BfString::Char(c) => c.to_string(),
                hayro::hayro_interpret::hayro_cmap::BfString::String(s) => s,
            };
            self.glyphs.push(G {
                s,
                x: o.x as f32,
                y: o.y as f32,
                size: up.length() as f32,
            });
        }
    }
    fn draw_image(&mut self, _: Image<'_, '_>, _: ImageDrawProps<'_>) {}
    fn pop_clip(&mut self) {}
    fn pop_transparency_group(&mut self) {}
}

/// Extracts text phrases from a page's content (annotations excluded).
pub fn extract<'a>(page: &Page<'a>, cache: &InterpreterCache<'a>) -> Vec<TextRun> {
    let (w, h) = page.render_dimensions();
    let settings = InterpreterSettings {
        render_content: RenderContent::PAGE,
        ..Default::default()
    };
    let mut ctx = Context::new(
        page.initial_transform(true).to_kurbo(),
        hayro::kurbo::Rect::new(0.0, 0.0, w as f64, h as f64),
        cache,
        page.xref(),
        settings,
    );
    let mut dev = Collector::default();
    interpret_page(page, &mut ctx, &mut dev);
    group(dev.glyphs)
}

fn group(mut gs: Vec<G>) -> Vec<TextRun> {
    // Sort by line (baseline), then x.
    gs.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));
    let mut lines: Vec<Vec<G>> = Vec::new();
    for g in gs {
        match lines.last_mut() {
            Some(l) if (l[0].y - g.y).abs() <= 0.25 * l[0].size.max(g.size) => l.push(g),
            _ => lines.push(vec![g]),
        }
    }
    let mut out = Vec::new();
    for mut l in lines {
        l.sort_by(|a, b| a.x.total_cmp(&b.x));
        let mut cur: Option<(TextRun, f32)> = None; // (run, x of last glyph)
        for g in l {
            let adv = 0.55 * g.size; // estimated width of one glyph
            if let Some((run, last_x)) = &mut cur {
                let gap = g.x - *last_x;
                if gap > 2.2 * g.size {
                    out.push(finish(cur.take().unwrap().0));
                } else {
                    if gap > 0.9 * g.size && !run.text.ends_with(' ') && g.s != " " {
                        run.text.push(' ');
                        run.xs.push(*last_x + adv);
                    }
                    run.text.push_str(&g.s);
                    run.xs.extend(std::iter::repeat_n(g.x, g.s.chars().count()));
                    run.rect.x1 = run.rect.x1.max(g.x + adv);
                    *last_x = g.x;
                    continue;
                }
            }
            let rect = Rect::new(g.x, g.y - 0.75 * g.size, g.x + adv, g.y + 0.22 * g.size);
            let xs = vec![g.x; g.s.chars().count()];
            cur = Some((
                TextRun {
                    rect,
                    text: g.s.clone(),
                    size: g.size,
                    xs,
                    ws: vec![],
                },
                g.x,
            ));
        }
        if let Some((run, _)) = cur {
            out.push(finish(run));
        }
    }
    out.retain(|r| !r.text.trim().is_empty());
    out
}

/// Collapses whitespace, keeping `xs` aligned with the characters.
fn finish(mut r: TextRun) -> TextRun {
    let mut text = String::new();
    let mut xs = Vec::new();
    for (c, x) in r.text.chars().zip(r.xs.iter().copied()) {
        if c.is_whitespace() {
            if text.is_empty() || text.ends_with(' ') {
                continue;
            }
            text.push(' ');
        } else {
            text.push(c);
        }
        xs.push(x);
    }
    if text.ends_with(' ') {
        text.pop();
        xs.pop();
    }
    r.ws = widths(&text, &mut xs, r.size);
    if let (Some(x), Some(w)) = (xs.last(), r.ws.last()) {
        r.rect.x1 = x + w;
    }
    r.text = text;
    r.xs = xs;
    r
}

/// Character widths from where each glyph starts: up to the next glyph
/// when it follows closely, else an estimate. Characters sharing one glyph
/// (a ligature like "fi") split its width, and their x is spread to match.
fn widths(text: &str, xs: &mut [f32], size: f32) -> Vec<f32> {
    let est = 0.55 * size;
    let spaces: Vec<bool> = text.chars().map(|c| c == ' ').collect();
    let mut ws = vec![est; xs.len()];
    let mut i = 0;
    while i < xs.len() {
        let mut j = i + 1;
        while j < xs.len() && (xs[j] - xs[i]).abs() < 1e-3 {
            j += 1;
        }
        let w = match xs.get(j) {
            Some(&nx) if nx > xs[i] && nx - xs[i] < 1.6 * size => nx - xs[i],
            _ if spaces.get(i) == Some(&true) => est * 0.5,
            _ => est,
        };
        let part = w / (j - i) as f32;
        let x0 = xs[i];
        for k in i..j {
            xs[k] = x0 + part * (k - i) as f32;
            ws[k] = part;
        }
        i = j;
    }
    ws
}

/// A caret position in a page's text: before character `ch` of phrase
/// `run` (`ch` may equal the phrase's length: after its last character).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextPos {
    pub run: usize,
    pub ch: usize,
}

/// Whether two phrases sit on one line.
fn same_line(a: &TextRun, b: &TextRun) -> bool {
    (a.rect.y1 - b.rect.y1).abs() < 0.3 * a.size.max(b.size).max(1.0)
}

/// Phrase indices in reading order: by line, then left to right.
pub fn reading_order(runs: &[TextRun]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..runs.len()).collect();
    idx.sort_by(|&a, &b| {
        let (ra, rb) = (&runs[a], &runs[b]);
        if same_line(ra, rb) {
            ra.rect.x0.total_cmp(&rb.rect.x0)
        } else {
            ra.rect.y1.total_cmp(&rb.rect.y1)
        }
    });
    idx
}

/// The caret position nearest to a point (display space), if the page has
/// any text. Lines count more than columns, so dragging along a line
/// stays on it.
pub fn pos_at(runs: &[TextRun], x: f32, y: f32) -> Option<TextPos> {
    let (run, _) = runs
        .iter()
        .enumerate()
        .filter(|(_, r)| !r.xs.is_empty())
        .map(|(i, r)| {
            let dy = (r.rect.y0 - y).max(y - r.rect.y1).max(0.0);
            let dx = (r.rect.x0 - x).max(x - r.rect.x1).max(0.0);
            (i, dy * 4.0 + dx)
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))?;
    let (ch, after) = runs[run].char_at(x);
    Some(TextPos {
        run,
        ch: ch + after as usize,
    })
}

/// The selection between two caret positions as `(phrase, from, to)`
/// character ranges, in reading order.
pub fn selection(runs: &[TextRun], a: TextPos, b: TextPos) -> Vec<(usize, usize, usize)> {
    let order = reading_order(runs);
    let rank = |p: TextPos| order.iter().position(|&r| r == p.run).unwrap_or(0);
    let (s, e) = if (rank(a), a.ch) <= (rank(b), b.ch) {
        (a, b)
    } else {
        (b, a)
    };
    let (rs, re) = (rank(s), rank(e));
    let mut out = Vec::new();
    for (k, &ri) in order.iter().enumerate().take(re + 1).skip(rs) {
        let n = runs[ri].xs.len();
        let from = if k == rs { s.ch.min(n) } else { 0 };
        let to = if k == re { e.ch.min(n) } else { n };
        if to > from {
            out.push((ri, from, to));
        }
    }
    out
}

/// The text of a selection: phrases on one line joined with a space,
/// lines with a line break.
pub fn selected_text(runs: &[TextRun], sel: &[(usize, usize, usize)]) -> String {
    let mut out = String::new();
    let mut prev: Option<usize> = None;
    for &(ri, from, to) in sel {
        if let Some(p) = prev {
            out.push(if same_line(&runs[p], &runs[ri]) {
                ' '
            } else {
                '\n'
            });
        }
        out.extend(runs[ri].text.chars().skip(from).take(to - from));
        prev = Some(ri);
    }
    out
}

/// The word around character `ch` of phrase `run`, as a caret range.
pub fn word_at(runs: &[TextRun], run: usize, ch: usize) -> (TextPos, TextPos) {
    let chars: Vec<char> = runs[run].text.chars().collect();
    let ch = ch.min(chars.len().saturating_sub(1));
    let word = |c: char| c.is_alphanumeric() || matches!(c, '@' | '.' | '-' | '_' | '/');
    let (mut a, mut b) = (ch, ch + 1);
    if chars.get(ch).is_some_and(|c| word(*c)) {
        while a > 0 && word(chars[a - 1]) {
            a -= 1;
        }
        while b < chars.len() && word(chars[b]) {
            b += 1;
        }
        // A sentence's full stop is not part of the word.
        while b > a + 1 && matches!(chars[b - 1], '.' | '-') {
            b -= 1;
        }
    }
    (TextPos { run, ch: a }, TextPos { run, ch: b })
}

/// The whole line (every phrase on it) around phrase `run`.
pub fn line_at(runs: &[TextRun], run: usize) -> (TextPos, TextPos) {
    let on_line: Vec<usize> = reading_order(runs)
        .into_iter()
        .filter(|&r| same_line(&runs[r], &runs[run]))
        .collect();
    let first = *on_line.first().unwrap_or(&run);
    let last = *on_line.last().unwrap_or(&run);
    (
        TextPos { run: first, ch: 0 },
        TextPos {
            run: last,
            ch: runs[last].xs.len(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, x: f32, y: f32) -> TextRun {
        let mut xs: Vec<f32> = (0..text.chars().count()).map(|i| x + i as f32 * 5.0).collect();
        let ws = widths(text, &mut xs, 10.0);
        TextRun {
            rect: Rect::new(x, y - 8.0, x + xs.len() as f32 * 5.0, y + 2.0),
            text: text.into(),
            size: 10.0,
            xs,
            ws,
        }
    }

    #[test]
    fn select_across_phrases_and_lines() {
        let runs = vec![
            run("Address:", 10.0, 120.0),
            run("Ref: AB-1234", 10.0, 100.0),
            run("Storgatan 12", 80.0, 120.0),
            run("411 01 Göteborg", 80.0, 140.0),
        ];
        // From inside "Ref" on the first line to the end of "Storgatan".
        let a = pos_at(&runs, 16.0, 100.0).unwrap();
        assert_eq!(a, TextPos { run: 1, ch: 1 });
        let b = pos_at(&runs, 500.0, 121.0).unwrap();
        let sel = selection(&runs, b, a);
        assert_eq!(selected_text(&runs, &sel), "ef: AB-1234\nAddress: Storgatan 12");
        let (w0, w1) = word_at(&runs, 1, 7);
        assert_eq!(selected_text(&runs, &selection(&runs, w0, w1)), "AB-1234");
        let (l0, l1) = line_at(&runs, 2);
        assert_eq!(selected_text(&runs, &selection(&runs, l0, l1)), "Address: Storgatan 12");
        assert!(pos_at(&[], 0.0, 0.0).is_none());
    }
}
