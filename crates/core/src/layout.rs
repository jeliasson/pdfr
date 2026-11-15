//! The one text layout routine. The screen draws from it and the PDF writer
//! writes from it, so on-screen text and saved text are identical.
//!
//! All positions are in display space (points, y down).

use crate::field::{Align, Field, FieldKind, TextMode};
use crate::font::{Font, font};
use crate::geom::Rect;

/// Automatic size bounds for single-line and comb fields.
pub const AUTO_MIN: f32 = 4.0;
pub const AUTO_MAX: f32 = 11.0;
/// Fraction of the field height used as the automatic size.
pub const AUTO_FRACTION: f32 = 0.62;
/// Horizontal padding inside boxed fields.
pub const PAD: f32 = 3.0;
/// Baseline offset above a write-on line.
pub const LINE_BASELINE_GAP: f32 = 2.0;
/// Line height as a multiple of the font size in multi-line fields.
pub const LINE_SPACING: f32 = 1.15;

#[derive(Clone, Debug, PartialEq)]
pub struct Glyph {
    /// The character drawn (a bullet for password fields).
    pub ch: char,
    /// Glyph id in the bundled font.
    pub gid: u16,
    /// Left edge of the glyph's advance box.
    pub x: f32,
    pub advance: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub baseline: f32,
    /// One glyph per character in `start..end`.
    pub glyphs: Vec<Glyph>,
    /// Char index of the first character of the line in the value.
    pub start: usize,
    /// Char index just past the line's last character (excludes a `\n`).
    pub end: usize,
    /// True when the line ends with an explicit newline (or is the last line).
    pub hard_end: bool,
    /// x where the line starts (used for the caret on an empty line).
    pub x_start: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub size: f32,
    pub line_height: f32,
    pub lines: Vec<Line>,
    /// Comb cells, if the field is a comb.
    pub cells: Option<Vec<(f32, f32)>>,
    /// Number of characters in the laid-out text.
    pub len: usize,
}

