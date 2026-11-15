//! The font for typed text: Noto Sans Regular (SIL OFL 1.1), built into the
//! binary so values always look the same, whatever fonts the PDF or the
//! system has.
//!
//! Everything that places typed text — the screen and the PDF writer — asks
//! this module for metrics, so both agree exactly.

use std::sync::OnceLock;
use ttf_parser::{Face, GlyphId};

pub static FONT_BYTES: &[u8] = include_bytes!("../../../assets/NotoSans-Regular.ttf");

/// PostScript name used when embedding the font.
pub const FONT_PS_NAME: &str = "NotoSans-Regular";

pub struct Font {
    face: Face<'static>,
    upem: f32,
    /// Advance per glyph id in font units (cached for speed).
    advances: Vec<u16>,
}

static FONT: OnceLock<Font> = OnceLock::new();

/// The built-in font.
pub fn font() -> &'static Font {
    FONT.get_or_init(|| Font::parse(FONT_BYTES).expect("built-in font parses"))
}

impl Font {
    fn parse(bytes: &'static [u8]) -> Option<Font> {
        let face = Face::parse(bytes, 0).ok()?;
        let n = face.number_of_glyphs();
        let advances = (0..n)
            .map(|g| face.glyph_hor_advance(GlyphId(g)).unwrap_or(0))
            .collect();
        Some(Font {
            upem: face.units_per_em() as f32,
            face,
            advances,
        })
    }

    pub fn face(&self) -> &Face<'static> {
        &self.face
    }
    pub fn units_per_em(&self) -> f32 {
        self.upem
    }
    pub fn num_glyphs(&self) -> u16 {
        self.advances.len() as u16
    }

    /// Maps a character to the character actually drawn: control characters
    /// are dropped (None) and characters missing from the font become `?`.
    pub fn display_char(&self, c: char) -> Option<char> {
        if c.is_control() {
            return None;
        }
        if self.face.glyph_index(c).is_some() {
            Some(c)
        } else {
            Some('?')
        }
    }

    pub fn glyph_id(&self, c: char) -> u16 {
        self.face
            .glyph_index(c)
            .or_else(|| self.face.glyph_index('?'))
            .map_or(0, |g| g.0)
    }

    /// Advance of a glyph id in font units.
    pub fn advance_units(&self, gid: u16) -> u16 {
        self.advances.get(gid as usize).copied().unwrap_or(0)
    }

    /// Advance of a character in points at `size`.
    pub fn advance(&self, c: char, size: f32) -> f32 {
        self.advance_units(self.glyph_id(c)) as f32 * size / self.upem
    }

    pub fn text_width(&self, text: &str, size: f32) -> f32 {
        text.chars()
            .filter_map(|c| self.display_char(c))
            .map(|c| self.advance(c, size))
            .sum()
    }

    /// Cap height as a fraction of the em.
    pub fn cap_height(&self) -> f32 {
        match self.face.capital_height() {
            Some(c) if c > 0 => c as f32 / self.upem,
            _ => 0.7,
        }
    }
    /// Ascender as a fraction of the em (positive).
    pub fn ascent(&self) -> f32 {
        self.face.ascender() as f32 / self.upem
    }
    /// Descender as a fraction of the em (positive number).
    pub fn descent(&self) -> f32 {
        -(self.face.descender() as f32) / self.upem
    }
    /// Font bounding box in font units `[xmin, ymin, xmax, ymax]`.
    pub fn bbox(&self) -> [i16; 4] {
        let b = self.face.global_bounding_box();
        [b.x_min, b.y_min, b.x_max, b.y_max]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metrics_are_sane() {
        let f = font();
        assert_eq!(f.units_per_em(), 1000.0);
        assert!((f.cap_height() - 0.714).abs() < 0.01, "{}", f.cap_height());
        assert!(f.advance('W', 10.0) > f.advance('i', 10.0));
        assert_eq!(f.display_char('\u{7}'), None);
        assert_eq!(f.display_char('é'), Some('é'));
        assert_eq!(f.display_char('Å'), Some('Å'));
        assert_eq!(f.display_char('Ж'), Some('Ж'), "Cyrillic");
        assert_eq!(f.display_char('Ω'), Some('Ω'), "Greek");
    }
}
