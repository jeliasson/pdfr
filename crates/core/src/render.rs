//! Thin wrapper over `hayro` for rasterising pages.
//!
//! A [`hayro::RenderCache`] borrows the parsed document, so callers keep the
//! [`Pdf`] and the cache together on one thread's stack (see the app's worker).

use crate::geom::PageGeom;
use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::hayro_syntax::page::{Page, Rotation};
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderSettings};
use std::sync::Arc;

pub use hayro::RenderCache;
pub use hayro::hayro_syntax::Pdf as HayroPdf;

/// An opaque RGBA8 image (straight alpha, always 255).
#[derive(Clone, Debug)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    /// Grayscale copy (luma), one byte per pixel.
    pub fn to_gray(&self) -> Vec<u8> {
        self.rgba
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| ((p[0] as u32 * 299 + p[1] as u32 * 587 + p[2] as u32 * 114) / 1000) as u8)
            .collect()
    }
}

pub fn open(bytes: Vec<u8>) -> Option<Pdf> {
    Pdf::new(Arc::new(bytes)).ok()
}

/// Display geometry of a hayro page (CropBox ∩ MediaBox, plus `/Rotate`).
pub fn page_geom(page: &Page<'_>) -> PageGeom {
    let c = page.intersected_crop_box();
    let rot = match page.rotation() {
        Rotation::None => 0,
        Rotation::Horizontal => 90,
        Rotation::Flipped => 180,
        Rotation::FlippedHorizontal => 270,
    };
    let crop = if c.width() <= 0.0 || c.height() <= 0.0 {
        [0.0, 0.0, 595.0, 842.0]
    } else {
        [c.x0 as f32, c.y0 as f32, c.x1 as f32, c.y1 as f32]
    };
    PageGeom::new(crop, rot)
}

/// Renders a page at `scale` pixels per point, with annotations (widget
/// appearances), on a white background.
pub fn render_page<'a>(page: &'a Page<'a>, cache: &RenderCache<'a>, scale: f32) -> Image {
    let settings = InterpreterSettings::default();
    let pix = hayro::render(
        page,
        cache,
        &settings,
        &RenderSettings::default(),
        &PixmapSettings {
            x_scale: scale,
            y_scale: scale,
            bg_color: WHITE,
        },
    );
    Image {
        width: pix.width() as u32,
        height: pix.height() as u32,
        rgba: pix.data_as_u8_slice().to_vec(),
    }
}
