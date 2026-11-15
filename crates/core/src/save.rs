//! Saving: writes field values and normal appearance streams built from the
//! shared layout, so the saved text is exactly what was on screen.

use crate::field::{Field, FieldKind, ObjId, Origin, TextMode};
use crate::forms::{
    F_PRINT, FF_COMB, FF_MULTILINE, FF_NO_TOGGLE_OFF, FF_PASSWORD, FF_RADIO, FormDoc, deref,
    find_tx_section, normal_appearance_id,
};
use crate::geom::{PageGeom, Rect};
use crate::layout::layout;
use crate::pdf_font::{ensure_font, num};
use lopdf::{Dictionary, Document, Object, Stream, StringFormat, dictionary, text_string};
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

/// Resource name of the embedded font inside appearance streams.
pub const FONT_RES: &str = "PdfrF";

#[derive(Debug)]
pub enum SaveError {
    ReadOnly,
    Write(String),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::ReadOnly => write!(f, "this file cannot be edited"),
            SaveError::Write(e) => write!(f, "could not write the file: {e}"),
        }
    }
}

/// Appearance-stream matrix for a page rotation, so text is upright on screen.
pub fn rotation_matrix(rot: u16) -> Option<[f32; 6]> {
    match rot {
        90 => Some([0.0, 1.0, -1.0, 0.0, 0.0, 0.0]),
        180 => Some([-1.0, 0.0, 0.0, -1.0, 0.0, 0.0]),
        270 => Some([0.0, -1.0, 1.0, 0.0, 0.0, 0.0]),
        _ => None,
    }
}

/// The `/Tx BMC ... EMC` section for `field` showing `text`, in the
/// upright form space of the field (origin bottom-left of the display rect).
pub fn tx_section(field: &Field, text: &str) -> Vec<u8> {
    let r = field.rect;
    let (w, h) = (r.width(), r.height());
    let l = layout(field, text);
    let mut s = String::new();
    let _ = writeln!(s, "/Tx BMC\nq");
    // Clip to the box (combs and on-line fields are not inset).
    let inset = if matches!(
        field.kind.text_mode(),
        Some(TextMode::Comb(_) | TextMode::OnLine)
    ) {
        0.0
    } else {
        1.0
    };
    let _ = writeln!(
        s,
        "{} {} {} {} re W n",
        num(inset),
        num(inset),
        num(w - 2.0 * inset),
        num(h - 2.0 * inset)
    );
    if l.len > 0 {
        let _ = writeln!(s, "BT\n0 g\n/{FONT_RES} {} Tf", num(l.size));
        let per_glyph = l.cells.is_some();
        for line in &l.lines {
            let y = r.y1 - line.baseline;
            if per_glyph {
                for g in &line.glyphs {
                    let _ = writeln!(
                        s,
                        "1 0 0 1 {} {} Tm <{:04X}> Tj",
                        num(g.x - r.x0),
                        num(y),
                        g.gid
                    );
                }
            } else if let Some(first) = line.glyphs.first() {
                let hex: String = line
                    .glyphs
                    .iter()
                    .map(|g| format!("{:04X}", g.gid))
                    .collect();
                let _ = writeln!(
                    s,
                    "1 0 0 1 {} {} Tm <{hex}> Tj",
                    num(first.x - r.x0),
                    num(y)
                );
            }
        }
        s.push_str("ET\n");
    }
    s.push_str("Q\nEMC\n");
    s.into_bytes()
}

fn color_ops(arr: &[Object], stroke: bool) -> Option<String> {
    let v: Vec<f32> = arr.iter().filter_map(|o| o.as_float().ok()).collect();
    let op = match (v.len(), stroke) {
        (1, false) => "g",
        (1, true) => "G",
        (3, false) => "rg",
        (3, true) => "RG",
        (4, false) => "k",
        (4, true) => "K",
        _ => return None,
    };
    Some(format!(
        "{} {op}",
        v.iter().map(|x| num(*x)).collect::<Vec<_>>().join(" ")
    ))
}

