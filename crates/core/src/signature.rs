//! Hand-drawn and typed signatures ("wet ink", not cryptographic signing).
//!
//! A signature is [`Ink`]: pen strokes drawn with the mouse or trackpad, and
//! filled outlines for a typed signature (glyphs of a handwriting font turned
//! into paths, so the saved PDF needs no extra font). The same ink is drawn
//! on screen (through [`render`], which rasterises exactly what is saved) and
//! written into the PDF, either as an annotation other PDF apps can move or
//! delete, or locked into the page content.

use crate::field::{Field, FieldKind, ObjId, Origin};
use crate::forms::deref;
use crate::geom::{PageGeom, Rect};
use crate::pdf_font::num;
use crate::render::Image;
use crate::save::rotation_matrix;
use lopdf::{Dictionary, Document, Object, Stream, dictionary, text_string};
use std::fmt::Write as _;
use std::sync::{Arc, OnceLock};

/// Pen colour: a dark blue-black, like a ballpoint pen.
pub const INK_RGB: [f32; 3] = [0.09, 0.13, 0.36];

/// Private key on pdfr's signature annotations holding the ink, so pdfr can
/// read a saved signature back as one that can still be moved and resized.
pub const INK_KEY: &[u8] = b"PdfrSignature";

/// One pen stroke: points in ink space with the pen width.
#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    pub width: f32,
    pub pts: Vec<[f32; 2]>,
}

/// A segment of a filled outline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    M(f32, f32),
    L(f32, f32),
    C(f32, f32, f32, f32, f32, f32),
    Z,
}

/// A signature's drawing. Ink space has y pointing down, height 1 and
/// width `aspect`, so it can be placed in any box keeping its shape.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Ink {
    pub aspect: f32,
    pub strokes: Vec<Stroke>,
    /// Filled outlines (non-zero winding), for typed signatures.
    pub fill: Vec<Seg>,
}

impl Ink {
    pub fn is_empty(&self) -> bool {
        self.strokes.iter().all(|s| s.pts.is_empty()) && self.fill.is_empty()
    }

    /// Builds ink from strokes drawn in any coordinates (y down) with pen
    /// width `pen` in the same units: trimmed to the drawing and normalised.
    pub fn from_drawing(strokes: &[Vec<[f32; 2]>], pen: f32) -> Ink {
        let pts = strokes.iter().flatten();
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for p in pts {
            x0 = x0.min(p[0]);
            y0 = y0.min(p[1]);
            x1 = x1.max(p[0]);
            y1 = y1.max(p[1]);
        }
        if x0 > x1 {
            return Ink::default();
        }
        // Room for the pen around the drawing.
        let (x0, y0, x1, y1) = (x0 - pen, y0 - pen, x1 + pen, y1 + pen);
        let h = (y1 - y0).max(pen * 4.0);
        let cy = (y0 + y1) / 2.0;
        let top = cy - h / 2.0;
        Ink {
            aspect: ((x1 - x0) / h).max(0.05),
            strokes: strokes
                .iter()
                .filter(|s| !s.is_empty())
                .map(|s| Stroke {
                    width: pen / h,
                    pts: s
                        .iter()
                        .map(|p| [(p[0] - x0) / h, (p[1] - top) / h])
                        .collect(),
                })
                .collect(),
            fill: vec![],
        }
    }

