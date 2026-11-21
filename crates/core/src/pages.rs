//! Organising pages: moving, deleting, inserting blank pages, pages of
//! another PDF or a picture, and extracting pages to a new file.
//!
//! Every change returns a new [`FormDoc`], like turning pages does. Object
//! ids are kept, so a page (and every field on it) is the same object before
//! and after: the app follows pages by their ids. The page tree is written
//! flat (one `/Pages` node), with inherited page attributes copied onto each
//! page first.
//!
//! A digitally signed file is rewritten without its signatures: they could
//! not stay valid once pages change. Fields of an inserted PDF stay fields
//! (renamed when a name is taken, so they stay separate from ours).

use crate::field::{Field, ObjId};
use crate::forms::{FormDoc, chain, deref, inherited};
use lopdf::{Dictionary, Document, Object, Stream, dictionary, text_string};
use std::collections::{HashMap, HashSet};

/// Page attributes a page may inherit from its parents.
const INHERITED: [&[u8]; 4] = [b"Resources", b"MediaBox", b"CropBox", b"Rotate"];

/// Size of a page made for a picture (A4, in points) and its margin.
const A4: (f32, f32) = (595.0, 842.0);
const IMAGE_MARGIN: f32 = 36.0;

type Result<T> = std::result::Result<T, String>;

impl FormDoc {
    /// A copy with its pages in `order` (old page indices). Pages left out
    /// are deleted, with their fields.
    pub fn arranged(&self, order: &[usize]) -> Result<FormDoc> {
        self.pages_edit(|_, ids, old| {
            *ids = order
                .iter()
                .map(|&i| {
                    old.get(i)
                        .copied()
                        .ok_or_else(|| "no such page".to_string())
                })
                .collect::<Result<_>>()?;
            Ok(())
        })
    }

    /// A copy with a blank page at `at`, the same size as page `like`.
    pub fn with_blank_page(&self, at: usize, like: usize) -> Result<FormDoc> {
        self.pages_edit(|d, ids, old| {
            let src = old.get(like).ok_or("no such page")?;
            let page = d.get_dictionary(*src).map_err(|e| e.to_string())?;
            let mut p = dictionary! { "Type" => "Page", "Resources" => Dictionary::new() };
            for k in [&b"MediaBox"[..], b"CropBox", b"Rotate"] {
                if let Ok(v) = page.get(k) {
                    p.set(k, v.clone());
                }
            }
            let contents = d.add_object(Stream::new(Dictionary::new(), vec![]));
            p.set("Contents", Object::Reference(contents));
            let id = d.add_object(p);
            ids.insert(at.min(ids.len()), id);
            Ok(())
        })
    }

    /// A copy with every page of the PDF `bytes` inserted at `at`.
    pub fn with_pdf(&self, at: usize, bytes: &[u8]) -> Result<FormDoc> {
        self.pages_edit(|d, ids, _| {
            let new = import_pdf(d, bytes)?;
            let at = at.min(ids.len());
            ids.splice(at..at, new);
            Ok(())
        })
    }

    /// A copy with a page showing the picture `bytes` (PNG or JPEG) at
    /// `at`: an A4 page (turned sideways for a wide picture) with the
    /// picture as large as fits, centred.
    pub fn with_image(&self, at: usize, bytes: &[u8]) -> Result<FormDoc> {
        self.pages_edit(|d, ids, _| {
            let id = image_page(d, bytes)?;
            ids.insert(at.min(ids.len()), id);
            Ok(())
        })
    }

    /// Applies `f` to a copy of the document and its page list (`f` gets
    /// the list to change and the original), then reopens it.
    fn pages_edit(
        &self,
        f: impl FnOnce(&mut Document, &mut Vec<ObjId>, &[ObjId]) -> Result<()>,
    ) -> Result<FormDoc> {
        let doc = self.doc.as_ref().ok_or("this file cannot be edited")?;
        let mut d = doc.clone();
        prepare(&mut d, &self.page_ids, self.is_digitally_signed());
        let mut ids = self.page_ids.clone();
        f(&mut d, &mut ids, &self.page_ids)?;
        finish(&mut d, &self.page_ids, &ids)?;
        let mut out = Vec::new();
        d.save_to(&mut out).map_err(|e| e.to_string())?;
        let mut fd = FormDoc::open(out).map_err(|e| e.to_string())?;
        fd.was_protected = self.was_protected;
        Ok(fd)
    }
}

