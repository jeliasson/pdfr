//! Drawing field contents with egui from the shared layout, so on-screen text
//! is placed exactly where the PDF writer will put it.

use eframe::egui::{self, Color32, FontFamily, FontId, Painter, Pos2, Rect, Stroke, pos2, vec2};
use pdfr_core::field::{Field, FieldKind};
use pdfr_core::layout::Layout;

pub const FONT_FAMILY: &str = "pdfr";

/// Maps display-space points of a page to screen points.
#[derive(Clone, Copy, Debug)]
pub struct PageXf {
    pub origin: Pos2,
    pub zoom: f32,
}

impl PageXf {
    pub fn pt(&self, x: f32, y: f32) -> Pos2 {
        pos2(self.origin.x + x * self.zoom, self.origin.y + y * self.zoom)
    }
    pub fn rect(&self, r: &pdfr_core::geom::Rect) -> Rect {
        Rect::from_min_max(self.pt(r.x0, r.y0), self.pt(r.x1, r.y1))
    }
    pub fn to_page(self, p: Pos2) -> (f32, f32) {
        (
            (p.x - self.origin.x) / self.zoom,
            (p.y - self.origin.y) / self.zoom,
        )
    }
}

/// Registers the built-in font as an egui font family.
pub fn install_font(ctx: &egui::Context) {
    let mut defs = egui::FontDefinitions::default();
    defs.font_data.insert(
        FONT_FAMILY.into(),
        std::sync::Arc::new(egui::FontData::from_static(pdfr_core::font::FONT_BYTES)),
    );
    defs.families.insert(
        FontFamily::Name(FONT_FAMILY.into()),
        vec![FONT_FAMILY.into()],
    );
    ctx.set_fonts(defs);
}

/// Draws one glyph with its pen position at `pen` (baseline).
fn glyph(painter: &Painter, ch: char, size: f32, family: FontFamily, pen: Pos2, color: Color32) {
    if ch == ' ' {
        return;
    }
    let galley = painter.layout_no_wrap(ch.to_string(), FontId::new(size, family), color);
    // Read the baseline from the galley's first glyph; do not assume the row top.
    let Some(row) = galley.rows.first() else {
        return;
    };
    let Some(g) = row.glyphs.first() else { return };
    let base = row.pos + g.pos.to_vec2();
    painter.galley(pen - base.to_vec2(), galley, color);
}

/// Draws laid-out text.
pub fn text(painter: &Painter, xf: &PageXf, l: &Layout, color: Color32) {
    let size = l.size * xf.zoom;
    let family = FontFamily::Name(FONT_FAMILY.into());
    for line in &l.lines {
        for g in &line.glyphs {
            glyph(
                painter,
                g.ch,
                size,
                family.clone(),
                xf.pt(g.x, line.baseline),
                color,
            );
        }
    }
}

pub fn caret(painter: &Painter, xf: &PageXf, l: &Layout, idx: usize, color: Color32) -> Rect {
    let (x, base, _) = l.caret_pos(idx.min(l.len));
    let f = pdfr_core::font::font();
    let top = xf.pt(x, base - l.size * f.ascent());
    let bot = xf.pt(x, base + l.size * f.descent());
    painter.line_segment([top, bot], Stroke::new(1.5, color));
    Rect::from_min_max(top - vec2(1.0, 0.0), bot + vec2(1.0, 0.0))
}

pub fn selection(painter: &Painter, xf: &PageXf, l: &Layout, a: usize, b: usize, color: Color32) {
    for r in l.selection_rects(a, b) {
        painter.rect_filled(xf.rect(&r), 0.0, color);
    }
}

/// The same cross the PDF writer draws (`save::check_paths`).
pub fn check(painter: &Painter, xf: &PageXf, f: &Field, color: Color32) {
    use pdfr_core::save::{CROSS_INSET, CROSS_WIDTH};
    let r = f.rect;
    let (w, h) = (r.width(), r.height());
    let m = w.min(h);
    let ox = r.x0 + (w - m) / 2.0;
    let oy = r.y1 - (h - m) / 2.0; // y-up form space -> display
    let p = |u: f32, v: f32| xf.pt(ox + u * m, oy - v * m);
    let stroke = Stroke::new(m * CROSS_WIDTH * xf.zoom, color);
    let (a, b) = (CROSS_INSET, 1.0 - CROSS_INSET);
    painter.line_segment([p(a, a), p(b, b)], stroke);
    painter.line_segment([p(a, b), p(b, a)], stroke);
}

pub fn radio_dot(painter: &Painter, xf: &PageXf, f: &Field, color: Color32) {
    let (cx, cy) = f.rect.center();
    let r = f.rect.width().min(f.rect.height()) * 0.25 * xf.zoom;
    painter.circle_filled(xf.pt(cx, cy), r, color);
}

/// Small drop-down indicator drawn on screen only.
pub fn dropdown_marker(painter: &Painter, xf: &PageXf, f: &Field, color: Color32) {
    if !matches!(f.kind, FieldKind::Choice { .. }) {
        return;
    }
    let r = xf.rect(&f.rect);
    let s = (r.height() * 0.25).min(6.0);
    let c = pos2(r.right() - s * 1.6, r.center().y);
    painter.add(egui::Shape::convex_polygon(
        vec![
            c + vec2(-s, -s * 0.5),
            c + vec2(s, -s * 0.5),
            c + vec2(0.0, s * 0.6),
        ],
        color,
        Stroke::NONE,
    ));
}
