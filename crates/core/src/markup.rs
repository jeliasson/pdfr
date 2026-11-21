//! Comments and markup: sticky notes, highlights, underlines,
//! strike-throughs, text boxes and shapes. Read from and written as
//! standard PDF annotations with appearance streams (`/AP`), so other PDF
//! apps show them as pdfr does. Kinds pdfr does not edit stay untouched.
//!
//! Geometry is kept in PDF user space, so markup stays on the paper when
//! a page is turned; the `display_*` helpers map it to the screen.

use crate::field::{Field, ObjId, Origin, TextMode};
use crate::forms::{F_HIDDEN, F_PRINT, da_font_size, deref, get_text};
use crate::geom::{PageGeom, Rect};
use crate::layout::layout;
use crate::pdf_font::{ensure_font, num};
use crate::save::{FONT_RES, rotation_matrix};
use lopdf::{Dictionary, Document, Object, Stream, StringFormat, dictionary, text_string};
use std::collections::HashSet;
use std::fmt::Write as _;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MarkKind {
    /// A sticky note (`/Text`).
    Note,
    Highlight,
    Underline,
    StrikeOut,
    /// A text box anywhere on the page (`/FreeText`).
    FreeText,
    /// A rectangle (`/Square`).
    Square,
    /// An ellipse (`/Circle`).
    Circle,
    Line,
    /// A line with an open arrowhead at its end.
    Arrow,
}

impl MarkKind {
    pub fn subtype(self) -> &'static str {
        match self {
            MarkKind::Note => "Text",
            MarkKind::Highlight => "Highlight",
            MarkKind::Underline => "Underline",
            MarkKind::StrikeOut => "StrikeOut",
            MarkKind::FreeText => "FreeText",
            MarkKind::Square => "Square",
            MarkKind::Circle => "Circle",
            MarkKind::Line | MarkKind::Arrow => "Line",
        }
    }

    /// Marks over the page's text.
    pub fn is_text_markup(self) -> bool {
        matches!(
            self,
            MarkKind::Highlight | MarkKind::Underline | MarkKind::StrikeOut
        )
    }

    pub fn is_line(self) -> bool {
        matches!(self, MarkKind::Line | MarkKind::Arrow)
    }

    /// Kinds with a box that can be resized by its corner.
    pub fn resizable(self) -> bool {
        matches!(
            self,
            MarkKind::FreeText | MarkKind::Square | MarkKind::Circle
        )
    }

    /// What people call it.
    pub fn label(self) -> &'static str {
        match self {
            MarkKind::Note => "Note",
            MarkKind::Highlight => "Highlight",
            MarkKind::Underline => "Underline",
            MarkKind::StrikeOut => "Strike-through",
            MarkKind::FreeText => "Text box",
            MarkKind::Square => "Rectangle",
            MarkKind::Circle => "Ellipse",
            MarkKind::Line => "Line",
            MarkKind::Arrow => "Arrow",
        }
    }
}

/// One comment or mark on a page.
#[derive(Clone, Debug, PartialEq)]
pub struct Markup {
    pub page: usize,
    pub kind: MarkKind,
    /// `[llx, lly, urx, ury]` in user space.
    pub rect: [f32; 4],
    /// Text marks: one quad per line, user space, in the order other apps
    /// use: upper-left, upper-right, lower-left, lower-right (x, y each).
    pub quads: Vec<[f32; 8]>,
    /// Lines and arrows: start and end `[x1, y1, x2, y2]`, user space.
    pub line: [f32; 4],
    pub color: [u8; 3],
    /// The comment (`/Contents`); a text box's text.
    pub text: String,
    /// Who wrote it (`/T`).
    pub author: String,
    /// A text box's font size; a shape's line width.
    pub size: f32,
    /// The annotation it was read from (`None`: new).
    pub source: Option<ObjId>,
}

