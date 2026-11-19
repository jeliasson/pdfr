//! Opening a document and reading its real form fields (AcroForm widgets).
//!
//! Fields are found by iterating each page's `/Annots` for Widget annotations
//! and walking `/Parent` for inherited keys. That is more robust than walking
//! `/AcroForm /Fields`, which is often incomplete.

use crate::field::{Align, Field, FieldKind, ObjId, Origin, TextMode};
use crate::geom::{PageGeom, Rect};
use lopdf::{Dictionary, Document, Object, decode_text_string};
use std::sync::Arc;

// Field flags (/Ff), 1-based bit positions from the PDF spec.
pub const FF_READONLY: i64 = 1 << 0;
pub const FF_MULTILINE: i64 = 1 << 12;
pub const FF_PASSWORD: i64 = 1 << 13;
pub const FF_NO_TOGGLE_OFF: i64 = 1 << 14;
pub const FF_RADIO: i64 = 1 << 15;
pub const FF_PUSHBUTTON: i64 = 1 << 16;
pub const FF_COMBO: i64 = 1 << 17;
pub const FF_EDIT: i64 = 1 << 18;
pub const FF_FILESELECT: i64 = 1 << 20;
pub const FF_COMB: i64 = 1 << 24;
// Annotation flags (/F).
pub const F_HIDDEN: i64 = 1 << 1;
pub const F_PRINT: i64 = 1 << 2;
pub const F_NOVIEW: i64 = 1 << 5;

/// An opened document: page geometry, the editable structure (if parseable)
/// and its fields.
pub struct FormDoc {
    pub bytes: Arc<Vec<u8>>,
    /// `None` if the structure could not be parsed for editing (read-only).
    pub doc: Option<Document>,
    pub pages: Vec<PageGeom>,
    /// Page object ids, parallel to `pages` (empty when read-only).
    pub page_ids: Vec<ObjId>,
    pub fields: Vec<Field>,
    /// Why the document is read-only, if it is.
    pub read_only_reason: Option<String>,
    /// The file was encrypted (pdfr reads and saves it unprotected).
    pub was_protected: bool,
    /// Empty signature fields (`/FT /Sig`) the file asks to be signed.
    pub sign_here: Vec<(usize, crate::geom::Rect)>,
    /// The file's link annotations.
    pub links: Vec<crate::links::Link>,
    /// The outline (bookmarks), flattened in reading order.
    pub outline: Vec<crate::outline::OutlineItem>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum OpenError {
    NotPdf,
    /// The file is protected with a password and none was given.
    NeedsPassword,
    /// The password given does not open the file.
    WrongPassword,
    /// The file is protected in a way pdfr cannot read.
    Encrypted,
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenError::NotPdf => write!(f, "not a readable PDF file"),
            OpenError::NeedsPassword => write!(f, "this PDF is protected with a password"),
            OpenError::WrongPassword => write!(f, "that password does not open this PDF"),
            OpenError::Encrypted => {
                write!(f, "this PDF is protected in a way pdfr can't read yet")
            }
        }
    }
}

impl FormDoc {
    pub fn open(bytes: Vec<u8>) -> Result<FormDoc, OpenError> {
        Self::open_with_password(bytes, None)
    }

