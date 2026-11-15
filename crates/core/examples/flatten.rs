//! Dev tool: write the print copy (flattened) of a PDF.
//! cargo run --release -p pdfr-core --example flatten -- in.pdf out.pdf
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let flat = pdfr_core::flatten::flatten_for_print(&std::fs::read(&a[1]).unwrap()).unwrap();
    std::fs::write(&a[2], flat).unwrap();
}