/// Lays out `text` in `field`.
pub fn layout(field: &Field, text: &str) -> Layout {
    let f = font();
    let (mode, password) = match &field.kind {
        FieldKind::Text { mode, password, .. } => (mode.clone(), *password),
        // Drop-downs show their value as a single line.
        _ => (TextMode::Single, false),
    };
    let chars: Vec<char> = text
        .chars()
        .map(|c| {
            if c == '\n' {
                c
            } else if password {
                '\u{2022}'
            } else {
                f.display_char(c).unwrap_or(' ')
            }
        })
        .collect();
    let r = field.rect;
    match mode {
        TextMode::Single | TextMode::OnLine => {
            let flat: Vec<char> = chars
                .iter()
                .map(|&c| if c == '\n' { ' ' } else { c })
                .collect();
            let avail = (r.width() - 2.0 * PAD).max(1.0);
            let width_at = |s: f32| flat.iter().map(|&c| f.advance(c, s)).sum::<f32>();
            let size = match field.font_size {
                Some(s) => s.min(max_size_for_height(f, r.height())).max(1.0),
                None => {
                    let mut s = (r.height() * AUTO_FRACTION).clamp(AUTO_MIN, AUTO_MAX);
                    let w = width_at(s);
                    if w > avail {
                        s = (s * avail / w).max(AUTO_MIN);
                    }
                    s
                }
            };
            let baseline = if mode == TextMode::OnLine {
                r.y1 - LINE_BASELINE_GAP
            } else {
                vcenter_baseline(f, &r, size)
            };
            let w = width_at(size);
            let x0 = align_x(field.align, &r, w);
            let line = place_line(f, &flat, 0, flat.len(), x0, baseline, size, true);
            Layout {
                size,
                line_height: size * LINE_SPACING,
                len: flat.len(),
                lines: vec![line],
                cells: None,
            }
        }
        TextMode::Comb(cells) => {
            let flat: Vec<char> = chars
                .iter()
                .map(|&c| if c == '\n' { ' ' } else { c })
                .collect();
            let cell_w = cells
                .iter()
                .map(|c| c.1 - c.0)
                .fold(f32::INFINITY, f32::min)
                .max(1.0);
            // Reference width: a capital M must fit its cell with a little air.
            let m_em = f.advance('M', 1.0);
            let size = match field.font_size {
                Some(s) => s.min(max_size_for_height(f, r.height())).max(1.0),
                None => (r.height() * AUTO_FRACTION).clamp(AUTO_MIN, AUTO_MAX),
            }
            .min(cell_w * 0.92 / m_em)
            .max(1.0);
            let baseline = vcenter_baseline(f, &r, size);
            let glyphs = flat
                .iter()
                .zip(cells.iter())
                .map(|(&c, &(a, b))| {
                    let adv = f.advance(c, size);
                    Glyph {
                        ch: c,
                        gid: f.glyph_id(c),
                        x: (a + b) * 0.5 - adv * 0.5,
                        advance: adv,
                    }
                })
                .collect::<Vec<_>>();
            let n = glyphs.len();
            Layout {
                size,
                line_height: size * LINE_SPACING,
                len: n,
                lines: vec![Line {
                    baseline,
                    glyphs,
                    start: 0,
                    end: n,
                    hard_end: true,
                    x_start: cells.first().map_or(r.x0, |c| c.0),
                }],
                cells: Some(cells),
            }
        }
        TextMode::Multi => {
            let avail = (r.width() - 2.0 * PAD).max(1.0);
            let avail_h = (r.height() - 2.0 * PAD).max(1.0);
            let size = match field.font_size {
                Some(s) => s,
                None => {
                    let mut s = AUTO_MAX;
                    while s > AUTO_MIN {
                        let n = wrap(f, &chars, s, avail).len() as f32;
                        if n * s * LINE_SPACING <= avail_h {
                            break;
                        }
                        s -= 0.5;
                    }
                    s.max(AUTO_MIN)
                }
            };
            let lh = size * LINE_SPACING;
            let first = r.y0 + PAD + size * f.ascent();
            let lines = wrap(f, &chars, size, avail)
                .into_iter()
                .enumerate()
                .map(|(i, (s, e, hard))| {
                    let w: f32 = chars[s..e]
                        .iter()
                        .rev()
                        .skip_while(|c| **c == ' ')
                        .map(|&c| f.advance(c, size))
                        .sum();
                    let x0 = align_x(field.align, &r, w);
                    place_line(f, &chars, s, e, x0, first + lh * i as f32, size, hard)
                })
                .collect();
            Layout {
                size,
                line_height: lh,
                len: chars.len(),
                lines,
                cells: None,
            }
        }
    }
}

/// Largest size whose cap height plus descender fits the box height.
fn max_size_for_height(f: &Font, h: f32) -> f32 {
    ((h - 1.0) / (f.cap_height() + f.descent())).max(1.0)
}

fn vcenter_baseline(f: &Font, r: &Rect, size: f32) -> f32 {
    r.center().1 + f.cap_height() * size * 0.5
}

fn align_x(align: Align, r: &Rect, w: f32) -> f32 {
    match align {
        Align::Left => r.x0 + PAD,
        Align::Center => r.center().0 - w * 0.5,
        Align::Right => r.x1 - PAD - w,
    }
}

#[allow(clippy::too_many_arguments)]
fn place_line(
    f: &Font,
    chars: &[char],
    start: usize,
    end: usize,
    x0: f32,
    baseline: f32,
    size: f32,
    hard_end: bool,
) -> Line {
    let mut x = x0;
    let glyphs = chars[start..end]
        .iter()
        .map(|&c| {
            let adv = f.advance(c, size);
            let g = Glyph {
                ch: c,
                gid: f.glyph_id(c),
                x,
                advance: adv,
            };
            x += adv;
            g
        })
        .collect();
    Line {
        baseline,
        glyphs,
        start,
        end,
        hard_end,
        x_start: x0,
    }
}

/// Word-wraps `chars` into `(start, end, hard_end)` line ranges. A soft-wrapped
/// line keeps its trailing spaces; a hard line excludes the `\n`.
fn wrap(f: &Font, chars: &[char], size: f32, avail: f32) -> Vec<(usize, usize, bool)> {
    let mut out = Vec::new();
    let mut start = 0;
    loop {
        let mut x = 0.0;
        let mut i = start;
        let mut last_break: Option<usize> = None; // index after a space
        let mut hard = false;
        while i < chars.len() {
            let c = chars[i];
            if c == '\n' {
                hard = true;
                break;
            }
            let adv = f.advance(c, size);
            if c != ' ' && x + adv > avail && i > start {
                break;
            }
            x += adv;
            i += 1;
            if c == ' ' {
                last_break = Some(i);
            }
        }
        if hard {
            out.push((start, i, true));
            start = i + 1;
            continue;
        }
        if i >= chars.len() {
            out.push((start, chars.len(), true));
            break;
        }
        // Overflow: break after the last space, or mid-word if there is none.
        let end = last_break.unwrap_or(i);
        out.push((start, end, false));
        start = end;
    }
    out
}