/// A new PDF of `pages` (in that order) of `fd`, with `fields` filled in.
pub fn extract(fd: &FormDoc, fields: &[Field], pages: &[usize]) -> Result<Vec<u8>> {
    let mut d = crate::save::build(fd, fields).map_err(|e| e.to_string())?;
    prepare(&mut d, &fd.page_ids, fd.is_digitally_signed());
    let ids: Vec<ObjId> = pages
        .iter()
        .map(|&i| {
            fd.page_ids
                .get(i)
                .copied()
                .ok_or_else(|| "no such page".to_string())
        })
        .collect::<Result<_>>()?;
    // Bookmarks and the structure tree point at pages left behind.
    if let Ok(cat) = d.catalog_mut() {
        for k in [
            &b"Outlines"[..],
            b"StructTreeRoot",
            b"OpenAction",
            b"PageLabels",
        ] {
            cat.remove(k);
        }
    }
    finish(&mut d, &fd.page_ids, &ids)?;
    let mut out = Vec::new();
    d.save_to(&mut out).map_err(|e| e.to_string())?;
    Ok(out)
}

/// Gets `d` ready for page changes: signatures removed if `signed`, the
/// form dictionary indirect, inherited attributes on every page.
fn prepare(d: &mut Document, page_ids: &[ObjId], signed: bool) {
    if signed {
        remove_digital_signatures(d, page_ids);
    }
    let has_form = d.catalog().is_ok_and(|c| c.has(b"AcroForm"));
    if has_form {
        let _ = crate::save::acroform_id(d);
    }
    for &id in page_ids {
        pin_inherited(d, id);
    }
}

/// Writes the page tree for `new`, removes the fields of pages no longer
/// in it, and drops objects nothing refers to any more.
fn finish(d: &mut Document, old: &[ObjId], new: &[ObjId]) -> Result<()> {
    if new.is_empty() {
        return Err("a PDF needs at least one page".into());
    }
    let keep: HashSet<ObjId> = new.iter().copied().collect();
    for &p in old.iter().filter(|p| !keep.contains(p)) {
        for w in widgets(d, p) {
            remove_field_widget(d, w);
        }
    }
    let root = d
        .catalog()
        .and_then(|c| c.get(b"Pages"))
        .and_then(|o| o.as_reference())
        .map_err(|e| e.to_string())?;
    let kids: Vec<Object> = new.iter().map(|&id| Object::Reference(id)).collect();
    let tree = d.get_dictionary_mut(root).map_err(|e| e.to_string())?;
    tree.set("Type", "Pages");
    tree.set("Kids", kids);
    tree.set("Count", new.len() as i64);
    tree.remove(b"Parent");
    for &id in new {
        if let Ok(p) = d.get_dictionary_mut(id) {
            p.set("Parent", Object::Reference(root));
        }
    }
    d.prune_objects();
    Ok(())
}

/// Copies attributes page `id` inherits from its parents onto the page.
fn pin_inherited(d: &mut Document, id: ObjId) {
    let mut found: Vec<(&[u8], Object)> = Vec::new();
    {
        let Ok(page) = d.get_dictionary(id) else {
            return;
        };
        for key in INHERITED {
            if page.has(key) {
                continue;
            }
            let mut cur = page.get(b"Parent").and_then(|o| o.as_reference()).ok();
            let mut seen = HashSet::new();
            while let Some(pid) = cur {
                if !seen.insert(pid) {
                    break;
                }
                let Ok(node) = d.get_dictionary(pid) else {
                    break;
                };
                if let Ok(v) = node.get(key) {
                    found.push((key, v.clone()));
                    break;
                }
                cur = node.get(b"Parent").and_then(|o| o.as_reference()).ok();
            }
        }
    }
    if let Ok(page) = d.get_dictionary_mut(id) {
        let has_resources = page.has(b"Resources") || found.iter().any(|(k, _)| *k == b"Resources");
        for (k, v) in found {
            page.set(k, v);
        }
        if !has_resources {
            page.set("Resources", Dictionary::new());
        }
    }
}

/// The widget annotations on page `page`.
fn widgets(d: &Document, page: ObjId) -> Vec<ObjId> {
    let Some(Object::Array(annots)) = d
        .get_dictionary(page)
        .ok()
        .and_then(|p| p.get(b"Annots").ok())
        .and_then(|o| deref(d, o))
    else {
        return vec![];
    };
    annots
        .iter()
        .filter_map(|a| a.as_reference().ok())
        .filter(|&id| {
            d.get_dictionary(id).is_ok_and(|w| {
                w.get(b"Subtype")
                    .and_then(|s| s.as_name())
                    .is_ok_and(|s| s == b"Widget")
            })
        })
        .collect()
}