/// Background and border from `/MK` and `/BS`, in form space of size `w`×`h`.
fn mk_decoration(d: &Document, widget: &Dictionary, w: f32, h: f32) -> String {
    let mut s = String::new();
    let mk = widget
        .get(b"MK")
        .ok()
        .and_then(|o| deref(d, o))
        .and_then(|o| o.as_dict().ok());
    let Some(mk) = mk else { return s };
    if let Some(bg) = mk
        .get(b"BG")
        .ok()
        .and_then(|o| deref(d, o))
        .and_then(|o| o.as_array().ok())
        && let Some(c) = color_ops(bg, false)
    {
        let _ = writeln!(s, "{c} 0 0 {} {} re f", num(w), num(h));
    }
    if let Some(bc) = mk
        .get(b"BC")
        .ok()
        .and_then(|o| deref(d, o))
        .and_then(|o| o.as_array().ok())
    {
        let bw = widget
            .get(b"BS")
            .ok()
            .and_then(|o| deref(d, o))
            .and_then(|o| o.as_dict().ok())
            .and_then(|bs| bs.get(b"W").ok())
            .and_then(|o| o.as_float().ok())
            .unwrap_or(1.0);
        if bw > 0.0
            && let Some(c) = color_ops(bc, true)
        {
            let _ = writeln!(
                s,
                "{c} {} w {} {} {} {} re S",
                num(bw),
                num(bw / 2.0),
                num(bw / 2.0),
                num(w - bw),
                num(h - bw)
            );
        }
    }
    s
}

fn matrix_obj(m: [f32; 6]) -> Object {
    Object::Array(m.iter().map(|v| Object::Real(*v)).collect())
}

fn floats(d: &Document, o: Option<&Object>) -> Option<Vec<f32>> {
    let a = deref(d, o?)?.as_array().ok()?;
    a.iter().map(|o| deref(d, o)?.as_float().ok()).collect()
}

fn font_resources(font_id: ObjId, base: Option<Dictionary>) -> Dictionary {
    let mut res = base.unwrap_or_default();
    let mut fonts = match res.get(b"Font") {
        Ok(Object::Dictionary(f)) => f.clone(),
        _ => Dictionary::new(),
    };
    fonts.set(FONT_RES, Object::Reference(font_id));
    res.set("Font", Object::Dictionary(fonts));
    res
}

/// Builds the normal appearance for a text-like widget and returns its new object id.
fn write_text_appearance(
    d: &mut Document,
    field: &Field,
    geom: &PageGeom,
    font_cache: &mut Option<ObjId>,
    widget: Option<ObjId>,
) -> ObjId {
    // The font is embedded once per file.
    let font_id = *font_cache.get_or_insert_with(|| ensure_font(d));
    let r = field.rect;
    let (w, h) = (r.width(), r.height());
    let matrix = rotation_matrix(geom.rotate);
    let tx = tx_section(field, field.shown_text());
    // Try to keep the existing appearance and replace only its /Tx section.
    if let Some(wid) = widget
        && let Some(ap_id) = normal_appearance_id(d, wid)
        && let Ok(Object::Stream(old)) = d.get_object(ap_id)
    {
        let bbox = floats(d, old.dict.get(b"BBox").ok());
        let om = floats(d, old.dict.get(b"Matrix").ok());
        let same_box = bbox.as_ref().is_some_and(|b| {
            b.len() == 4
                && ((b[2] - b[0]) - w).abs() < 0.5
                && ((b[3] - b[1]) - h).abs() < 0.5
                && b[0].abs() < 0.01
                && b[1].abs() < 0.01
        });
        let want = matrix.unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        let same_matrix = match &om {
            None => matrix.is_none(),
            Some(m) => {
                m.len() == 6
                    && m.iter()
                        .zip(want)
                        .take(4)
                        .all(|(a, b)| (a - b).abs() < 1e-3)
            }
        };
        let content = old
            .decompressed_content()
            .unwrap_or_else(|_| old.content.clone());
        if same_box
            && same_matrix
            && let Some((a, b)) = find_tx_section(&content)
        {
            let mut nc = content[..a].to_vec();
            nc.extend_from_slice(&tx);
            nc.extend_from_slice(&content[b..]);
            let res = match old.dict.get(b"Resources") {
                Ok(o) => deref(d, o).and_then(|o| o.as_dict().ok()).cloned(),
                Err(_) => None,
            };
            let mut dict = old.dict.clone();
            dict.remove(b"Filter");
            dict.remove(b"DecodeParms");
            dict.remove(b"Length");
            dict.set(
                "Resources",
                Object::Dictionary(font_resources(font_id, res)),
            );
            return d.add_object(Stream::new(dict, nc));
        }
    }
    // Generate a complete appearance.
    let mut content = String::new();
    if let Some(wid) = widget
        && let Ok(wd) = d.get_dictionary(wid)
    {
        content = mk_decoration(d, wd, w, h);
    }
    let mut bytes = content.into_bytes();
    bytes.extend_from_slice(&tx);
    let mut dict = dictionary! {
        "Type" => "XObject",
        "Subtype" => "Form",
        "BBox" => vec![0.into(), 0.into(), Object::Real(w), Object::Real(h)],
        "Resources" => Object::Dictionary(font_resources(font_id, None)),
    };
    if let Some(m) = matrix {
        dict.set("Matrix", matrix_obj(m));
    }
    d.add_object(Stream::new(dict, bytes))
}

