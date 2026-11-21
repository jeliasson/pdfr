//! Reading the synthetic scans in samples/. These need the OCR models,
//! which are downloaded, not committed: the tests pass without reading
//! anything when they are absent. Point `PDFR_OCR_MODELS` at a folder
//! holding them (or let pdfr download them into its data folder) to run
//! them for real.

use pdfr_assist::labels::label_fields;
use pdfr_core::pipeline::{Loaded, load, load_with};
use pdfr_core::text::TextRun;
use std::path::PathBuf;

fn models() -> Option<PathBuf> {
    let dir = std::env::var_os("PDFR_OCR_MODELS")
        .map(PathBuf::from)
        .or_else(|| pdfr_license::data_dir(None).map(|d| pdfr_ocr::model_dir(&d)))?;
    if pdfr_ocr::models_present(&dir) {
        Some(dir)
    } else {
        eprintln!("OCR models not found: skipping (set PDFR_OCR_MODELS)");
        None
    }
}

fn sample(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../../samples/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn load_read(name: &str, dir: &std::path::Path) -> Loaded {
    let reader = pdfr_ocr::Reader::load(dir).expect("models load");
    load_with(sample(name), Some(&|g| reader.read(g))).unwrap()
}

/// The phrase containing `text`, if one was read.
fn find<'a>(runs: &'a [TextRun], text: &str) -> Option<&'a TextRun> {
    runs.iter()
        .find(|r| r.text.to_lowercase().contains(&text.to_lowercase()))
}

#[test]
fn scanned_form_text_is_read_where_it_is_printed() {
    let Some(dir) = models() else { return };
    let loaded = load_read("scanned_form.pdf", &dir);
    let runs = &loaded.text[&0];
    for (text, x, y) in [
        ("Membership Application", 52.0, 43.0),
        ("Full name", 52.0, 175.0),
        ("Occupation", 51.0, 304.0),
        ("Email", 51.0, 332.0),
        ("Signature", 49.0, 712.0),
    ] {
        let r = find(runs, text).unwrap_or_else(|| panic!("{text} not read: {runs:#?}"));
        assert!(
            (r.rect.x0 - x).abs() < 6.0 && (r.rect.y0 - y).abs() < 6.0,
            "{text} at {:?}",
            r.rect
        );
        assert_eq!(r.xs.len(), r.text.chars().count());
    }
}

/// Reading a scan keeps every box detection found on the synthetic
/// samples (their boxes are all real).
#[test]
fn reading_scans_keeps_real_boxes() {
    let Some(dir) = models() else { return };
    for name in [
        "scanned_form.pdf",
        "scanned_table.pdf",
        "household_scanned.pdf",
    ] {
        let plain = load(sample(name)).unwrap();
        let read = load_read(name, &dir);
        assert_eq!(
            read.fields.len(),
            plain.fields.len(),
            "{name}: boxes dropped"
        );
    }
}

/// The scanned form's boxes get the labels its digital twin's boxes get
/// from the text layer (allowing a misread letter or two).
#[test]
fn scanned_boxes_get_the_labels_of_the_digital_form() {
    let Some(dir) = models() else { return };
    let digital = load(sample("flat_form.pdf")).unwrap();
    let want = label_fields(&digital.fields, &digital.text);
    let read = load_read("scanned_form.pdf", &dir);
    let got = label_fields(&read.fields, &read.text);
    let (mut right, mut wrong) = (0, vec![]);
    for (f, w) in digital.fields.iter().zip(&want) {
        let Some(w) = &w.label else { continue };
        let (cx, cy) = f.rect.center();
        let g = read
            .fields
            .iter()
            .position(|o| {
                let (ox, oy) = o.rect.center();
                (ox - cx).abs() < 6.0 && (oy - cy).abs() < 6.0
            })
            .and_then(|i| got[i].label.clone())
            .unwrap_or_default();
        if similar(w, &g) {
            right += 1;
        } else {
            wrong.push(format!("{w} -> {g}"));
        }
    }
    let total = right + wrong.len();
    assert!(
        total > 10 && right * 10 >= total * 8,
        "{right} of {total} right: {wrong:#?}"
    );
}

/// Equal but for a few letters (edit distance).
fn similar(a: &str, b: &str) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (
        a.to_lowercase().chars().collect(),
        b.to_lowercase().chars().collect(),
    );
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut prev = row[0];
        row[0] = i;
        for j in 1..=b.len() {
            let cur = row[j];
            row[j] = (prev + usize::from(a[i - 1] != b[j - 1]))
                .min(row[j] + 1)
                .min(row[j - 1] + 1);
            prev = cur;
        }
    }
    row[b.len()] <= (a.len() / 5).max(2)
}

#[test]
fn digital_pages_are_not_read() {
    let Some(dir) = models() else { return };
    let plain = load(sample("flat_form.pdf")).unwrap();
    let read = load_read("flat_form.pdf", &dir);
    assert_eq!(plain.text, read.text);
}