    /// Opens a file, with the password if it is protected. A protected
    /// file is decrypted once here: `bytes` hold the readable document, so
    /// rendering and printing never need the password, and the saved copy
    /// is not protected (`was_protected` says so).
    pub fn open_with_password(bytes: Vec<u8>, password: Option<&str>) -> Result<FormDoc, OpenError> {
        use hayro::hayro_syntax::{DecryptionError, LoadPdfError};
        let mut bytes = Arc::new(bytes);
        // Page geometry comes from hayro so it matches rendering exactly.
        let pdf = match hayro::hayro_syntax::Pdf::new_with_password(bytes.clone(), password.unwrap_or("")) {
            Ok(p) => p,
            Err(LoadPdfError::Decryption(DecryptionError::PasswordProtected)) => {
                return Err(if password.is_some() {
                    OpenError::WrongPassword
                } else {
                    OpenError::NeedsPassword
                });
            }
            Err(LoadPdfError::Decryption(_)) => return Err(OpenError::Encrypted),
            Err(LoadPdfError::Invalid) => return Err(OpenError::NotPdf),
        };
        let pages: Vec<PageGeom> = pdf
            .pages()
            .iter()
            .map(|p| crate::render::page_geom(p))
            .collect();
        drop(pdf);
        let mut fd = FormDoc {
            bytes: bytes.clone(),
            doc: None,
            pages,
            page_ids: vec![],
            fields: vec![],
            read_only_reason: None,
            was_protected: false,
            sign_here: vec![],
            links: vec![],
            outline: vec![],
        };
        let loaded = match password {
            Some(p) => Document::load_mem_with_options(&bytes, lopdf::LoadOptions::with_password(p)),
            None => Document::load_mem(&bytes),
        };
        match loaded {
            Ok(doc) if doc.is_encrypted() => return Err(OpenError::Encrypted),
            Ok(doc) => {
                if doc.encryption_state.is_some() {
                    // Decrypted: keep the readable bytes for rendering.
                    let mut plain = Vec::new();
                    if doc.clone().save_to(&mut plain).is_ok() {
                        bytes = Arc::new(plain);
                        fd.bytes = bytes.clone();
                    }
                    fd.was_protected = true;
                }
                let ids: Vec<ObjId> = doc.get_pages().values().copied().collect();
                if ids.len() != fd.pages.len() {
                    fd.read_only_reason =
                        Some("the page tree could not be read consistently".into());
                } else {
                    fd.fields = read_fields(&doc, &ids, &fd.pages);
                    fd.fields
                        .extend(crate::signature::read(&doc, &ids, &fd.pages));
                    fd.sign_here = crate::signature::sign_here_spots(&doc, &ids, &fd.pages);
                    fd.links = crate::links::read(&doc, &ids, &fd.pages);
                    fd.outline = crate::outline::read(&doc, &ids, &fd.pages);
                    fd.page_ids = ids;
                    fd.doc = Some(doc);
                }
            }
            Err(e) => {
                fd.read_only_reason = Some(format!("the file structure could not be parsed ({e})"));
            }
        }
        Ok(fd)
    }

    pub fn editable(&self) -> bool {
        self.doc.is_some()
    }

    /// A copy with `pages` turned clockwise by `turns` quarter turns (the
    /// page's `/Rotate`, so it is saved turned too).
    pub fn rotated(&self, pages: &[usize], turns: i32) -> Result<FormDoc, String> {
        let Some(doc) = &self.doc else {
            return Err("this file cannot be edited".into());
        };
        let mut d = doc.clone();
        for &p in pages {
            let (Some(&id), Some(g)) = (self.page_ids.get(p), self.pages.get(p)) else {
                continue;
            };
            let rot = (g.rotate as i64 + 90 * turns as i64).rem_euclid(360);
            if let Ok(page) = d.get_dictionary_mut(id) {
                page.set("Rotate", rot);
            }
        }
        let mut out = Vec::new();
        d.save_to(&mut out).map_err(|e| e.to_string())?;
        FormDoc::open(out).map_err(|e| e.to_string())
    }

    /// Bytes for rendering: the document with text-field values blanked from
    /// their appearances and toggles switched off, so live values drawn on
    /// top never show over stale ones. Borders and backgrounds are kept.
    pub fn render_bytes(&self) -> Arc<Vec<u8>> {
        let Some(doc) = &self.doc else {
            return self.bytes.clone();
        };
        let signed = self.fields.iter().any(|f| f.kind.is_signature());
        if !signed && self.fields.iter().all(|f| !f.is_real()) {
            return self.bytes.clone();
        }
        let mut d = doc.clone();
        // Signatures are drawn live on top (they can be moved).
        crate::signature::remove_all(&mut d, &self.page_ids);
        for f in &self.fields {
            let Origin::AcroForm { widget, .. } = f.origin else {
                continue;
            };
            if f.kind.is_toggle() {
                if let Ok(w) = d.get_dictionary_mut(widget) {
                    w.set("AS", Object::Name(b"Off".to_vec()));
                }
                continue;
            }
            blank_appearance(&mut d, widget);
        }
        let mut out = Vec::new();
        match d.save_to(&mut out) {
            Ok(()) => Arc::new(out),
            Err(_) => self.bytes.clone(),
        }
    }
}

/// Removes the variable text from a widget's normal appearance.
fn blank_appearance(d: &mut Document, widget: ObjId) {
    let Some(ap_id) = normal_appearance_id(d, widget) else {
        return;
    };
    let Ok(Object::Stream(s)) = d.get_object(ap_id) else {
        return;
    };
    let content = s
        .decompressed_content()
        .unwrap_or_else(|_| s.content.clone());
    let blanked = strip_variable_text(&content);
    if blanked != content {
        let mut ns = s.clone();
        ns.dict.remove(b"Filter");
        ns.dict.remove(b"DecodeParms");
        ns.set_plain_content(blanked);
        // Write a fresh object: the stream may be shared by other widgets.
        let nid = d.add_object(Object::Stream(ns));
        // /AP may be inline or a reference; either way give this widget its
        // own copy pointing at the blanked stream.
        let ap = d
            .get_dictionary(widget)
            .ok()
            .and_then(|w| w.get(b"AP").ok())
            .and_then(|o| deref(d, o))
            .and_then(|o| o.as_dict().ok())
            .cloned();
        if let Some(mut ap) = ap {
            ap.set("N", Object::Reference(nid));
            if let Ok(w) = d.get_dictionary_mut(widget) {
                w.set("AP", Object::Dictionary(ap));
            }
        }
    }
}