impl Layout {
    /// The line index that holds caret position `idx`.
    pub fn line_of(&self, idx: usize) -> usize {
        for (li, l) in self.lines.iter().enumerate() {
            if idx >= l.start && (idx < l.end || (idx == l.end && l.hard_end)) {
                return li;
            }
        }
        self.lines.len().saturating_sub(1)
    }

    /// Caret position for char index `idx`: `(x, baseline, line index)`.
    pub fn caret_pos(&self, idx: usize) -> (f32, f32, usize) {
        let li = self.line_of(idx);
        let Some(l) = self.lines.get(li) else {
            return (0.0, 0.0, 0);
        };
        if let Some(cells) = &self.cells {
            let x = if idx < cells.len() {
                cells[idx].0
            } else {
                cells.last().map_or(l.x_start, |c| c.1)
            };
            return (x, l.baseline, li);
        }
        let k = idx.saturating_sub(l.start);
        let x = if let Some(g) = l.glyphs.get(k) {
            g.x
        } else if let Some(g) = l.glyphs.last() {
            g.x + g.advance
        } else {
            l.x_start
        };
        (x, l.baseline, li)
    }

    /// Char index closest to the point `(x, y)` — click-to-caret.
    pub fn hit(&self, x: f32, y: f32) -> usize {
        if self.lines.is_empty() {
            return 0;
        }
        // Pick the line whose band (baseline - size .. baseline + descent) is nearest.
        let li = self
            .lines
            .iter()
            .enumerate()
            .min_by(|a, b| {
                let da = (a.1.baseline - self.size * 0.35 - y).abs();
                let db = (b.1.baseline - self.size * 0.35 - y).abs();
                da.total_cmp(&db)
            })
            .map(|(i, _)| i)
            .unwrap_or(0);
        self.hit_in_line(li, x)
    }

    /// Char index in line `li` closest to `x`.
    pub fn hit_in_line(&self, li: usize, x: f32) -> usize {
        let l = &self.lines[li];
        if let Some(cells) = &self.cells {
            let cell = cells
                .iter()
                .position(|&(_, b)| x < b)
                .unwrap_or(cells.len().saturating_sub(1));
            return cell.min(self.len);
        }
        for (k, g) in l.glyphs.iter().enumerate() {
            if x < g.x + g.advance * 0.5 {
                return l.start + k;
            }
        }
        if l.hard_end || l.end == l.start {
            l.end
        } else {
            // A soft-wrapped line's end belongs to the next line.
            l.end - 1
        }
    }

    /// Selection highlight rectangles for the char range `a..b`.
    pub fn selection_rects(&self, a: usize, b: usize) -> Vec<Rect> {
        let (a, b) = (a.min(b), a.max(b));
        let mut out = Vec::new();
        if a == b {
            return out;
        }
        for (li, l) in self.lines.iter().enumerate() {
            let s = a.max(l.start);
            let e = b.min(l.end);
            if s >= e {
                continue;
            }
            let x0 = self.caret_pos_in_line(li, s);
            let x1 = self.caret_pos_in_line(li, e);
            let f = font();
            let top = l.baseline - self.size * f.ascent();
            let bot = l.baseline + self.size * f.descent();
            out.push(Rect::new(x0, top, x1.max(x0 + 1.0), bot));
        }
        out
    }

