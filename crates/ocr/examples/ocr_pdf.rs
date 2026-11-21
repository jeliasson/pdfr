//! Reads the scanned text of a PDF and prints it with timings, the boxes
//! found before and after dropping printed text, and their labels.
//! `cargo run -p pdfr-ocr --example ocr_pdf -- file.pdf MODEL_DIR [pages]`

use pdfr_core::detect::{detect_page_gray, drop_text_boxes};
use pdfr_core::render::RenderCache;
use std::collections::HashMap;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let bytes = std::fs::read(&args[1]).expect("read pdf");
    let t = Instant::now();
    let reader = pdfr_ocr::Reader::load(std::path::Path::new(&args[2])).expect("models");
    eprintln!("load: {:.0} ms", t.elapsed().as_secs_f64() * 1e3);
    let pdf = pdfr_core::render::open(bytes).expect("pdf");
    let cache = RenderCache::new();
    let limit: usize = args
        .get(3)
        .and_then(|s| s.parse().ok())
        .unwrap_or(usize::MAX);
    for (i, page) in pdf.pages().iter().enumerate().take(limit) {
        let t = Instant::now();
        let (mut fields, gray) = detect_page_gray(page, &cache, i);
        let detect_ms = t.elapsed().as_secs_f64() * 1e3;
        let t = Instant::now();
        let runs = reader.read(&gray);
        let ocr_ms = t.elapsed().as_secs_f64() * 1e3;
        let before = fields.len();
        let dropped: Vec<_> = fields
            .iter()
            .filter(|f| {
                let mut one = vec![(*f).clone()];
                drop_text_boxes(&mut one, &runs);
                one.is_empty()
            })
            .map(|f| f.rect)
            .collect();
        drop_text_boxes(&mut fields, &runs);
        println!(
            "== page {} ({} phrases) render+detect {detect_ms:.0} ms, OCR {ocr_ms:.0} ms, boxes {before} -> {}",
            i + 1,
            runs.len(),
            fields.len()
        );
        for r in &dropped {
            println!("  dropped box {r:?}");
        }
        for r in &runs {
            println!(
                "  [{:.0},{:.0} {:.0}x{:.0}] {}",
                r.rect.x0,
                r.rect.y0,
                r.rect.width(),
                r.rect.height(),
                r.text
            );
        }
        let text = HashMap::from([(i, runs)]);
        let labels = pdfr_assist::labels::label_fields(&fields, &text);
        for (f, l) in fields.iter().zip(&labels) {
            println!("  box {:?} label {:?}", f.rect, l.label);
        }
    }
}