/// The `/AcroForm` dictionary's id, if the file has one as an object.
fn form_id(d: &Document) -> Option<ObjId> {
    d.catalog().ok()?.get(b"AcroForm").ok()?.as_reference().ok()
}

/// Removes references to `target` from the array at `owner`'s `key`
/// (inline or indirect). Returns how many are left.
fn remove_from_array(d: &mut Document, owner: ObjId, key: &[u8], target: ObjId) -> usize {
    let is = |o: &Object| o.as_reference().ok() == Some(target);
    let indirect = d
        .get_dictionary(owner)
        .ok()
        .and_then(|o| o.get(key).ok())
        .and_then(|o| o.as_reference().ok());
    let arr = match indirect {
        Some(a) => d.get_object_mut(a).ok().and_then(|o| o.as_array_mut().ok()),
        None => d
            .get_dictionary_mut(owner)
            .ok()
            .and_then(|o| o.get_mut(key).ok())
            .and_then(|o| o.as_array_mut().ok()),
    };
    match arr {
        Some(a) => {
            a.retain(|o| !is(o));
            a.len()
        }
        None => 0,
    }
}

/// Takes widget `w` out of the field tree: from its parent's `/Kids`, and
/// the parent too when it has no widgets left.
fn remove_field_widget(d: &mut Document, w: ObjId) {
    let mut cur = w;
    for _ in 0..32 {
        let parent = d
            .get_dictionary(cur)
            .ok()
            .and_then(|o| o.get(b"Parent").ok())
            .and_then(|o| o.as_reference().ok());
        match parent {
            Some(p) => {
                if remove_from_array(d, p, b"Kids", cur) > 0 {
                    return;
                }
                cur = p;
            }
            None => {
                if let Some(form) = form_id(d) {
                    remove_from_array(d, form, b"Fields", cur);
                }
                return;
            }
        }
    }
}

/// Removes signed signature fields (and the permissions that rely on
/// them), so a changed file does not carry signatures that no longer hold.
fn remove_digital_signatures(d: &mut Document, page_ids: &[ObjId]) {
    let signed: Vec<ObjId> = crate::digital::sig_widgets(d, page_ids)
        .into_iter()
        .map(|(w, _)| w)
        .filter(|&w| inherited(&chain(d, w), b"V").is_some())
        .collect();
    for &w in &signed {
        for &p in page_ids {
            remove_from_array(d, p, b"Annots", w);
        }
        remove_field_widget(d, w);
    }
    if let Ok(cat) = d.catalog_mut() {
        cat.remove(b"Perms");
    }
    let form = d
        .catalog()
        .ok()
        .and_then(|c| c.get(b"AcroForm").ok().cloned());
    match form {
        Some(Object::Reference(id)) => {
            if let Ok(f) = d.get_dictionary_mut(id) {
                f.remove(b"SigFlags");
            }
        }
        Some(Object::Dictionary(_)) => {
            if let Ok(Object::Dictionary(f)) = d.catalog_mut().and_then(|c| c.get_mut(b"AcroForm"))
            {
                f.remove(b"SigFlags");
            }
        }
        _ => {}
    }
}

/// Copies every page of the PDF `bytes` into `d` with what they use
/// (fonts, images, fields) and returns the new pages' ids.
fn import_pdf(d: &mut Document, bytes: &[u8]) -> Result<Vec<ObjId>> {
    let mut o = Document::load_mem(bytes).map_err(|_| "not a readable PDF file".to_string())?;
    if o.is_encrypted() {
        return Err("that PDF is protected with a password".into());
    }
    o.renumber_objects_with(d.max_id + 1);
    let pages: Vec<ObjId> = o.get_pages().values().copied().collect();
    if pages.is_empty() {
        return Err("that PDF has no pages".into());
    }
    for &p in &pages {
        pin_inherited(&mut o, p);
    }
    let other_form = o
        .catalog()
        .ok()
        .and_then(|c| c.get(b"AcroForm").ok())
        .and_then(|f| deref(&o, f))
        .and_then(|f| f.as_dict().ok())
        .cloned();
    let dr = other_form.as_ref().and_then(|f| f.get(b"DR").ok()).cloned();
    let mut roots: Vec<Object> = pages.iter().map(|&p| Object::Reference(p)).collect();
    roots.extend(dr.clone());
    for id in reachable(&o, &roots) {
        if let Some(obj) = o.objects.remove(&id) {
            d.objects.insert(id, obj);
        }
    }
    d.max_id = d.max_id.max(o.max_id);
    // The new pages' fields join the form, under names not taken yet.
    let tops = top_fields(d, &pages);
    if !tops.is_empty() {
        let form = crate::save::acroform_id(d).map_err(|e| e.to_string())?;
        let mut taken = crate::save::field_names(d, form);
        for top in tops {
            rename_if_taken(d, top, &mut taken);
            if let Ok(f) = d.get_dictionary_mut(form) {
                match f.get_mut(b"Fields") {
                    Ok(Object::Array(a)) => a.push(Object::Reference(top)),
                    Ok(Object::Reference(_)) => {}
                    _ => f.set("Fields", vec![Object::Reference(top)]),
                }
            }
            let fields_ref = d
                .get_dictionary(form)
                .ok()
                .and_then(|f| f.get(b"Fields").ok())
                .and_then(|o| o.as_reference().ok());
            if let Some(a) = fields_ref
                && let Ok(Object::Array(a)) = d.get_object_mut(a)
            {
                a.push(Object::Reference(top));
            }
        }
        merge_fonts(d, form, other_form.as_ref());
    }
    Ok(pages)
}

