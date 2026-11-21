//! Screenshots for agents: scaled down to a size a model reads well, as PNG.
//!
//! Agents give positions in the pixels of the screenshot they were sent, so
//! the scale is a fixed function of the window's size: it can be known
//! without taking a screenshot, and stays the same between two of them
//! unless the window is resized.

/// The longest side of a screenshot sent to an agent, in pixels.
pub const MAX_EDGE: f32 = 1568.0;

/// The factor from physical pixels to screenshot pixels for a window of
/// this size (never enlarges).
pub fn scale_for(width_px: f32, height_px: f32) -> f32 {
    let long = width_px.max(height_px);
    if long <= MAX_EDGE {
        1.0
    } else {
        MAX_EDGE / long
    }
}

/// Scales an RGBA image by `scale` (at most 1), averaging the source pixels
/// under each new one. Returns the pixels and the new size.
pub fn downscale(rgba: &[u8], w: usize, h: usize, scale: f32) -> (Vec<u8>, usize, usize) {
    if scale >= 1.0 || w == 0 || h == 0 {
        return (rgba.to_vec(), w, h);
    }
    let nw = ((w as f32 * scale).round() as usize).max(1);
    let nh = ((h as f32 * scale).round() as usize).max(1);
    let mut out = vec![0u8; nw * nh * 4];
    // Source column range of each new column (and the same for rows).
    let spans = |n: usize, src: usize| -> Vec<(usize, usize)> {
        (0..n)
            .map(|i| {
                let a = i * src / n;
                let b = (((i + 1) * src).div_ceil(n)).clamp(a + 1, src);
                (a, b)
            })
            .collect()
    };
    let (xs, ys) = (spans(nw, w), spans(nh, h));
    for (oy, &(y0, y1)) in ys.iter().enumerate() {
        for (ox, &(x0, x1)) in xs.iter().enumerate() {
            let mut sum = [0u32; 4];
            for y in y0..y1 {
                let row = &rgba[(y * w + x0) * 4..(y * w + x1) * 4];
                for px in row.as_chunks::<4>().0 {
                    for c in 0..4 {
                        sum[c] += px[c] as u32;
                    }
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u32;
            let at = (oy * nw + ox) * 4;
            for c in 0..4 {
                out[at + c] = ((sum[c] + n / 2) / n) as u8;
            }
        }
    }
    (out, nw, nh)
}

/// Encodes RGBA pixels as a PNG.
pub fn png(rgba: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut enc = png::Encoder::new(&mut bytes, w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_compression(png::Compression::Fast);
    if let Ok(mut wr) = enc.write_header() {
        let _ = wr.write_image_data(rgba);
    }
    bytes
}

/// A window screenshot ready to send: scaled, encoded, base64.
pub fn encode(rgba: &[u8], w: usize, h: usize) -> (String, usize, usize) {
    use base64::Engine;
    let scale = scale_for(w as f32, h as f32);
    let (small, nw, nh) = downscale(rgba, w, h, scale);
    let data = base64::engine::general_purpose::STANDARD.encode(png(&small, nw, nh));
    (data, nw, nh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_windows_are_not_scaled() {
        assert_eq!(scale_for(1100.0, 900.0), 1.0);
        let s = scale_for(3136.0, 2000.0);
        assert!((s - 0.5).abs() < 1e-6);
    }

    #[test]
    fn downscale_averages() {
        // 4x2, left half black, right half white: 2x1 of the same.
        let mut src = vec![];
        for _ in 0..2 {
            for x in 0..4 {
                let v = if x < 2 { 0 } else { 255 };
                src.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let (out, w, h) = downscale(&src, 4, 2, 0.5);
        assert_eq!((w, h), (2, 1));
        assert_eq!(out, vec![0, 0, 0, 255, 255, 255, 255, 255]);
    }

    #[test]
    fn png_has_the_signature() {
        let bytes = png(&[1, 2, 3, 255], 1, 1);
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    }
}
