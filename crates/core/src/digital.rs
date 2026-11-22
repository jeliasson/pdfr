//! Digital (certificate) signatures in the PDF: reading the ones a file has,
//! and writing a new one.
//!
//! The cryptography lives in `pdfr-sign`; this module only knows the PDF
//! side. A new signature is written as an incremental update ending in a
//! signature dictionary whose `/Contents` is a zero-filled placeholder.
//! `/ByteRange` is patched to cover the whole file except that placeholder,
//! those bytes go to the caller's signing callback, and the CMS it returns
//! is written into the placeholder. Nothing else in the file moves.

use crate::field::{Field, ObjId};
use crate::forms::{FormDoc, chain, deref, get_text, inherited};
use crate::geom::{PageGeom, Rect};
use crate::pdf_font::{ensure_font, num};
use crate::save::{FONT_RES, SaveError, append_update, build_with, rotation_matrix};
use crate::signature::Ink;
use lopdf::{Dictionary, Document, Object, Stream, StringFormat, dictionary, text_string};
use std::fmt::Write as _;
use std::sync::Arc;

/// Bytes reserved for the signature (CMS) by default: room for a 4096-bit
/// RSA signature, a certificate chain and a timestamp.
pub const DEFAULT_RESERVE: usize = 24 * 1024;

/// A signed signature field of the file.
#[derive(Clone, Debug)]
pub struct DigitalSig {
    /// Fully qualified field name.
    pub field: String,
    /// Page and display rectangle of its widget (`None` if invisible).
    pub page: Option<usize>,
    pub rect: Option<Rect>,
    /// `/SubFilter`: `ETSI.CAdES.detached`, `adbe.pkcs7.detached`, ...
    pub sub_filter: String,
    /// `/ByteRange` as (offset, length) pairs into the file.
    pub byte_range: Vec<(usize, usize)>,
    /// `/Contents`: the CMS (DER), possibly followed by zero padding.
    pub contents: Vec<u8>,
    /// `/Name`, `/M`, `/Reason`, `/Location` as written by the signer.
    pub name: Option<String>,
    pub time: Option<String>,
    pub reason: Option<String>,
    pub location: Option<String>,
}

impl DigitalSig {
    /// End of the revision this signature covers (where its ranges stop).
    pub fn covered_end(&self) -> usize {
        self.byte_range
            .iter()
            .map(|(o, l)| o + l)
            .max()
            .unwrap_or(0)
    }

    /// The signed bytes of `file`, or `None` if the ranges don't fit it or
    /// don't have the shape of a PDF signature (two ranges around the
    /// `/Contents` hole, starting at 0).
    pub fn signed_bytes(&self, file: &[u8]) -> Option<Vec<u8>> {
        let [(o1, l1), (o2, l2)] = self.byte_range[..] else {
            return None;
        };
        if o1 != 0 || o2 < l1 || o2.checked_add(l2)? > file.len() {
            return None;
        }
        let mut v = Vec::with_capacity(l1 + l2);
        v.extend_from_slice(&file[o1..o1 + l1]);
        v.extend_from_slice(&file[o2..o2 + l2]);
        Some(v)
    }
}

/// A signature dictionary as it was in the revision its signature covers:
/// what the signer actually signed, whatever later updates say.
#[derive(Clone, Debug)]
pub struct SignedDict {
    /// The dictionary's object.
    pub id: ObjId,
    /// `/Contents`, decoded.
    pub contents: Vec<u8>,
    pub name: Option<String>,
    pub time: Option<String>,
    pub reason: Option<String>,
    pub location: Option<String>,
}

/// The signature dictionary of `rev` (the revision `sig` covers, read on
/// its own) with `sig`'s `/ByteRange`, if there is exactly one.
pub fn signed_dict(rev: &Document, sig: &DigitalSig) -> Option<SignedDict> {
    let mut found = rev.objects.iter().filter_map(|(id, o)| {
        let d = o.as_dict().ok()?;
        let nums: Vec<i64> = d
            .get(b"ByteRange")
            .ok()
            .and_then(|o| deref(rev, o))?
            .as_array()
            .ok()?
            .iter()
            .filter_map(|o| o.as_i64().ok())
            .collect();
        let ranges: Vec<(usize, usize)> = nums
            .chunks(2)
            .filter_map(|c| {
                Some((
                    usize::try_from(c[0]).ok()?,
                    usize::try_from(*c.get(1)?).ok()?,
                ))
            })
            .collect();
        if ranges != sig.byte_range {
            return None;
        }
        let Ok(Object::String(contents, _)) = d.get(b"Contents") else {
            return None;
        };
        Some(SignedDict {
            id: *id,
            contents: contents.clone(),
            name: get_text(rev, d, b"Name"),
            time: get_text(rev, d, b"M"),
            reason: get_text(rev, d, b"Reason"),
            location: get_text(rev, d, b"Location"),
        })
    });
    let first = found.next()?;
    found.next().is_none().then_some(first)
}

