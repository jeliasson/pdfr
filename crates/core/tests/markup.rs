//! Comments and markup survive save and reopen as standard annotations
//! with appearances; annotations of kinds pdfr does not edit stay as they
//! were.

use lopdf::{Document, Object, dictionary};
use pdfr_core::forms::FormDoc;
use pdfr_core::geom::Rect;
use pdfr_core::markup::{MarkKind, Markup};
use pdfr_core::save::save_with;

fn sample(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../../samples/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn close(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.01)
}

/// One mark of every kind on page 0 of `fd`.
fn every_kind(fd: &FormDoc) -> Vec<Markup> {
    let g = fd.pages[0];
    let mut note = Markup::note(0, &g, 400.0, 80.0, [255, 212, 0]);
    note.text = "Check this — åäö".into();
    note.author = "Anna".into();
    let line = Rect::new(50.0, 170.0, 140.0, 184.0);
    let mut hl = Markup::over_text(MarkKind::Highlight, 0, &g, &[line], [255, 212, 0]);
    hl.text = "Important".into();
    let ul = Markup::over_text(MarkKind::Underline, 0, &g, &[line], [0, 122, 255]);
    let so = Markup::over_text(MarkKind::StrikeOut, 0, &g, &[line], [255, 59, 48]);
    let mut tb = Markup::text_box(0, &g, Rect::new(300.0, 300.0, 480.0, 340.0), [0, 0, 0]);
    tb.text = "A text box\nwith two lines".into();
    tb.size = 14.0;
    let sq = Markup::shape(
        MarkKind::Square,
        0,
        &g,
        (60.0, 400.0),
        (200.0, 470.0),
        [255, 59, 48],
    );
    let ci = Markup::shape(
        MarkKind::Circle,
        0,
        &g,
        (220.0, 400.0),
        (300.0, 470.0),
        [52, 199, 89],
    );
    let ar = Markup::shape(
        MarkKind::Arrow,
        0,
        &g,
        (320.0, 400.0),
        (420.0, 470.0),
        [0, 122, 255],
    );
    let li = Markup::shape(
        MarkKind::Line,
        0,
        &g,
        (320.0, 500.0),
        (420.0, 500.0),
        [0, 0, 0],
    );
    vec![note, hl, ul, so, tb, sq, ci, ar, li]
}

fn same_ignoring_source(a: &Markup, b: &Markup) {
    assert_eq!(a.kind, b.kind);
    assert_eq!(a.page, b.page);
    assert_eq!(a.color, b.color, "{:?}", a.kind);
    assert_eq!(a.text, b.text);
    assert_eq!(a.author, b.author);
    assert!((a.size - b.size).abs() < 0.01, "{:?}", a.kind);
    assert!(
        close(&a.rect, &b.rect),
        "{:?} {:?} {:?}",
        a.kind,
        a.rect,
        b.rect
    );
    assert_eq!(a.quads.len(), b.quads.len());
    for (x, y) in a.quads.iter().zip(&b.quads) {
        assert!(close(x, y));
    }
    if a.kind.is_line() {
        assert!(close(&a.line, &b.line));
    }
}

#[test]
fn every_kind_round_trips_with_appearances() {
    let fd = FormDoc::open(sample("flat_form.pdf")).unwrap();
    assert!(fd.markups.is_empty());
    let marks = every_kind(&fd);
    let bytes = save_with(&fd, &fd.fields, &marks).unwrap();
    let fd2 = FormDoc::open(bytes.clone()).unwrap();
    assert_eq!(fd2.markups.len(), marks.len());
    for (a, b) in marks.iter().zip(&fd2.markups) {
        same_ignoring_source(a, b);
        assert!(b.source.is_some());
    }
    // Every one carries a normal appearance, so other apps draw it as is.
    let d = Document::load_mem(&bytes).unwrap();
    for m in &fd2.markups {
        let a = d.get_dictionary(m.source.unwrap()).unwrap();
        assert_eq!(a.get(b"Type").unwrap().as_name().unwrap(), b"Annot");
        let ap = a.get(b"AP").unwrap().as_dict().unwrap();
        let id = ap.get(b"N").unwrap().as_reference().unwrap();
        assert!(matches!(d.get_object(id).unwrap(), Object::Stream(_)));
    }
    // Saving again with nothing changed writes nothing new.
    let again = pdfr_core::save::save(&fd2, &fd2.fields).unwrap();
    let fd3 = FormDoc::open(again).unwrap();
    assert_eq!(fd3.markups, fd2.markups);
    // Drawn live in pdfr: the render copy leaves them out.
    let rd = Document::load_mem(&fd2.render_bytes()).unwrap();
    let p0 = *rd.get_pages().values().next().unwrap();
    let left = rd
        .get_dictionary(p0)
        .unwrap()
        .get(b"Annots")
        .ok()
        .and_then(|o| o.as_array().ok())
        .map_or(0, |a| a.len());
    assert_eq!(left, 0);
}

