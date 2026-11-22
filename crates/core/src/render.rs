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

/// Darkest and lightest grey of a page shown dark: soft, not pure black
/// and white.
pub const DARK_PAPER: u8 = 28;
pub const DARK_INK: u8 = 226;

impl Image {
    /// Swaps light and dark but keeps colours' hue and saturation (HSL
    /// lightness L becomes 1 − L), on a soft grey range: white paper turns
    /// dark grey, black text light grey, a blue stamp stays blue.
    pub fn invert_lightness(&mut self) {
        let (lo, hi) = (DARK_PAPER as f32, DARK_INK as f32);
        for p in self.rgba.as_chunks_mut::<4>().0 {
            let max = p[0].max(p[1]).max(p[2]) as i32;
            let min = p[0].min(p[1]).min(p[2]) as i32;
            for c in &mut p[..3] {
                let v = (*c as i32 + 255 - max - min).clamp(0, 255) as f32;
                *c = (lo + v * (hi - lo) / 255.0).round() as u8;
            }
        }
    }

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

/// The longest side, in pixels, of any page render (and so of any texture
/// made from one). [`render_page`] lowers the scale to stay within it.
pub const MAX_RENDER_SIDE: f32 = 8192.0;

/// The scale [`render_page`] really renders `page` at when asked for
/// `scale`: lowered so that neither side exceeds [`MAX_RENDER_SIDE`] (a page
/// can claim to be kilometres wide), and 1 when `scale` is not a positive
/// number.
pub fn render_scale(page: &Page<'_>, scale: f32) -> f32 {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let (w, h) = page.render_dimensions();
    let side = w.abs().max(h.abs());
    if side.is_finite() && side * scale > MAX_RENDER_SIDE {
        MAX_RENDER_SIDE / side
    } else {
        scale
    }
}

/// Renders a page at `scale` pixels per point (lowered by
/// [`render_scale`] for a huge page), with annotations (widget
/// appearances), on a white background.
pub fn render_page<'a>(page: &'a Page<'a>, cache: &RenderCache<'a>, scale: f32) -> Image {
    let scale = render_scale(page, scale);
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

/// The paper colour just around `r` (display points) on page `page` of
/// the PDF `bytes`: the most common colour in a thin ring outside it, so
/// covering printed text matches a shaded or tinted page. White when the
/// page can't be read.
pub fn paper_around(bytes: Arc<Vec<u8>>, page: usize, r: &crate::geom::Rect) -> [u8; 3] {
    const S: f32 = 2.0;
    let Ok(pdf) = Pdf::new(bytes) else {
        return [255; 3];
    };
    let pages = pdf.pages();
    let Some(p) = pages.get(page) else {
        return [255; 3];
    };
    let cache = RenderCache::new();
    let img = render_page(p, &cache, S);
    let (w, h) = (img.width as i64, img.height as i64);
    let px = |v: f32| (v * S).round() as i64;
    let (inner, outer) = (1.0, 4.0);
    let (ox0, oy0, ox1, oy1) = (
        px(r.x0 - outer).max(0),
        px(r.y0 - outer).max(0),
        px(r.x1 + outer).min(w),
        px(r.y1 + outer).min(h),
    );
    let (ix0, iy0, ix1, iy1) = (
        px(r.x0 - inner),
        px(r.y0 - inner),
        px(r.x1 + inner),
        px(r.y1 + inner),
    );
    // Colours binned to 4 bits a channel; the fullest bin's average wins.
    let mut bins: std::collections::HashMap<[u8; 3], ([u64; 3], u64)> = Default::default();
    for y in oy0..oy1 {
        for x in ox0..ox1 {
            if (ix0..ix1).contains(&x) && (iy0..iy1).contains(&y) {
                continue;
            }
            let i = ((y * w + x) * 4) as usize;
            let c = [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]];
            let e = bins.entry([c[0] >> 4, c[1] >> 4, c[2] >> 4]).or_default();
            for (sum, v) in e.0.iter_mut().zip(c) {
                *sum += v as u64;
            }
            e.1 += 1;
        }
    }
    bins.values()
        .max_by_key(|(_, n)| *n)
        .map(|(sum, n)| [(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8])
        .unwrap_or([255; 3])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_pages_keep_hue() {
        let mut img = Image {
            width: 3,
            height: 1,
            rgba: vec![255, 255, 255, 255, 0, 0, 0, 255, 30, 60, 200, 255],
        };
        img.invert_lightness();
        assert_eq!(&img.rgba[0..3], &[DARK_PAPER; 3], "white paper turns dark");
        assert_eq!(&img.rgba[4..7], &[DARK_INK; 3], "black text turns light");
        let b = &img.rgba[8..11];
        assert!(b[2] > b[0] && b[2] > b[1], "blue stays blue: {b:?}");
    }
}