impl FormDoc {
    /// Whether the file holds a digital signature. Changes to it are then
    /// saved as incremental updates so the signature stays valid.
    pub fn is_digitally_signed(&self) -> bool {
        !self.digital.is_empty()
    }
}

/// The file's signed signature fields.
pub(crate) fn read(d: &Document, page_ids: &[ObjId], geoms: &[PageGeom]) -> Vec<DigitalSig> {
    let mut out = Vec::new();
    for (wid, page) in sig_widgets(d, page_ids) {
        let ch = chain(d, wid);
        let Some(v) = inherited(&ch, b"V")
            .and_then(|o| deref(d, o))
            .and_then(|o| o.as_dict().ok())
        else {
            continue;
        };
        let Some(Object::String(contents, _)) = v.get(b"Contents").ok().and_then(|o| deref(d, o))
        else {
            continue;
        };
        let nums: Vec<i64> = v
            .get(b"ByteRange")
            .ok()
            .and_then(|o| deref(d, o))
            .and_then(|o| o.as_array().ok())
            .map(|a| {
                a.iter()
                    .filter_map(|o| deref(d, o)?.as_i64().ok())
                    .collect()
            })
            .unwrap_or_default();
        if nums.len() < 2 || !nums.len().is_multiple_of(2) || nums.iter().any(|n| *n < 0) {
            continue;
        }
        let byte_range = nums
            .chunks(2)
            .map(|c| (c[0] as usize, c[1] as usize))
            .collect();
        let mut names: Vec<String> = ch
            .iter()
            .filter_map(|(_, dict)| get_text(d, dict, b"T"))
            .collect();
        names.reverse();
        let (page, rect) = match page {
            Some(p) => {
                let r = widget_rect(d, wid).map(|r| geoms[p].rect_to_display(r));
                match r {
                    Some(r) if r.width() >= 1.0 && r.height() >= 1.0 => (Some(p), Some(r)),
                    _ => (Some(p), None),
                }
            }
            None => (None, None),
        };
        out.push(DigitalSig {
            field: names.join("."),
            page,
            rect,
            sub_filter: v
                .get(b"SubFilter")
                .ok()
                .and_then(|o| o.as_name().ok())
                .map(|n| String::from_utf8_lossy(n).into_owned())
                .unwrap_or_default(),
            byte_range,
            contents: contents.clone(),
            name: get_text(d, v, b"Name"),
            time: get_text(d, v, b"M"),
            reason: get_text(d, v, b"Reason"),
            location: get_text(d, v, b"Location"),
        });
    }
    out
}

/// Widgets of signature fields: from `/AcroForm /Fields` (so invisible ones
/// count too), with the page each sits on.
pub(crate) fn sig_widgets(d: &Document, page_ids: &[ObjId]) -> Vec<(ObjId, Option<usize>)> {
    let mut on_page = std::collections::HashMap::new();
    for (pi, &pid) in page_ids.iter().enumerate() {
        if let Some(Object::Array(a)) = d
            .get_dictionary(pid)
            .ok()
            .and_then(|p| p.get(b"Annots").ok())
            .and_then(|o| deref(d, o))
        {
            for o in a {
                if let Ok(id) = o.as_reference() {
                    on_page.entry(id).or_insert(pi);
                }
            }
        }
    }
    let fields = d
        .catalog()
        .ok()
        .and_then(|c| c.get(b"AcroForm").ok())
        .and_then(|o| deref(d, o))
        .and_then(|o| o.as_dict().ok())
        .and_then(|f| f.get(b"Fields").ok())
        .and_then(|o| deref(d, o))
        .and_then(|o| o.as_array().ok())
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    let mut stack: Vec<(Object, usize)> = fields.into_iter().map(|o| (o, 0)).collect();
    let mut seen = std::collections::HashSet::new();
    while let Some((o, depth)) = stack.pop() {
        let Ok(id) = o.as_reference() else { continue };
        if depth > 32 || !seen.insert(id) {
            continue;
        }
        let Ok(dict) = d.get_dictionary(id) else {
            continue;
        };
        let kids = dict
            .get(b"Kids")
            .ok()
            .and_then(|o| deref(d, o))
            .and_then(|o| o.as_array().ok());
        match kids {
            Some(k) if !k.is_empty() => {
                stack.extend(k.iter().cloned().map(|o| (o, depth + 1)));
            }
            _ => {
                let ft = inherited(&chain(d, id), b"FT").and_then(|o| o.as_name().ok());
                if ft == Some(b"Sig") {
                    let page = on_page.get(&id).copied().or_else(|| {
                        let p = dict.get(b"P").ok()?.as_reference().ok()?;
                        page_ids.iter().position(|x| *x == p)
                    });
                    out.push((id, page));
                }
            }
        }
    }
    out.sort_by_key(|(id, _)| *id);
    out
}