/// A note's icon, in points (it is square).
pub const NOTE_SIZE: f32 = 20.0;
/// The note icon, in display space of the icon (y down): the bubble, its
/// tail and the lines of writing. The screen and the saved file share it.
pub const NOTE_BODY: [f32; 4] = [1.5, 1.5, 18.5, 14.5];
pub const NOTE_TAIL: [(f32, f32); 3] = [(5.0, 14.0), (5.0, 19.0), (10.5, 14.0)];
pub const NOTE_LINES: [f32; 3] = [5.0, 8.0, 11.0];
pub const NOTE_LINE_X: (f32, f32) = (5.0, 15.0);
/// Default text box font size and shape line width.
pub const TEXT_SIZE: f32 = 12.0;
pub const SHAPE_WIDTH: f32 = 2.0;
/// Underline thickness and strike-through height, as fractions of a line.
pub const UNDERLINE_AT: f32 = 0.08;
pub const STRIKE_AT: f32 = 0.42;
/// Arrowhead: length (times the line width, at least `ARROW_MIN`) and
/// half-angle.
pub const ARROW_LEN: f32 = 4.0;
pub const ARROW_MIN: f32 = 7.0;
pub const ARROW_ANGLE: f32 = 0.5;

impl Markup {
    fn blank(page: usize, kind: MarkKind, color: [u8; 3]) -> Markup {
        Markup {
            page,
            kind,
            rect: [0.0; 4],
            quads: vec![],
            line: [0.0; 4],
            color,
            text: String::new(),
            author: String::new(),
            size: match kind {
                MarkKind::FreeText => TEXT_SIZE,
                _ => SHAPE_WIDTH,
            },
            source: None,
        }
    }

    /// A note with its icon's top-left corner at display point `(x, y)`.
    pub fn note(page: usize, g: &PageGeom, x: f32, y: f32, color: [u8; 3]) -> Markup {
        let mut m = Markup::blank(page, MarkKind::Note, color);
        m.rect = g.rect_to_user(&Rect::new(x, y, x + NOTE_SIZE, y + NOTE_SIZE));
        m
    }

    /// A highlight, underline or strike-through over display rects (one
    /// per line of text).
    pub fn over_text(
        kind: MarkKind,
        page: usize,
        g: &PageGeom,
        rects: &[Rect],
        color: [u8; 3],
    ) -> Markup {
        let mut m = Markup::blank(page, kind, color);
        m.quads = rects
            .iter()
            .map(|r| {
                let ul = g.to_user(r.x0, r.y0);
                let ur = g.to_user(r.x1, r.y0);
                let ll = g.to_user(r.x0, r.y1);
                let lr = g.to_user(r.x1, r.y1);
                [ul.0, ul.1, ur.0, ur.1, ll.0, ll.1, lr.0, lr.1]
            })
            .collect();
        m.fit_rect();
        m
    }

    /// A text box over a display rect.
    pub fn text_box(page: usize, g: &PageGeom, r: Rect, color: [u8; 3]) -> Markup {
        let mut m = Markup::blank(page, MarkKind::FreeText, color);
        m.rect = g.rect_to_user(&r);
        m
    }

    /// A rectangle or ellipse filling the display rect `r`, or a line or
    /// arrow from its top-left `a` to `b`.
    pub fn shape(
        kind: MarkKind,
        page: usize,
        g: &PageGeom,
        a: (f32, f32),
        b: (f32, f32),
        color: [u8; 3],
    ) -> Markup {
        let mut m = Markup::blank(page, kind, color);
        if kind.is_line() {
            let (p, q) = (g.to_user(a.0, a.1), g.to_user(b.0, b.1));
            m.line = [p.0, p.1, q.0, q.1];
            m.fit_rect();
        } else {
            m.rect = g.rect_to_user(&Rect::from_points(a, b));
        }
        m
    }

    /// Sets `rect` around the quads or the line (with room for the stroke
    /// and arrowhead).
    pub fn fit_rect(&mut self) {
        let pts: Vec<(f32, f32)> = if self.kind.is_line() {
            vec![(self.line[0], self.line[1]), (self.line[2], self.line[3])]
        } else if self.kind.is_text_markup() {
            self.quads
                .iter()
                .flat_map(|q| (0..4).map(move |i| (q[2 * i], q[2 * i + 1])))
                .collect()
        } else {
            return;
        };
        if pts.is_empty() {
            return;
        }
        let pad = if self.kind.is_line() {
            self.arrow_len() + self.size
        } else {
            1.0
        };
        let x0 = pts.iter().map(|p| p.0).fold(f32::MAX, f32::min);
        let y0 = pts.iter().map(|p| p.1).fold(f32::MAX, f32::min);
        let x1 = pts.iter().map(|p| p.0).fold(f32::MIN, f32::max);
        let y1 = pts.iter().map(|p| p.1).fold(f32::MIN, f32::max);
        self.rect = [x0 - pad, y0 - pad, x1 + pad, y1 + pad];
    }

