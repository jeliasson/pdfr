//! Redaction: removing what lies under black boxes for good, not just
//! covering it.
//!
//! Each marked page's content is rewritten (and every Form XObject it draws
//! that reaches a box, as a new copy so other pages keep theirs):
//! - glyphs under a box are taken out of their strings, replaced by an equal
//!   move so the text around them stays where it was;
//! - images get the covered pixels painted black in a new copy;
//! - drawings entirely inside a box are dropped (overlapping ones stay,
//!   drawn over);
//! - a black box is drawn on top;
//! - annotations and form fields touching a box are removed.
//!
//! The result is always a full rewrite: no earlier revisions, thumbnails or
//! page piece info that could still hold the text.

use hayro::hayro_interpret::font::GlyphRun;
use hayro::hayro_interpret::hayro_syntax::object::{Dict, ObjectIdentifier};
use hayro::hayro_interpret::hayro_syntax::xref::XRef;
use hayro::hayro_interpret::{
    BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, Image, ImageData, ImageDrawProps,
    InterpreterCache, InterpreterSettings, RenderContent, SoftMask, interpret_page,
};
use hayro::kurbo::{Affine, BezPath, Point, Rect as KRect};
use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, StringFormat};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// An area to redact: a page and a rectangle in PDF user space
/// (`[llx, lly, urx, ury]`, points).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mark {
    pub page: usize,
    pub rect: [f32; 4],
}

/// The redacted file and what was done to it.
#[derive(Debug, Default)]
pub struct Redacted {
    pub bytes: Vec<u8>,
    /// The file held digital signatures; they are no longer valid.
    pub signatures_broken: bool,
    pub glyphs_removed: usize,
    /// Strings in fonts whose glyph widths are unknown, removed whole.
    pub strings_removed_whole: usize,
    /// Images with the covered pixels painted black.
    pub images_patched: usize,
    /// Images that could not be decoded (or lay fully under a box),
    /// replaced by black.
    pub images_blacked: usize,
    pub paths_removed: usize,
    pub annotations_removed: usize,
}

/// Redacts `marks` in the PDF `bytes` and returns the new file.
pub fn redact(bytes: &[u8], marks: &[Mark]) -> Result<Redacted, String> {
    let mut doc =
        Document::load_mem(bytes).map_err(|e| format!("could not read the file ({e})"))?;
    if doc.is_encrypted() {
        return Err("the file is protected".into());
    }
    let pdf = hayro::hayro_syntax::Pdf::new(Arc::new(bytes.to_vec()))
        .map_err(|_| "could not read the file".to_string())?;
    let hpages = pdf.pages();
    let page_ids: Vec<ObjectId> = doc.get_pages().values().copied().collect();
    if page_ids.len() != hpages.len() {
        return Err("the page tree could not be read consistently".into());
    }
    let mut out = Redacted {
        signatures_broken: is_signed(&doc),
        ..Default::default()
    };
    let mut by_page: HashMap<usize, Vec<KRect>> = HashMap::new();
    for m in marks {
        let [x0, y0, x1, y1] = m.rect;
        let r = KRect::new(x0 as f64, y0 as f64, x1 as f64, y1 as f64).abs();
        if m.page < page_ids.len() && r.area() > 0.0 {
            by_page.entry(m.page).or_default().push(r);
        }
    }
    let mut pages: Vec<_> = by_page.into_iter().collect();
    pages.sort_by_key(|(p, _)| *p);
    let cache = InterpreterCache::new();
    let mut removed_annots = HashSet::new();
    for (p, rects) in pages {
        let images = decode_images(&hpages[p], &cache, &rects);
        let mut w = Walker {
            doc: &mut doc,
            fonts: Fonts::new(pdf.xref()),
            images,
            rects,
            out: &mut out,
            names: 0,
        };
        w.page(page_ids[p], p)?;
        removed_annots.extend(w.annotations(page_ids[p]));
    }
    remove_fields(&mut doc, &removed_annots);
    out.annotations_removed = removed_annots.len();
    // A full rewrite: no previous revisions, and no object that only an
    // old revision (or the original page content) used.
    doc.trailer.remove(b"Prev");
    doc.trailer.remove(b"XRefStm");
    doc.prune_objects();
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes)
        .map_err(|e| format!("could not write the file ({e})"))?;
    out.bytes = bytes;
    Ok(out)
}

