//! A print copy of a filled PDF: every printable annotation's appearance
//! (filled-in values, ticks) drawn into the page content itself, the
//! annotations and the form removed. Printers that read PDFs themselves often
//! skip form fields, and some drivers do too; a flattened copy prints the
//! same everywhere. The user's saved file is never flattened.

use crate::forms::deref;
use crate::pdf_font::num;
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};

const F_HIDDEN: i64 = 1 << 1;
const F_PRINT: i64 = 1 << 2;

/// Flattens `bytes` for printing.
pub fn flatten_for_print(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut d =
        Document::load_mem(bytes).map_err(|e| format!("could not read the document: {e}"))?;
    let pages: Vec<ObjectId> = d.get_pages().into_values().collect();
    for page in pages {
        flatten_page(&mut d, page);
    }
    // The fields no longer have widgets: drop the form.
    if let Ok(cat) = d.catalog_mut() {
        cat.remove(b"AcroForm");
    }
    let mut out = Vec::new();
    d.save_to(&mut out)
        .map_err(|e| format!("could not prepare the document: {e}"))?;
    Ok(out)
}

fn flatten_page(d: &mut Document, page: ObjectId) {
    let annots: Vec<Object> = match d
        .get_dictionary(page)
        .ok()
        .and_then(|p| p.get(b"Annots").ok())
        .and_then(|o| deref(d, o))
    {
        Some(Object::Array(a)) => a.clone(),
        _ => return,
    };
    let mut keep: Vec<Object> = Vec::new();
    let mut items: Vec<(ObjectId, [f32; 6])> = Vec::new();
    for a in annots {
        let Some(dict) = deref(d, &a).and_then(|o| o.as_dict().ok()).cloned() else {
            continue;
        };
        let flags = dict
            .get(b"F")
            .ok()
            .and_then(|o| deref(d, o))
            .and_then(|o| o.as_i64().ok())
            .unwrap_or(0);
        let Some(ap) = normal_appearance(d, &dict) else {
            // Nothing to draw (links, popups): keep it as it is.
            keep.push(a);
            continue;
        };
        // Annotations not meant for print are dropped from the print copy.
        if flags & F_PRINT == 0 || flags & F_HIDDEN != 0 {
            continue;
        }
        let Some(rect) = floats(d, dict.get(b"Rect").ok()).filter(|r| r.len() == 4) else {
            continue;
        };
        let Some(m) = placement(d, ap, &rect) else {
            continue;
        };
        items.push((ap, m));
    }
    if items.is_empty() && keep.len() == count_annots(d, page) {
        return;
    }
    draw_on_page(d, page, "PdfrFlat", &items);
    if let Ok(p) = d.get_dictionary_mut(page) {
        if keep.is_empty() {
            p.remove(b"Annots");
        } else {
            p.set("Annots", Object::Array(keep));
        }
    }
}

/// Draws form XObjects into a page's content after everything else, each
/// with its placement matrix. The page's own content is wrapped in q … Q so
/// its graphics state cannot shift what is drawn after it. XObject names
/// start with `prefix` and never reuse a name the page already has.
pub(crate) fn draw_on_page(
    d: &mut Document,
    page: ObjectId,
    prefix: &str,
    items: &[(ObjectId, [f32; 6])],
) {
    // Resources (possibly inherited from the page tree) plus our XObjects.
    let mut res = inherited_resources(d, page);
    let mut xo = match res.get(b"XObject").ok().and_then(|o| deref(d, o)) {
        Some(Object::Dictionary(x)) => x.clone(),
        _ => Dictionary::new(),
    };
    let mut draw = String::new();
    let mut k = 0usize;
    for &(ap, m) in items {
        let name = loop {
            let n = format!("{prefix}{k}");
            k += 1;
            if !xo.has(n.as_bytes()) {
                break n;
            }
        };
        let _ = std::fmt::Write::write_fmt(
            &mut draw,
            format_args!(
                "q {} {} {} {} {} {} cm /{name} Do Q\n",
                num(m[0]),
                num(m[1]),
                num(m[2]),
                num(m[3]),
                num(m[4]),
                num(m[5])
            ),
        );
        ensure_form_xobject(d, ap);
        xo.set(name.into_bytes(), Object::Reference(ap));
    }
    res.set("XObject", Object::Dictionary(xo));
    let mut contents: Vec<Object> = vec![Object::Reference(
        d.add_object(Stream::new(Dictionary::new(), b"q\n".to_vec())),
    )];
    match d
        .get_dictionary(page)
        .ok()
        .and_then(|p| p.get(b"Contents").ok())
        .cloned()
    {
        Some(Object::Array(a)) => contents.extend(a),
        Some(Object::Reference(id)) => match d.get_object(id) {
            Ok(Object::Array(a)) => contents.extend(a.clone()),
            _ => contents.push(Object::Reference(id)),
        },
        _ => {}
    }
    let tail = format!("Q\n{draw}");
    contents.push(Object::Reference(
        d.add_object(Stream::new(Dictionary::new(), tail.into_bytes())),
    ));
    if let Ok(p) = d.get_dictionary_mut(page) {
        p.set("Resources", Object::Dictionary(res));
        p.set("Contents", Object::Array(contents));
    }
}