fn widget_rect(d: &Document, wid: ObjId) -> Option<[f32; 4]> {
    let a = deref(d, d.get_dictionary(wid).ok()?.get(b"Rect").ok()?)?
        .as_array()
        .ok()?;
    let v: Vec<f32> = a
        .iter()
        .filter_map(|o| deref(d, o)?.as_float().ok())
        .collect();
    (v.len() == 4).then(|| [v[0], v[1], v[2], v[3]])
}

// --- Writing a signature -----------------------------------------------------

/// Where and how a new signature appears.
#[derive(Clone, Debug)]
pub struct SignRequest {
    /// Page and display rectangle; `None` for an invisible signature.
    pub spot: Option<(usize, Rect)>,
    /// The handwritten part of the appearance, if any.
    pub ink: Option<Arc<Ink>>,
    /// Signer's name, for the appearance and `/Name`.
    pub signer: String,
    /// Without ink: this name in plain type beside the details, as Adobe
    /// Acrobat shows a certificate signature.
    pub name_beside: Option<String>,
    pub reason: Option<String>,
    pub location: Option<String>,
    /// Signing time, Unix seconds (UTC).
    pub time: u64,
    /// Bytes reserved for the CMS.
    pub reserve: usize,
}

#[derive(Debug)]
pub enum SignError {
    Save(SaveError),
    /// The signing callback failed (wrong PIN, card removed, ...).
    Signer(String),
    /// The signature did not fit the space reserved for it.
    TooLarge {
        needed: usize,
        reserved: usize,
    },
}

impl std::fmt::Display for SignError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SignError::Save(e) => e.fmt(f),
            SignError::Signer(e) => write!(f, "{e}"),
            SignError::TooLarge { needed, reserved } => write!(
                f,
                "the signature needs {needed} bytes but only {reserved} were reserved"
            ),
        }
    }
}

impl From<SaveError> for SignError {
    fn from(e: SaveError) -> Self {
        SignError::Save(e)
    }
}

/// Placeholder for each `/ByteRange` number: wide enough for any offset.
const PH: i64 = 9_999_999_999;

/// Writes `fields` and a new digital signature, returning the signed file.
///
/// `cms` gets the bytes the signature covers and returns the DER-encoded
/// CMS SignedData (detached) to embed.
pub fn sign(
    fd: &FormDoc,
    fields: &[Field],
    req: &SignRequest,
    cms: impl FnOnce(&[u8]) -> Result<Vec<u8>, String>,
) -> Result<Vec<u8>, SignError> {
    sign_with(fd, fields, &fd.markups, req, cms)
}