    fn caret_pos_in_line(&self, li: usize, idx: usize) -> f32 {
        let l = &self.lines[li];
        if let Some(cells) = &self.cells {
            return if idx < cells.len() {
                cells[idx].0
            } else {
                cells.last().map_or(l.x_start, |c| c.1)
            };
        }
        let k = idx - l.start;
        match l.glyphs.get(k) {
            Some(g) => g.x,
            None => l.glyphs.last().map_or(l.x_start, |g| g.x + g.advance),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::Origin;

    fn text_field(rect: Rect, mode: TextMode) -> Field {
        Field::new_text(0, rect, mode, Origin::Detected)
    }

    #[test]
    fn comb_glyphs_centred_in_cells() {
        let rect = Rect::new(100.0, 100.0, 220.0, 118.0);
        let cells = Field::equal_cells(&rect, 6);
        let f = text_field(rect, TextMode::Comb(cells.clone()));
        let l = layout(&f, "AB1iW");
        let line = &l.lines[0];
        assert_eq!(line.glyphs.len(), 5);
        for (g, c) in line.glyphs.iter().zip(&cells) {
            let centre = g.x + g.advance / 2.0;
            assert!((centre - (c.0 + c.1) / 2.0).abs() < 1e-3);
            // Every glyph sits inside its cell.
            assert!(
                g.x >= c.0 - 0.5 && g.x + g.advance <= c.1 + 0.5,
                "{g:?} {c:?}"
            );
        }
    }

    #[test]
    fn comb_with_uneven_cells() {
        // Detected cells need not be equal: e.g. a date row with gaps.
        let cells = vec![(10.0, 22.0), (22.0, 34.0), (40.0, 52.0), (52.0, 64.0)];
        let f = text_field(
            Rect::new(10.0, 0.0, 64.0, 14.0),
            TextMode::Comb(cells.clone()),
        );
        let l = layout(&f, "1207");
        for (g, c) in l.lines[0].glyphs.iter().zip(&cells) {
            assert!((g.x + g.advance / 2.0 - (c.0 + c.1) / 2.0).abs() < 1e-3);
        }
        // Caret before index 2 is at the third cell's left edge.
        assert_eq!(l.caret_pos(2).0, 40.0);
        assert_eq!(l.caret_pos(4).0, 64.0);
    }

    #[test]
    fn single_line_auto_size_and_vertical_centre() {
        let f = text_field(Rect::new(0.0, 0.0, 300.0, 16.0), TextMode::Single);
        let l = layout(&f, "Hello");
        assert!((l.size - 16.0 * AUTO_FRACTION).abs() < 1e-3);
        // Cap height is centred: the cap top and baseline are symmetric around the centre.
        let cap = crate::font::font().cap_height() * l.size;
        let base = l.lines[0].baseline;
        assert!(((base - cap) + base - 16.0).abs() < 1e-3);
        // Tall fields are clamped to 11pt; tiny ones to 4pt.
        let big = text_field(Rect::new(0.0, 0.0, 300.0, 40.0), TextMode::Single);
        assert_eq!(layout(&big, "x").size, AUTO_MAX);
        let tiny = text_field(Rect::new(0.0, 0.0, 300.0, 3.0), TextMode::Single);
        assert_eq!(layout(&tiny, "x").size, AUTO_MIN);
    }

    #[test]
    fn shrink_to_fit_width() {
        let f = text_field(Rect::new(0.0, 0.0, 60.0, 16.0), TextMode::Single);
        let text = "A rather long piece of text";
        let l = layout(&f, text);
        assert!(l.size < 16.0 * AUTO_FRACTION);
        let last = l.lines[0].glyphs.last().unwrap();
        assert!(last.x + last.advance <= 60.0 - PAD + 0.01 || l.size == AUTO_MIN);
        // Short text keeps the full size.
        assert!((layout(&f, "Hi").size - 16.0 * AUTO_FRACTION).abs() < 1e-3);
    }

    #[test]
    fn fixed_size_is_honoured() {
        let mut f = text_field(Rect::new(0.0, 0.0, 200.0, 20.0), TextMode::Single);
        f.font_size = Some(9.0);
        assert_eq!(layout(&f, "abc").size, 9.0);
    }

    #[test]
    fn write_on_line_baseline() {
        let f = text_field(Rect::new(0.0, 80.0, 200.0, 100.0), TextMode::OnLine);
        let l = layout(&f, "Signed");
        assert_eq!(l.lines[0].baseline, 98.0);
    }

    #[test]
    fn alignment() {
        let mut f = text_field(Rect::new(0.0, 0.0, 200.0, 16.0), TextMode::Single);
        f.align = Align::Right;
        let l = layout(&f, "abc");
        let g = l.lines[0].glyphs.last().unwrap();
        assert!((g.x + g.advance - (200.0 - PAD)).abs() < 1e-3);
        f.align = Align::Center;
        let l = layout(&f, "abc");
        let g0 = &l.lines[0].glyphs[0];
        let g2 = &l.lines[0].glyphs[2];
        assert!(((g0.x + g2.x + g2.advance) / 2.0 - 100.0).abs() < 1e-3);
    }

    #[test]
    fn multiline_wraps_and_shrinks() {
        let f = text_field(Rect::new(0.0, 0.0, 100.0, 200.0), TextMode::Multi);
        let l = layout(&f, "The quick brown fox jumps over the lazy dog");
        assert_eq!(l.size, AUTO_MAX);
        assert!(l.lines.len() >= 3, "{}", l.lines.len());
        for line in &l.lines {
            let w: f32 = line
                .glyphs
                .iter()
                .rev()
                .skip_while(|g| g.ch == ' ')
                .map(|g| g.advance)
                .sum();
            assert!(w <= 100.0 - 2.0 * PAD + 0.01);
        }
        // Lines are contiguous and cover the text.
        assert_eq!(l.lines[0].start, 0);
        for w in l.lines.windows(2) {
            assert_eq!(w[0].end, w[1].start);
        }
        assert_eq!(l.lines.last().unwrap().end, l.len);
        // Top-aligned.
        assert!(l.lines[0].baseline < 20.0);
        // A small box forces shrinking until it fits.
        let small = text_field(Rect::new(0.0, 0.0, 100.0, 30.0), TextMode::Multi);
        let ls = layout(&small, "The quick brown fox jumps over the lazy dog");
        assert!(ls.size < AUTO_MAX);
        assert!(ls.lines.len() as f32 * ls.line_height <= 30.0 - 2.0 * PAD + 0.01);
    }

    #[test]
    fn multiline_hard_breaks() {
        let f = text_field(Rect::new(0.0, 0.0, 300.0, 200.0), TextMode::Multi);
        let l = layout(&f, "ab\n\ncd");
        assert_eq!(l.lines.len(), 3);
        assert_eq!((l.lines[0].start, l.lines[0].end), (0, 2));
        assert_eq!((l.lines[1].start, l.lines[1].end), (3, 3));
        assert_eq!((l.lines[2].start, l.lines[2].end), (4, 6));
        assert_eq!(l.line_of(2), 0);
        assert_eq!(l.line_of(3), 1);
        assert_eq!(l.line_of(4), 2);
    }

    #[test]
    fn click_to_caret() {
        let f = text_field(Rect::new(0.0, 0.0, 300.0, 16.0), TextMode::Single);
        let l = layout(&f, "WWW");
        let g = &l.lines[0].glyphs;
        let y = l.lines[0].baseline - 3.0;
        assert_eq!(l.hit(g[0].x + 1.0, y), 0);
        assert_eq!(l.hit(g[0].x + g[0].advance * 0.75, y), 1);
        assert_eq!(l.hit(g[1].x + g[1].advance * 0.4, y), 1);
        assert_eq!(l.hit(g[2].x + g[2].advance * 0.6, y), 3);
        assert_eq!(l.hit(299.0, y), 3);
        assert_eq!(l.hit(-5.0, y), 0);
        // Hit and caret_pos agree.
        for i in 0..=3 {
            let (x, b, _) = l.caret_pos(i);
            assert_eq!(l.hit(x + 0.01, b - 2.0), i);
        }
    }

    #[test]
    fn click_to_caret_comb_and_multiline() {
        let rect = Rect::new(0.0, 0.0, 100.0, 14.0);
        let f = text_field(rect, TextMode::Comb(Field::equal_cells(&rect, 10)));
        let l = layout(&f, "ABC");
        assert_eq!(l.hit(25.0, 7.0), 2); // the third cell
        assert_eq!(l.hit(75.0, 7.0), 3); // past the text: clamp to the end
        let m = text_field(Rect::new(0.0, 0.0, 300.0, 200.0), TextMode::Multi);
        let l = layout(&m, "first\nsecond line");
        let y2 = l.lines[1].baseline - 2.0;
        assert_eq!(l.hit(0.0, y2), 6);
        assert_eq!(l.hit(299.0, y2), 17);
        assert_eq!(l.hit(299.0, l.lines[0].baseline - 2.0), 5);
    }
}