/// Whether the document holds a digital signature.
pub fn is_signed(doc: &Document) -> bool {
    doc.objects.values().any(|o| {
        let d = match o {
            Object::Dictionary(d) => d,
            Object::Stream(s) => &s.dict,
            _ => return false,
        };
        let name = |k: &[u8]| d.get(k).and_then(Object::as_name).ok();
        name(b"Type") == Some(b"Sig".as_slice())
            || name(b"Type") == Some(b"DocTimeStamp".as_slice())
            || (name(b"FT") == Some(b"Sig".as_slice()) && d.has(b"V"))
    })
}

// ---------------------------------------------------------------------------
// Images: decoded through hayro, which knows every filter and colour space.

struct Decoded {
    w: u32,
    h: u32,
    gray: bool,
    data: Vec<u8>,
    alpha: Option<Vec<u8>>,
    interpolate: bool,
}

struct ImageGrabber<'r> {
    rects: &'r [KRect],
    found: HashMap<ObjectId, Decoded>,
}

impl<'a> Device<'a> for ImageGrabber<'_> {
    fn draw_path(&mut self, _: &BezPath, _: DrawProps<'_>, _: &DrawMode) {}
    fn push_clip_path(&mut self, _: &ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'_>>, _: BlendMode) {}
    fn draw_glyph_run(&mut self, _: &GlyphRun<'_, '_>, _: DrawProps<'_>, _: &DrawMode) {}
    fn draw_image(&mut self, image: Image<'a, '_>, props: ImageDrawProps<'a>) {
        let Image::Raster(r) = image else { return };
        let oid = r.stream().obj_id();
        let id = (oid.obj_number, oid.gen_number as u16);
        if oid.obj_number == 0 || self.found.contains_key(&id) {
            return;
        }
        let px = KRect::new(0.0, 0.0, r.width() as f64, r.height() as f64);
        let bb = props.transform.transform_rect_bbox(px);
        if !self.rects.iter().any(|q| overlaps(q, &bb)) {
            return;
        }
        r.with_rgba(
            |img, alpha| {
                let (w, h, interpolate) = (img.width(), img.height(), img.interpolate());
                let (gray, data) = match img {
                    ImageData::Rgb(d) => (false, d.data),
                    ImageData::Luma(d) => (true, d.data),
                };
                let alpha = alpha
                    .filter(|a| a.width == w && a.height == h)
                    .map(|a| a.data);
                self.found.insert(
                    id,
                    Decoded {
                        w,
                        h,
                        gray,
                        data,
                        alpha,
                        interpolate,
                    },
                );
            },
            None,
        );
    }
    fn pop_clip(&mut self) {}
    fn pop_transparency_group(&mut self) {}
}

/// The page's raster images that reach a box, decoded, by object id.
fn decode_images<'a>(
    page: &hayro::hayro_syntax::page::Page<'a>,
    cache: &InterpreterCache<'a>,
    rects: &[KRect],
) -> HashMap<ObjectId, Decoded> {
    let settings = InterpreterSettings {
        render_content: RenderContent::PAGE,
        ..Default::default()
    };
    let c = page.media_box();
    let bbox = KRect::new(c.x0, c.y0, c.x1, c.y1).abs().inflate(1.0, 1.0);
    // Identity: positions come out in PDF user space.
    let mut ctx = Context::new(Affine::IDENTITY, bbox, cache, page.xref(), settings);
    let mut dev = ImageGrabber {
        rects,
        found: HashMap::new(),
    };
    interpret_page(page, &mut ctx, &mut dev);
    dev.found
}

/// Paints black every pixel of `d` that lies under a box, for an image drawn
/// with `ctm` (its unit square). Rotated placements black the box's whole
/// extent in image space (a little more than the box).
fn black_out(d: &Decoded, ctm: Affine, rects: &[KRect]) -> Option<(Vec<u8>, Option<Vec<u8>>)> {
    if ctm.determinant().abs() < 1e-9 {
        return None;
    }
    let inv = ctm.inverse();
    let (w, h) = (d.w as usize, d.h as usize);
    let ch = if d.gray { 1 } else { 3 };
    let mut data = d.data.clone();
    let mut alpha = d.alpha.clone();
    for r in rects {
        let uv = inv.transform_rect_bbox(*r);
        let x0 = (uv.x0 * w as f64).floor().clamp(0.0, w as f64) as usize;
        let x1 = (uv.x1 * w as f64).ceil().clamp(0.0, w as f64) as usize;
        // Image rows run top (v = 1) to bottom (v = 0).
        let y0 = ((1.0 - uv.y1) * h as f64).floor().clamp(0.0, h as f64) as usize;
        let y1 = ((1.0 - uv.y0) * h as f64).ceil().clamp(0.0, h as f64) as usize;
        for y in y0..y1 {
            for x in x0..x1 {
                let i = y * w + x;
                data[i * ch..(i + 1) * ch].fill(0);
                if let Some(a) = &mut alpha {
                    a[i] = 255;
                }
            }
        }
    }
    Some((data, alpha))
}

fn image_stream(w: u32, h: u32, gray: bool, data: Vec<u8>, interpolate: bool) -> Stream {
    let mut d = Dictionary::new();
    d.set("Type", Object::Name(b"XObject".to_vec()));
    d.set("Subtype", Object::Name(b"Image".to_vec()));
    d.set("Width", w as i64);
    d.set("Height", h as i64);
    let cs: &[u8] = if gray { b"DeviceGray" } else { b"DeviceRGB" };
    d.set("ColorSpace", Object::Name(cs.to_vec()));
    d.set("BitsPerComponent", 8);
    if interpolate {
        d.set("Interpolate", true);
    }
    let mut s = Stream::new(d, data);
    let _ = s.compress();
    s
}

// ---------------------------------------------------------------------------
// Fonts: hayro's font model gives each code's advance (embedded widths, CID
// widths, the standard 14 fonts' metrics, Type 3).

struct Fonts<'p> {
    xref: &'p XRef,
    cache: hayro_font::FontCache<'p>,
    settings: InterpreterSettings,
    by_id: HashMap<ObjectId, Option<hayro_font::Font<'p>>>,
}