/// Like [`sign`], with the document's comments and markup now `marks`.
pub fn sign_with(
    fd: &FormDoc,
    fields: &[Field],
    marks: &[crate::markup::Markup],
    req: &SignRequest,
    cms: impl FnOnce(&[u8]) -> Result<Vec<u8>, String>,
) -> Result<Vec<u8>, SignError> {
    // The revision to sign on top of: the edits appended after existing
    // signatures, or (unsigned file) the edits saved as a fresh file.
    let (prev_bytes, prev, mut cur) = if fd.is_digitally_signed() {
        let cur = build_with(fd, fields, marks)?;
        (
            fd.bytes.as_ref().clone(),
            fd.doc.clone().ok_or(SaveError::ReadOnly)?,
            cur,
        )
    } else {
        let bytes = crate::save::save_with(fd, fields, marks)?;
        let doc = Document::load_mem(&bytes).map_err(|e| SaveError::Write(e.to_string()))?;
        let cur = doc.clone();
        (bytes, doc, cur)
    };
    let page_ids: Vec<ObjId> = cur.get_pages().values().copied().collect();
    if page_ids.is_empty() {
        return Err(SaveError::Write("the file has no pages".into()).into());
    }

    let mut sig = dictionary! {
        "Type" => "Sig",
        "Filter" => "Adobe.PPKLite",
        "SubFilter" => "ETSI.CAdES.detached",
        "ByteRange" => vec![0.into(), PH.into(), PH.into(), PH.into()],
        "Contents" => Object::String(vec![0; req.reserve], StringFormat::Hexadecimal),
        "M" => text_string(&pdf_date(req.time)),
        "Prop_Build" => dictionary! {
            "App" => dictionary! { "Name" => "pdfr", "REx" => text_string(env!("CARGO_PKG_VERSION")) },
        },
    };
    if !req.signer.is_empty() {
        sig.set("Name", text_string(&req.signer));
    }
    if let Some(r) = req.reason.as_deref().filter(|s| !s.is_empty()) {
        sig.set("Reason", text_string(r));
    }
    if let Some(l) = req.location.as_deref().filter(|s| !s.is_empty()) {
        sig.set("Location", text_string(l));
    }
    let sig_id = cur.add_object(sig);

    let form = form_id(&mut cur)?;
    // Signed, and changes only as incremental updates.
    if let Ok(f) = cur.get_dictionary_mut(form) {
        f.set("SigFlags", 3);
    }

    let (page, rect) = match req.spot {
        Some((p, r)) if p < page_ids.len() => (p, Some(r)),
        _ => (0, None),
    };
    let page_id = page_ids[page];
    let Some(&geom) = fd.pages.get(page) else {
        return Err(SaveError::Write("the page could not be found".into()).into());
    };
    let ap = rect.map(|r| appearance(&mut cur, req, r, &geom));
    let user = rect.map(|r| geom.rect_to_user(&r)).unwrap_or([0.0; 4]);
    // Locked (128) and printed (4).
    let flags = 132;

    match rect.and_then(|r| empty_sig_widget(&cur, &page_ids, page, &geom, &r)) {
        Some(wid) => {
            // The file's own "sign here" field.
            let field_id = chain(&cur, wid)
                .iter()
                .find(|(_, d)| d.has(b"T"))
                .map_or(wid, |(id, _)| *id);
            if let Ok(f) = cur.get_dictionary_mut(field_id) {
                f.set("V", Object::Reference(sig_id));
            }
            if let Ok(w) = cur.get_dictionary_mut(wid) {
                if let Some(ap) = ap {
                    w.set("AP", dictionary! { "N" => Object::Reference(ap) });
                }
                w.set("F", flags);
            }
        }
        None => {
            let name = unique_name(&cur, form, "Signature");
            let mut w = dictionary! {
                "Type" => "Annot",
                "Subtype" => "Widget",
                "FT" => "Sig",
                "T" => text_string(&name),
                "Rect" => user.iter().map(|v| Object::Real(*v)).collect::<Vec<_>>(),
                "F" => flags,
                "P" => Object::Reference(page_id),
                "V" => Object::Reference(sig_id),
            };
            if let Some(ap) = ap {
                w.set("AP", dictionary! { "N" => Object::Reference(ap) });
            }
            let wid = cur.add_object(w);
            push_ref(&mut cur, page_id, b"Annots", wid);
            push_ref(&mut cur, form, b"Fields", wid);
        }
    }

    let out = append_update(&prev_bytes, &prev, &cur)?;
    fill_placeholder(out, prev_bytes.len(), req.reserve, cms)
}