    pub fn arrow_len(&self) -> f32 {
        (self.size * ARROW_LEN).max(ARROW_MIN)
    }

    pub fn display_rect(&self, g: &PageGeom) -> Rect {
        g.rect_to_display(self.rect)
    }

    /// The text marks' quads on screen: upper-left, upper-right,
    /// lower-left, lower-right.
    pub fn display_quads(&self, g: &PageGeom) -> Vec<[(f32, f32); 4]> {
        self.quads
            .iter()
            .map(|q| std::array::from_fn(|i| g.to_display(q[2 * i], q[2 * i + 1])))
            .collect()
    }

    /// A line's ends on screen.
    pub fn display_line(&self, g: &PageGeom) -> ((f32, f32), (f32, f32)) {
        (
            g.to_display(self.line[0], self.line[1]),
            g.to_display(self.line[2], self.line[3]),
        )
    }

    /// The same mark moved by `(dx, dy)` display points.
    pub fn moved(&self, g: &PageGeom, dx: f32, dy: f32) -> Markup {
        let mv = |x: f32, y: f32| {
            let (a, b) = g.to_display(x, y);
            g.to_user(a + dx, b + dy)
        };
        let mut m = self.clone();
        let r = self.display_rect(g);
        m.rect = g.rect_to_user(&Rect::new(r.x0 + dx, r.y0 + dy, r.x1 + dx, r.y1 + dy));
        for q in &mut m.quads {
            for i in 0..4 {
                let (x, y) = mv(q[2 * i], q[2 * i + 1]);
                q[2 * i] = x;
                q[2 * i + 1] = y;
            }
        }
        let (a, b) = (mv(m.line[0], m.line[1]), mv(m.line[2], m.line[3]));
        m.line = [a.0, a.1, b.0, b.1];
        m
    }

    /// Gives a box-like mark the display rect `r`.
    pub fn set_display_rect(&mut self, g: &PageGeom, r: Rect) {
        self.rect = g.rect_to_user(&r);
    }

    /// Moves one end of a line (`end`: the arrow's end) to a display point.
    pub fn set_line_end(&mut self, g: &PageGeom, end: bool, p: (f32, f32)) {
        let (x, y) = g.to_user(p.0, p.1);
        let i = if end { 2 } else { 0 };
        self.line[i] = x;
        self.line[i + 1] = y;
        self.fit_rect();
    }

    /// The text box as a field, for the shared text layout.
    pub fn text_field(&self, g: &PageGeom) -> Field {
        let mut f = Field::new_text(
            self.page,
            self.display_rect(g),
            TextMode::Multi,
            Origin::Manual,
        );
        f.font_size = Some(self.size);
        f.value = self.text.clone();
        f
    }

    /// The height a text box needs for its text at its width (display pt).
    pub fn text_height(&self, g: &PageGeom) -> f32 {
        let f = self.text_field(g);
        let l = layout(&f, &self.text);
        let lines = l.lines.len().max(1) as f32;
        lines * l.line_height + 2.0 * crate::layout::PAD + self.size * 0.3
    }
}

// --- Writing -------------------------------------------------------------------

fn rgb(c: [u8; 3]) -> [f32; 3] {
    c.map(|v| v as f32 / 255.0)
}

fn rgb_ops(c: [u8; 3], op: &str) -> String {
    let [r, g, b] = rgb(c);
    format!("{} {} {} {op}", num(r), num(g), num(b))
}

fn reals(v: &[f32]) -> Object {
    Object::Array(v.iter().map(|x| Object::Real(*x)).collect())
}

/// An ellipse path inside `(x0, y0)`–`(x1, y1)`, from Bézier quarters.
fn ellipse_path(x0: f32, y0: f32, x1: f32, y1: f32) -> String {
    const K: f32 = 0.552_284_8;
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let (rx, ry) = ((x1 - x0) / 2.0, (y1 - y0) / 2.0);
    let (kx, ky) = (rx * K, ry * K);
    let p = |x: f32, y: f32| format!("{} {}", num(x), num(y));
    format!(
        "{} m\n{} {} {} c\n{} {} {} c\n{} {} {} c\n{} {} {} c\n",
        p(cx + rx, cy),
        p(cx + rx, cy + ky),
        p(cx + kx, cy + ry),
        p(cx, cy + ry),
        p(cx - kx, cy + ry),
        p(cx - rx, cy + ky),
        p(cx - rx, cy),
        p(cx - rx, cy - ky),
        p(cx - kx, cy - ry),
        p(cx, cy - ry),
        p(cx + kx, cy - ry),
        p(cx + rx, cy - ky),
        p(cx + rx, cy),
    )
}