/// A check mark (on) or nothing (off), drawn with paths so no font is needed.
pub fn check_paths(w: f32, h: f32) -> String {
    // A cross ("mark with an X"), centred in a square.
    let m = w.min(h);
    let ox = (w - m) / 2.0;
    let oy = (h - m) / 2.0;
    let (a, b) = (CROSS_INSET, 1.0 - CROSS_INSET);
    format!(
        "q 0 g 0 G {} w 1 J {} {} m {} {} l S {} {} m {} {} l S Q\n",
        num(m * CROSS_WIDTH),
        num(ox + m * a),
        num(oy + m * a),
        num(ox + m * b),
        num(oy + m * b),
        num(ox + m * a),
        num(oy + m * b),
        num(ox + m * b),
        num(oy + m * a)
    )
}

/// Cross geometry shared with the screen drawing: inset from the square's
/// edges and stroke width, as fractions of the square's side.
pub const CROSS_INSET: f32 = 0.22;
pub const CROSS_WIDTH: f32 = 0.11;

fn toggle_appearances(d: &mut Document, field: &Field, geom: &PageGeom, on: &str) -> Object {
    let (w, h) = (field.rect.width(), field.rect.height());
    let mk = |content: String| {
        let mut dict = dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), Object::Real(w), Object::Real(h)],
            "Resources" => Dictionary::new(),
        };
        if let Some(m) = rotation_matrix(geom.rotate) {
            dict.set("Matrix", matrix_obj(m));
        }
        Stream::new(dict, content.into_bytes())
    };
    let on_id = d.add_object(mk(check_paths(w, h)));
    let off_id = d.add_object(mk(String::new()));
    let mut n = Dictionary::new();
    n.set(on.as_bytes().to_vec(), Object::Reference(on_id));
    n.set("Off", Object::Reference(off_id));
    Object::Dictionary(dictionary! { "N" => Object::Dictionary(n) })
}

fn has_on_appearance(d: &Document, widget: ObjId, on: &str) -> bool {
    let Ok(w) = d.get_dictionary(widget) else {
        return false;
    };
    let Some(ap) = w
        .get(b"AP")
        .ok()
        .and_then(|o| deref(d, o))
        .and_then(|o| o.as_dict().ok())
    else {
        return false;
    };
    ap.get(b"N")
        .ok()
        .and_then(|o| deref(d, o))
        .and_then(|o| o.as_dict().ok())
        .is_some_and(|n| n.has(on.as_bytes()))
}

/// The `/AcroForm` dictionary id, creating it (as an indirect object) if needed.
fn acroform_id(d: &mut Document) -> Result<ObjId, SaveError> {
    let cat = d.catalog().map_err(|e| SaveError::Write(e.to_string()))?;
    match cat.get(b"AcroForm") {
        Ok(Object::Reference(id)) => Ok(*id),
        Ok(Object::Dictionary(dict)) => {
            let dict = dict.clone();
            let id = d.add_object(dict);
            d.catalog_mut()
                .map_err(|e| SaveError::Write(e.to_string()))?
                .set("AcroForm", Object::Reference(id));
            Ok(id)
        }
        _ => {
            let id = d.add_object(dictionary! { "Fields" => Vec::<Object>::new() });
            d.catalog_mut()
                .map_err(|e| SaveError::Write(e.to_string()))?
                .set("AcroForm", Object::Reference(id));
            Ok(id)
        }
    }
}

