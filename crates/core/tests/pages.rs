//! Organising pages: moving, deleting, inserting and extracting, with the
//! content, fields and saved file checked after each.

use hayro::hayro_interpret::InterpreterCache;
use pdfr_core::field::Origin;
use pdfr_core::forms::FormDoc;
use pdfr_core::pages::{extract, page_map};
use pdfr_core::save::save;

fn sample(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../../samples/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn open(name: &str) -> FormDoc {
    FormDoc::open(sample(name)).unwrap()
}

/// All text on each page, joined.
fn texts(bytes: &[u8]) -> Vec<String> {
    let pdf = pdfr_core::render::open(bytes.to_vec()).unwrap();
    let cache = InterpreterCache::new();
    pdf.pages()
        .iter()
        .map(|p| {
            pdfr_core::text::extract(p, &cache)
                .iter()
                .map(|r| r.text.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}

fn names(fd: &FormDoc) -> Vec<(String, usize)> {
    fd.fields
        .iter()
        .filter_map(|f| match &f.origin {
            Origin::AcroForm { name, .. } => Some((name.clone(), f.page)),
            _ => None,
        })
        .collect()
}

#[test]
fn reorder_keeps_content_and_ids() {
    let fd = open("links.pdf");
    assert_eq!(fd.pages.len(), 3);
    let before = texts(&fd.bytes);
    let moved = fd.arranged(&[2, 0, 1]).unwrap();
    assert_eq!(moved.pages.len(), 3);
    assert_eq!(page_map(&fd, &moved), vec![Some(2), Some(0), Some(1)]);
    let after = texts(&moved.bytes);
    assert_eq!(
        after,
        vec![before[2].clone(), before[0].clone(), before[1].clone()]
    );
    // Saved and opened again: the same order.
    let saved = save(&moved, &moved.fields).unwrap();
    assert_eq!(texts(&saved), after);
}

#[test]
fn delete_removes_page_and_its_fields() {
    let fd = open("real_form.pdf");
    let n = fd.pages.len();
    // Add a blank page first, so a page can be deleted with fields left.
    let two = fd.with_blank_page(0, 0).unwrap();
    assert_eq!(two.pages.len(), n + 1);
    assert_eq!(two.pages[0].crop, fd.pages[0].crop, "same size as the page");
    assert!(texts(&two.bytes)[0].trim().is_empty(), "blank");
    // The fields moved to page 1.
    assert!(names(&two).iter().all(|(_, p)| *p == 1));
    assert_eq!(names(&two).len(), names(&fd).len());
    // Delete the form page: its fields go with it.
    let gone = two.arranged(&[0]).unwrap();
    assert_eq!(gone.pages.len(), 1);
    assert!(gone.fields.is_empty());
    let doc = gone.doc.as_ref().unwrap();
    let fields = doc
        .catalog()
        .unwrap()
        .get(b"AcroForm")
        .ok()
        .and_then(|f| doc.dereference(f).ok())
        .and_then(|(_, f)| f.as_dict().ok().cloned())
        .and_then(|f| f.get(b"Fields").ok().cloned())
        .and_then(|a| doc.dereference(&a).ok().map(|(_, a)| a.clone()))
        .and_then(|a| a.as_array().ok().cloned())
        .unwrap_or_default();
    assert!(
        fields.is_empty(),
        "no fields left pointing at a deleted page"
    );
    // Deleting every page is refused.
    assert!(fd.arranged(&[]).is_err());
}

#[test]
fn insert_pdf_brings_content_and_fields() {
    let fd = open("links.pdf");
    let form = sample("real_form.pdf");
    let merged = fd.with_pdf(1, &form).unwrap();
    let form_pages = FormDoc::open(form.clone()).unwrap().pages.len();
    assert_eq!(merged.pages.len(), 3 + form_pages);
    let t = texts(&merged.bytes);
    let t_links = texts(&fd.bytes);
    let t_form = texts(&form);
    assert_eq!(t[0], t_links[0]);
    assert_eq!(
        t[1], t_form[0],
        "the inserted page's text (its fonts came along)"
    );
    assert_eq!(t[1 + form_pages], t_links[1]);
    // Its fields are fields here too, on the inserted page.
    let n = names(&merged);
    assert_eq!(n.len(), open("real_form.pdf").fields.len());
    assert!(n.iter().all(|(_, p)| *p == 1));
    // Filled in and saved: the values are there after reopening.
    let mut fields = merged.fields.clone();
    let i = fields
        .iter()
        .position(|f| matches!(&f.origin, Origin::AcroForm { name, .. } if name == "fullname"))
        .unwrap();
    fields[i].value = "Merged Name".into();
    let saved = FormDoc::open(save(&merged, &fields).unwrap()).unwrap();
    assert_eq!(saved.pages.len(), merged.pages.len());
    assert!(saved.fields.iter().any(|f| f.value == "Merged Name"));

    // Inserting the same form twice keeps the two sets of fields apart.
    let twice = merged.with_pdf(0, &form).unwrap();
    let n = names(&twice);
    assert!(n.iter().any(|(x, _)| x == "fullname"));
    assert!(n.iter().any(|(x, _)| x == "fullname 2"));
}

#[test]
fn insert_picture_page() {
    let fd = open("flat_form.pdf");
    // A wide 40x20 picture, red with a transparent half.
    let mut png = Vec::new();
    {
        let mut e = png::Encoder::new(&mut png, 40, 20);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        let mut w = e.write_header().unwrap();
        let px: Vec<u8> = (0..40 * 20)
            .flat_map(|i| [255, 0, 0, if i % 40 < 20 { 255 } else { 0 }])
            .collect();
        w.write_image_data(&px).unwrap();
    }
    let with = fd.with_image(1, &png).unwrap();
    assert_eq!(with.pages.len(), fd.pages.len() + 1);
    let (w, h) = with.pages[1].display_size();
    assert!(
        w > h && (w - 842.0).abs() < 1.0,
        "A4 sideways for a wide picture"
    );
    // It draws: the middle of the page is red.
    let pdf = pdfr_core::render::open(with.bytes.to_vec()).unwrap();
    let cache = pdfr_core::render::RenderCache::new();
    let img = pdfr_core::render::render_page(&pdf.pages()[1], &cache, 0.5);
    let (x, y) = (img.width as usize / 3, img.height as usize / 2);
    let p = &img.rgba[(y * img.width as usize + x) * 4..][..3];
    assert!(p[0] > 200 && p[1] < 60, "red picture: {p:?}");
    assert!(fd.with_image(0, b"not a picture").is_err());
}

#[test]
fn extract_pages_to_a_new_file() {
    let fd = open("real_form.pdf");
    let mut fields = fd.fields.clone();
    let i = fields
        .iter()
        .position(|f| matches!(&f.origin, Origin::AcroForm { name, .. } if name == "fullname"))
        .unwrap();
    fields[i].value = "Extracted".into();
    let two = fd.with_blank_page(1, 0).unwrap();
    let mut fields2 = two.fields.clone();
    fields2[i].value = "Extracted".into();
    let out = extract(&two, &fields2, &two.markups, &[0]).unwrap();
    let ex = FormDoc::open(out.clone()).unwrap();
    assert_eq!(ex.pages.len(), 1);
    assert_eq!(texts(&out)[0], texts(&fd.bytes)[0]);
    assert!(ex.fields.iter().any(|f| f.value == "Extracted"));
    // The original is unchanged.
    assert_eq!(two.pages.len(), 2);
    // Page order follows the list.
    let links = open("links.pdf");
    let out = extract(&links, &[], &links.markups, &[2, 1]).unwrap();
    let t = texts(&links.bytes);
    assert_eq!(texts(&out), vec![t[2].clone(), t[1].clone()]);
}