impl<'p> Fonts<'p> {
    fn new(xref: &'p XRef) -> Self {
        Fonts {
            xref,
            cache: hayro_font::FontCache::new(),
            settings: InterpreterSettings::default(),
            by_id: HashMap::new(),
        }
    }

    fn get(&mut self, id: ObjectId) -> Option<hayro_font::Font<'p>> {
        if let Some(f) = self.by_id.get(&id) {
            return f.clone();
        }
        let oid = ObjectIdentifier::new(id.0, id.1 as i32);
        let f = self.xref.get::<Dict<'p>>(oid).and_then(|d| {
            self.cache.font(
                &d,
                &self.settings.font_resolver,
                &self.settings.cmap_resolver,
            )
        });
        self.by_id.insert(id, f.clone());
        f
    }
}

// ---------------------------------------------------------------------------
// The content rewriter.

#[derive(Clone)]
struct Gs<'p> {
    ctm: Affine,
    /// The current font; `None` when unknown (widths are then estimated).
    font: Option<hayro_font::Font<'p>>,
    size: f64,
    tc: f64,
    tw: f64,
    th: f64,
    tl: f64,
    rise: f64,
}

impl Gs<'_> {
    fn new(ctm: Affine) -> Self {
        Gs {
            ctm,
            font: None,
            size: 0.0,
            tc: 0.0,
            tw: 0.0,
            th: 1.0,
            tl: 0.0,
            rise: 0.0,
        }
    }
}

/// What to do with one `Do` or inline image.
enum ImgOut {
    Keep,
    Replace(ObjectId),
    Black,
}

struct Walker<'d, 'p> {
    doc: &'d mut Document,
    fonts: Fonts<'p>,
    images: HashMap<ObjectId, Decoded>,
    rects: Vec<KRect>,
    out: &'d mut Redacted,
    names: usize,
}

fn overlaps(a: &KRect, b: &KRect) -> bool {
    let i = a.intersect(*b);
    i.width() > 0.0 && i.height() > 0.0
}

fn num(o: &Object) -> f64 {
    o.as_float().map(|v| v as f64).unwrap_or(0.0)
}

fn nums(op: &Operation) -> Vec<f64> {
    op.operands.iter().map(num).collect()
}

fn op(name: &str, operands: Vec<Object>) -> Operation {
    Operation::new(name, operands)
}

/// `q 0 g 0 0 1 1 re f Q`: black over the current unit square.
fn black_square() -> Vec<Operation> {
    vec![
        op("q", vec![]),
        op("g", vec![0.into()]),
        op("re", vec![0.into(), 0.into(), 1.into(), 1.into()]),
        op("f", vec![]),
        op("Q", vec![]),
    ]
}

