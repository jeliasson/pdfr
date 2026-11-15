//! Dev tool: render one page at an exact scale.
//! cargo run --release -p pdfr-core --example render -- in.pdf page scale out.png
use pdfr_core::render::{RenderCache, open, render_page};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let pdf = open(std::fs::read(&a[1]).unwrap()).unwrap();
    let page: usize = a[2].parse().unwrap();
    let scale: f32 = a[3].parse().unwrap();
    let cache = RenderCache::new();
    let img = render_page(&pdf.pages()[page], &cache, scale);
    let f = std::fs::File::create(&a[4]).unwrap();
    let mut enc = png::Encoder::new(f, img.width, img.height);
    enc.set_color(png::ColorType::Rgba);
    enc.write_header()
        .unwrap()
        .write_image_data(&img.rgba)
        .unwrap();
}