/// Patches `/ByteRange` in the update starting at `start` of `out`, hands
/// the covered bytes to `cms` and writes what it returns into the
/// `/Contents` placeholder of `reserve` bytes.
fn fill_placeholder(
    mut out: Vec<u8>,
    start: usize,
    reserve: usize,
    cms: impl FnOnce(&[u8]) -> Result<Vec<u8>, String>,
) -> Result<Vec<u8>, SignError> {
    // Find the placeholders in the update.
    let hole = {
        let mut n = vec![b'<'];
        n.extend(std::iter::repeat_n(b'0', reserve * 2));
        n.push(b'>');
        find(&out[start..], &n).map(|i| (start + i, start + i + n.len()))
    };
    let br_needle = format!("0 {PH} {PH} {PH}");
    let br = find(&out[start..], br_needle.as_bytes()).map(|i| start + i);
    let (Some((a, b)), Some(br)) = (hole, br) else {
        return Err(SaveError::Write("signature placeholder not found".into()).into());
    };
    let len = out.len();
    let ranges = format!("0 {} {} {}", a, b, len - b);
    let mut padded = ranges.into_bytes();
    padded.resize(br_needle.len(), b' ');
    out[br..br + br_needle.len()].copy_from_slice(&padded);

    let mut signed = Vec::with_capacity(len - (b - a));
    signed.extend_from_slice(&out[..a]);
    signed.extend_from_slice(&out[b..]);
    let der = cms(&signed).map_err(SignError::Signer)?;
    if der.len() > reserve {
        return Err(SignError::TooLarge {
            needed: der.len(),
            reserved: reserve,
        });
    }
    let hex: String = der.iter().map(|x| format!("{x:02X}")).collect();
    out[a + 1..a + 1 + hex.len()].copy_from_slice(hex.as_bytes());
    Ok(out)
}

// --- Long-term validation ------------------------------------------------------

/// Bytes reserved for a document timestamp's token.
pub const TIMESTAMP_RESERVE: usize = 16 * 1024;

/// Validation data for the Document Security Store: DER certificates,
/// OCSP responses and CRLs.
#[derive(Clone, Debug, Default)]
pub struct DssData {
    pub certs: Vec<Vec<u8>>,
    pub ocsps: Vec<Vec<u8>>,
    pub crls: Vec<Vec<u8>>,
}

/// Appends an update to the signed file `bytes` adding `data` to its
/// Document Security Store (`/DSS`, PAdES), keeping what is there.
pub fn add_dss(bytes: &[u8], data: &DssData) -> Result<Vec<u8>, SaveError> {
    let prev = Document::load_mem(bytes).map_err(|e| SaveError::Write(e.to_string()))?;
    let mut cur = prev.clone();
    let existing = cur.catalog().ok().and_then(|c| c.get(b"DSS").ok()).cloned();
    let dss_id = existing.as_ref().and_then(|o| o.as_reference().ok());
    let mut dss = existing
        .as_ref()
        .and_then(|o| deref(&cur, o))
        .and_then(|o| o.as_dict().ok())
        .cloned()
        .unwrap_or_default();
    for (key, items) in [
        (&b"Certs"[..], &data.certs),
        (b"OCSPs", &data.ocsps),
        (b"CRLs", &data.crls),
    ] {
        let mut arr: Vec<Object> = dss
            .get(key)
            .ok()
            .and_then(|o| deref(&cur, o))
            .and_then(|o| o.as_array().ok())
            .cloned()
            .unwrap_or_default();
        let mut have: Vec<Vec<u8>> = arr
            .iter()
            .filter_map(|o| deref(&cur, o)?.as_stream().ok()?.get_plain_content().ok())
            .collect();
        for item in items {
            if !have.contains(item) {
                have.push(item.clone());
                let id = cur.add_object(Stream::new(Dictionary::new(), item.clone()));
                arr.push(Object::Reference(id));
            }
        }
        if !arr.is_empty() {
            dss.set(key, arr);
        }
    }
    dss.set("Type", "DSS");
    let id = match dss_id {
        Some(id) => {
            cur.objects.insert(id, Object::Dictionary(dss));
            id
        }
        None => cur.add_object(dss),
    };
    cur.catalog_mut()
        .map_err(|e| SaveError::Write(e.to_string()))?
        .set("DSS", Object::Reference(id));
    append_update(bytes, &prev, &cur)
}

