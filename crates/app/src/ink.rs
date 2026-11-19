//! Signature ink as egui textures, rasterised by the core from the same
//! content stream that is saved (so the screen shows what is written).
//! Cached per drawing and size bucket in egui's temporary memory.

use eframe::egui::{self, ColorImage, Id, TextureHandle, TextureOptions};
use pdfr_core::signature::Ink;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Default)]
struct Cache {
    /// (drawing, width bucket, height bucket) -> texture, last frame used.
    tex: HashMap<(usize, u32, u32), (TextureHandle, u64)>,
    /// Keeps the drawings alive while their textures are cached, so a
    /// pointer key is never reused by another drawing.
    inks: HashMap<usize, Arc<Ink>>,
}

/// Rounds a pixel size up to a step of about 15%, so zooming re-rasterises
/// now and then rather than every frame.
fn bucket(px: f32) -> u32 {
    let px = px.clamp(8.0, 4096.0);
    let step = 1.15f32;
    step.powf((px.ln() / step.ln()).ceil()).round() as u32
}

/// A texture of `ink` for a box of `size` screen pixels.
pub fn texture(ctx: &egui::Context, ink: &Arc<Ink>, size: egui::Vec2) -> TextureHandle {
    // Only the scale is bucketed; the box keeps its exact shape.
    let h = bucket(size.y);
    let w = ((h as f32 * size.x / size.y.max(1.0)).round() as u32).clamp(1, 4096);
    let key = (Arc::as_ptr(ink) as usize, w, h);
    let frame = ctx.cumulative_frame_nr();
    let id = Id::unique("pdfr-ink-cache");
    if let Some(t) = ctx.data_mut(|d| {
        let c = d.get_temp_mut_or_default::<Cache>(id);
        c.tex.get_mut(&key).map(|e| {
            e.1 = frame;
            e.0.clone()
        })
    }) {
        return t;
    }
    let img = pdfr_core::signature::render(ink, key.1, key.2);
    let color =
        ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.rgba);
    let tex = ctx.load_texture(format!("ink-{key:?}"), color, TextureOptions::LINEAR);
    ctx.data_mut(|d| {
        let c = d.get_temp_mut_or_default::<Cache>(id);
        c.tex.insert(key, (tex.clone(), frame));
        c.inks.insert(key.0, ink.clone());
        // Forget what has not been drawn for a while.
        if c.tex.len() > 48 {
            c.tex.retain(|_, (_, f)| frame.saturating_sub(*f) < 120);
            let live: std::collections::HashSet<usize> = c.tex.keys().map(|k| k.0).collect();
            c.inks.retain(|k, _| live.contains(k));
        }
    });
    tex
}

/// Paints `ink` fitted (keeping its shape) into `rect`.
pub fn paint(painter: &egui::Painter, ink: &Arc<Ink>, rect: egui::Rect, tint: egui::Color32) {
    if ink.is_empty() || rect.width() < 1.0 || rect.height() < 1.0 {
        return;
    }
    let ppp = painter.ctx().pixels_per_point();
    let tex = texture(painter.ctx(), ink, rect.size() * ppp);
    painter.image(
        tex.id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        tint,
    );
}