/// Makes sure `/DR /Font /Helv` exists so new fields' `/DA` works in other viewers.
/// `/AcroForm /DR /Font /Helv` exists already.
fn has_helv(d: &Document, form: ObjId) -> bool {
    let dr = d
        .get_dictionary(form)
        .ok()
        .and_then(|f| f.get(b"DR").ok())
        .and_then(|o| deref(d, o))
        .and_then(|o| o.as_dict().ok());
    let fonts = dr
        .and_then(|dr| dr.get(b"Font").ok())
        .and_then(|o| deref(d, o))
        .and_then(|o| o.as_dict().ok());
    fonts.is_some_and(|f| f.has(b"Helv"))
}

fn ensure_helv(d: &mut Document, form: ObjId) {
    if has_helv(d, form) {
        return;
    }
    let helv = d.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    // Resolve /DR (may be a reference).
    let dr_ref = d
        .get_dictionary(form)
        .ok()
        .and_then(|f| f.get(b"DR").ok())
        .and_then(|o| o.as_reference().ok());
    let apply = |dr: &mut Dictionary, d_fonts: Option<Dictionary>| {
        let mut fonts = d_fonts.unwrap_or_default();
        if !fonts.has(b"Helv") {
            fonts.set("Helv", Object::Reference(helv));
        }
        dr.set("Font", Object::Dictionary(fonts));
    };
    let font_dict = |d: &Document, dr: &Dictionary| -> Option<Dictionary> {
        dr.get(b"Font")
            .ok()
            .and_then(|o| deref(d, o))
            .and_then(|o| o.as_dict().ok())
            .cloned()
    };
    if let Some(id) = dr_ref {
        let fonts = d.get_dictionary(id).ok().and_then(|dr| font_dict(d, dr));
        if let Ok(dr) = d.get_dictionary_mut(id) {
            apply(dr, fonts);
        }
    } else {
        let existing = d
            .get_dictionary(form)
            .ok()
            .and_then(|f| f.get(b"DR").ok())
            .and_then(|o| o.as_dict().ok())
            .cloned();
        let fonts = existing.as_ref().and_then(|dr| font_dict(d, dr));
        let mut dr = existing.unwrap_or_default();
        apply(&mut dr, fonts);
        if let Ok(f) = d.get_dictionary_mut(form) {
            f.set("DR", Object::Dictionary(dr));
            if !f.has(b"DA") {
                f.set(
                    "DA",
                    Object::String(b"/Helv 0 Tf 0 g".to_vec(), StringFormat::Literal),
                );
            }
        }
    }
}

/// True if a detected comb's cells are close enough to equal that other
/// viewers' comb rendering (equal cells across the rect) will line up.
fn cells_are_even(rect: &Rect, cells: &[(f32, f32)]) -> bool {
    let eq = Field::equal_cells(rect, cells.len());
    let w = rect.width() / cells.len().max(1) as f32;
    cells
        .iter()
        .zip(eq)
        .all(|(a, b)| ((a.0 + a.1) / 2.0 - (b.0 + b.1) / 2.0).abs() < w * 0.12)
}