/// Appends a document timestamp (PAdES, `ETSI.RFC3161`) to `bytes`: an
/// invisible signature field whose `/Contents` is the RFC 3161 token
/// `tst` returns for the bytes it covers.
pub fn doc_timestamp(
    bytes: &[u8],
    reserve: usize,
    tst: impl FnOnce(&[u8]) -> Result<Vec<u8>, String>,
) -> Result<Vec<u8>, SignError> {
    let prev = Document::load_mem(bytes).map_err(|e| SaveError::Write(e.to_string()))?;
    let mut cur = prev.clone();
    let sig_id = cur.add_object(dictionary! {
        "Type" => "DocTimeStamp",
        "Filter" => "Adobe.PPKLite",
        "SubFilter" => "ETSI.RFC3161",
        "ByteRange" => vec![0.into(), PH.into(), PH.into(), PH.into()],
        "Contents" => Object::String(vec![0; reserve], StringFormat::Hexadecimal),
    });
    let form = form_id(&mut cur)?;
    let name = unique_name(&cur, form, "Timestamp");
    let wid = cur.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Widget",
        "FT" => "Sig",
        "T" => text_string(&name),
        "Rect" => vec![0.into(), 0.into(), 0.into(), 0.into()],
        "F" => 132,
        "V" => Object::Reference(sig_id),
    });
    push_ref(&mut cur, form, b"Fields", wid);
    let out = append_update(bytes, &prev, &cur)?;
    fill_placeholder(out, bytes.len(), reserve, tst)
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn form_id(d: &mut Document) -> Result<ObjId, SaveError> {
    let cat = d.catalog().map_err(|e| SaveError::Write(e.to_string()))?;
    let id = match cat.get(b"AcroForm") {
        Ok(Object::Reference(id)) => return Ok(*id),
        Ok(Object::Dictionary(dict)) => {
            let dict = dict.clone();
            d.add_object(dict)
        }
        _ => d.add_object(dictionary! { "Fields" => Vec::<Object>::new() }),
    };
    d.catalog_mut()
        .map_err(|e| SaveError::Write(e.to_string()))?
        .set("AcroForm", Object::Reference(id));
    Ok(id)
}

/// Appends `id` to the array `key` of dictionary `owner` (the array may be
/// direct or a reference).
fn push_ref(d: &mut Document, owner: ObjId, key: &[u8], id: ObjId) {
    let arr_ref = d
        .get_dictionary(owner)
        .ok()
        .and_then(|p| p.get(key).ok())
        .and_then(|o| o.as_reference().ok());
    if let Some(aid) = arr_ref {
        if let Ok(Object::Array(a)) = d.get_object_mut(aid) {
            a.push(Object::Reference(id));
        }
    } else if let Ok(p) = d.get_dictionary_mut(owner) {
        match p.get_mut(key) {
            Ok(Object::Array(a)) => a.push(Object::Reference(id)),
            _ => p.set(key, vec![Object::Reference(id)]),
        }
    }
}

/// `<prefix>N`, not yet a top-level field name.
fn unique_name(d: &Document, form: ObjId, prefix: &str) -> String {
    let taken: Vec<String> = d
        .get_dictionary(form)
        .ok()
        .and_then(|f| f.get(b"Fields").ok())
        .and_then(|o| deref(d, o))
        .and_then(|o| o.as_array().ok())
        .map(|a| {
            a.iter()
                .filter_map(|o| deref(d, o)?.as_dict().ok())
                .filter_map(|f| get_text(d, f, b"T"))
                .collect()
        })
        .unwrap_or_default();
    (1..)
        .map(|n| format!("{prefix}{n}"))
        .find(|n| !taken.contains(n))
        .unwrap()
}

/// An unsigned signature widget on `page` overlapping `r` well.
fn empty_sig_widget(
    d: &Document,
    page_ids: &[ObjId],
    page: usize,
    geom: &PageGeom,
    r: &Rect,
) -> Option<ObjId> {
    sig_widgets(d, page_ids)
        .into_iter()
        .filter(|(_, p)| *p == Some(page))
        .find(|(wid, _)| {
            let signed = inherited(&chain(d, *wid), b"V").is_some();
            !signed
                && widget_rect(d, *wid)
                    .map(|u| geom.rect_to_display(u))
                    .is_some_and(|wr| wr.overlap_ratio(r) > 0.3 || r.overlap_ratio(&wr) > 0.3)
        })
        .map(|(w, _)| w)
}