/// The object id of a widget's `/AP /N` stream (only when it is a stream
/// reference, not a state dictionary).
pub fn normal_appearance_id(d: &Document, widget: ObjId) -> Option<ObjId> {
    let w = d.get_dictionary(widget).ok()?;
    let ap = deref(d, w.get(b"AP").ok()?)?.as_dict().ok()?;
    match ap.get(b"N").ok()? {
        Object::Reference(id) => match d.get_object(*id).ok()? {
            Object::Stream(_) => Some(*id),
            _ => None,
        },
        _ => None,
    }
}

/// Removes `/Tx BMC ... EMC` sections, or if there are none, all `BT ... ET`
/// text objects.
pub fn strip_variable_text(content: &[u8]) -> Vec<u8> {
    if let Some((a, b)) = find_tx_section(content) {
        let mut out = content[..a].to_vec();
        out.extend_from_slice(&content[b..]);
        return out;
    }
    let mut out = Vec::with_capacity(content.len());
    let mut i = 0;
    while i < content.len() {
        if is_op_at(content, i, b"BT")
            && let Some(e) = find_op(content, i + 2, b"ET")
        {
            i = e + 2;
            continue;
        }
        out.push(content[i]);
        i += 1;
    }
    out
}

/// Byte range of the `/Tx BMC ... EMC` section (inclusive of both markers).
pub fn find_tx_section(content: &[u8]) -> Option<(usize, usize)> {
    let start = find_sub(content, b"/Tx")?;
    let bmc = find_op(content, start + 3, b"BMC")?;
    // The marker must directly follow /Tx (only whitespace between).
    if !content[start + 3..bmc]
        .iter()
        .all(|c| c.is_ascii_whitespace())
    {
        return None;
    }
    // Find the matching EMC, honouring nested BMC/BDC.
    let mut depth = 1;
    let mut i = bmc + 3;
    while i < content.len() {
        if is_op_at(content, i, b"BMC") || is_op_at(content, i, b"BDC") {
            depth += 1;
            i += 3;
            continue;
        }
        if is_op_at(content, i, b"EMC") {
            depth -= 1;
            if depth == 0 {
                return Some((start, i + 3));
            }
            i += 3;
            continue;
        }
        i += 1;
    }
    None
}

fn find_sub(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).position(|w| w == n)
}

fn is_delim(c: u8) -> bool {
    c.is_ascii_whitespace() || b"()<>[]{}/%".contains(&c)
}

fn is_op_at(c: &[u8], i: usize, op: &[u8]) -> bool {
    c.len() >= i + op.len()
        && &c[i..i + op.len()] == op
        && (i == 0 || is_delim(c[i - 1]))
        && (i + op.len() == c.len() || is_delim(c[i + op.len()]))
}

fn find_op(c: &[u8], from: usize, op: &[u8]) -> Option<usize> {
    (from..c.len()).find(|&i| is_op_at(c, i, op))
}

/// Largest `/MaxLen` honoured (characters).
const MAX_LEN_CAP: i64 = 10_000;

pub(crate) fn deref<'a>(d: &'a Document, o: &'a Object) -> Option<&'a Object> {
    match o {
        Object::Reference(id) => d.get_object(*id).ok(),
        o => Some(o),
    }
}

fn get_name(d: &Document, dict: &Dictionary, key: &[u8]) -> Option<String> {
    match deref(d, dict.get(key).ok()?)? {
        Object::Name(n) => Some(String::from_utf8_lossy(n).into_owned()),
        _ => None,
    }
}

fn get_text(d: &Document, dict: &Dictionary, key: &[u8]) -> Option<String> {
    let o = deref(d, dict.get(key).ok()?)?;
    match o {
        Object::String(..) => decode_text_string(o).ok(),
        Object::Name(n) => Some(String::from_utf8_lossy(n).into_owned()),
        Object::Array(a) => a
            .first()
            .and_then(|o| deref(d, o))
            .and_then(|o| decode_text_string(o).ok()),
        _ => None,
    }
}

