//! Dev probe: renders every page of the given PDFs with hayro, reports timing,
//! checks our rotation mapping against hayro's page transform, and writes PNGs.
//! cargo run --release -p pdfr-core --example probe -- out_dir a.pdf b.pdf ...
use hayro::hayro_interpret::util::TransformExt;
use pdfr_core::render::{RenderCache, open, page_geom, render_page};
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().expect("out dir");
    for path in args {
        let bytes = std::fs::read(&path).unwrap();
        let t = Instant::now();
        let pdf = open(bytes).expect("parse");
        let parse = t.elapsed();
        let cache = RenderCache::new();
        let name = std::path::Path::new(&path)
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_string();
        for (i, page) in pdf.pages().iter().enumerate().take(3) {
            let g = page_geom(page);
            // Rotation check: our user->display mapping must equal hayro's.
            let tr = page.initial_transform(true).to_kurbo();
            let mut maxerr = 0f64;
            for &(ux, uy) in &[
                (g.crop[0], g.crop[1]),
                (g.crop[2], g.crop[3]),
                (100.0, 200.0),
                (g.crop[0] + 7.0, g.crop[3] - 3.0),
            ] {
                let p = tr * hayro::kurbo::Point::new(ux as f64, uy as f64);
                let (dx, dy) = g.to_display(ux, uy);
                maxerr = maxerr
                    .max((p.x - dx as f64).abs())
                    .max((p.y - dy as f64).abs());
            }
            let mut times = vec![];
            for &scale in &[1.0f32, 1.333, 2.0] {
                // Warm run excluded? No: report first (cold) and second (warm) run.
                let t0 = Instant::now();
                let img = render_page(page, &cache, scale);
                let cold = t0.elapsed();
                let t1 = Instant::now();
                let _ = render_page(page, &cache, scale);
                let warm = t1.elapsed();
                times.push(format!(
                    "{scale}x {}x{}: cold {:.1} ms, warm {:.1} ms",
                    img.width,
                    img.height,
                    cold.as_secs_f64() * 1e3,
                    warm.as_secs_f64() * 1e3
                ));
                if scale == 2.0 {
                    let f = std::fs::File::create(format!("{out}/{name}-{i}.png")).unwrap();
                    let mut enc = png::Encoder::new(f, img.width, img.height);
                    enc.set_color(png::ColorType::Rgba);
                    enc.write_header()
                        .unwrap()
                        .write_image_data(&img.rgba)
                        .unwrap();
                }
            }
            println!(
                "{name} p{i} rot={} size={:?} rot-mapping max err {:.4} pt (parse {:.1} ms)\n   {}",
                g.rotate,
                g.display_size(),
                maxerr,
                parse.as_secs_f64() * 1e3,
                times.join("\n   ")
            );
        }
    }
}