/// Saves `fields` into a copy of the document and returns the new file bytes.
pub fn save(fd: &FormDoc, fields: &[Field]) -> Result<Vec<u8>, SaveError> {
    let Some(orig) = &fd.doc else {
        return Err(SaveError::ReadOnly);
    };
    let mut d = orig.clone();
    let originals: HashMap<ObjId, &Field> = fd
        .fields
        .iter()
        .filter_map(|f| match f.origin {
            Origin::AcroForm { widget, .. } => Some((widget, f)),
            _ => None,
        })
        .collect();
    let mut font_cache: Option<ObjId> = None;
    let form = acroform_id(&mut d)?;
    if let Ok(fdict) = d.get_dictionary_mut(form) {
        fdict.remove(b"NeedAppearances");
        fdict.remove(b"XFA");
    }

    // Checkbox and radio fields -> on-state of a checked widget, if any.
    let mut radio_values: HashMap<ObjId, Option<String>> = HashMap::new();
    let mut written_v: HashSet<ObjId> = HashSet::new();
    let mut new_count = 0usize;
    // Names already in the file: new fields never reuse one (fields with the
    // same name are one field to other viewers).
    let mut taken: HashSet<String> = field_names(&d, form);

    for f in fields {
        let geom = fd.pages[f.page];
        match &f.origin {
            Origin::AcroForm { widget, field, .. } => {
                let (widget, field_id) = (*widget, *field);
                match &f.kind {
                    FieldKind::Text { .. } | FieldKind::Choice { .. } => {
                        let changed = originals.get(&widget).is_none_or(|o| o.value != f.value);
                        // Untouched fields keep their own appearance (fonts,
                        // colours, formatted numbers) unless they have none.
                        let has_ap = d.get_dictionary(widget).is_ok_and(|w| w.has(b"AP"));
                        if !changed && (f.value.is_empty() || has_ap) {
                            continue;
                        }
                        if written_v.insert(field_id)
                            && let Ok(fdict) = d.get_dictionary_mut(field_id)
                        {
                            fdict.set("V", text_string(&f.value));
                        }
                        let ap =
                            write_text_appearance(&mut d, f, &geom, &mut font_cache, Some(widget));
                        if let Ok(w) = d.get_dictionary_mut(widget) {
                            w.set("AP", dictionary! { "N" => Object::Reference(ap) });
                        }
                    }
                    FieldKind::Checkbox { on_state } | FieldKind::Radio { on_state, .. } => {
                        let state = if f.checked {
                            on_state.clone()
                        } else {
                            "Off".into()
                        };
                        if f.checked && !has_on_appearance(&d, widget, on_state) {
                            let ap = toggle_appearances(&mut d, f, &geom, on_state);
                            if let Ok(w) = d.get_dictionary_mut(widget) {
                                w.set("AP", ap);
                            }
                        }
                        if let Ok(w) = d.get_dictionary_mut(widget) {
                            w.set("AS", Object::Name(state.clone().into_bytes()));
                        }
                        // One field may have several widgets: its value is the
                        // on-state of a checked one, written after the loop.
                        let e = radio_values.entry(field_id).or_insert(None);
                        if f.checked {
                            *e = Some(state);
                        }
                    }
                }
            }
            Origin::Detected | Origin::Manual => {
                if f.is_empty() {
                    continue; // detected fields are saved only if filled
                }
                new_count += 1;
                let Some(&page_id) = fd.page_ids.get(f.page) else {
                    continue;
                };
                let mut k = new_count;
                let name = loop {
                    let n = format!("pdfr_p{}_{}", f.page + 1, k);
                    if taken.insert(n.clone()) {
                        break n;
                    }
                    k += 1;
                };
                let ur = geom.rect_to_user(&f.rect);
                let mut w = dictionary! {
                    "Type" => "Annot",
                    "Subtype" => "Widget",
                    "Rect" => ur.iter().map(|v| Object::Real(*v)).collect::<Vec<_>>(),
                    "F" => F_PRINT,
                    "P" => Object::Reference(page_id),
                    "T" => text_string(&name),
                };
                let mut mk = Dictionary::new();
                if geom.rotate != 0 {
                    mk.set("R", geom.rotate as i64);
                }
                w.set("MK", Object::Dictionary(mk));
                w.set("BS", dictionary! { "W" => 0 });
                if let Some(l) = &f.label {
                    w.set("TU", text_string(l));
                }
                match &f.kind {
                    FieldKind::Text {
                        mode,
                        password,
                        max_len,
                    } => {
                        let mut ff = 0i64;
                        if *password {
                            ff |= FF_PASSWORD;
                        }
                        let size = f.font_size.unwrap_or(0.0);
                        match mode {
                            TextMode::Multi => ff |= FF_MULTILINE,
                            TextMode::Comb(cells) => {
                                w.set("MaxLen", cells.len() as i64);
                                if cells_are_even(&f.rect, cells) {
                                    ff |= FF_COMB;
                                }
                            }
                            _ => {
                                if let Some(m) = max_len {
                                    w.set("MaxLen", *m as i64);
                                }
                            }
                        }
                        w.set("FT", "Tx");
                        w.set("Ff", ff);
                        w.set("Q", f.align.to_q());
                        w.set(
                            "DA",
                            Object::String(
                                format!("/Helv {} Tf 0 g", num(size)).into_bytes(),
                                StringFormat::Literal,
                            ),
                        );
                        w.set("V", text_string(&f.value));
                        let wid = d.add_object(w);
                        let ap = write_text_appearance(&mut d, f, &geom, &mut font_cache, None);
                        if let Ok(wd) = d.get_dictionary_mut(wid) {
                            wd.set("AP", dictionary! { "N" => Object::Reference(ap) });
                        }
                        attach(&mut d, page_id, form, wid)?;
                    }
                    FieldKind::Checkbox { on_state } => {
                        w.set("FT", "Btn");
                        w.set("V", Object::Name(on_state.clone().into_bytes()));
                        w.set("AS", Object::Name(on_state.clone().into_bytes()));
                        w.set(
                            "DA",
                            Object::String(b"/ZaDb 0 Tf 0 g".to_vec(), StringFormat::Literal),
                        );
                        let ap = toggle_appearances(&mut d, f, &geom, on_state);
                        w.set("AP", ap);
                        let wid = d.add_object(w);
                        attach(&mut d, page_id, form, wid)?;
                    }
                    FieldKind::Radio { .. } | FieldKind::Choice { .. } => {
                        // Detection never produces these.
                        let _ = (FF_RADIO, FF_NO_TOGGLE_OFF);
                    }
                }
            }
        }
    }
    for (fid, v) in radio_values {
        if let Ok(fdict) = d.get_dictionary_mut(fid) {
            fdict.set(
                "V",
                Object::Name(v.unwrap_or_else(|| "Off".into()).into_bytes()),
            );
        }
    }
    if new_count > 0 {
        ensure_helv(&mut d, form);
    }
    let mut out = Vec::new();
    d.save_to(&mut out)
        .map_err(|e| SaveError::Write(e.to_string()))?;
    Ok(out)
}