/// Objects reachable from `roots`, not following page parents or an
/// annotation's page (`/P`), which lead to the rest of the document.
fn reachable(d: &Document, roots: &[Object]) -> Vec<ObjId> {
    fn refs(o: &Object, out: &mut Vec<ObjId>) {
        match o {
            Object::Reference(id) => out.push(*id),
            Object::Array(a) => a.iter().for_each(|x| refs(x, out)),
            Object::Dictionary(dict) => dict_refs(dict, out),
            Object::Stream(s) => dict_refs(&s.dict, out),
            _ => {}
        }
    }
    fn dict_refs(dict: &Dictionary, out: &mut Vec<ObjId>) {
        let ty = dict.get(b"Type").and_then(|t| t.as_name()).ok();
        let page_node = matches!(ty, Some(b"Page" | b"Pages"));
        let annot = dict.has(b"Subtype") && dict.has(b"Rect");
        for (k, v) in dict.iter() {
            if (page_node && k == b"Parent") || (annot && k == b"P") {
                continue;
            }
            refs(v, out);
        }
    }
    let mut stack = Vec::new();
    roots.iter().for_each(|r| refs(r, &mut stack));
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        if let Ok(obj) = d.get_object(id) {
            out.push(id);
            refs(obj, &mut stack);
        }
    }
    out
}

/// The top-level fields of the widgets on `pages`, in page order.
fn top_fields(d: &Document, pages: &[ObjId]) -> Vec<ObjId> {
    let mut out = Vec::new();
    for &p in pages {
        for w in widgets(d, p) {
            let ch = chain(d, w);
            let top = ch.last().map(|(id, _)| *id).unwrap_or(w);
            let is_field = ch.iter().any(|(_, x)| x.has(b"FT") || x.has(b"T"));
            if is_field && !out.contains(&top) {
                out.push(top);
            }
        }
    }
    out
}

/// Gives field `id` a new `/T` if its name is in `taken` ("name 2", ...).
fn rename_if_taken(d: &mut Document, id: ObjId, taken: &mut HashSet<String>) {
    let Some(name) = d
        .get_dictionary(id)
        .ok()
        .and_then(|f| f.get(b"T").ok())
        .and_then(|t| lopdf::decode_text_string(t).ok())
    else {
        return;
    };
    let mut new = name.clone();
    let mut n = 2;
    while taken.contains(&new) {
        new = format!("{name} {n}");
        n += 1;
    }
    if new != name
        && let Ok(f) = d.get_dictionary_mut(id)
    {
        f.set("T", text_string(&new));
    }
    taken.insert(new);
}

/// Adds the inserted file's form fonts (`/DR /Font`) that ours lacks, so
/// its fields' `/DA` keep working.
fn merge_fonts(d: &mut Document, form: ObjId, other: Option<&Dictionary>) {
    let fonts_of = |d: &Document, dr: Option<&Object>| -> Dictionary {
        dr.and_then(|o| deref(d, o))
            .and_then(|o| o.as_dict().ok())
            .and_then(|dr| dr.get(b"Font").ok())
            .and_then(|o| deref(d, o))
            .and_then(|o| o.as_dict().ok())
            .cloned()
            .unwrap_or_default()
    };
    let theirs = fonts_of(d, other.and_then(|f| f.get(b"DR").ok()));
    if theirs.is_empty() {
        return;
    }
    let our_dr: Option<Object> = d
        .get_dictionary(form)
        .ok()
        .and_then(|f| f.get(b"DR").ok())
        .cloned();
    let mut fonts = fonts_of(d, our_dr.as_ref());
    for (k, v) in theirs.iter() {
        if !fonts.has(k) {
            fonts.set(k.clone(), v.clone());
        }
    }
    let mut dr = our_dr
        .as_ref()
        .and_then(|o| deref(d, o))
        .and_then(|o| o.as_dict().ok())
        .cloned()
        .unwrap_or_default();
    dr.set("Font", Object::Dictionary(fonts));
    if let Ok(f) = d.get_dictionary_mut(form) {
        f.set("DR", Object::Dictionary(dr));
        if !f.has(b"DA")
            && let Some(da) = other.and_then(|o| o.get(b"DA").ok())
        {
            f.set("DA", da.clone());
        }
    }
}