    /// `name` set in the handwriting font, as filled outlines.
    pub fn typed(name: &str) -> Ink {
        let f = hand();
        let face = &f.face;
        let asc = face.ascender() as f32;
        let desc = -(face.descender() as f32);
        let mut b = Outline {
            out: vec![],
            x: 0.0,
            asc,
        };
        let mut prev: Option<ttf_parser::GlyphId> = None;
        let mut pen = 0.0f32;
        for c in name.trim().chars() {
            if c.is_control() {
                continue;
            }
            let Some(g) = face.glyph_index(c).or_else(|| face.glyph_index('?')) else {
                continue;
            };
            if let Some(p) = prev {
                pen += kern(face, p, g);
            }
            b.x = pen;
            face.outline_glyph(g, &mut b);
            pen += face.glyph_hor_advance(g).unwrap_or(0) as f32;
            prev = Some(g);
        }
        if b.out.is_empty() {
            return Ink::default();
        }
        // Height 1 covers ascender to descender; trim the width to the ink.
        let h = asc + desc;
        let mut xmin = f32::MAX;
        let mut xmax = f32::MIN;
        for s in &b.out {
            for x in seg_xs(s) {
                xmin = xmin.min(x);
                xmax = xmax.max(x);
            }
        }
        let pad = 0.06 * h;
        let shift = xmin - pad;
        let fill = b
            .out
            .iter()
            .map(|s| map_seg(*s, |x, y| ((x - shift) / h, y / h)))
            .collect();
        Ink {
            aspect: ((xmax - xmin) + 2.0 * pad) / h,
            strokes: vec![],
            fill,
        }
    }

    /// Compact text form, used in the PDF annotation and in pdfr's memory.
    pub fn encode(&self) -> String {
        let mut s = format!("ink1 {}", num4(self.aspect));
        for st in &self.strokes {
            let _ = write!(s, "|s {}", num4(st.width));
            for p in &st.pts {
                let _ = write!(s, " {} {}", num4(p[0]), num4(p[1]));
            }
        }
        if !self.fill.is_empty() {
            s.push_str("|f");
            for g in &self.fill {
                match *g {
                    Seg::M(x, y) => {
                        let _ = write!(s, " M {} {}", num4(x), num4(y));
                    }
                    Seg::L(x, y) => {
                        let _ = write!(s, " L {} {}", num4(x), num4(y));
                    }
                    Seg::C(a, b, c, d, e, f) => {
                        let _ = write!(
                            s,
                            " C {} {} {} {} {} {}",
                            num4(a),
                            num4(b),
                            num4(c),
                            num4(d),
                            num4(e),
                            num4(f)
                        );
                    }
                    Seg::Z => s.push_str(" Z"),
                }
            }
        }
        s
    }

    pub fn decode(s: &str) -> Option<Ink> {
        let mut parts = s.split('|');
        let mut head = parts.next()?.split_whitespace();
        if head.next()? != "ink1" {
            return None;
        }
        let aspect: f32 = head.next()?.parse().ok()?;
        if !(aspect.is_finite() && aspect > 0.0 && aspect < 1000.0) {
            return None;
        }
        let mut ink = Ink {
            aspect,
            ..Ink::default()
        };
        for p in parts {
            let mut t = p.split_whitespace();
            match t.next()? {
                "s" => {
                    let width: f32 = t.next()?.parse().ok()?;
                    let v: Vec<f32> = t.map(|x| x.parse().ok()).collect::<Option<_>>()?;
                    if !v.len().is_multiple_of(2) || !width.is_finite() {
                        return None;
                    }
                    let pts = v.as_chunks::<2>().0.to_vec();
                    ink.strokes.push(Stroke { width, pts });
                }
                "f" => {
                    let toks: Vec<&str> = t.collect();
                    let mut i = 0;
                    let n = |i: usize| -> Option<f32> { toks.get(i)?.parse().ok() };
                    while i < toks.len() {
                        let seg = match toks[i] {
                            "M" => Seg::M(n(i + 1)?, n(i + 2)?),
                            "L" => Seg::L(n(i + 1)?, n(i + 2)?),
                            "C" => Seg::C(
                                n(i + 1)?,
                                n(i + 2)?,
                                n(i + 3)?,
                                n(i + 4)?,
                                n(i + 5)?,
                                n(i + 6)?,
                            ),
                            "Z" => Seg::Z,
                            _ => return None,
                        };
                        i += match seg {
                            Seg::M(..) | Seg::L(..) => 3,
                            Seg::C(..) => 7,
                            Seg::Z => 1,
                        };
                        ink.fill.push(seg);
                    }
                }
                _ => return None,
            }
        }
        Some(ink)
    }

