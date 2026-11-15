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
}

impl TextRun {
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
    r.text = text;
    r.xs = xs;
    r
}
