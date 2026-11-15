//! Dev tool: run detection on every page and draw the result.
//! cargo run --release -p pdfr-core --example detect -- in.pdf out_prefix
use pdfr_core::detect::{DETECT_SCALE, Gray, detect_with_analysis};
use pdfr_core::field::FieldKind;
use pdfr_core::render::{RenderCache, open, render_page};
use std::time::Instant;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let pdf = open(std::fs::read(&a[1]).unwrap()).unwrap();
    let cache = RenderCache::new();
    let only: Option<usize> = std::env::var("PAGE").ok().and_then(|s| s.parse().ok());
    for (pi, page) in pdf
        .pages()
        .iter()
        .enumerate()
        .filter(|(i, _)| only.is_none_or(|p| p == *i))
    {
        let t = Instant::now();
        let img = render_page(page, &cache, DETECT_SCALE);
        let tr = t.elapsed();
        let gray = img.to_gray();
        let g = Gray {
            w: img.width as usize,
            h: img.height as usize,
            px: &gray,
            scale: DETECT_SCALE,
        };
        let t = Instant::now();
        let (fields, an) = detect_with_analysis(&g);
        println!(
            "page {pi}: render {:.0} ms, detect {:.0} ms, {} h-lines, {} v-lines, {} boxes, {} fields",
            tr.as_secs_f64() * 1e3,
            t.elapsed().as_secs_f64() * 1e3,
            an.hlines.len(),
            an.vlines.len(),
            an.boxes.len(),
            fields.len()
        );
        if std::env::var("DEBUG").is_ok() {
            for b in &an.boxes {
                println!("  box {:.1},{:.1} {:.1},{:.1}", b.x0, b.y0, b.x1, b.y1);
            }
            for l in an.hlines.iter().filter(|l| l.len() > 20.0) {
                println!(
                    "  h y={:.2} {:.1}..{:.1} t={:.2}",
                    l.pos, l.a0, l.a1, l.thick
                );
            }
            for l in an.vlines.iter().filter(|l| l.len() > 8.0) {
                println!(
                    "  v x={:.2} {:.1}..{:.1} t={:.2}",
                    l.pos, l.a0, l.a1, l.thick
                );
            }
        }
        let mut rgba = img.rgba.clone();
        let w = img.width as i64;
        let mut rect = |r: &pdfr_core::geom::Rect, c: [u8; 3]| {
            let s = DETECT_SCALE;
            let (x0, y0, x1, y1) = (
                (r.x0 * s) as i64,
                (r.y0 * s) as i64,
                (r.x1 * s) as i64,
                (r.y1 * s) as i64,
            );
            for y in y0.max(0)..y1.min(img.height as i64) {
                for x in x0.max(0)..x1.min(w) {
                    let edge = x - x0 < 2 || x1 - x <= 2 || y - y0 < 2 || y1 - y <= 2;
                    let i = ((y * w + x) * 4) as usize;
                    for k in 0..3 {
                        rgba[i + k] = if edge {
                            c[k]
                        } else {
                            ((rgba[i + k] as u16 + (c[k] as u16)) / 2) as u8
                        };
                    }
                }
            }
        };
        for f in &fields {
            let c = match &f.kind {
                FieldKind::Checkbox { .. } => [0, 160, 0],
                k if k.is_comb() => [200, 0, 200],
                k if k.is_multiline() => [0, 120, 255],
                FieldKind::Text {
                    mode: pdfr_core::field::TextMode::OnLine,
                    ..
                } => [255, 120, 0],
                _ => [255, 0, 0],
            };
            rect(&f.rect, c);
            if let Some(pdfr_core::field::TextMode::Comb(cells)) = f.kind.text_mode() {
                for c in cells {
                    rect(
                        &pdfr_core::geom::Rect::new(
                            c.0 + 1.0,
                            f.rect.y1 - 3.0,
                            c.1 - 1.0,
                            f.rect.y1 - 1.0,
                        ),
                        [0, 0, 0],
                    );
                }
            }
            println!(
                "  {} {:.1},{:.1} {:.1},{:.1}{}",
                f.kind.tag(),
                f.rect.x0,
                f.rect.y0,
                f.rect.x1,
                f.rect.y1,
                f.kind
                    .text_mode()
                    .and_then(|m| if let pdfr_core::field::TextMode::Comb(c) = m {
                        Some(format!(" cells={}", c.len()))
                    } else {
                        None
                    })
                    .unwrap_or_default()
            );
        }
        let f = std::fs::File::create(format!("{}-{pi}.png", a[2])).unwrap();
        let mut enc = png::Encoder::new(f, img.width, img.height);
        enc.set_color(png::ColorType::Rgba);
        enc.write_header().unwrap().write_image_data(&rgba).unwrap();
    }
}
#[allow(dead_code)]
fn unused() {}