/// Appearance of a visible signature: the handwriting on the left (if
/// any), and who signed and when on the right.
fn appearance(d: &mut Document, req: &SignRequest, r: Rect, geom: &PageGeom) -> ObjId {
    let (w, h) = (r.width(), r.height());
    let font_id = ensure_font(d);
    let mut res = match &req.ink {
        Some(ink) => crate::signature::resources(d, ink),
        None => Dictionary::new(),
    };
    let mut content = String::new();
    let text_x = match &req.ink {
        Some(ink) if !ink.is_empty() => {
            let iw = if w / h > 2.2 { w * 0.48 } else { w };
            content.push_str(&crate::signature::appearance(ink, iw, h));
            if iw >= w { None } else { Some(iw + h * 0.08) }
        }
        _ if req.name_beside.is_some() && w / h > 2.2 => {
            let name = req.name_beside.as_deref().unwrap_or_default();
            name_block(&mut content, name, w * 0.46, h);
            // A thin rule between the name and the details.
            let x = w * 0.5;
            let _ = writeln!(
                content,
                "q 0.6 0.6 0.6 RG 0.5 w {} {} m {} {} l S Q",
                num(x),
                num(h * 0.12),
                num(x),
                num(h * 0.88)
            );
            Some(x + h * 0.1)
        }
        _ => Some(h * 0.08),
    };
    if let Some(x0) = text_x {
        let mut lines = vec!["Digitally signed by".to_string(), req.signer.clone()];
        lines.push(format!("Date: {}", display_date(req.time)));
        if let Some(reason) = req.reason.as_deref().filter(|s| !s.is_empty()) {
            lines.push(format!("Reason: {reason}"));
        }
        let f = crate::font::font();
        let avail_w = (w - x0 - h * 0.06).max(1.0);
        let widest = lines
            .iter()
            .map(|l| f.text_width(l, 1.0))
            .fold(0.0f32, f32::max)
            .max(1e-3);
        let size = (h * 0.9 / (lines.len() as f32 * 1.2))
            .min(avail_w / widest)
            .clamp(2.0, 12.0);
        let lead = size * 1.2;
        let top = (h + lead * lines.len() as f32) / 2.0;
        let _ = writeln!(content, "BT 0.15 0.15 0.15 rg /{FONT_RES} {} Tf", num(size));
        for (i, l) in lines.iter().enumerate() {
            let y = top - lead * (i as f32 + 1.0) + (lead - size) / 2.0 + size * 0.2;
            let hex: String = l
                .chars()
                .map(|c| format!("{:04X}", f.glyph_id(c)))
                .collect();
            let _ = writeln!(content, "1 0 0 1 {} {} Tm <{hex}> Tj", num(x0), num(y));
        }
        content.push_str("ET\n");
    }
    let mut fonts = Dictionary::new();
    fonts.set(FONT_RES, Object::Reference(font_id));
    res.set("Font", Object::Dictionary(fonts));
    let mut dict = dictionary! {
        "Type" => "XObject",
        "Subtype" => "Form",
        "BBox" => vec![0.into(), 0.into(), Object::Real(w), Object::Real(h)],
        "Resources" => res,
    };
    if let Some(m) = rotation_matrix(geom.rotate) {
        dict.set(
            "Matrix",
            m.iter().map(|v| Object::Real(*v)).collect::<Vec<_>>(),
        );
    }
    d.add_object(Stream::new(dict, content.into_bytes()))
}

/// The signer's name in plain type, as large as fits `w`×`h` at the
/// left; on two lines when that makes it larger.
fn name_block(content: &mut String, name: &str, w: f32, h: f32) {
    let f = crate::font::font();
    let pad = h * 0.08;
    let (aw, ah) = ((w - pad).max(1.0), h - 2.0 * pad);
    let fit = |lines: &[String]| {
        let widest = lines
            .iter()
            .map(|l| f.text_width(l, 1.0))
            .fold(0.0f32, f32::max)
            .max(1e-3);
        (ah / (lines.len() as f32 * 1.15))
            .min(aw / widest)
            .min(h * 0.3)
    };
    let one = vec![name.to_string()];
    let words: Vec<&str> = name.split_whitespace().collect();
    let mut lines = one.clone();
    if words.len() > 1 {
        // The split with the shortest longer line.
        let two = (1..words.len())
            .map(|k| vec![words[..k].join(" "), words[k..].join(" ")])
            .max_by(|a, b| fit(a).total_cmp(&fit(b)))
            .unwrap();
        if fit(&two) > fit(&one) * 1.25 {
            lines = two;
        }
    }
    let size = fit(&lines).max(2.0);
    let lead = size * 1.15;
    let top = (h + lead * lines.len() as f32) / 2.0;
    let _ = writeln!(content, "BT 0.1 0.1 0.1 rg /{FONT_RES} {} Tf", num(size));
    for (i, l) in lines.iter().enumerate() {
        let y = top - lead * (i as f32 + 1.0) + (lead - size) / 2.0 + size * 0.2;
        let hex: String = l
            .chars()
            .map(|c| format!("{:04X}", f.glyph_id(c)))
            .collect();
        let _ = writeln!(content, "1 0 0 1 {} {} Tm <{hex}> Tj", num(pad), num(y));
    }
    content.push_str("ET\n");
}