    /// The rectangle of `aspect` shape, centred in `r` and as large as fits.
    pub fn fit(&self, r: &Rect) -> Rect {
        let (w, h) = (r.width(), r.height());
        let (fw, fh) = if w / h > self.aspect {
            (h * self.aspect, h)
        } else {
            (w, w / self.aspect)
        };
        let (cx, cy) = r.center();
        Rect::new(cx - fw / 2.0, cy - fh / 2.0, cx + fw / 2.0, cy + fh / 2.0)
    }
}

fn num4(v: f32) -> String {
    let mut s = format!("{v:.4}");
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    if s == "-0" { "0".into() } else { s }
}

fn seg_xs(s: &Seg) -> Vec<f32> {
    match *s {
        Seg::M(x, _) | Seg::L(x, _) => vec![x],
        Seg::C(a, _, c, _, e, _) => vec![a, c, e],
        Seg::Z => vec![],
    }
}

fn map_seg(s: Seg, f: impl Fn(f32, f32) -> (f32, f32)) -> Seg {
    match s {
        Seg::M(x, y) => {
            let (x, y) = f(x, y);
            Seg::M(x, y)
        }
        Seg::L(x, y) => {
            let (x, y) = f(x, y);
            Seg::L(x, y)
        }
        Seg::C(a, b, c, d, e, g) => {
            let (a, b) = f(a, b);
            let (c, d) = f(c, d);
            let (e, g) = f(e, g);
            Seg::C(a, b, c, d, e, g)
        }
        Seg::Z => Seg::Z,
    }
}

// --- The handwriting font ---------------------------------------------------

/// Great Vibes (SIL OFL 1.1), for typed signatures.
pub static HAND_FONT_BYTES: &[u8] = include_bytes!("../../../assets/GreatVibes-Regular.ttf");

struct Hand {
    face: ttf_parser::Face<'static>,
}

fn hand() -> &'static Hand {
    static H: OnceLock<Hand> = OnceLock::new();
    H.get_or_init(|| Hand {
        face: ttf_parser::Face::parse(HAND_FONT_BYTES, 0).expect("handwriting font parses"),
    })
}

fn kern(face: &ttf_parser::Face, a: ttf_parser::GlyphId, b: ttf_parser::GlyphId) -> f32 {
    let Some(kern) = face.tables().kern else {
        return 0.0;
    };
    kern.subtables
        .into_iter()
        .filter(|s| s.horizontal && !s.variable)
        .find_map(|s| s.glyphs_kerning(a, b))
        .unwrap_or(0) as f32
}

/// Collects glyph outlines in font units with y flipped to point down
/// (0 at the ascender line).
struct Outline {
    out: Vec<Seg>,
    x: f32,
    asc: f32,
}

impl Outline {
    fn p(&self, x: f32, y: f32) -> (f32, f32) {
        (self.x + x, self.asc - y)
    }
    fn last(&self) -> (f32, f32) {
        match self.out.last() {
            Some(Seg::M(x, y)) | Some(Seg::L(x, y)) | Some(Seg::C(_, _, _, _, x, y)) => (*x, *y),
            _ => (0.0, 0.0),
        }
    }
}

impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.p(x, y);
        self.out.push(Seg::M(x, y));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.p(x, y);
        self.out.push(Seg::L(x, y));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (x0, y0) = self.last();
        let (x1, y1) = self.p(x1, y1);
        let (x, y) = self.p(x, y);
        // Quadratic to cubic.
        self.out.push(Seg::C(
            x0 + 2.0 / 3.0 * (x1 - x0),
            y0 + 2.0 / 3.0 * (y1 - y0),
            x + 2.0 / 3.0 * (x1 - x),
            y + 2.0 / 3.0 * (y1 - y),
            x,
            y,
        ));
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (x1, y1) = self.p(x1, y1);
        let (x2, y2) = self.p(x2, y2);
        let (x, y) = self.p(x, y);
        self.out.push(Seg::C(x1, y1, x2, y2, x, y));
    }
    fn close(&mut self) {
        self.out.push(Seg::Z);
    }
}