/// A new page showing the picture `bytes`; returns its id.
fn image_page(d: &mut Document, bytes: &[u8]) -> Result<ObjId> {
    let (img, w, h) = image_xobject(d, bytes)?;
    let (pw, ph) = if w > h { (A4.1, A4.0) } else { A4 };
    let s = ((pw - 2.0 * IMAGE_MARGIN) / w as f32).min((ph - 2.0 * IMAGE_MARGIN) / h as f32);
    let (iw, ih) = (w as f32 * s, h as f32 * s);
    let (x, y) = ((pw - iw) / 2.0, (ph - ih) / 2.0);
    let ops = format!("q {iw:.3} 0 0 {ih:.3} {x:.3} {y:.3} cm /Im0 Do Q\n");
    let contents = d.add_object(Stream::new(Dictionary::new(), ops.into_bytes()));
    Ok(d.add_object(dictionary! {
        "Type" => "Page",
        "MediaBox" => vec![0.into(), 0.into(), Object::Real(pw), Object::Real(ph)],
        "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => Object::Reference(img) } },
        "Contents" => Object::Reference(contents),
    }))
}

/// An image XObject for a PNG or JPEG, with its size in pixels. A JPEG is
/// embedded as it is; anything else is decoded and compressed losslessly.
fn image_xobject(doc: &mut Document, bytes: &[u8]) -> Result<(ObjId, u32, u32)> {
    if let Some((s, w, h)) = jpeg_as_is(bytes) {
        return Ok((doc.add_object(s), w, h));
    }
    let img = crate::signature::decode_any(bytes)?;
    let (w, h) = (img.width, img.height);
    let px = img.rgba.as_chunks::<4>().0;
    let rgb: Vec<u8> = px.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
    let dict = |cs: &str| {
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image",
            "Width" => w as i64, "Height" => h as i64,
            "ColorSpace" => cs, "BitsPerComponent" => 8,
        }
    };
    let mut d = dict("DeviceRGB");
    if px.iter().any(|p| p[3] < 255) {
        let mut mask = Stream::new(dict("DeviceGray"), px.iter().map(|p| p[3]).collect());
        let _ = mask.compress();
        d.set("SMask", Object::Reference(doc.add_object(mask)));
    }
    let mut s = Stream::new(d, rgb);
    let _ = s.compress();
    Ok((doc.add_object(s), w, h))
}

/// A JPEG in grey or colour (not CMYK), as a DCT-encoded image.
fn jpeg_as_is(bytes: &[u8]) -> Option<(Stream, u32, u32)> {
    use zune_jpeg::zune_core::colorspace::ColorSpace;
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut dec = zune_jpeg::JpegDecoder::new(std::io::Cursor::new(bytes));
    dec.decode_headers().ok()?;
    let (w, h) = dec.dimensions()?;
    let cs = match dec.input_colorspace()? {
        ColorSpace::Luma => "DeviceGray",
        ColorSpace::YCbCr | ColorSpace::RGB => "DeviceRGB",
        _ => return None,
    };
    let dict = dictionary! {
        "Type" => "XObject", "Subtype" => "Image",
        "Width" => w as i64, "Height" => h as i64,
        "ColorSpace" => cs, "BitsPerComponent" => 8,
        "Filter" => "DCTDecode",
    };
    Some((
        Stream::new(dict, bytes.to_vec()).with_compression(false),
        w as u32,
        h as u32,
    ))
}

/// Where each page of `new` was in `old` (`None`: a page that is new),
/// matching pages by their object ids.
pub fn page_map(old: &FormDoc, new: &FormDoc) -> Vec<Option<usize>> {
    let at: HashMap<ObjId, usize> = old
        .page_ids
        .iter()
        .enumerate()
        .map(|(i, id)| (*id, i))
        .collect();
    new.page_ids.iter().map(|id| at.get(id).copied()).collect()
}
