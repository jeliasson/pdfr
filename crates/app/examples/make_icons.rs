//! Renders the app icon from `assets/icon/*.svg` into the PNG sizes used by
//! Linux desktops, `pdfr.ico` (Windows) and `pdfr.icns` (macOS). The outputs
//! are committed; run this only after changing the SVGs:
//!
//!     cargo run -p pdfr --example make_icons

use std::path::Path;

/// Linux hicolor sizes. Small sizes use simplified drawings.
const PNG_SIZES: &[u32] = &[16, 22, 24, 32, 48, 64, 128, 256, 512, 1024];
const ICO_SIZES: &[u32] = &[16, 24, 32, 48, 64, 128, 256];

fn source_for(size: u32) -> &'static str {
    match size {
        0..=24 => "pdfr-tiny.svg",
        25..=48 => "pdfr-small.svg",
        _ => "pdfr.svg",
    }
}

fn render(dir: &Path, svg: &str, size: u32) -> Vec<u8> {
    let data = std::fs::read(dir.join(svg)).expect("read svg");
    let tree =
        resvg::usvg::Tree::from_data(&data, &resvg::usvg::Options::default()).expect("parse svg");
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size).unwrap();
    let scale = size as f32 / tree.size().width();
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap.encode_png().expect("encode png")
}

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/icon");
    let png_dir = dir.join("png");
    std::fs::create_dir_all(&png_dir).unwrap();
    for &s in PNG_SIZES {
        let png = render(&dir, source_for(s), s);
        std::fs::write(png_dir.join(format!("pdfr-{s}.png")), png).unwrap();
    }

    // The icon macOS shows while pdfr runs (the Dock takes it from the
    // window): drawn on Apple's grid like the .icns, so it is as large as
    // other apps' icons, not edge to edge.
    let png = render(&dir, "pdfr-macos.svg", 512);
    std::fs::write(png_dir.join("pdfr-macos-512.png"), png).unwrap();

    // .ico: a directory of PNG images (Windows Vista and later).
    let images: Vec<(u32, Vec<u8>)> = ICO_SIZES
        .iter()
        .map(|&s| (s, render(&dir, source_for(s), s)))
        .collect();
    let mut ico = vec![0, 0, 1, 0];
    ico.extend((images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (s, png) in &images {
        let b = if *s >= 256 { 0 } else { *s as u8 };
        ico.extend([b, b, 0, 0]);
        ico.extend(1u16.to_le_bytes());
        ico.extend(32u16.to_le_bytes());
        ico.extend((png.len() as u32).to_le_bytes());
        ico.extend(offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in &images {
        ico.extend(png);
    }
    std::fs::write(dir.join("pdfr.ico"), ico).unwrap();

    // .icns: PNG entries, drawn on Apple's icon grid.
    let entries: &[(&[u8; 4], u32)] = &[
        (b"icp4", 16),
        (b"icp5", 32),
        (b"icp6", 64),
        (b"ic07", 128),
        (b"ic08", 256),
        (b"ic09", 512),
        (b"ic10", 1024),
        (b"ic11", 32),
        (b"ic12", 64),
        (b"ic13", 256),
        (b"ic14", 512),
    ];
    let mut body = Vec::new();
    for (kind, s) in entries {
        let png = render(&dir, "pdfr-macos.svg", *s);
        body.extend(*kind);
        body.extend((png.len() as u32 + 8).to_be_bytes());
        body.extend(png);
    }
    let mut icns = b"icns".to_vec();
    icns.extend((body.len() as u32 + 8).to_be_bytes());
    icns.extend(body);
    std::fs::write(dir.join("pdfr.icns"), icns).unwrap();
    println!("wrote icons to {}", dir.display());
}