impl<'p> Walker<'_, 'p> {
    /// Whether a glyph box is mostly under a box (or its middle is).
    fn glyph_hit(&self, b: &KRect) -> bool {
        let c = b.center();
        self.rects.iter().any(|r| {
            r.contains(c) || {
                let i = r.intersect(*b);
                i.width() > 0.0 && i.height() > 0.0 && i.area() >= 0.4 * b.area()
            }
        })
    }

    fn any_overlap(&self, b: &KRect) -> bool {
        self.rects.iter().any(|r| overlaps(r, b))
    }

    fn inside_one(&self, b: &KRect) -> bool {
        let b = b.inflate(-0.01, -0.01);
        self.rects.iter().any(|r| r.contains_rect(b))
    }

    fn resolve<'o>(&'o self, o: &'o Object) -> &'o Object {
        match o {
            Object::Reference(id) => self.doc.get_object(*id).unwrap_or(o),
            _ => o,
        }
    }

    /// A resources sub-dictionary (`/Font`, `/XObject`), resolved.
    fn sub(&self, res: &Dictionary, key: &[u8]) -> Dictionary {
        res.get(key)
            .ok()
            .and_then(|o| self.resolve(o).as_dict().ok())
            .cloned()
            .unwrap_or_default()
    }

    /// Adds an XObject to `res` under a fresh name.
    fn add_xobject(&mut self, res: &mut Dictionary, id: ObjectId) -> Vec<u8> {
        let mut xo = self.sub(res, b"XObject");
        let name = loop {
            self.names += 1;
            let n = format!("Rdx{}", self.names).into_bytes();
            if !xo.has(&n) {
                break n;
            }
        };
        xo.set(name.clone(), Object::Reference(id));
        res.set("XObject", Object::Dictionary(xo));
        name
    }

    /// Rewrites one page.
    fn page(&mut self, page_id: ObjectId, index: usize) -> Result<(), String> {
        let content = self.doc.get_page_content(page_id);
        let ops = Content::decode(&content)
            .map_err(|e| format!("could not read page {} ({e})", index + 1))?
            .operations;
        let mut res = self.page_resources(page_id);
        let (body, _) = self.walk(ops, &mut res, Affine::IDENTITY, 0);
        let mut all = vec![op("q", vec![])];
        all.extend(body);
        all.push(op("Q", vec![]));
        all.push(op("q", vec![]));
        all.push(op("g", vec![0.into()]));
        for r in &self.rects {
            all.push(op(
                "re",
                vec![
                    Object::Real(r.x0 as f32),
                    Object::Real(r.y0 as f32),
                    Object::Real(r.width() as f32),
                    Object::Real(r.height() as f32),
                ],
            ));
        }
        all.push(op("f", vec![]));
        all.push(op("Q", vec![]));
        let bytes = Content { operations: all }
            .encode()
            .map_err(|e| format!("could not write page {} ({e})", index + 1))?;
        let mut s = Stream::new(Dictionary::new(), bytes);
        let _ = s.compress();
        let cid = self.doc.add_object(s);
        let page = self
            .doc
            .get_dictionary_mut(page_id)
            .map_err(|e| e.to_string())?;
        page.set("Contents", Object::Reference(cid));
        page.set("Resources", Object::Dictionary(res));
        // Private application data and the page's thumbnail picture can
        // still show what was removed.
        page.remove(b"PieceInfo");
        page.remove(b"Thumb");
        Ok(())
    }

    /// The page's resources (possibly inherited), as a private copy.
    fn page_resources(&self, page_id: ObjectId) -> Dictionary {
        let mut id = Some(page_id);
        let mut guard = 0;
        while let Some(cur) = id {
            let Ok(d) = self.doc.get_dictionary(cur) else {
                break;
            };
            if let Ok(r) = d.get(b"Resources")
                && let Ok(r) = self.resolve(r).as_dict()
            {
                return r.clone();
            }
            id = d.get(b"Parent").and_then(Object::as_reference).ok();
            guard += 1;
            if guard > 64 {
                break;
            }
        }
        Dictionary::new()
    }

    /// Rewrites a list of operations drawn with `ctm`. Returns the new list
    /// and whether anything changed.
    fn walk(
        &mut self,
        ops: Vec<Operation>,
        res: &mut Dictionary,
        ctm: Affine,
        depth: u32,
    ) -> (Vec<Operation>, bool) {
        let fonts = self.sub(res, b"Font");
        let mut out: Vec<Operation> = Vec::with_capacity(ops.len());
        let mut changed = false;
        let mut gs = Gs::new(ctm);
        let mut stack: Vec<Gs<'p>> = Vec::new();
        let (mut tm, mut tlm) = (Affine::IDENTITY, Affine::IDENTITY);
        // The path being built: its points' bounds in user space.
        let mut path: Option<KRect> = None;
        // Open marked-content sections: index in `out` (BDC only), and
        // whether text in them was removed.
        let mut marked: Vec<Option<usize>> = Vec::new();
        let mut touched: HashSet<usize> = HashSet::new();
        // XObject names whose object was replaced by a redacted copy.
        let mut replaced: HashSet<Vec<u8>> = HashSet::new();

        let add_pt = |path: &mut Option<KRect>, ctm: Affine, x: f64, y: f64| {
            let p = ctm * Point::new(x, y);
            let r = KRect::from_points(p, p);
            *path = Some(path.map_or(r, |q| q.union(r)));
        };

        for o in ops {
            let v = nums(&o);
            let a = |i: usize| v.get(i).copied().unwrap_or(0.0);
            match o.operator.as_str() {
                "q" => stack.push(gs.clone()),
                "Q" => {
                    if let Some(g) = stack.pop() {
                        gs = g;
                    }
                }
                "cm" if v.len() == 6 => {
                    gs.ctm *= Affine::new([a(0), a(1), a(2), a(3), a(4), a(5)]);
                }
                // Paths.
                "m" | "l" => add_pt(&mut path, gs.ctm, a(0), a(1)),
                "c" => {
                    for i in 0..3 {
                        add_pt(&mut path, gs.ctm, a(2 * i), a(2 * i + 1));
                    }
                }
                "v" | "y" => {
                    add_pt(&mut path, gs.ctm, a(0), a(1));
                    add_pt(&mut path, gs.ctm, a(2), a(3));
                }
                "re" => {
                    add_pt(&mut path, gs.ctm, a(0), a(1));
                    add_pt(&mut path, gs.ctm, a(0) + a(2), a(1) + a(3));
                    add_pt(&mut path, gs.ctm, a(0), a(1) + a(3));
                    add_pt(&mut path, gs.ctm, a(0) + a(2), a(1));
                }
                "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" => {
                    let bb = path.take();
                    if bb.is_some_and(|b| self.inside_one(&b)) {
                        // Ending the path without painting keeps any clip.
                        out.push(op("n", vec![]));
                        self.out.paths_removed += 1;
                        changed = true;
                        continue;
                    }
                }
                "n" => path = None,
                // Text state.
                "BT" => {
                    tm = Affine::IDENTITY;
                    tlm = Affine::IDENTITY;
                }
                "Tc" => gs.tc = a(0),
                "Tw" => gs.tw = a(0),
                "Tz" => gs.th = a(0) / 100.0,
                "TL" => gs.tl = a(0),
                "Ts" => gs.rise = a(0),
                "Tf" => {
                    gs.size = v.get(1).copied().unwrap_or(0.0);
                    gs.font = o
                        .operands
                        .first()
                        .and_then(|n| n.as_name().ok())
                        .and_then(|n| fonts.get(n).ok())
                        .and_then(|f| f.as_reference().ok())
                        .and_then(|id| self.fonts.get(id));
                }
                "Td" | "TD" => {
                    if o.operator == "TD" {
                        gs.tl = -a(1);
                    }
                    tlm *= Affine::translate((a(0), a(1)));
                    tm = tlm;
                }
                "Tm" if v.len() == 6 => {
                    tlm = Affine::new([a(0), a(1), a(2), a(3), a(4), a(5)]);
                    tm = tlm;
                }
                "T*" => {
                    tlm *= Affine::translate((0.0, -gs.tl));
                    tm = tlm;
                }
                "Tj" | "TJ" | "'" | "\"" => {
                    let mut pre = Vec::new();
                    if o.operator == "\"" {
                        gs.tw = a(0);
                        gs.tc = a(1);
                        pre.push(op("Tw", vec![o.operands[0].clone()]));
                        pre.push(op("Tc", vec![o.operands[1].clone()]));
                    }
                    if o.operator == "'" || o.operator == "\"" {
                        tlm *= Affine::translate((0.0, -gs.tl));
                        tm = tlm;
                        pre.push(op("T*", vec![]));
                    }
                    let elems: Vec<Object> = match (o.operator.as_str(), o.operands.last()) {
                        ("TJ", Some(Object::Array(arr))) => arr.clone(),
                        (_, Some(s @ Object::String(..))) => vec![s.clone()],
                        _ => vec![],
                    };
                    if let Some(new) = self.show(&gs, &mut tm, &elems) {
                        out.extend(pre);
                        out.push(op("TJ", vec![Object::Array(new)]));
                        touched.extend(marked.iter().flatten().copied());
                        changed = true;
                        continue;
                    }
                }
                // Marked content: alternate text of removed text goes too.
                "BDC" => {
                    let inline = matches!(o.operands.get(1), Some(Object::Dictionary(_)));
                    marked.push(inline.then_some(out.len()));
                }
                "BMC" => marked.push(None),
                "EMC" => {
                    marked.pop();
                }
                // Images and forms.
                "Do" => {
                    if let Some(new) = self.do_op(&o, res, gs.ctm, depth) {
                        if let Some(Ok(n)) = o.operands.first().map(Object::as_name) {
                            replaced.insert(n.to_vec());
                        }
                        out.extend(new);
                        changed = true;
                        continue;
                    }
                }
                "BI" => {
                    let bb = gs.ctm.transform_rect_bbox(KRect::new(0.0, 0.0, 1.0, 1.0));
                    if self.any_overlap(&bb) {
                        // Inline images are small; not decoded, made black.
                        out.extend(black_square());
                        self.out.images_blacked += 1;
                        changed = true;
                        continue;
                    }
                }
                _ => {}
            }
            out.push(o);
        }
        self.drop_unused(res, &out, replaced);
        for i in touched {
            if let Some(Object::Dictionary(d)) = out[i].operands.get_mut(1) {
                for k in [&b"ActualText"[..], b"Alt", b"E"] {
                    d.remove(k);
                }
            }
        }
        (out, changed)
    }

    /// Takes replaced XObjects no longer drawn out of `res`, so the
    /// originals (which still show what was removed) leave the file.
    fn drop_unused(&self, res: &mut Dictionary, out: &[Operation], replaced: HashSet<Vec<u8>>) {
        if replaced.is_empty() {
            return;
        }
        let used: HashSet<&[u8]> = out
            .iter()
            .filter(|o| o.operator == "Do")
            .filter_map(|o| o.operands.first()?.as_name().ok())
            .collect();
        let mut xo = self.sub(res, b"XObject");
        for n in replaced {
            if !used.contains(n.as_slice()) {
                xo.remove(&n);
            }
        }
        res.set("XObject", Object::Dictionary(xo));
    }

    /// A text show's elements (strings and adjustments) with the glyphs
    /// under a box replaced by moves, or `None` when nothing is removed.
    /// Advances `tm` past the text.
    fn show(&mut self, gs: &Gs<'p>, tm: &mut Affine, elems: &[Object]) -> Option<Vec<Object>> {
        let exact = gs.font.as_ref().is_some_and(|f| f.is_horizontal());
        let scale = gs.size * gs.th;
        let mut out: Vec<Object> = Vec::new();
        let mut kept: Vec<u8> = Vec::new();
        let mut adj = 0.0f64;
        let mut fmt = StringFormat::Literal;
        let mut any = false;
        let flush_str = |out: &mut Vec<Object>, kept: &mut Vec<u8>, fmt: StringFormat| {
            if !kept.is_empty() {
                out.push(Object::String(std::mem::take(kept), fmt));
            }
        };
        let flush_adj = |out: &mut Vec<Object>, adj: &mut f64| {
            if adj.abs() > 1e-6 {
                out.push(Object::Real(*adj as f32));
            }
            *adj = 0.0;
        };
        for e in elems {
            match e {
                Object::String(bytes, f) => {
                    fmt = *f;
                    let glyphs = self.glyphs(gs, *tm, bytes);
                    let hits: Vec<bool> = glyphs.iter().map(|g| self.glyph_hit(&g.2)).collect();
                    // Widths unknown: positions are only estimates, so a
                    // string touching a box goes whole.
                    let whole = !exact && hits.iter().any(|h| *h);
                    if whole {
                        self.out.strings_removed_whole += 1;
                    }
                    for (g, hit) in glyphs.iter().zip(hits) {
                        let (range, tx, _) = g;
                        if hit || whole {
                            flush_str(&mut out, &mut kept, fmt);
                            if scale.abs() > 1e-9 {
                                adj -= tx * 1000.0 / scale;
                            }
                            self.out.glyphs_removed += 1;
                            any = true;
                        } else {
                            flush_adj(&mut out, &mut adj);
                            kept.extend_from_slice(&bytes[range.clone()]);
                        }
                        *tm *= Affine::translate((*tx, 0.0));
                    }
                }
                Object::Integer(_) | Object::Real(_) => {
                    flush_str(&mut out, &mut kept, fmt);
                    let n = num(e);
                    adj += n;
                    *tm *= Affine::translate((-n / 1000.0 * scale, 0.0));
                }
                _ => {}
            }
        }
        flush_str(&mut out, &mut kept, fmt);
        flush_adj(&mut out, &mut adj);
        any.then_some(out)
    }

    /// Each glyph of a string: its bytes, its advance (text space) and its
    /// box in user space.
    fn glyphs(
        &self,
        gs: &Gs<'p>,
        tm: Affine,
        bytes: &[u8],
    ) -> Vec<(std::ops::Range<usize>, f64, KRect)> {
        let trm = gs.ctm * tm;
        let mut out = Vec::new();
        let mut off = 0;
        let mut x = 0.0;
        while off < bytes.len() {
            let (code, len, w) = match &gs.font {
                Some(f) => {
                    let (code, len) = f.read_code(bytes, off);
                    let len = len.max(1).min(bytes.len() - off);
                    (code, len, f.code_advance(code).x)
                }
                // Unknown font: one byte per glyph, half an em wide.
                None => (bytes[off] as u32, 1, 500.0),
            };
            let space = if len == 1 && code == 32 { gs.tw } else { 0.0 };
            let tx = (w / 1000.0 * gs.size + gs.tc + space) * gs.th;
            let gw = (w / 1000.0 * gs.size * gs.th).max(0.0);
            let glyph = KRect::new(
                x,
                gs.rise - 0.22 * gs.size,
                x + gw,
                gs.rise + 0.75 * gs.size,
            );
            out.push((off..off + len, tx, trm.transform_rect_bbox(glyph)));
            x += tx;
            off += len;
        }
        out
    }

    /// The replacement for a `Do`, if the object it draws reaches a box.
    fn do_op(
        &mut self,
        o: &Operation,
        res: &mut Dictionary,
        ctm: Affine,
        depth: u32,
    ) -> Option<Vec<Operation>> {
        let name = o.operands.first()?.as_name().ok()?.to_vec();
        let xo = self.sub(res, b"XObject");
        let id = match xo.get(&name).ok()? {
            Object::Reference(id) => *id,
            _ => return None,
        };
        let subtype = self
            .doc
            .get_object(id)
            .ok()?
            .as_stream()
            .ok()?
            .dict
            .get(b"Subtype")
            .and_then(Object::as_name)
            .ok()?
            .to_vec();
        match subtype.as_slice() {
            b"Image" => match self.image(id, ctm) {
                ImgOut::Keep => None,
                ImgOut::Black => {
                    self.out.images_blacked += 1;
                    Some(black_square())
                }
                ImgOut::Replace(new) => {
                    self.out.images_patched += 1;
                    let n = self.add_xobject(res, new);
                    Some(vec![op("Do", vec![Object::Name(n)])])
                }
            },
            b"Form" if depth < 12 => {
                let new = self.form(id, ctm, res, depth)?;
                let n = self.add_xobject(res, new);
                Some(vec![op("Do", vec![Object::Name(n)])])
            }
            _ => None,
        }
    }

    fn image(&mut self, id: ObjectId, ctm: Affine) -> ImgOut {
        let bb = ctm.transform_rect_bbox(KRect::new(0.0, 0.0, 1.0, 1.0));
        if !self.any_overlap(&bb) {
            return ImgOut::Keep;
        }
        if self.inside_one(&bb) {
            return ImgOut::Black;
        }
        // Not decoded (a stencil mask, or a format hayro can't read): black.
        let Some(d) = self.images.get(&id) else {
            return ImgOut::Black;
        };
        let Some((data, alpha)) = black_out(d, ctm, &self.rects) else {
            return ImgOut::Black;
        };
        let mut s = image_stream(d.w, d.h, d.gray, data, d.interpolate);
        if let Some(a) = alpha {
            let mask = image_stream(d.w, d.h, true, a, d.interpolate);
            let mid = self.doc.add_object(mask);
            s.dict.set("SMask", Object::Reference(mid));
        }
        ImgOut::Replace(self.doc.add_object(s))
    }

    /// A rewritten copy of a Form XObject drawn with `ctm`, if it changes.
    fn form(
        &mut self,
        id: ObjectId,
        ctm: Affine,
        parent_res: &Dictionary,
        depth: u32,
    ) -> Option<ObjectId> {
        let s = self.doc.get_object(id).ok()?.as_stream().ok()?.clone();
        let m = s
            .dict
            .get(b"Matrix")
            .and_then(Object::as_array)
            .ok()
            .filter(|a| a.len() == 6)
            .map(|a| {
                let v: Vec<f64> = a.iter().map(num).collect();
                Affine::new([v[0], v[1], v[2], v[3], v[4], v[5]])
            })
            .unwrap_or(Affine::IDENTITY);
        let fctm = ctm * m;
        if let Ok(b) = s.dict.get(b"BBox").and_then(Object::as_array)
            && b.len() == 4
        {
            let v: Vec<f64> = b.iter().map(num).collect();
            let bb = fctm.transform_rect_bbox(KRect::new(v[0], v[1], v[2], v[3]).abs());
            if !self.any_overlap(&bb) {
                return None;
            }
        }
        let content = s
            .decompressed_content()
            .unwrap_or_else(|_| s.content.clone());
        let ops = Content::decode(&content).ok()?.operations;
        let mut res = match s.dict.get(b"Resources") {
            Ok(r) => self.resolve(r).as_dict().cloned().unwrap_or_default(),
            Err(_) => parent_res.clone(),
        };
        let (new_ops, changed) = self.walk(ops, &mut res, fctm, depth + 1);
        if !changed {
            return None;
        }
        let bytes = Content {
            operations: new_ops,
        }
        .encode()
        .ok()?;
        let mut dict = s.dict.clone();
        for k in [&b"Filter"[..], b"DecodeParms", b"Length"] {
            dict.remove(k);
        }
        dict.set("Resources", Object::Dictionary(res));
        let mut ns = Stream::new(dict, bytes);
        let _ = ns.compress();
        Some(self.doc.add_object(ns))
    }

    /// Removes the page's annotations that touch a box (and their popups);
    /// returns their ids.
    fn annotations(&mut self, page_id: ObjectId) -> HashSet<ObjectId> {
        let annots: Vec<Object> = self
            .doc
            .get_dictionary(page_id)
            .ok()
            .and_then(|p| p.get(b"Annots").ok())
            .map(|a| self.resolve(a).as_array().cloned().unwrap_or_default())
            .unwrap_or_default();
        let mut removed = HashSet::new();
        for a in &annots {
            let Object::Reference(id) = a else { continue };
            let Ok(d) = self.doc.get_dictionary(*id) else {
                continue;
            };
            let rect = d
                .get(b"Rect")
                .and_then(Object::as_array)
                .ok()
                .filter(|r| r.len() == 4);
            if let Some(r) = rect {
                let v: Vec<f64> = r.iter().map(|o| num(self.resolve(o))).collect();
                if self.any_overlap(&KRect::new(v[0], v[1], v[2], v[3]).abs()) {
                    removed.insert(*id);
                }
            }
        }
        // Popups belong to the annotation they open from.
        for a in &annots {
            let Object::Reference(id) = a else { continue };
            let parent = self
                .doc
                .get_dictionary(*id)
                .ok()
                .and_then(|d| d.get(b"Parent").and_then(Object::as_reference).ok());
            if parent.is_some_and(|p| removed.contains(&p)) {
                removed.insert(*id);
            }
        }
        if !removed.is_empty() {
            let keep: Vec<Object> = annots
                .into_iter()
                .filter(|a| !matches!(a, Object::Reference(id) if removed.contains(id)))
                .collect();
            if let Ok(p) = self.doc.get_dictionary_mut(page_id) {
                p.set("Annots", Object::Array(keep));
            }
        }
        removed
    }
}