// --- Drawing ------------------------------------------------------------------

/// Content stream drawing `ink` fitted into a `w`×`h` box (PDF form space,
/// origin bottom-left, y up).
pub fn appearance(ink: &Ink, w: f32, h: f32) -> String {
    let r = ink.fit(&Rect::new(0.0, 0.0, w, h));
    let s = r.height();
    // Ink (x right, y down) -> form space (y up).
    let tx = |x: f32| r.x0 + x * s;
    let ty = |y: f32| h - (r.y0 + y * s);
    let [cr, cg, cb] = INK_RGB;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "q {} {} {} RG {} {} {} rg 1 J 1 j",
        num(cr),
        num(cg),
        num(cb),
        num(cr),
        num(cg),
        num(cb)
    );
    for st in &ink.strokes {
        let Some(first) = st.pts.first() else {
            continue;
        };
        let _ = write!(out, "{} w ", num((st.width * s).max(0.1)));
        let _ = write!(out, "{} {} m", num(tx(first[0])), num(ty(first[1])));
        if st.pts.len() == 1 {
            // A dot: a zero-length line drawn with round caps.
            let _ = write!(out, " {} {} l", num(tx(first[0])), num(ty(first[1])));
        }
        // Smooth curve through the points (Catmull-Rom as cubic Béziers).
        let p = &st.pts;
        for i in 0..p.len().saturating_sub(1) {
            let p0 = p[i.saturating_sub(1)];
            let p1 = p[i];
            let p2 = p[i + 1];
            let p3 = p[(i + 2).min(p.len() - 1)];
            let c1 = [p1[0] + (p2[0] - p0[0]) / 6.0, p1[1] + (p2[1] - p0[1]) / 6.0];
            let c2 = [p2[0] - (p3[0] - p1[0]) / 6.0, p2[1] - (p3[1] - p1[1]) / 6.0];
            let _ = write!(
                out,
                " {} {} {} {} {} {} c",
                num(tx(c1[0])),
                num(ty(c1[1])),
                num(tx(c2[0])),
                num(ty(c2[1])),
                num(tx(p2[0])),
                num(ty(p2[1]))
            );
        }
        out.push_str(" S\n");
    }
    if !ink.fill.is_empty() {
        for g in &ink.fill {
            match *g {
                Seg::M(x, y) => {
                    let _ = write!(out, "{} {} m ", num(tx(x)), num(ty(y)));
                }
                Seg::L(x, y) => {
                    let _ = write!(out, "{} {} l ", num(tx(x)), num(ty(y)));
                }
                Seg::C(a, b, c, d, e, f) => {
                    let _ = write!(
                        out,
                        "{} {} {} {} {} {} c ",
                        num(tx(a)),
                        num(ty(b)),
                        num(tx(c)),
                        num(ty(d)),
                        num(tx(e)),
                        num(ty(f))
                    );
                }
                Seg::Z => out.push_str("h "),
            }
        }
        out.push_str("f\n");
    }
    out.push_str("Q\n");
    out
}

