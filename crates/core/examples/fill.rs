//! Dev tool: open a PDF, fill every field with demo values, save.
//! cargo run -p pdfr-core --example fill -- in.pdf out.pdf
use pdfr_core::{demo::fill_demo, forms::FormDoc, save::save};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let fd = FormDoc::open(std::fs::read(&a[1]).unwrap()).unwrap();
    let mut fields = fd.fields.clone();
    // Detect on every page (skipping anything overlapping a real field).
    let pdf = pdfr_core::render::open((*fd.render_bytes()).clone()).unwrap();
    let cache = pdfr_core::render::RenderCache::new();
    for (i, page) in pdf.pages().iter().enumerate() {
        for d in pdfr_core::detect::detect_page(page, &cache, i) {
            if !pdfr_core::forms::overlaps_any(&fd.fields, i, &d.rect, 0.3) {
                fields.push(d);
            }
        }
    }
    for f in fields.iter().filter(|f| f.is_real()) {
        println!(
            "{:?} {:?} {:?} {:?} v={:?}",
            f.kind.tag(),
            f.origin,
            f.rect,
            f.font_size,
            f.value
        );
    }
    fill_demo(&mut fields);
    std::fs::write(&a[2], save(&fd, &fields).unwrap()).unwrap();
    std::fs::write(format!("{}.blank.pdf", a[2]), &*fd.render_bytes()).unwrap();
}
