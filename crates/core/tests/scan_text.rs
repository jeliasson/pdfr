//! Words read from a scan: which pages need reading, and found boxes that
//! are printed text rather than places to write.

use pdfr_core::detect::{drop_text_boxes, fit_scan_words, lacks_text};
use pdfr_core::field::{Field, Origin, TextMode};
use pdfr_core::geom::Rect;
use pdfr_core::text::TextRun;

fn word(text: &str, x0: f32, y0: f32, x1: f32, y1: f32) -> TextRun {
    TextRun {
        rect: Rect::new(x0, y0, x1, y1),
        text: text.into(),
        size: (y1 - y0) * 0.8,
        xs: vec![],
        ws: vec![],
    }
}

fn boxed(x0: f32, y0: f32, x1: f32, y1: f32, mode: TextMode) -> Field {
    Field::new_text(0, Rect::new(x0, y0, x1, y1), mode, Origin::Detected)
}

#[test]
fn a_page_number_is_no_text_layer() {
    assert!(lacks_text(&[]));
    assert!(lacks_text(&[word("Page 1 of 3", 0.0, 0.0, 50.0, 10.0)]));
    let para = word(
        "Please complete this form in capital letters using black ink.",
        0.0,
        0.0,
        300.0,
        10.0,
    );
    assert!(!lacks_text(&[para]));
}

#[test]
fn boxes_full_of_printed_words_are_dropped() {
    let mut fields = vec![
        // A table cell holding its text: dropped.
        boxed(50.0, 100.0, 200.0, 116.0, TextMode::Single),
        // An empty box with its label above it: kept.
        boxed(50.0, 140.0, 200.0, 156.0, TextMode::Single),
        // A notes box with a small heading inside: kept.
        boxed(50.0, 200.0, 400.0, 300.0, TextMode::Multi),
        // A framed paragraph: dropped.
        boxed(50.0, 320.0, 300.0, 360.0, TextMode::Multi),
        // A checkbox with a mark read as a letter: kept.
        Field::new_checkbox(0, Rect::new(10.0, 100.0, 20.0, 110.0), Origin::Detected),
    ];
    let words = vec![
        word("Intangible assets", 54.0, 103.0, 180.0, 113.0),
        word("Name", 50.0, 128.0, 80.0, 137.0),
        word("Notes", 54.0, 204.0, 84.0, 213.0),
        word("This box holds a printed", 54.0, 323.0, 296.0, 335.0),
        word("paragraph of text across", 54.0, 336.0, 296.0, 347.0),
        word("all of its lines here.", 54.0, 348.0, 250.0, 358.0),
        word("X", 11.0, 101.0, 19.0, 109.0),
    ];
    drop_text_boxes(&mut fields, &words);
    let left: Vec<Rect> = fields.iter().map(|f| f.rect).collect();
    assert_eq!(
        left,
        vec![
            Rect::new(50.0, 140.0, 200.0, 156.0),
            Rect::new(50.0, 200.0, 400.0, 300.0),
            Rect::new(10.0, 100.0, 20.0, 110.0),
        ]
    );
}

#[test]
fn checkbox_outlines_read_as_letters_are_cut() {
    let mut fields = vec![Field::new_checkbox(
        0,
        Rect::new(330.0, 268.0, 340.0, 278.0),
        Origin::Detected,
    )];
    let mut w = word("[ Other", 330.0, 268.0, 366.0, 278.0);
    w.xs = vec![331.0, 338.0, 344.0, 349.0, 353.0, 357.0, 361.0];
    w.ws = vec![6.0, 6.0, 5.0, 4.0, 4.0, 4.0, 4.0];
    let mut words = vec![w];
    fit_scan_words(&mut fields, &mut words);
    assert_eq!(fields.len(), 1);
    assert_eq!(words[0].text, "Other");
    assert_eq!(words[0].xs.len(), 5);
    assert!((words[0].rect.x0 - 344.0).abs() < 0.01);
}

#[test]
fn boxes_drawn_by_hand_are_kept() {
    let mut f = boxed(50.0, 100.0, 200.0, 116.0, TextMode::Single);
    f.origin = Origin::Manual;
    let mut fields = vec![f];
    drop_text_boxes(&mut fields, &[word("Printed", 54.0, 103.0, 190.0, 113.0)]);
    assert_eq!(fields.len(), 1);
}