/// The chain widget -> parent -> ... (ids and dicts), widget first.
fn chain(d: &Document, widget: ObjId) -> Vec<(ObjId, &Dictionary)> {
    let mut out = Vec::new();
    let mut cur = Some(widget);
    while let Some(id) = cur {
        if out.len() > 32 || out.iter().any(|(i, _)| *i == id) {
            break;
        }
        let Ok(dict) = d.get_dictionary(id) else {
            break;
        };
        out.push((id, dict));
        cur = dict.get(b"Parent").ok().and_then(|p| p.as_reference().ok());
    }
    out
}

/// First value of an inheritable key along the chain.
fn inherited<'a>(chain: &[(ObjId, &'a Dictionary)], key: &[u8]) -> Option<&'a Object> {
    chain.iter().find_map(|(_, d)| d.get(key).ok())
}

/// Font size from a `/DA` string (`/Helv 0 Tf 0 g` -> `Some(0.0)`).
pub fn da_font_size(da: &str) -> Option<f32> {
    let toks: Vec<&str> = da.split_whitespace().collect();
    let i = toks.iter().rposition(|t| *t == "Tf")?;
    toks.get(i.checked_sub(1)?)?.parse().ok()
}

/// The appearance state names of a widget's `/AP /N` dictionary other than Off.
fn on_state(d: &Document, w: &Dictionary) -> Option<String> {
    let ap = deref(d, w.get(b"AP").ok()?)?.as_dict().ok()?;
    let n = deref(d, ap.get(b"N").ok()?)?.as_dict().ok()?;
    n.iter()
        .map(|(k, _)| String::from_utf8_lossy(k).into_owned())
        .find(|k| k != "Off")
}

fn read_fields(doc: &Document, page_ids: &[ObjId], geoms: &[PageGeom]) -> Vec<Field> {
    let mut out = Vec::new();
    for (pi, &pid) in page_ids.iter().enumerate() {
        let Ok(page) = doc.get_dictionary(pid) else {
            continue;
        };
        let Some(Object::Array(annots)) = page.get(b"Annots").ok().and_then(|o| deref(doc, o))
        else {
            continue;
        };
        for a in annots {
            let Ok(wid) = a.as_reference() else { continue };
            if let Some(f) = read_widget(doc, wid, pi, &geoms[pi]) {
                out.push(f);
            }
        }
    }
    out
}

