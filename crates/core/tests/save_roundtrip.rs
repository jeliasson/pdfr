//! End-to-end: real-form values survive save and reload; manual fields are
//! saved as real fields at the same position, including on rotated pages.

use pdfr_core::field::{Field, FieldKind, Origin, TextMode};
use pdfr_core::forms::FormDoc;
use pdfr_core::geom::Rect;
use pdfr_core::layout::layout;
use pdfr_core::save::save;

fn sample(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../../samples/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn by_name<'a>(fields: &'a mut [Field], n: &str) -> Vec<&'a mut Field> {
    fields
        .iter_mut()
        .filter(|f| matches!(&f.origin, Origin::AcroForm { name, .. } if name == n))
        .collect()
}

#[test]
fn real_form_values_survive_save_and_reload() {
    let fd = FormDoc::open(sample("real_form.pdf")).unwrap();
    assert!(fd.editable());
    let mut fields = fd.fields.clone();
    assert_eq!(fields.len(), 9);
    by_name(&mut fields, "fullname")[0].value = "Åsa Öberg-Núñez".into();
    by_name(&mut fields, "contact.email")[0].value = "asa@example.se".into();
    by_name(&mut fields, "postcode")[0].value = "41301".into();
    by_name(&mut fields, "subscribe")[0].checked = true;
    by_name(&mut fields, "plan")[1].checked = true;
    by_name(&mut fields, "country")[0].value = "Norway".into();
    by_name(&mut fields, "comments")[0].value = "Line one\nLine two".into();
    let bytes = save(&fd, &fields).unwrap();

    let fd2 = FormDoc::open(bytes.clone()).unwrap();
    let mut f2 = fd2.fields.clone();
    assert_eq!(by_name(&mut f2, "fullname")[0].value, "Åsa Öberg-Núñez");
    assert_eq!(by_name(&mut f2, "contact.email")[0].value, "asa@example.se");
    assert_eq!(by_name(&mut f2, "postcode")[0].value, "41301");
    assert!(by_name(&mut f2, "postcode")[0].kind.is_comb());
    assert!(by_name(&mut f2, "subscribe")[0].checked);
    let plan: Vec<bool> = by_name(&mut f2, "plan").iter().map(|f| f.checked).collect();
    assert_eq!(plan, vec![false, true, false]);
    assert_eq!(by_name(&mut f2, "country")[0].value, "Norway");
    assert_eq!(by_name(&mut f2, "comments")[0].value, "Line one\nLine two");

    // NeedAppearances / XFA removed; existing /DA untouched.
    let doc = lopdf::Document::load_mem(&bytes).unwrap();
    let s = String::from_utf8_lossy(&bytes);
    assert!(!s.contains("NeedAppearances"));
    let email = doc
        .objects
        .values()
        .filter_map(|o| o.as_dict().ok())
        .find(|d| d.get(b"T").ok().and_then(|t| t.as_str().ok()) == Some(b"contact.email"))
        .unwrap();
    let da = email
        .get(b"DA")
        .ok()
        .and_then(|o| o.as_str().ok())
        .map(|s| String::from_utf8_lossy(s).into_owned());
    let orig = lopdf::Document::load_mem(&sample("real_form.pdf")).unwrap();
    let email0 = orig
        .objects
        .values()
        .filter_map(|o| o.as_dict().ok())
        .find(|d| d.get(b"T").ok().and_then(|t| t.as_str().ok()) == Some(b"contact.email"))
        .unwrap();
    let da0 = email0
        .get(b"DA")
        .ok()
        .and_then(|o| o.as_str().ok())
        .map(|s| String::from_utf8_lossy(s).into_owned());
    assert_eq!(da, da0);

    // Saving again reuses the embedded font.
    let mut f3 = fd2.fields.clone();
    by_name(&mut f3, "fullname")[0].value = "Changed".into();
    let bytes3 = save(&fd2, &f3).unwrap();
    let doc3 = lopdf::Document::load_mem(&bytes3).unwrap();
    let fonts = doc3
        .objects
        .values()
        .filter_map(|o| o.as_dict().ok())
        .filter(|d| d.has(b"PdfrFont"))
        .count();
    assert_eq!(fonts, 1);
}