/// Rasterises `ink` fitted into a `w`×`h` pixel image with a transparent
/// background, drawn by the same renderer as the pages from the same content
/// stream that is saved, so the screen shows exactly what is written.
pub fn render(ink: &Ink, w: u32, h: u32) -> Image {
    let (w, h) = (w.clamp(1, 4096), h.clamp(1, 4096));
    let content = appearance(ink, w as f32, h as f32);
    let mut d = Document::with_version("1.7");
    let pages_id = d.new_object_id();
    let cid = d.add_object(Stream::new(Dictionary::new(), content.into_bytes()));
    let page = d.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => Object::Reference(pages_id),
        "MediaBox" => vec![0.into(), 0.into(), (w as i64).into(), (h as i64).into()],
        "Contents" => Object::Reference(cid),
        "Resources" => Dictionary::new(),
    });
    d.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page)],
            "Count" => 1,
        }),
    );
    let cat =
        d.add_object(dictionary! { "Type" => "Catalog", "Pages" => Object::Reference(pages_id) });
    d.trailer.set("Root", Object::Reference(cat));
    let mut bytes = Vec::new();
    let mut img = Image {
        width: w,
        height: h,
        rgba: vec![0; (w * h * 4) as usize],
    };
    if d.save_to(&mut bytes).is_err() {
        return img;
    }
    let Some(pdf) = crate::render::open(bytes) else {
        return img;
    };
    let Some(p) = pdf.pages().first() else {
        return img;
    };
    let cache = crate::render::RenderCache::new();
    let white = crate::render::render_page(p, &cache, 1.0);
    if white.width != w || white.height != h {
        return img;
    }
    // On white, how dark a pixel is says how much ink covers it.
    let [cr, cg, cb] = INK_RGB.map(|c| (c * 255.0).round() as u8);
    let min = cr.min(cg).min(cb) as f32;
    for (o, p) in img
        .rgba
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(white.rgba.as_chunks::<4>().0)
    {
        let darkest = p[0].min(p[1]).min(p[2]) as f32;
        let a = ((255.0 - darkest) / (255.0 - min)).clamp(0.0, 1.0);
        o.copy_from_slice(&[cr, cg, cb, (a * 255.0).round() as u8]);
    }
    img
}

// --- Saving and reading back --------------------------------------------------

/// The appearance XObject for a signature field on a page.
fn appearance_xobject(d: &mut Document, f: &Field, ink: &Ink, geom: &PageGeom) -> ObjId {
    let (w, h) = (f.rect.width(), f.rect.height());
    let mut dict = dictionary! {
        "Type" => "XObject",
        "Subtype" => "Form",
        "BBox" => vec![0.into(), 0.into(), Object::Real(w), Object::Real(h)],
        "Resources" => Dictionary::new(),
    };
    if let Some(m) = rotation_matrix(geom.rotate) {
        dict.set(
            "Matrix",
            m.iter().map(|v| Object::Real(*v)).collect::<Vec<_>>(),
        );
    }
    d.add_object(Stream::new(dict, appearance(ink, w, h).into_bytes()))
}

/// Writes one signature field into `d`: an annotation, or page content when
/// the field is locked.
pub(crate) fn write(d: &mut Document, page_id: ObjId, f: &Field, geom: &PageGeom) {
    let FieldKind::Signature { ink, lock } = &f.kind else {
        return;
    };
    let ap = appearance_xobject(d, f, ink, geom);
    let ur = geom.rect_to_user(&f.rect);
    if *lock {
        if let Some(m) = crate::flatten::placement(d, ap, &ur) {
            crate::flatten::draw_on_page(d, page_id, "PdfrSig", &[(ap, m)]);
        }
        return;
    }
    let annot = dictionary! {
        "Type" => "Annot",
        "Subtype" => "Stamp",
        "Rect" => ur.iter().map(|v| Object::Real(*v)).collect::<Vec<_>>(),
        "F" => crate::forms::F_PRINT,
        "P" => Object::Reference(page_id),
        "Name" => "Signature",
        "Contents" => text_string("Signature"),
        "AP" => dictionary! { "N" => Object::Reference(ap) },
        "PdfrSignature" => text_string(&ink.encode()),
    };
    let id = d.add_object(annot);
    add_annot(d, page_id, id);
}