fn count_annots(d: &Document, page: ObjectId) -> usize {
    match d
        .get_dictionary(page)
        .ok()
        .and_then(|p| p.get(b"Annots").ok())
        .and_then(|o| deref(d, o))
    {
        Some(Object::Array(a)) => a.len(),
        _ => 0,
    }
}

/// The normal appearance stream to draw: `/AP /N`, or for widgets with
/// states (checkboxes) the one named by `/AS`. None for "Off" / missing.
fn normal_appearance(d: &Document, annot: &Dictionary) -> Option<ObjectId> {
    let ap = deref(d, annot.get(b"AP").ok()?)?.as_dict().ok()?;
    let n = ap.get(b"N").ok()?;
    let id = n.as_reference().ok();
    match deref(d, n)? {
        Object::Stream(_) => id,
        Object::Dictionary(states) => {
            let state = annot
                .get(b"AS")
                .ok()
                .and_then(|o| deref(d, o))
                .and_then(|o| o.as_name().ok())?;
            states
                .get(state)
                .ok()?
                .as_reference()
                .ok()
                .filter(|id| matches!(d.get_object(*id), Ok(Object::Stream(_))))
        }
        _ => None,
    }
}

/// The matrix that puts the appearance's box (after its own `/Matrix`)
/// onto the annotation rectangle (PDF 32000-1, 12.5.5).
pub(crate) fn placement(d: &Document, ap: ObjectId, rect: &[f32]) -> Option<[f32; 6]> {
    let Ok(Object::Stream(s)) = d.get_object(ap) else {
        return None;
    };
    let bbox = floats(d, s.dict.get(b"BBox").ok()).filter(|b| b.len() == 4)?;
    let m = floats(d, s.dict.get(b"Matrix").ok())
        .filter(|m| m.len() == 6)
        .unwrap_or(vec![1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    let corners = [
        (bbox[0], bbox[1]),
        (bbox[2], bbox[1]),
        (bbox[0], bbox[3]),
        (bbox[2], bbox[3]),
    ];
    let t: Vec<(f32, f32)> = corners
        .iter()
        .map(|&(x, y)| (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]))
        .collect();
    let (x0, x1) = t
        .iter()
        .fold((f32::MAX, f32::MIN), |a, p| (a.0.min(p.0), a.1.max(p.0)));
    let (y0, y1) = t
        .iter()
        .fold((f32::MAX, f32::MIN), |a, p| (a.0.min(p.1), a.1.max(p.1)));
    let (rx0, ry0) = (rect[0].min(rect[2]), rect[1].min(rect[3]));
    let (rx1, ry1) = (rect[0].max(rect[2]), rect[1].max(rect[3]));
    if x1 - x0 <= 0.0 || y1 - y0 <= 0.0 {
        return None;
    }
    let sx = (rx1 - rx0) / (x1 - x0);
    let sy = (ry1 - ry0) / (y1 - y0);
    Some([sx, 0.0, 0.0, sy, rx0 - x0 * sx, ry0 - y0 * sy])
}

/// Appearance streams are form XObjects, but some omit the type keys.
fn ensure_form_xobject(d: &mut Document, id: ObjectId) {
    if let Ok(Object::Stream(s)) = d.get_object_mut(id) {
        if !s.dict.has(b"Type") {
            s.dict.set("Type", Object::Name(b"XObject".to_vec()));
        }
        if !s.dict.has(b"Subtype") {
            s.dict.set("Subtype", Object::Name(b"Form".to_vec()));
        }
    }
}

/// The page's resources, following `/Parent` for inherited ones.
fn inherited_resources(d: &Document, page: ObjectId) -> Dictionary {
    let mut id = Some(page);
    for _ in 0..32 {
        let Some(cur) = id else { break };
        let Ok(dict) = d.get_dictionary(cur) else {
            break;
        };
        if let Some(Object::Dictionary(r)) = dict.get(b"Resources").ok().and_then(|o| deref(d, o)) {
            return r.clone();
        }
        id = dict.get(b"Parent").ok().and_then(|o| o.as_reference().ok());
    }
    Dictionary::new()
}

fn floats(d: &Document, o: Option<&Object>) -> Option<Vec<f32>> {
    let a = deref(d, o?)?.as_array().ok()?;
    a.iter().map(|o| deref(d, o)?.as_float().ok()).collect()
}