#[test]
fn manual_field_saved_as_real_field_at_same_position_on_every_rotation() {
    for name in ["flat_form.pdf", "flat_rotated.pdf"] {
        let fd = FormDoc::open(sample(name)).unwrap();
        assert!(fd.fields.is_empty());
        let rect = Rect::new(150.0, 168.0, 545.0, 188.0);
        let mut f = Field::new_text(0, rect, TextMode::Single, Origin::Manual);
        f.value = "Anna Lindqvist".into();
        let cells_rect = Rect::new(150.0, 200.0, 290.0, 218.0);
        let mut comb = Field::new_text(
            0,
            cells_rect,
            TextMode::Comb(Field::equal_cells(&cells_rect, 10)),
            Origin::Detected,
        );
        comb.value = "1234567890".into();
        let empty = Field::new_text(
            0,
            Rect::new(50.0, 566.0, 545.0, 676.0),
            TextMode::Multi,
            Origin::Detected,
        );
        let mut cb =
            Field::new_checkbox(0, Rect::new(150.0, 268.0, 160.0, 278.0), Origin::Detected);
        cb.checked = true;
        let bytes = save(&fd, &[f.clone(), comb.clone(), empty, cb]).unwrap();
        let fd2 = FormDoc::open(bytes).unwrap();
        // The empty detected field is not saved.
        assert_eq!(fd2.fields.len(), 3, "{name}");
        let g = fd2
            .fields
            .iter()
            .find(|x| x.value == "Anna Lindqvist")
            .unwrap();
        for (a, b) in [
            (g.rect.x0, rect.x0),
            (g.rect.y0, rect.y0),
            (g.rect.x1, rect.x1),
            (g.rect.y1, rect.y1),
        ] {
            assert!((a - b).abs() < 0.01, "{name}: {:?} vs {:?}", g.rect, rect);
        }
        let c = fd2.fields.iter().find(|x| x.kind.is_comb()).unwrap();
        assert_eq!(c.value, "1234567890");
        // The reopened comb lays out glyphs at the same positions.
        let l1 = layout(&comb, &comb.value);
        let l2 = layout(c, &c.value);
        for (a, b) in l1.lines[0].glyphs.iter().zip(&l2.lines[0].glyphs) {
            assert!((a.x - b.x).abs() < 0.01);
        }
        assert!(
            fd2.fields
                .iter()
                .any(|x| matches!(x.kind, FieldKind::Checkbox { .. }) && x.checked)
        );
    }
}

#[test]
fn render_copy_blanks_values() {
    let fd = FormDoc::open(sample("real_form.pdf")).unwrap();
    let mut fields = fd.fields.clone();
    by_name(&mut fields, "fullname")[0].value = "Visible Text".into();
    let fd2 = FormDoc::open(save(&fd, &fields).unwrap()).unwrap();
    let blank = fd2.render_bytes();
    let doc = lopdf::Document::load_mem(&blank).unwrap();
    // No appearance stream of the reopened file still draws text in a /Tx section.
    for f in &fd2.fields {
        if let Origin::AcroForm { widget, .. } = f.origin
            && let Some(id) = pdfr_core::forms::normal_appearance_id(&doc, widget)
        {
            let s = doc.get_object(id).unwrap().as_stream().unwrap();
            let c = s.decompressed_content().unwrap_or(s.content.clone());
            assert!(
                !String::from_utf8_lossy(&c).contains(" Tj"),
                "{:?}",
                f.origin
            );
        }
    }
}