fn add_annot(d: &mut Document, page_id: ObjId, id: ObjId) {
    let annots_ref = d
        .get_dictionary(page_id)
        .ok()
        .and_then(|p| p.get(b"Annots").ok())
        .and_then(|o| o.as_reference().ok());
    if let Some(aid) = annots_ref {
        if let Ok(Object::Array(a)) = d.get_object_mut(aid) {
            a.push(Object::Reference(id));
        }
    } else if let Ok(p) = d.get_dictionary_mut(page_id) {
        match p.get_mut(b"Annots") {
            Ok(Object::Array(a)) => a.push(Object::Reference(id)),
            _ => p.set("Annots", vec![Object::Reference(id)]),
        }
    }
}

/// pdfr's signature annotations on a page: `(annotation id, ink, user rect)`.
fn page_signatures(d: &Document, page_id: ObjId) -> Vec<(ObjId, Ink, [f32; 4])> {
    let Some(Object::Array(annots)) = d
        .get_dictionary(page_id)
        .ok()
        .and_then(|p| p.get(b"Annots").ok())
        .and_then(|o| deref(d, o))
    else {
        return vec![];
    };
    let mut out = Vec::new();
    for a in annots {
        let Ok(id) = a.as_reference() else { continue };
        let Ok(dict) = d.get_dictionary(id) else {
            continue;
        };
        let Some(ink) = dict
            .get(INK_KEY)
            .ok()
            .and_then(|o| deref(d, o))
            .and_then(|o| lopdf::decode_text_string(o).ok())
            .and_then(|s| Ink::decode(&s))
        else {
            continue;
        };
        let Some(r) = dict
            .get(b"Rect")
            .ok()
            .and_then(|o| deref(d, o))
            .and_then(|o| o.as_array().ok())
            .and_then(|a| {
                let v: Vec<f32> = a
                    .iter()
                    .filter_map(|o| deref(d, o)?.as_float().ok())
                    .collect();
                (v.len() == 4).then(|| [v[0], v[1], v[2], v[3]])
            })
        else {
            continue;
        };
        out.push((id, ink, r));
    }
    out
}

/// pdfr's own signature annotations, read back as signature fields.
pub(crate) fn read(d: &Document, page_ids: &[ObjId], geoms: &[PageGeom]) -> Vec<Field> {
    let mut out = Vec::new();
    for (pi, &pid) in page_ids.iter().enumerate() {
        for (_, ink, r) in page_signatures(d, pid) {
            let rect = geoms[pi].rect_to_display(r);
            out.push(Field::new_signature(pi, rect, Arc::new(ink), false));
        }
    }
    out
}

/// Removes pdfr's signature annotations (and their appearances) from every
/// page. Saving writes the current ones afresh; rendering draws them live.
pub(crate) fn remove_all(d: &mut Document, page_ids: &[ObjId]) {
    for &pid in page_ids {
        let found = page_signatures(d, pid);
        if found.is_empty() {
            continue;
        }
        let ids: Vec<ObjId> = found.iter().map(|(id, ..)| *id).collect();
        let keep = |o: &Object| !o.as_reference().is_ok_and(|r| ids.contains(&r));
        let annots_ref = d
            .get_dictionary(pid)
            .ok()
            .and_then(|p| p.get(b"Annots").ok())
            .and_then(|o| o.as_reference().ok());
        if let Some(aid) = annots_ref {
            if let Ok(Object::Array(a)) = d.get_object_mut(aid) {
                a.retain(keep);
            }
        } else if let Ok(p) = d.get_dictionary_mut(pid)
            && let Ok(Object::Array(a)) = p.get_mut(b"Annots")
        {
            a.retain(keep);
        }
        for id in ids {
            let ap = d
                .get_dictionary(id)
                .ok()
                .and_then(|a| a.get(b"AP").ok())
                .and_then(|o| deref(d, o))
                .and_then(|o| o.as_dict().ok())
                .and_then(|ap| ap.get(b"N").ok())
                .and_then(|o| o.as_reference().ok());
            if let Some(ap) = ap {
                d.objects.remove(&ap);
            }
            d.objects.remove(&id);
        }
    }
}

