//! Redaction really removes what is under the boxes: the text is gone from
//! the text layer and from every (decompressed) byte of the file, covered
//! pixels are black in the image itself, and everything else stays put.

use hayro::hayro_interpret::InterpreterCache;
use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object, Stream, dictionary};
use pdfr_core::forms::FormDoc;
use pdfr_core::geom::Rect;
use pdfr_core::redact::{Mark, Redacted, redact};
use pdfr_core::render::{RenderCache, open, page_geom, render_page};
use pdfr_core::text::{TextRun, extract};

fn sample(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../../samples/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

/// Each page's text runs (display space).
fn runs(bytes: &[u8]) -> Vec<Vec<TextRun>> {
    let pdf = open(bytes.to_vec()).expect("opens");
    let cache = InterpreterCache::new();
    pdf.pages().iter().map(|p| extract(p, &cache)).collect()
}

fn all_text(bytes: &[u8]) -> String {
    runs(bytes)
        .iter()
        .flatten()
        .map(|r| r.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The file's bytes plus every stream decompressed.
fn raw(bytes: &[u8]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    let doc = Document::load_mem(bytes).unwrap();
    for o in doc.objects.values() {
        if let Object::Stream(s) = o {
            out.extend(
                s.decompressed_content()
                    .unwrap_or_else(|_| s.content.clone()),
            );
        }
    }
    out
}

/// How many streams (decompressed) hold `n`.
fn streams_with(bytes: &[u8], n: &str) -> usize {
    let doc = Document::load_mem(bytes).unwrap();
    doc.objects
        .values()
        .filter_map(|o| o.as_stream().ok())
        .filter(|s| {
            contains(
                &s.decompressed_content()
                    .unwrap_or_else(|_| s.content.clone()),
                n,
            )
        })
        .count()
}

fn contains(h: &[u8], n: &str) -> bool {
    h.windows(n.len()).any(|w| w == n.as_bytes())
}

/// A display rect of page `p` as a mark.
fn mark(bytes: &[u8], p: usize, r: &Rect) -> Mark {
    let pdf = open(bytes.to_vec()).unwrap();
    let g = page_geom(&pdf.pages()[p]);
    Mark {
        page: p,
        rect: g.rect_to_user(r),
    }
}

/// The page rendered at 1 px per point.
fn render(bytes: &[u8], p: usize) -> pdfr_core::render::Image {
    let pdf = open(bytes.to_vec()).unwrap();
    let pages = pdf.pages();
    let cache = RenderCache::new();
    render_page(&pages[p], &cache, 1.0)
}

fn pixel(img: &pdfr_core::render::Image, x: f32, y: f32) -> [u8; 3] {
    let i = (y as usize * img.width as usize + x as usize) * 4;
    [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
}

fn assert_black_inside(bytes: &[u8], p: usize, r: &Rect) {
    let img = render(bytes, p);
    let (cx, cy) = r.center();
    assert_eq!(pixel(&img, cx, cy), [0, 0, 0], "black box drawn");
}

/// Runs that do not touch any of `rects` are unchanged: same text, same place.
fn assert_rest_kept(before: &[TextRun], after: &[TextRun], rects: &[Rect]) {
    let mut n = 0;
    for b in before {
        if rects
            .iter()
            .any(|r| r.inset(-1.0).intersect(&b.rect).is_some())
        {
            continue;
        }
        let a = after.iter().find(|a| a.text == b.text).unwrap_or_else(|| {
            panic!("{:?} is still there", b.text);
        });
        assert!(
            (a.rect.x0 - b.rect.x0).abs() < 0.05 && (a.rect.y1 - b.rect.y1).abs() < 0.05,
            "{:?} kept its place: {:?} vs {:?}",
            b.text,
            a.rect,
            b.rect
        );
        n += 1;
    }
    assert!(n > 0, "some text is outside the boxes");
}

fn do_redact(bytes: &[u8], marks: &[Mark]) -> Redacted {
    let out = redact(bytes, marks).expect("redacts");
    assert!(out.bytes.starts_with(b"%PDF"));
    assert!(open(out.bytes.clone()).is_some(), "result opens");
    let doc = Document::load_mem(&out.bytes).unwrap();
    assert!(doc.trailer.get(b"Prev").is_err(), "no earlier revisions");
    out
}

#[test]
fn flat_form_whole_phrase_is_gone() {
    let bytes = sample("flat_form.pdf");
    let before = runs(&bytes);
    // A phrase written literally in the content, so the raw check means something.
    let raw_before = raw(&bytes);
    let target = before[0]
        .iter()
        .find(|r| r.text.len() >= 6 && contains(&raw_before, &r.text))
        .expect("a literal phrase")
        .clone();
    let rect = target.rect.inset(-0.5);
    let out = do_redact(&bytes, &[mark(&bytes, 0, &rect)]);
    assert!(out.glyphs_removed >= target.text.chars().count());
    assert!(!all_text(&out.bytes).contains(&target.text));
    assert!(!contains(&raw(&out.bytes), &target.text), "not in any byte");
    assert_rest_kept(&before[0], &runs(&out.bytes)[0], &[rect]);
    assert_black_inside(&out.bytes, 0, &rect);
}

#[test]
fn part_of_a_phrase_keeps_the_rest_in_place() {
    let bytes = sample("flat_form.pdf");
    let before = runs(&bytes);
    // A phrase of several words: redact the second word only.
    let run = before[0]
        .iter()
        .find(|r| r.text.split(' ').filter(|w| w.len() >= 3).count() >= 3)
        .expect("a phrase with words")
        .clone();
    let chars: Vec<char> = run.text.chars().collect();
    let s = run.text.find(' ').unwrap() + 1;
    let e = s + run.text[s..].find(' ').unwrap();
    let word: String = chars[s..e].iter().collect();
    let (a, b) = (run.char_rect(s), run.char_rect(e - 1));
    let rect = Rect::new(a.x0 + 0.2, run.rect.y0, b.x1 - 0.2, run.rect.y1);
    let out = do_redact(&bytes, &[mark(&bytes, 0, &rect)]);
    assert_eq!(out.glyphs_removed, word.chars().count(), "only that word");
    let after = runs(&out.bytes);
    // The words before and after are still there, at the same x.
    let tail: String = chars[e + 1..].iter().collect();
    let head: String = chars[..s - 1].iter().collect();
    let find = |t: &str| {
        after[0]
            .iter()
            .find(|r| r.text.contains(t) && (r.rect.y1 - run.rect.y1).abs() < 0.1)
            .unwrap_or_else(|| panic!("{t:?} kept"))
            .clone()
    };
    let h = find(&head);
    assert!((h.rect.x0 - run.rect.x0).abs() < 0.05);
    let t = find(&tail);
    let ti = t.text.find(&tail).unwrap();
    let tx = t.xs[t.text[..ti].chars().count()];
    assert!(
        (tx - run.xs[e + 1]).abs() < 0.05,
        "{tx} vs {}",
        run.xs[e + 1]
    );
    assert!(!after[0].iter().any(|r| r.text.contains(&word)));
}

#[test]
fn perf_text_search_hits_are_removed_in_an_embedded_font() {
    let bytes = sample("perf_text.pdf");
    let before = runs(&bytes);
    let pages: Vec<(usize, &[TextRun])> = vec![(0, before[0].as_slice())];
    // A word that occurs a few times.
    let word = ["the", "and", "of"]
        .into_iter()
        .find(|w| pdfr_core::search::search(&pages, w).len() >= 2)
        .unwrap();
    let hits = pdfr_core::search::search(&pages, word);
    let hit = &hits[hits.len() / 2];
    let rects: Vec<Rect> = hit.rects.clone();
    let marks: Vec<Mark> = rects.iter().map(|r| mark(&bytes, 0, r)).collect();
    let out = do_redact(&bytes, &marks);
    assert_eq!(out.strings_removed_whole, 0, "widths are known");
    assert_eq!(out.glyphs_removed, word.len());
    let after = runs(&out.bytes);
    let after_pages: Vec<(usize, &[TextRun])> = vec![(0, after[0].as_slice())];
    assert_eq!(
        pdfr_core::search::search(&after_pages, word).len(),
        hits.len() - 1,
        "exactly that one is gone"
    );
    assert_rest_kept(&before[0], &after[0], &rects);
    assert_black_inside(&out.bytes, 0, &rects[0]);
}

#[test]
fn real_form_field_under_a_box_is_removed() {
    let bytes = sample("real_form.pdf");
    let fd = FormDoc::open(bytes.clone()).unwrap();
    let field = fd
        .fields
        .iter()
        .find(|f| f.value == "Sweden")
        .expect("a filled field");
    let rect = field.rect;
    assert!(contains(&raw(&bytes), "Sweden"));
    let out = do_redact(&bytes, &[mark(&bytes, field.page, &rect)]);
    assert!(out.annotations_removed >= 1);
    assert!(!contains(&raw(&out.bytes), "Sweden"), "the value is gone");
    let fd2 = FormDoc::open(out.bytes.clone()).unwrap();
    assert_eq!(fd2.fields.len(), fd.fields.len() - 1, "other fields stay");
    assert_black_inside(&out.bytes, field.page, &rect);
}

/// The scan's image: (width, height, gray pixels) of its only image.
fn page_image(bytes: &[u8]) -> (usize, usize, Vec<u8>) {
    let doc = Document::load_mem(bytes).unwrap();
    for o in doc.objects.values() {
        let Object::Stream(s) = o else { continue };
        if s.dict.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Image") {
            continue;
        }
        let w = s.dict.get(b"Width").unwrap().as_i64().unwrap() as usize;
        let h = s.dict.get(b"Height").unwrap().as_i64().unwrap() as usize;
        assert_eq!(
            s.dict.get(b"ColorSpace").unwrap().as_name().unwrap(),
            b"DeviceGray"
        );
        return (w, h, s.decompressed_content().unwrap());
    }
    panic!("no image");
}

#[test]
fn scanned_page_pixels_under_the_box_are_black() {
    let bytes = sample("scanned_form.pdf");
    let pdf = open(bytes.clone()).unwrap();
    let (pw, ph) = page_geom(&pdf.pages()[0]).display_size();
    drop(pdf);
    // Somewhere with print on it: the upper left quarter.
    let rect = Rect::new(pw * 0.1, ph * 0.1, pw * 0.4, ph * 0.2);
    let out = do_redact(&bytes, &[mark(&bytes, 0, &rect)]);
    assert_eq!(out.images_patched, 1, "{out:?}");
    let doc = Document::load_mem(&out.bytes).unwrap();
    let images = doc
        .objects
        .values()
        .filter_map(|o| o.as_stream().ok())
        .filter(|s| s.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Image"))
        .count();
    assert_eq!(images, 1, "the original scan is not kept in the file");
    let (w, h, px) = page_image(&out.bytes);
    // The image covers the page: map the box into pixels (a pixel inside).
    let to_px = |x: f32, y: f32| ((x / pw * w as f32) as usize, (y / ph * h as f32) as usize);
    let (x0, y0) = to_px(rect.x0 + 1.0, rect.y0 + 1.0);
    let (x1, y1) = to_px(rect.x1 - 1.0, rect.y1 - 1.0);
    for y in y0..y1 {
        for x in x0..x1 {
            assert_eq!(px[y * w + x], 0, "pixel {x},{y} is black");
        }
    }
    // Outside the box the scan is still paper (mostly light).
    let (bx, by) = to_px(pw * 0.5, ph * 0.9);
    let light = (0..50).filter(|i| px[by * w + bx + i] > 128).count();
    assert!(light > 25, "rest of the scan kept");
    assert_eq!(px.len(), w * h);
    assert_black_inside(&out.bytes, 0, &rect);
}

// ---------------------------------------------------------------------------
// A made-up document with everything at once.

const SECRET: &str = "19850412-1234";

fn ops(s: &str) -> Vec<u8> {
    s.as_bytes().to_vec()
}

/// Two pages sharing a Form XObject that writes the secret; page 1 also
/// has the secret as plain text (Tj and TJ), alternate text, a small
/// gray image, a vector box and a note over it; the file is "signed".
fn synthetic(signed: bool) -> Vec<u8> {
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let font = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let form = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form", "BBox" => vec![(-10).into(), 0.into(), 300.into(), 30.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
        },
        ops(&format!("BT /F1 12 Tf -4 10 Td (Ref {SECRET}) Tj ET")),
    ));
    // 10x10 gray image, mid grey.
    let mut img = Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => 10, "Height" => 10,
            "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8,
        },
        vec![128u8; 100],
    );
    img.compress().unwrap();
    let img = doc.add_object(img);
    let res = dictionary! {
        "Font" => dictionary! { "F1" => font },
        "XObject" => dictionary! { "Fm1" => form, "Im1" => img },
    };
    let c1 = format!(
        "BT /F1 12 Tf 72 700 Td (ID: {SECRET} end) Tj ET\n\
         BT /F1 12 Tf 72 680 Td [(ID: ) -50 (19850412) 120 (-1234) ( tail)] TJ ET\n\
         /Span <</ActualText (Personnummer {SECRET})>> BDC BT /F1 12 Tf 82 660 Td (X{SECRET}) Tj ET EMC\n\
         q 1 0 0 1 72 600 cm /Fm1 Do Q\n\
         q 100 0 0 100 300 400 cm /Im1 Do Q\n\
         0 0 1 rg 80 300 20 20 re f 300 300 40 40 re f\n"
    );
    let c2 = "q 1 0 0 1 72 600 cm /Fm1 Do Q".to_string();
    let mut kids = vec![];
    let mut first = None;
    for c in [c1, c2] {
        let cid = doc.add_object(Stream::new(Dictionary::new(), ops(&c)));
        let pid = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Contents" => cid, "Resources" => res.clone(),
            "PieceInfo" => dictionary! { "App" => dictionary! { "Private" => Object::string_literal(SECRET) } },
        });
        first.get_or_insert(pid);
        kids.push(Object::Reference(pid));
    }
    let note = doc.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Text",
        "Rect" => vec![100.into(), 695.into(), 120.into(), 715.into()],
        "Contents" => Object::string_literal(format!("note {SECRET}")),
    });
    doc.get_dictionary_mut(first.unwrap())
        .unwrap()
        .set("Annots", vec![Object::Reference(note)]);
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! { "Type" => "Pages", "Count" => 2, "Kids" => kids }),
    );
    let mut cat = dictionary! { "Type" => "Catalog", "Pages" => pages_id };
    if signed {
        let sig = doc.add_object(dictionary! {
            "Type" => "Sig", "Filter" => "Adobe.PPKLite", "Contents" => Object::string_literal("x"),
        });
        let field = doc.add_object(dictionary! {
            "FT" => "Sig", "T" => Object::string_literal("sig"), "V" => sig,
            "Rect" => vec![0.into(), 0.into(), 0.into(), 0.into()],
        });
        cat.set(
            "AcroForm",
            dictionary! { "Fields" => vec![Object::Reference(field)], "SigFlags" => 3 },
        );
    }
    let cat = doc.add_object(cat);
    doc.trailer.set("Root", cat);
    let mut out = Vec::new();
    doc.save_to(&mut out).unwrap();
    out
}