#[test]
fn every_rotation_saves_upright_text_in_place() {
    use pdfr_core::render::{RenderCache, open, render_page};
    for rot in [0i64, 90, 180, 270] {
        let mut doc = lopdf::Document::load_mem(&sample("flat_form.pdf")).unwrap();
        let pid = *doc.get_pages().values().next().unwrap();
        doc.get_dictionary_mut(pid).unwrap().set("Rotate", rot);
        let mut bytes = vec![];
        doc.save_to(&mut bytes).unwrap();
        let fd = FormDoc::open(bytes).unwrap();
        assert_eq!(fd.pages[0].rotate as i64, rot);
        let (w, h) = fd.pages[0].display_size();
        let rect = Rect::new(w * 0.2, h * 0.3, w * 0.2 + 200.0, h * 0.3 + 20.0);
        let mut f = Field::new_text(0, rect, TextMode::Single, Origin::Manual);
        f.value = "WWWWWWWWWW".into();
        let saved = save(&fd, &[f]).unwrap();
        // Reopened at the same display position.
        let fd2 = FormDoc::open(saved.clone()).unwrap();
        let g = &fd2.fields[0];
        assert!(
            (g.rect.x0 - rect.x0).abs() < 0.01 && (g.rect.y1 - rect.y1).abs() < 0.01,
            "rot {rot}: {:?}",
            g.rect
        );
        // Rendered: the ink is inside the field and wider than tall (upright).
        let pdf = open(saved).unwrap();
        let cache = RenderCache::new();
        let img = render_page(&pdf.pages()[0], &cache, 2.0);
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
        for y in (rect.y0 * 2.0) as u32..(rect.y1 * 2.0) as u32 {
            for x in (rect.x0 * 2.0) as u32..(rect.x1 * 2.0) as u32 {
                let p = &img.rgba[((y * img.width + x) * 4) as usize..][..3];
                if p.iter().map(|&v| v as u32).sum::<u32>() < 200 {
                    (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
                }
            }
        }
        assert!(x1 > x0, "rot {rot}: no text drawn in the field");
        let (iw, ih) = ((x1 - x0) as f32, (y1 - y0) as f32);
        assert!(iw > 4.0 * ih, "rot {rot}: text not upright ({iw}x{ih})");
    }
}

#[test]
fn new_fields_get_unique_names_across_saves() {
    let manual = |text: &str, y: f32| {
        let mut f = Field::new_text(
            0,
            Rect::new(300.0, y, 450.0, y + 16.0),
            TextMode::Single,
            Origin::Manual,
        );
        f.value = text.into();
        f
    };
    let fd = FormDoc::open(sample("flat_form.pdf")).unwrap();
    let first = save(&fd, &[manual("first", 700.0)]).unwrap();
    let fd2 = FormDoc::open(first).unwrap();
    let mut fields = fd2.fields.clone();
    fields.push(manual("second", 740.0));
    let fd3 = FormDoc::open(save(&fd2, &fields).unwrap()).unwrap();
    let names: Vec<String> = fd3
        .fields
        .iter()
        .filter_map(|f| match &f.origin {
            Origin::AcroForm { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(names.len(), 2, "{names:?}");
    assert_ne!(names[0], names[1], "two fields must not share a name");
    let values: Vec<&str> = fd3.fields.iter().map(|f| f.value.as_str()).collect();
    assert!(values.contains(&"first") && values.contains(&"second"));
}

#[test]
fn one_builtin_font_embedded_once_across_saves() {
    let mut f = Field::new_text(
        0,
        Rect::new(300.0, 700.0, 480.0, 716.0),
        TextMode::Single,
        Origin::Manual,
    );
    f.value = "Göteborg RASMUS".into();
    let fd = FormDoc::open(sample("flat_form.pdf")).unwrap();
    let bytes = save(&fd, &[f]).unwrap();
    // Save the saved file again with another value: the font is reused.
    let fd2 = FormDoc::open(bytes).unwrap();
    let mut fields = fd2.fields.clone();
    let mut g = Field::new_text(
        0,
        Rect::new(300.0, 650.0, 480.0, 666.0),
        TextMode::Single,
        Origin::Manual,
    );
    g.value = "Ωmega Жук".into();
    fields.push(g);
    let fd3 = FormDoc::open(save(&fd2, &fields).unwrap()).unwrap();
    let doc = fd3.doc.as_ref().unwrap();
    let ours: Vec<Vec<u8>> = doc
        .objects
        .values()
        .filter_map(|o| o.as_dict().ok())
        .filter(|d| d.has(b"PdfrFont"))
        .filter_map(|d| d.get(b"BaseFont").ok()?.as_name().ok().map(|n| n.to_vec()))
        .collect();
    assert_eq!(
        ours,
        vec![pdfr_core::font::FONT_PS_NAME.as_bytes().to_vec()],
        "exactly one embedded copy of the built-in font"
    );
    let values: Vec<&str> = fd3.fields.iter().map(|f| f.value.as_str()).collect();
    assert!(
        values.contains(&"Göteborg RASMUS") && values.contains(&"Ωmega Жук"),
        "{values:?}"
    );
}

#[test]
fn print_copy_draws_values_into_the_page() {
    let fd = FormDoc::open(sample("flat_form.pdf")).unwrap();
    let mut f = Field::new_text(
        0,
        Rect::new(300.0, 700.0, 480.0, 716.0),
        TextMode::Single,
        Origin::Manual,
    );
    f.value = "Printed Value".into();
    let saved = save(&fd, &[f]).unwrap();
    let flat = pdfr_core::flatten::flatten_for_print(&saved).unwrap();
    // No form fields or widgets left...
    let doc = lopdf::Document::load_mem(&flat).unwrap();
    assert!(
        doc.catalog().unwrap().get(b"AcroForm").is_err(),
        "the form is removed"
    );
    let widgets = doc
        .objects
        .values()
        .filter_map(|o| o.as_dict().ok())
        .filter(|d| {
            d.get(b"Subtype").ok().and_then(|s| s.as_name().ok()) == Some(b"Widget".as_slice())
        })
        .count();
    let annotated = doc
        .get_pages()
        .values()
        .filter(|p| doc.get_dictionary(**p).unwrap().has(b"Annots"))
        .count();
    assert_eq!(
        annotated, 0,
        "no annotations on the pages ({widgets} widget objects left unreferenced)"
    );
    // ...and the value is now page text.
    let pdf = pdfr_core::render::open(flat).unwrap();
    let cache = hayro::hayro_interpret::InterpreterCache::new();
    let pages = pdf.pages();
    let runs = pdfr_core::text::extract(pages.first().unwrap(), &cache);
    assert!(
        runs.iter().any(|r| r.text.contains("Printed Value")),
        "{:?}",
        runs.iter().map(|r| &r.text).collect::<Vec<_>>()
    );
}