/// Places where the file asks for a signature: empty `/FT /Sig` widgets.
pub(crate) fn sign_here_spots(
    d: &Document,
    page_ids: &[ObjId],
    geoms: &[PageGeom],
) -> Vec<(usize, Rect)> {
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
            let Some(w) = deref(d, a).and_then(|o| o.as_dict().ok()) else {
                continue;
            };
            if w.get(b"Subtype").ok().and_then(|o| o.as_name().ok()) != Some(b"Widget") {
                continue;
            }
            // /FT may sit on the parent field.
            let mut ft = w.get(b"FT").ok().and_then(|o| o.as_name().ok());
            if ft.is_none() {
                ft = w
                    .get(b"Parent")
                    .ok()
                    .and_then(|o| deref(d, o))
                    .and_then(|o| o.as_dict().ok())
                    .and_then(|p| p.get(b"FT").ok())
                    .and_then(|o| o.as_name().ok());
            }
            if ft != Some(b"Sig") {
                continue;
            }
            let Some(r) = w
                .get(b"Rect")
                .ok()
                .and_then(|o| deref(d, o))
                .and_then(|o| o.as_array().ok())
                .and_then(|a| {
                    let v: Vec<f32> = a
                        .iter()
                        .filter_map(|o| deref(d, o)?.as_float().ok())
                        .collect();
                    (v.len() == 4).then(|| [v[0], v[1], v[2], v[3]])
                })
            else {
                continue;
            };
            let rect = geoms[pi].rect_to_display(r);
            if rect.width() >= 4.0 && rect.height() >= 4.0 {
                out.push((pi, rect));
            }
        }
    }
    out
}

impl Field {
    pub fn new_signature(page: usize, rect: Rect, ink: Arc<Ink>, lock: bool) -> Field {
        Field {
            kind: FieldKind::Signature { ink, lock },
            ..Field::new_text(page, rect, crate::field::TextMode::Single, Origin::Manual)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_round_trip() {
        let ink = Ink::from_drawing(
            &[vec![[0.0, 0.0], [10.0, 5.0], [20.0, 0.0]], vec![[5.0, 8.0]]],
            1.0,
        );
        let back = Ink::decode(&ink.encode()).unwrap();
        assert_eq!(back.strokes.len(), 2);
        assert!((back.aspect - ink.aspect).abs() < 1e-3);
        let t = Ink::typed("Anna Ek");
        assert!(!t.fill.is_empty());
        let back = Ink::decode(&t.encode()).unwrap();
        assert_eq!(back.fill.len(), t.fill.len());
        assert!(Ink::decode("nope").is_none());
        assert!(Ink::decode("ink1 2|s 0.1 1").is_none(), "odd coordinates");
    }

    #[test]
    fn typed_ink_is_wide_and_rendered() {
        let t = Ink::typed("Åsa Öberg");
        assert!(t.aspect > 2.0, "{}", t.aspect);
        let img = render(&t, 300, 80);
        let inked = img
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] > 128)
            .count();
        assert!(inked > 300, "{inked} pixels");
        // Transparent around the ink.
        assert_eq!(img.rgba[3], 0);
    }

    #[test]
    fn drawn_ink_renders() {
        let ink = Ink::from_drawing(&[vec![[0.0, 0.0], [50.0, 20.0], [100.0, 0.0]]], 3.0);
        let img = render(&ink, 200, 60);
        assert!(img.rgba.as_chunks::<4>().0.iter().any(|p| p[3] > 200));
    }

    #[test]
    fn fit_keeps_shape() {
        let ink = Ink {
            aspect: 4.0,
            ..Ink::default()
        };
        let r = ink.fit(&Rect::new(0.0, 0.0, 100.0, 100.0));
        assert!((r.width() - 100.0).abs() < 1e-3 && (r.height() - 25.0).abs() < 1e-3);
    }
}