/// Top-level field names (`/T`) in `/AcroForm /Fields`.
fn field_names(d: &Document, form: ObjId) -> HashSet<String> {
    let mut out = HashSet::new();
    let fields = d
        .get_dictionary(form)
        .ok()
        .and_then(|f| f.get(b"Fields").ok())
        .and_then(|o| deref(d, o))
        .and_then(|o| o.as_array().ok())
        .cloned()
        .unwrap_or_default();
    for f in fields {
        let Some(dict) = deref(d, &f).and_then(|o| o.as_dict().ok()) else {
            continue;
        };
        if let Some(t) = dict
            .get(b"T")
            .ok()
            .and_then(|o| lopdf::decode_text_string(o).ok())
        {
            out.insert(t);
        }
    }
    out
}

/// Adds a widget to its page's `/Annots` and to `/AcroForm /Fields`.
fn attach(d: &mut Document, page_id: ObjId, form: ObjId, wid: ObjId) -> Result<(), SaveError> {
    // /Annots may be a reference to an array.
    let annots_ref = d
        .get_dictionary(page_id)
        .ok()
        .and_then(|p| p.get(b"Annots").ok())
        .and_then(|o| o.as_reference().ok());
    if let Some(aid) = annots_ref {
        if let Ok(Object::Array(a)) = d.get_object_mut(aid) {
            a.push(Object::Reference(wid));
        }
    } else if let Ok(p) = d.get_dictionary_mut(page_id) {
        match p.get_mut(b"Annots") {
            Ok(Object::Array(a)) => a.push(Object::Reference(wid)),
            _ => p.set("Annots", vec![Object::Reference(wid)]),
        }
    }
    let fields_ref = d
        .get_dictionary(form)
        .ok()
        .and_then(|f| f.get(b"Fields").ok())
        .and_then(|o| o.as_reference().ok());
    if let Some(fid) = fields_ref {
        if let Ok(Object::Array(a)) = d.get_object_mut(fid) {
            a.push(Object::Reference(wid));
        }
    } else if let Ok(f) = d.get_dictionary_mut(form) {
        match f.get_mut(b"Fields") {
            Ok(Object::Array(a)) => a.push(Object::Reference(wid)),
            _ => f.set("Fields", vec![Object::Reference(wid)]),
        }
    }
    Ok(())
}
