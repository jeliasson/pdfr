//! Link annotations of every kind are read with their targets.

use lopdf::{Document, Object, Stream, dictionary};
use pdfr_core::forms::FormDoc;
use pdfr_core::links::Target;

fn rect(r: [i64; 4]) -> Object {
    Object::Array(r.iter().map(|v| Object::Integer(*v)).collect())
}

/// Two 600×800 pages; page 1 has every kind of link, page 2 is rotated.
fn linked_pdf() -> Vec<u8> {
    let mut d = Document::with_version("1.7");
    let pages_id = d.new_object_id();
    let p2 = d.new_object_id();
    let content = d.add_object(Stream::new(
        dictionary! {},
        b"BT /F1 12 Tf 50 700 Td (Instructions: https://example.org/form.) Tj ET".to_vec(),
    ));
    let font = d.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let uri = d.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Link", "Rect" => rect([50, 600, 200, 620]),
        "A" => dictionary! { "S" => "URI", "URI" => Object::string_literal("https://pdfr.app/") },
    });
    let goto = d.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Link", "Rect" => rect([50, 500, 200, 520]),
        "A" => dictionary! { "S" => "GoTo", "D" => vec![Object::Reference(p2), "XYZ".into(), 0.into(), 400.into(), 0.into()] },
    });
    let named = d.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Link", "Rect" => rect([50, 400, 200, 420]),
        "Dest" => Object::string_literal("chapter"),
    });
    let next = d.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Link", "Rect" => rect([50, 300, 200, 320]),
        "A" => dictionary! { "S" => "Named", "N" => "NextPage" },
    });
    let p1 = d.add_object(dictionary! {
        "Type" => "Page", "Parent" => Object::Reference(pages_id),
        "MediaBox" => rect([0, 0, 600, 800]),
        "Contents" => Object::Reference(content),
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => Object::Reference(font) } },
        "Annots" => vec![uri.into(), goto.into(), named.into(), next.into()],
    });
    d.objects.insert(
        p2,
        Object::Dictionary(dictionary! {
            "Type" => "Page", "Parent" => Object::Reference(pages_id),
            "MediaBox" => rect([0, 0, 600, 800]), "Rotate" => 90,
        }),
    );
    d.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![p1.into(), p2.into()], "Count" => 2,
        }),
    );
    let tree = d.add_object(dictionary! {
        "Names" => vec![
            Object::string_literal("chapter"),
            vec![Object::Reference(p2), "FitH".into(), 300.into()].into(),
        ],
    });
    let cat = d.add_object(dictionary! {
        "Type" => "Catalog", "Pages" => Object::Reference(pages_id),
        "Names" => dictionary! { "Dests" => Object::Reference(tree) },
    });
    d.trailer.set("Root", Object::Reference(cat));
    let mut out = Vec::new();
    d.save_to(&mut out).unwrap();
    out
}

#[test]
fn link_annotations_and_their_targets() {
    let fd = FormDoc::open(linked_pdf()).unwrap();
    assert_eq!(fd.links.len(), 4);
    let l = &fd.links;
    assert_eq!(l[0].target, Target::Uri("https://pdfr.app/".into()));
    // User y 620..600 on an 800-high page is display y 180..200.
    assert!((l[0].rect.y0 - 180.0).abs() < 0.01 && (l[0].rect.y1 - 200.0).abs() < 0.01);
    // Page 2 is turned 90°: user point (0, 400) is display y 0, x 400.
    match &l[1].target {
        Target::Dest { page: 1, y: Some(y) } => assert!(y.abs() < 0.01, "{y}"),
        t => panic!("{t:?}"),
    }
    match &l[2].target {
        Target::Dest { page: 1, .. } => {}
        t => panic!("named destination: {t:?}"),
    }
    assert_eq!(l[3].target, Target::Named("NextPage".into()));
}

#[test]
fn printed_address_becomes_a_link() {
    let pdf = pdfr_core::render::open(linked_pdf()).unwrap();
    let cache = hayro::hayro_interpret::InterpreterCache::new();
    let runs = pdfr_core::text::extract(&pdf.pages()[0], &cache);
    let links = pdfr_core::links::text_links(0, &runs);
    assert_eq!(links.len(), 1, "{runs:?}");
    assert_eq!(links[0].target, Target::Uri("https://example.org/form".into()));
    assert!(links[0].rect.x0 > 100.0, "the box starts at the address");
}

#[test]
fn outline_of_the_links_sample() {
    let bytes = std::fs::read(format!("{}/../../samples/links.pdf", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let fd = FormDoc::open(bytes).unwrap();
    let titles: Vec<(&str, usize)> = fd.outline.iter().map(|o| (o.title.as_str(), o.depth)).collect();
    assert_eq!(
        titles,
        vec![
            ("Part A: Applicant", 0),
            ("Part B: Household", 0),
            ("Children", 1),
            ("Part C: Signatures", 0),
        ]
    );
    match &fd.outline[3].target {
        Some(Target::Dest { page: 2, y: Some(y) }) => assert!((*y - 60.0).abs() < 1.0, "{y}"),
        t => panic!("{t:?}"),
    }
    assert!(fd.outline[1].has_children);
}