fn user_mark(page: usize, r: [f32; 4]) -> Mark {
    Mark { page, rect: r }
}

#[test]
fn synthetic_secret_is_gone_everywhere_on_the_page() {
    let bytes = synthetic(false);
    assert!(all_text(&bytes).contains(SECRET));
    // One box over the column where the secret is written (x 90..174),
    // from y 595 to 712: the three lines and the form's line.
    let marks = [
        user_mark(0, [90.0, 595.0, 174.0, 712.0]),
        user_mark(0, [350.0, 410.0, 450.0, 520.0]),
        user_mark(0, [75.0, 295.0, 105.0, 325.0]),
    ];
    let out = do_redact(&bytes, &marks);
    assert!(!out.signatures_broken);
    let text = runs(&out.bytes);
    let p1: String = text[0].iter().map(|r| r.text.clone() + "\n").collect();
    for frag in ["19850412", "1234", "Personnummer"] {
        assert!(!p1.contains(frag), "{frag:?} gone from the text: {p1}");
    }
    // Text around it stays: "ID:" at its place, "end" / "tail" after.
    assert!(p1.contains("ID:"), "{p1}");
    assert!(p1.contains("end") && p1.contains("tail"), "{p1}");
    let before = runs(&bytes);
    let pos = |runs: &[TextRun], t: &str| -> f32 {
        let r = runs.iter().find(|r| r.text.contains(t)).unwrap();
        r.xs[r.text[..r.text.find(t).unwrap()].chars().count()]
    };
    assert!((pos(&before[0], "end") - pos(&text[0], "end")).abs() < 0.05);
    assert!((pos(&before[0], "tail") - pos(&text[0], "tail")).abs() < 0.05);
    // The other page shares the form and keeps its text.
    assert!(text[1].iter().any(|r| r.text.contains(SECRET)));
    // Not in the bytes of page 1's content, the note, the piece info or
    // the alternate text; only page 2's (shared) form still holds it once.
    let raw = raw(&out.bytes);
    assert_eq!(
        streams_with(&out.bytes, SECRET),
        1,
        "only the untouched page's form keeps it"
    );
    assert!(
        !contains(&out.bytes, "Private ("),
        "page 1's piece info is gone"
    );
    assert!(!contains(&raw, "Personnummer"));
    assert!(!contains(&raw, "note 1985"));
    assert_eq!(out.annotations_removed, 1);
    // The image: patched (part covered), its covered pixels black.
    assert_eq!(out.images_patched, 1);
    // Fully covered small box removed, the other kept.
    assert_eq!(out.paths_removed, 1);
    let img = render(&out.bytes, 0);
    // Display y = 792 - user y.
    assert_eq!(
        pixel(&img, 320.0, 792.0 - 320.0),
        [0, 0, 255],
        "other box kept"
    );
    assert_eq!(
        pixel(&img, 320.0, 792.0 - 450.0),
        [128, 128, 128],
        "uncovered image part"
    );
    assert_eq!(pixel(&img, 380.0, 792.0 - 450.0), [0, 0, 0]);
}