fn read_widget(doc: &Document, wid: ObjId, page: usize, geom: &PageGeom) -> Option<Field> {
    let w = doc.get_dictionary(wid).ok()?;
    if get_name(doc, w, b"Subtype").as_deref() != Some("Widget") {
        return None;
    }
    let ch = chain(doc, wid);
    let ft = inherited(&ch, b"FT")
        .and_then(|o| o.as_name().ok())?
        .to_vec();
    let ff = inherited(&ch, b"Ff")
        .and_then(|o| o.as_i64().ok())
        .unwrap_or(0);
    let flags = w.get(b"F").ok().and_then(|o| o.as_i64().ok()).unwrap_or(0);
    if flags & (F_HIDDEN | F_NOVIEW) != 0 {
        return None;
    }
    let rect_arr = deref(doc, w.get(b"Rect").ok()?)?.as_array().ok()?;
    let mut r = [0f32; 4];
    for (i, o) in rect_arr.iter().take(4).enumerate() {
        r[i] = match deref(doc, o)? {
            Object::Integer(v) => *v as f32,
            Object::Real(v) => *v,
            _ => return None,
        };
    }
    let rect = geom.rect_to_display(r);
    if rect.width() < 1.0 || rect.height() < 1.0 {
        return None;
    }
    // The terminal field: the nearest dictionary in the chain with a /T.
    let (field_id, _) = ch
        .iter()
        .find(|(_, d)| d.has(b"T"))
        .copied()
        .unwrap_or(ch[0]);
    let mut names: Vec<String> = ch
        .iter()
        .filter_map(|(_, d)| get_text(doc, d, b"T"))
        .collect();
    names.reverse();
    let name = names.join(".");
    let da = inherited(&ch, b"DA")
        .and_then(|o| deref(doc, o))
        .and_then(|o| decode_text_string(o).ok())
        .unwrap_or_default();
    let font_size = da_font_size(&da).filter(|s| *s > 0.0);
    let q = inherited(&ch, b"Q")
        .and_then(|o| o.as_i64().ok())
        .unwrap_or(0);
    let value_owner: Vec<(ObjId, &Dictionary)> = ch.clone();
    let value = value_owner
        .iter()
        .find_map(|(_, d)| get_text(doc, d, b"V"))
        .unwrap_or_default();
    let tooltip = ch.iter().find_map(|(_, d)| get_text(doc, d, b"TU"));
    let kind = match ft.as_slice() {
        b"Tx" => {
            if ff & FF_FILESELECT != 0 {
                return None;
            }
            let max_len = inherited(&ch, b"MaxLen")
                .and_then(|o| o.as_i64().ok())
                .filter(|m| *m > 0)
                .map(|m| m.min(MAX_LEN_CAP) as usize);
            // A comb needs cells of a usable width; an absurd /MaxLen (from a
            // damaged or hostile file) makes it a plain text field.
            let comb_ok = |n: usize| n as f32 <= rect.width() / 2.0;
            let mode = if let Some(n) = max_len.filter(|&n| {
                comb_ok(n) && ff & FF_COMB != 0 && ff & (FF_MULTILINE | FF_PASSWORD) == 0
            }) {
                TextMode::Comb(Field::equal_cells(&rect, n))
            } else if ff & FF_MULTILINE != 0 {
                TextMode::Multi
            } else {
                TextMode::Single
            };
            FieldKind::Text {
                mode,
                password: ff & FF_PASSWORD != 0,
                max_len,
            }
        }
        b"Btn" => {
            if ff & FF_PUSHBUTTON != 0 {
                return None;
            }
            let on = on_state(doc, w).unwrap_or_else(|| "Yes".into());
            if ff & FF_RADIO != 0 {
                FieldKind::Radio {
                    on_state: on,
                    group: format!("{} {}", field_id.0, field_id.1),
                }
            } else {
                FieldKind::Checkbox { on_state: on }
            }
        }
        b"Ch" => {
            if ff & FF_COMBO == 0 {
                return None; // list boxes are out of scope
            }
            let opts = inherited(&ch, b"Opt")
                .and_then(|o| deref(doc, o))
                .and_then(|o| o.as_array().ok())
                .map(|a| {
                    a.iter()
                        .filter_map(|o| match deref(doc, o)? {
                            Object::Array(pair) if pair.len() >= 2 => {
                                let e = decode_text_string(deref(doc, &pair[0])?).ok()?;
                                let v = decode_text_string(deref(doc, &pair[1])?).ok()?;
                                Some((e, v))
                            }
                            s @ Object::String(..) => {
                                let t = decode_text_string(s).ok()?;
                                Some((t.clone(), t))
                            }
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default();
            FieldKind::Choice {
                options: opts,
                editable: ff & FF_EDIT != 0,
            }
        }
        _ => return None, // signatures and unknown types
    };
    let checked = match &kind {
        FieldKind::Checkbox { on_state } | FieldKind::Radio { on_state, .. } => {
            let as_ = get_name(doc, w, b"AS");
            match as_ {
                Some(s) => &s == on_state,
                None => &value == on_state,
            }
        }
        _ => false,
    };
    Some(Field {
        page,
        rect,
        value: if kind.is_toggle() {
            String::new()
        } else {
            value
        },
        kind,
        checked,
        font_size,
        align: Align::from_q(q),
        read_only: ff & FF_READONLY != 0,
        origin: Origin::AcroForm {
            widget: wid,
            field: field_id,
            name,
        },
        label: None,
        tooltip,
    })
}

/// Fields overlapping `rect` on `page` (used by detection to skip real fields).
pub fn overlaps_any(fields: &[Field], page: usize, rect: &Rect, min_ratio: f32) -> bool {
    fields
        .iter()
        .any(|f| f.page == page && f.rect.overlap_ratio(rect) >= min_ratio)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn da_parsing() {
        assert_eq!(da_font_size("/Helv 0 Tf 0 g"), Some(0.0));
        assert_eq!(da_font_size("0 0 1 rg /F1 10.5 Tf"), Some(10.5));
        assert_eq!(da_font_size(""), None);
    }

    #[test]
    fn strip_tx_section_and_bt() {
        let c = b"0.9 g 0 0 10 10 re f /Tx BMC q BT (x) Tj ET Q EMC 0 G";
        assert_eq!(
            strip_variable_text(c),
            b"0.9 g 0 0 10 10 re f  0 G".to_vec()
        );
        let c = b"1 g 0 0 5 5 re f BT /F 9 Tf (abc) Tj ET";
        assert_eq!(strip_variable_text(c), b"1 g 0 0 5 5 re f ".to_vec());
        // "BT" inside a name is not an operator.
        let c = b"/BTX gs";
        assert_eq!(strip_variable_text(c), c.to_vec());
    }
}