/// Width over height of a good box for `req`'s appearance: the
/// handwriting's shape plus room for the details beside it.
pub fn appearance_aspect(req: &SignRequest) -> f32 {
    match &req.ink {
        Some(ink) if !ink.is_empty() => (ink.aspect / 0.48).clamp(2.6, 7.0),
        _ if req.name_beside.is_some() => 4.0,
        _ => 3.4,
    }
}

/// The appearance of `req` drawn into a `w`×`h` pixel image with a
/// transparent background, by the same code that writes it into the PDF.
pub fn preview(req: &SignRequest, w: u32, h: u32) -> crate::render::Image {
    let (w, h) = (w.clamp(8, 4096), h.clamp(8, 4096));
    let mut img = crate::render::Image {
        width: w,
        height: h,
        rgba: vec![0; (w * h * 4) as usize],
    };
    let mut d = Document::with_version("1.7");
    let geom = PageGeom::new([0.0, 0.0, w as f32, h as f32], 0);
    let ap = appearance(&mut d, req, Rect::new(0.0, 0.0, w as f32, h as f32), &geom);
    let pages_id = d.new_object_id();
    let cid = d.add_object(Stream::new(Dictionary::new(), b"q /Ap Do Q".to_vec()));
    let page = d.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => Object::Reference(pages_id),
        "MediaBox" => vec![0.into(), 0.into(), (w as i64).into(), (h as i64).into()],
        "Contents" => Object::Reference(cid),
        "Resources" => dictionary! { "XObject" => dictionary! { "Ap" => Object::Reference(ap) } },
    });
    d.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1,
        }),
    );
    let cat =
        d.add_object(dictionary! { "Type" => "Catalog", "Pages" => Object::Reference(pages_id) });
    d.trailer.set("Root", Object::Reference(cat));
    let mut bytes = Vec::new();
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
    // Un-blend from the white paper: how much darker than white a pixel is
    // gives its coverage, and the colour is recovered from that.
    for (o, px) in img
        .rgba
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(white.rgba.as_chunks::<4>().0)
    {
        let a = 255 - px[0].min(px[1]).min(px[2]);
        if a == 0 {
            continue;
        }
        let af = a as f32 / 255.0;
        let un = |c: u8| ((c as f32 - 255.0 * (1.0 - af)) / af).clamp(0.0, 255.0) as u8;
        o.copy_from_slice(&[un(px[0]), un(px[1]), un(px[2]), a]);
    }
    img
}

// --- Dates -------------------------------------------------------------------

/// (year, month, day, hour, minute, second) in UTC.
fn civil(t: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = (t / 86_400) as i64;
    let secs = t % 86_400;
    // Howard Hinnant's days-to-civil.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (
        y,
        m,
        d,
        (secs / 3600) as u32,
        (secs / 60 % 60) as u32,
        (secs % 60) as u32,
    )
}

/// A PDF date string (`D:YYYYMMDDHHmmSSZ`).
pub fn pdf_date(t: u64) -> String {
    let (y, mo, d, h, mi, s) = civil(t);
    format!("D:{y:04}{mo:02}{d:02}{h:02}{mi:02}{s:02}Z")
}

/// `2026-11-20 14:03 UTC`.
pub fn display_date(t: u64) -> String {
    let (y, mo, d, h, mi, _) = civil(t);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02} UTC")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(pdf_date(0), "D:19700101000000Z");
        // 2026-11-20 13:45:10 UTC
        assert_eq!(pdf_date(1_795_182_310), "D:20261120134510Z");
        assert_eq!(display_date(1_795_182_310), "2026-11-20 13:45 UTC");
        // Leap day.
        assert_eq!(pdf_date(951_782_400), "D:20000229000000Z");
    }
}