/// Takes removed widgets out of the form's field tree (and fields left
/// without widgets).
fn remove_fields(doc: &mut Document, removed: &HashSet<ObjectId>) {
    if removed.is_empty() {
        return;
    }
    let Some(form_obj) = doc
        .catalog()
        .ok()
        .and_then(|c| c.get(b"AcroForm").ok())
        .cloned()
    else {
        return;
    };
    let (form_id, mut form) = match &form_obj {
        Object::Reference(id) => match doc.get_dictionary(*id) {
            Ok(d) => (Some(*id), d.clone()),
            Err(_) => return,
        },
        Object::Dictionary(d) => (None, d.clone()),
        _ => return,
    };
    for key in [&b"Fields"[..], b"CO"] {
        if let Ok(arr) = form.get(key).and_then(Object::as_array) {
            let arr = arr.clone();
            let kept = prune_kids(doc, arr, removed, 0);
            form.set(key, Object::Array(kept));
        }
    }
    match form_id {
        Some(id) => {
            doc.objects.insert(id, Object::Dictionary(form));
        }
        None => {
            if let Ok(c) = doc.catalog_mut() {
                c.set("AcroForm", Object::Dictionary(form));
            }
        }
    }
}

fn prune_kids(
    doc: &mut Document,
    kids: Vec<Object>,
    removed: &HashSet<ObjectId>,
    depth: u32,
) -> Vec<Object> {
    let mut out = Vec::new();
    for k in kids {
        let Object::Reference(id) = k else {
            out.push(k);
            continue;
        };
        if removed.contains(&id) {
            continue;
        }
        let sub = doc
            .get_dictionary(id)
            .ok()
            .and_then(|d| d.get(b"Kids").and_then(Object::as_array).ok())
            .cloned();
        if let Some(sub) = sub
            && depth < 32
        {
            let had = !sub.is_empty();
            let left = prune_kids(doc, sub, removed, depth + 1);
            if had && left.is_empty() {
                continue;
            }
            if let Ok(d) = doc.get_dictionary_mut(id) {
                d.set("Kids", Object::Array(left));
            }
        }
        out.push(k);
    }
    out
}