/// `flat_form.pdf` with an ink drawing (a kind pdfr leaves alone) and a
/// highlight made by another app, with a pop-up.
fn with_foreign_annots() -> Vec<u8> {
    let mut d = Document::load_mem(&sample("flat_form.pdf")).unwrap();
    let page = *d.get_pages().values().next().unwrap();
    let ink = d.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Ink",
        "Rect" => vec![10.into(), 10.into(), 60.into(), 60.into()],
        "InkList" => vec![Object::Array(vec![10.into(), 10.into(), 60.into(), 60.into()])],
        "C" => vec![1.into(), 0.into(), 0.into()],
    });
    let popup = d.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Popup",
        "Rect" => vec![300.into(), 600.into(), 450.into(), 700.into()],
    });
    let hl = d.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Highlight",
        "Rect" => vec![50.into(), 600.into(), 140.into(), 620.into()],
        "QuadPoints" => vec![50.into(), 620.into(), 140.into(), 620.into(), 50.into(), 600.into(), 140.into(), 600.into()],
        "C" => vec![0.into(), 1.into(), 0.into()],
        "Contents" => lopdf::text_string("Theirs"),
        "T" => lopdf::text_string("Bo"),
        "Popup" => Object::Reference(popup),
        "NM" => lopdf::text_string("keep-me"),
    });
    d.get_dictionary_mut(page).unwrap().set(
        "Annots",
        vec![
            Object::Reference(ink),
            Object::Reference(hl),
            Object::Reference(popup),
        ],
    );
    let mut out = Vec::new();
    d.save_to(&mut out).unwrap();
    out
}

fn annot_subtypes(bytes: &[u8]) -> Vec<String> {
    let d = Document::load_mem(bytes).unwrap();
    let page = *d.get_pages().values().next().unwrap();
    let annots = d
        .get_dictionary(page)
        .unwrap()
        .get(b"Annots")
        .unwrap()
        .as_array()
        .unwrap()
        .clone();
    annots
        .iter()
        .map(|o| {
            let a = d.get_dictionary(o.as_reference().unwrap()).unwrap();
            String::from_utf8_lossy(a.get(b"Subtype").unwrap().as_name().unwrap()).into_owned()
        })
        .collect()
}

#[test]
fn others_marks_are_read_edited_and_unknown_kinds_survive() {
    let fd = FormDoc::open(with_foreign_annots()).unwrap();
    assert_eq!(
        fd.markups.len(),
        1,
        "the ink drawing and pop-up are left alone"
    );
    let hl = &fd.markups[0];
    assert_eq!(hl.kind, MarkKind::Highlight);
    assert_eq!(hl.color, [0, 255, 0]);
    assert_eq!(hl.text, "Theirs");
    assert_eq!(hl.author, "Bo");
    let ink_before = {
        let d = fd.doc.as_ref().unwrap();
        d.objects
            .values()
            .find(|o| {
                o.as_dict().is_ok_and(|d| {
                    d.get(b"Subtype")
                        .is_ok_and(|s| s.as_name().ok() == Some(&b"Ink"[..]))
                })
            })
            .cloned()
            .unwrap()
    };

    // Recolour theirs and add a note: their other keys stay.
    let mut marks = fd.markups.clone();
    marks[0].color = [255, 59, 48];
    marks.push(Markup::note(0, &fd.pages[0], 20.0, 20.0, [255, 212, 0]));
    let bytes = save_with(&fd, &fd.fields, &marks).unwrap();
    assert_eq!(
        annot_subtypes(&bytes),
        vec!["Ink", "Highlight", "Popup", "Text"]
    );
    let fd2 = FormDoc::open(bytes.clone()).unwrap();
    assert_eq!(fd2.markups.len(), 2);
    assert_eq!(fd2.markups[0].color, [255, 59, 48]);
    let d = Document::load_mem(&bytes).unwrap();
    let a = d.get_dictionary(fd2.markups[0].source.unwrap()).unwrap();
    assert_eq!(a.get(b"NM").unwrap().as_str().unwrap(), b"keep-me");
    assert!(a.get(b"Popup").is_ok());
    let ink_after = d
        .objects
        .values()
        .find(|o| {
            o.as_dict().is_ok_and(|d| {
                d.get(b"Subtype")
                    .is_ok_and(|s| s.as_name().ok() == Some(&b"Ink"[..]))
            })
        })
        .cloned()
        .unwrap();
    assert_eq!(ink_before, ink_after, "the ink drawing is untouched");

    // Delete theirs: it goes with its pop-up; the ink drawing stays.
    let bytes = save_with(&fd, &fd.fields, &[]).unwrap();
    assert_eq!(annot_subtypes(&bytes), vec!["Ink"]);
    assert!(FormDoc::open(bytes).unwrap().markups.is_empty());
}

#[test]
fn moved_and_resized_marks_save_where_they_are() {
    let fd = FormDoc::open(sample("flat_rotated.pdf")).unwrap();
    let g = fd.pages[0];
    let mut tb = Markup::text_box(0, &g, Rect::new(100.0, 100.0, 250.0, 140.0), [0, 0, 255]);
    tb.text = "Turned page".into();
    let tb = tb.moved(&g, 20.0, 30.0);
    let r = tb.display_rect(&g);
    assert!((r.x0 - 120.0).abs() < 0.01 && (r.y0 - 130.0).abs() < 0.01);
    let mut ar = Markup::shape(
        MarkKind::Arrow,
        0,
        &g,
        (50.0, 50.0),
        (90.0, 90.0),
        [0, 0, 0],
    );
    ar.set_line_end(&g, true, (150.0, 60.0));
    let bytes = save_with(&fd, &fd.fields, &[tb.clone(), ar.clone()]).unwrap();
    let fd2 = FormDoc::open(bytes).unwrap();
    let r2 = fd2.markups[0].display_rect(&fd2.pages[0]);
    assert!((r2.x0 - r.x0).abs() < 0.01 && (r2.y1 - r.y1).abs() < 0.01);
    let (_, end) = fd2.markups[1].display_line(&fd2.pages[0]);
    assert!((end.0 - 150.0).abs() < 0.01 && (end.1 - 60.0).abs() < 0.01);
}