#[test]
fn page_two_untouched_and_signed_files_warn() {
    let bytes = synthetic(true);
    let out = do_redact(&bytes, &[user_mark(1, [70.0, 595.0, 400.0, 640.0])]);
    assert!(
        out.signatures_broken,
        "signatures can't survive a redaction"
    );
    let text = runs(&out.bytes);
    assert!(
        text[0].iter().any(|r| r.text.contains(SECRET)),
        "page 1 kept"
    );
    assert!(
        !text[1].iter().any(|r| r.text.contains("1985")),
        "page 2 redacted"
    );
    // Page 2's PieceInfo is gone; page 1's (not redacted) stays.
    let doc = Document::load_mem(&out.bytes).unwrap();
    let pages: Vec<_> = doc.get_pages().values().copied().collect();
    assert!(doc.get_dictionary(pages[0]).unwrap().has(b"PieceInfo"));
    assert!(!doc.get_dictionary(pages[1]).unwrap().has(b"PieceInfo"));
}

#[test]
fn earlier_revisions_are_not_kept() {
    // The file plus an incremental update: the old content stays in the
    // first revision's bytes unless the redacted file is a full rewrite.
    let bytes = synthetic(false);
    let doc = Document::load_mem(&bytes).unwrap();
    let mut inc = lopdf::IncrementalDocument::create_from(bytes.clone(), doc.clone());
    let info = inc
        .new_document
        .add_object(dictionary! { "Title" => Object::string_literal("v2") });
    inc.new_document.trailer.set("Info", info);
    inc.new_document.max_id = inc.new_document.max_id.max(doc.max_id);
    let mut updated = Vec::new();
    inc.save_to(&mut updated).unwrap();
    assert!(contains(&updated, "/Prev"));
    let out = do_redact(&updated, &[user_mark(0, [60.0, 595.0, 400.0, 720.0])]);
    assert_eq!(
        out.bytes.windows(5).filter(|w| *w == b"%%EOF").count(),
        1,
        "one revision"
    );
    // Page 2 still has it (shared form, not redacted); page 1's own text is gone.
    assert!(!contains(&raw(&out.bytes), "X1985"));
    assert!(!contains(&raw(&out.bytes), "ID: 1985"));
}

#[test]
fn unknown_operators_are_kept() {
    // A page with only a box: redaction elsewhere changes nothing visible.
    let bytes = synthetic(false);
    let out = do_redact(&bytes, &[user_mark(0, [500.0, 50.0, 550.0, 60.0])]);
    assert_eq!(out.glyphs_removed, 0);
    let doc = Document::load_mem(&out.bytes).unwrap();
    let pages: Vec<_> = doc.get_pages().values().copied().collect();
    let ops = Content::decode(&doc.get_page_content(pages[0]))
        .unwrap()
        .operations;
    let names: Vec<&str> = ops
        .iter()
        .map(|o: &Operation| o.operator.as_str())
        .collect();
    assert!(names.contains(&"BDC") && names.contains(&"Do"));
    assert!(all_text(&out.bytes).contains(SECRET));
}