/// The arrowhead's two barbs at `(x2, y2)` for a line from `(x1, y1)`.
pub fn arrow_barbs(x1: f32, y1: f32, x2: f32, y2: f32, len: f32) -> [(f32, f32); 2] {
    let a = (y1 - y2).atan2(x1 - x2);
    [a - ARROW_ANGLE, a + ARROW_ANGLE].map(|t| (x2 + len * t.cos(), y2 + len * t.sin()))
}

/// Interpolates from `a` to `b`.
fn lerp(a: (f32, f32), b: (f32, f32), t: f32) -> (f32, f32) {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

/// The ends of an underline or strike-through over quad `q` (upper-left,
/// upper-right, lower-left, lower-right) at height `t` of the line, and
/// its thickness. Works in any space.
pub fn text_line(q: [(f32, f32); 4], t: f32) -> ((f32, f32), (f32, f32), f32) {
    let a = lerp(q[2], q[0], t);
    let b = lerp(q[3], q[1], t);
    let w = (dist(q[0], q[2]) * 0.07).max(0.6);
    (a, b, w)
}

/// A form XObject with the given box, content and resources.
fn form(
    d: &mut Document,
    bbox: [f32; 4],
    m: Option<[f32; 6]>,
    res: Dictionary,
    c: String,
) -> ObjId {
    let mut dict = dictionary! {
        "Type" => "XObject",
        "Subtype" => "Form",
        "BBox" => reals(&bbox),
        "Resources" => res,
    };
    if let Some(m) = m {
        dict.set("Matrix", reals(&m));
    }
    d.add_object(Stream::new(dict, c.into_bytes()))
}

/// The appearance of a mark drawn in user space (BBox = Rect).
fn user_space_ap(d: &mut Document, m: &Markup) -> ObjId {
    let mut s = String::new();
    let mut res = Dictionary::new();
    let stroke = rgb_ops(m.color, "RG");
    let pt = |p: (f32, f32)| format!("{} {}", num(p.0), num(p.1));
    let quads = || {
        m.quads
            .iter()
            .map(|q| -> [(f32, f32); 4] { std::array::from_fn(|i| (q[2 * i], q[2 * i + 1])) })
    };
    match m.kind {
        MarkKind::Highlight => {
            // Multiply, so the text under it stays dark.
            res.set(
                "ExtGState",
                dictionary! { "PdfrMul" => dictionary! { "Type" => "ExtGState", "BM" => "Multiply" } },
            );
            let _ = writeln!(s, "/PdfrMul gs {}", rgb_ops(m.color, "rg"));
            for q in quads() {
                let _ = writeln!(
                    s,
                    "{} m {} l {} l {} l h f",
                    pt(q[0]),
                    pt(q[1]),
                    pt(q[3]),
                    pt(q[2])
                );
            }
        }
        MarkKind::Underline | MarkKind::StrikeOut => {
            let t = if m.kind == MarkKind::Underline {
                UNDERLINE_AT
            } else {
                STRIKE_AT
            };
            for q in quads() {
                let (a, b, w) = text_line(q, t);
                let _ = writeln!(s, "{stroke} {} w {} m {} l S", num(w), pt(a), pt(b));
            }
        }
        MarkKind::Square | MarkKind::Circle => {
            let w = m.size;
            let r = m.rect;
            let (x0, y0, x1, y1) = (
                r[0] + w / 2.0,
                r[1] + w / 2.0,
                r[2] - w / 2.0,
                r[3] - w / 2.0,
            );
            let _ = write!(s, "{stroke} {} w ", num(w));
            if m.kind == MarkKind::Square {
                let _ = writeln!(
                    s,
                    "{} {} {} {} re S",
                    num(x0),
                    num(y0),
                    num(x1 - x0),
                    num(y1 - y0)
                );
            } else {
                s.push('\n');
                s.push_str(&ellipse_path(x0, y0, x1, y1));
                s.push_str("S\n");
            }
        }
        MarkKind::Line | MarkKind::Arrow => {
            let [x1, y1, x2, y2] = m.line;
            let _ = writeln!(
                s,
                "{stroke} {} w 1 J 1 j {} m {} l S",
                num(m.size),
                pt((x1, y1)),
                pt((x2, y2))
            );
            if m.kind == MarkKind::Arrow {
                let [a, b] = arrow_barbs(x1, y1, x2, y2, m.arrow_len());
                let _ = writeln!(s, "{} m {} l {} l S", pt(a), pt((x2, y2)), pt(b));
            }
        }
        MarkKind::Note | MarkKind::FreeText => {}
    }
    form(d, m.rect, None, res, s)
}

/// The appearance of a note or text box: drawn upright on screen, in a
/// form space the size of its display rect (origin bottom-left).
fn upright_ap(d: &mut Document, m: &Markup, g: &PageGeom) -> ObjId {
    let r = m.display_rect(g);
    let (w, h) = (r.width(), r.height());
    let mut s = String::new();
    let mut res = Dictionary::new();
    if m.kind == MarkKind::Note {
        // Icon space is display space of the icon: flip y.
        let sx = w / NOTE_SIZE;
        let sy = h / NOTE_SIZE;
        let p = |x: f32, y: f32| format!("{} {}", num(x * sx), num(h - y * sy));
        let dark = m.color.map(|v| (v as f32 * 0.55) as u8);
        let b = NOTE_BODY;
        let _ = writeln!(
            s,
            "{} {} 1 w 1 j\n{} m {} l {} l {} l {} l {} l {} l h B",
            rgb_ops(m.color, "rg"),
            rgb_ops(dark, "RG"),
            p(b[0], b[1]),
            p(b[2], b[1]),
            p(b[2], b[3]),
            p(NOTE_TAIL[2].0, NOTE_TAIL[2].1),
            p(NOTE_TAIL[1].0, NOTE_TAIL[1].1),
            p(NOTE_TAIL[0].0, NOTE_TAIL[0].1),
            p(b[0], b[3]),
        );
        let _ = writeln!(s, "{} 1.2 w 1 J", rgb_ops(dark, "RG"));
        for y in NOTE_LINES {
            let _ = writeln!(s, "{} m {} l S", p(NOTE_LINE_X.0, y), p(NOTE_LINE_X.1, y));
        }
    } else {
        let font = ensure_font(d);
        res.set("Font", dictionary! { FONT_RES => Object::Reference(font) });
        let f = m.text_field(g);
        let l = layout(&f, &m.text);
        if !l.lines.is_empty() {
            let _ = writeln!(
                s,
                "BT\n{}\n/{FONT_RES} {} Tf",
                rgb_ops(m.color, "rg"),
                num(l.size)
            );
            for line in &l.lines {
                let Some(first) = line.glyphs.first() else {
                    continue;
                };
                let hex: String = line
                    .glyphs
                    .iter()
                    .map(|g| format!("{:04X}", g.gid))
                    .collect();
                let _ = writeln!(
                    s,
                    "1 0 0 1 {} {} Tm <{hex}> Tj",
                    num(first.x - r.x0),
                    num(r.y1 - line.baseline)
                );
            }
            s.push_str("ET\n");
        }
    }
    form(d, [0.0, 0.0, w, h], rotation_matrix(g.rotate), res, s)
}

fn appearance(d: &mut Document, m: &Markup, g: &PageGeom) -> ObjId {
    match m.kind {
        MarkKind::Note | MarkKind::FreeText => upright_ap(d, m, g),
        _ => user_space_ap(d, m),
    }
}

/// Sets the keys pdfr owns on an annotation dictionary.
fn fill_dict(a: &mut Dictionary, m: &Markup, ap: ObjId, text_changed: bool) {
    a.set("Type", "Annot");
    a.set("Subtype", m.kind.subtype());
    a.set("Rect", reals(&m.rect));
    a.set("AP", dictionary! { "N" => Object::Reference(ap) });
    a.set("Contents", text_string(&m.text));
    if text_changed {
        // Rich text would be shown instead of the new plain text.
        a.remove(b"RC");
    }
    if m.author.is_empty() {
        a.remove(b"T");
    } else {
        a.set("T", text_string(&m.author));
    }
    let c = reals(&rgb(m.color));
    match m.kind {
        MarkKind::Note => {
            a.set("C", c);
            if !a.has(b"Name") {
                a.set("Name", "Comment");
            }
        }
        MarkKind::Highlight | MarkKind::Underline | MarkKind::StrikeOut => {
            a.set("C", c);
            let q: Vec<f32> = m.quads.iter().flatten().copied().collect();
            a.set("QuadPoints", reals(&q));
        }
        MarkKind::FreeText => {
            a.set(
                "DA",
                Object::String(
                    format!("/Helv {} Tf {}", num(m.size), rgb_ops(m.color, "rg")).into_bytes(),
                    StringFormat::Literal,
                ),
            );
            a.remove(b"DS");
            a.set("BS", dictionary! { "W" => 0 });
        }
        MarkKind::Square | MarkKind::Circle => {
            a.set("C", c);
            a.set("BS", dictionary! { "W" => Object::Real(m.size) });
        }
        MarkKind::Line | MarkKind::Arrow => {
            a.set("C", c);
            a.set("L", reals(&m.line));
            a.set("BS", dictionary! { "W" => Object::Real(m.size) });
            let end = if m.kind == MarkKind::Arrow {
                "OpenArrow"
            } else {
                "None"
            };
            a.set(
                "LE",
                vec![Object::Name(b"None".to_vec()), Object::Name(end.into())],
            );
        }
    }
}

/// The page's `/Annots` array, mutably (it may be a reference).
fn with_annots(d: &mut Document, page_id: ObjId, f: impl FnOnce(&mut Vec<Object>)) {
    let annots_ref = d
        .get_dictionary(page_id)
        .ok()
        .and_then(|p| p.get(b"Annots").ok())
        .and_then(|o| o.as_reference().ok());
    if let Some(aid) = annots_ref {
        if let Ok(Object::Array(a)) = d.get_object_mut(aid) {
            f(a);
        }
    } else if let Ok(p) = d.get_dictionary_mut(page_id) {
        if !matches!(p.get(b"Annots"), Ok(Object::Array(_))) {
            p.set("Annots", Vec::<Object>::new());
        }
        if let Ok(Object::Array(a)) = p.get_mut(b"Annots") {
            f(a);
        }
    }
}

/// Removes annotations from a page's `/Annots`.
pub(crate) fn unlink(d: &mut Document, page_id: ObjId, ids: &HashSet<ObjId>) {
    with_annots(d, page_id, |a| {
        a.retain(|o| !o.as_reference().is_ok_and(|r| ids.contains(&r)))
    });
}

/// Writes `current` marks into `d`, given the marks the file had
/// (`original`): unchanged ones stay exactly as they were, changed ones
/// get new values and appearances (keeping their other keys), removed
/// ones go (with their pop-ups), new ones are added.
pub(crate) fn write(
    d: &mut Document,
    page_ids: &[ObjId],
    geoms: &[PageGeom],
    original: &[Markup],
    current: &[Markup],
) {
    let kept: HashSet<ObjId> = current.iter().filter_map(|m| m.source).collect();
    for o in original {
        let Some(id) = o.source.filter(|id| !kept.contains(id)) else {
            continue;
        };
        let mut gone = HashSet::from([id]);
        if let Some(p) = d
            .get_dictionary(id)
            .ok()
            .and_then(|a| a.get(b"Popup").ok())
            .and_then(|o| o.as_reference().ok())
        {
            gone.insert(p);
        }
        if let Some(&pid) = page_ids.get(o.page) {
            unlink(d, pid, &gone);
        }
        for g in gone {
            d.objects.remove(&g);
        }
    }
    for m in current {
        let (Some(&pid), Some(g)) = (page_ids.get(m.page), geoms.get(m.page)) else {
            continue;
        };
        let before = m
            .source
            .and_then(|id| original.iter().find(|o| o.source == Some(id)));
        if before == Some(m) {
            continue;
        }
        let ap = appearance(d, m, g);
        match m.source {
            Some(id) => {
                let text_changed = before.is_none_or(|b| b.text != m.text);
                if let Ok(a) = d.get_dictionary_mut(id) {
                    fill_dict(a, m, ap, text_changed);
                }
            }
            None => {
                let mut a = dictionary! {
                    "P" => Object::Reference(pid),
                    "F" => if m.kind == MarkKind::Note { 0 } else { F_PRINT },
                };
                fill_dict(&mut a, m, ap, true);
                let id = d.add_object(a);
                with_annots(d, pid, |v| v.push(Object::Reference(id)));
            }
        }
    }
}

// --- Reading -------------------------------------------------------------------

fn floats(d: &Document, o: Option<&Object>) -> Option<Vec<f32>> {
    let a = deref(d, o?)?.as_array().ok()?;
    a.iter().map(|o| deref(d, o)?.as_float().ok()).collect()
}

fn name(d: &Document, dict: &Dictionary, key: &[u8]) -> Option<Vec<u8>> {
    Some(deref(d, dict.get(key).ok()?)?.as_name().ok()?.to_vec())
}

/// A colour array (grey, RGB or CMYK) as RGB.
fn color_of(v: &[f32]) -> Option<[u8; 3]> {
    let c = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    match v.len() {
        1 => Some([c(v[0]); 3]),
        3 => Some([c(v[0]), c(v[1]), c(v[2])]),
        4 => {
            let k = 1.0 - v[3];
            Some([
                c((1.0 - v[0]) * k),
                c((1.0 - v[1]) * k),
                c((1.0 - v[2]) * k),
            ])
        }
        _ => None,
    }
}

/// The text colour in a `/DA` string (`… 1 0 0 rg` or `… 0 g`).
fn da_color(da: &str) -> Option<[u8; 3]> {
    let t: Vec<&str> = da.split_whitespace().collect();
    let i = t.iter().rposition(|x| matches!(*x, "rg" | "g" | "k"))?;
    let n = match t[i] {
        "rg" => 3,
        "g" => 1,
        _ => 4,
    };
    let v: Vec<f32> = t
        .get(i.checked_sub(n)?..i)?
        .iter()
        .map(|x| x.parse().ok())
        .collect::<Option<_>>()?;
    color_of(&v)
}

/// The border width (`/BS /W`, else `/Border`), or `None` for borders
/// pdfr does not draw (dashed, cloudy).
fn border_width(d: &Document, a: &Dictionary) -> Option<f32> {
    if a.has(b"BE") {
        return None;
    }
    if let Some(bs) = a
        .get(b"BS")
        .ok()
        .and_then(|o| deref(d, o))
        .and_then(|o| o.as_dict().ok())
    {
        if name(d, bs, b"S").is_some_and(|s| s != b"S") {
            return None;
        }
        return Some(
            bs.get(b"W")
                .ok()
                .and_then(|o| deref(d, o))
                .and_then(|o| o.as_float().ok())
                .unwrap_or(1.0),
        );
    }
    Some(
        floats(d, a.get(b"Border").ok())
            .and_then(|b| b.get(2).copied())
            .unwrap_or(1.0),
    )
}

/// One annotation as a mark pdfr can edit, or `None` to leave it alone.
fn read_one(d: &Document, id: ObjId, page: usize) -> Option<Markup> {
    let a = d.get_dictionary(id).ok()?;
    // Replies, hidden marks and pdfr's signatures are someone else's.
    let flags = a
        .get(b"F")
        .ok()
        .and_then(|o| deref(d, o))
        .and_then(|o| o.as_i64().ok());
    if a.has(b"IRT") || flags.unwrap_or(0) & F_HIDDEN != 0 || a.has(crate::signature::INK_KEY) {
        return None;
    }
    let rect = floats(d, a.get(b"Rect").ok()).filter(|r| r.len() == 4)?;
    let rect = [
        rect[0].min(rect[2]),
        rect[1].min(rect[3]),
        rect[0].max(rect[2]),
        rect[1].max(rect[3]),
    ];
    let color = floats(d, a.get(b"C").ok()).and_then(|c| color_of(&c));
    let kind = match name(d, a, b"Subtype")?.as_slice() {
        b"Text" => MarkKind::Note,
        b"Highlight" => MarkKind::Highlight,
        b"Underline" => MarkKind::Underline,
        b"StrikeOut" => MarkKind::StrikeOut,
        b"FreeText" => MarkKind::FreeText,
        b"Square" => MarkKind::Square,
        b"Circle" => MarkKind::Circle,
        b"Line" => MarkKind::Line,
        _ => return None,
    };
    let mut m = Markup::blank(page, kind, color.unwrap_or([255, 212, 0]));
    m.rect = rect;
    m.source = Some(id);
    m.text = get_text(d, a, b"Contents").unwrap_or_default();
    m.author = get_text(d, a, b"T").unwrap_or_default();
    match kind {
        MarkKind::Note => {}
        MarkKind::Highlight | MarkKind::Underline | MarkKind::StrikeOut => {
            let q = floats(d, a.get(b"QuadPoints").ok()).unwrap_or_default();
            m.quads = q.as_chunks::<8>().0.to_vec();
            if m.quads.is_empty() {
                let [x0, y0, x1, y1] = rect;
                m.quads = vec![[x0, y1, x1, y1, x0, y0, x1, y0]];
            }
        }
        MarkKind::FreeText => {
            // Callouts and typewriter variants keep their own looks.
            if a.has(b"CL") || name(d, a, b"IT").is_some_and(|n| n != b"FreeText") {
                return None;
            }
            let da = get_text(d, a, b"DA").unwrap_or_default();
            m.size = da_font_size(&da).filter(|s| *s > 0.0).unwrap_or(TEXT_SIZE);
            m.color = da_color(&da).unwrap_or([0, 0, 0]);
        }
        MarkKind::Square | MarkKind::Circle => {
            // Filled shapes keep their own looks.
            if floats(d, a.get(b"IC").ok()).is_some_and(|c| !c.is_empty()) {
                return None;
            }
            m.size = border_width(d, a)?;
            color?;
        }
        MarkKind::Line | MarkKind::Arrow => {
            let l = floats(d, a.get(b"L").ok()).filter(|l| l.len() == 4)?;
            m.line = [l[0], l[1], l[2], l[3]];
            m.size = border_width(d, a)?;
            if a.has(b"LL") || a.has(b"IC") {
                return None;
            }
            let le: Vec<Vec<u8>> = a
                .get(b"LE")
                .ok()
                .and_then(|o| deref(d, o))
                .and_then(|o| o.as_array().ok())
                .map(|v| {
                    v.iter()
                        .filter_map(|o| o.as_name().ok().map(<[u8]>::to_vec))
                        .collect()
                })
                .unwrap_or_default();
            let start = le.first().map_or(&b"None"[..], |v| v.as_slice());
            let end = le.get(1).map_or(&b"None"[..], |v| v.as_slice());
            if start != b"None" {
                return None;
            }
            m.kind = match end {
                b"None" => MarkKind::Line,
                b"OpenArrow" | b"ClosedArrow" => MarkKind::Arrow,
                _ => return None,
            };
            color?;
        }
    }
    Some(m)
}

/// The marks on every page that pdfr can show and edit.
pub(crate) fn read(d: &Document, page_ids: &[ObjId]) -> Vec<Markup> {
    let mut out = Vec::new();
    for (pi, &pid) in page_ids.iter().enumerate() {
        let Some(Object::Array(annots)) = d
            .get_dictionary(pid)
            .ok()
            .and_then(|p| p.get(b"Annots").ok())
            .and_then(|o| deref(d, o))
        else {
            continue;
        };
        for a in annots {
            if let Ok(id) = a.as_reference()
                && let Some(m) = read_one(d, id, pi)
            {
                out.push(m);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn da_colours_and_cmyk() {
        assert_eq!(da_color("/Helv 12 Tf 1 0 0 rg"), Some([255, 0, 0]));
        assert_eq!(da_color("0.5 g /Helv 9 Tf"), Some([128; 3]));
        assert_eq!(color_of(&[0.0, 0.0, 0.0, 1.0]), Some([0, 0, 0]));
    }

    #[test]
    fn moving_keeps_shape_on_turned_pages() {
        let g = PageGeom::new([0.0, 0.0, 600.0, 800.0], 90);
        let m = Markup::over_text(
            MarkKind::Underline,
            0,
            &g,
            &[Rect::new(10.0, 20.0, 110.0, 32.0)],
            [0, 0, 255],
        );
        let q = m.display_quads(&g)[0];
        assert!((q[0].0 - 10.0).abs() < 0.01 && (q[0].1 - 20.0).abs() < 0.01);
        assert!((q[3].0 - 110.0).abs() < 0.01 && (q[3].1 - 32.0).abs() < 0.01);
        let q2 = m.moved(&g, 5.0, 7.0).display_quads(&g)[0];
        assert!((q2[0].0 - 15.0).abs() < 0.01 && (q2[0].1 - 27.0).abs() < 0.01);
    }
}
