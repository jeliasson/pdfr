//! Embeds the built-in font once per file as a Type0 font (Identity-H,
//! CIDFontType2, `/W` widths, ToUnicode) so filled text is searchable.

use crate::font::{FONT_BYTES, FONT_PS_NAME, Font, font};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, StringFormat, dictionary};
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Formats a number compactly for content streams.
pub fn num(v: f32) -> String {
    let mut s = format!("{:.3}", v);
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if s == "-0" { "0".into() } else { s }
}

/// Finds the copy of the built-in font embedded by an earlier save. Matched
/// by name too, so a file saved by a version with another font gets the
/// current one added rather than mixing glyph ids.
pub fn find_font(d: &Document) -> Option<ObjectId> {
    d.objects.iter().find_map(|(id, o)| {
        let dict = o.as_dict().ok()?;
        let ours = dict.get(b"Subtype").ok()?.as_name().ok()? == b"Type0" && dict.has(b"PdfrFont");
        (ours && dict.get(b"BaseFont").ok()?.as_name().ok()? == FONT_PS_NAME.as_bytes())
            .then_some(*id)
    })
}

/// Returns the font object id, embedding the font if needed.
pub fn ensure_font(d: &mut Document) -> ObjectId {
    if let Some(id) = find_font(d) {
        return id;
    }
    let f: &Font = font();
    let ps = FONT_PS_NAME;
    let font_bytes = FONT_BYTES;
    let upem = f.units_per_em();
    let scale = 1000.0 / upem;
    let mut file = Stream::new(
        dictionary! { "Length1" => font_bytes.len() as i64 },
        font_bytes.to_vec(),
    );
    let _ = file.compress();
    let file_id = d.add_object(file);
    let bb = f.bbox();
    let descriptor = dictionary! {
        "Type" => "FontDescriptor",
        "FontName" => Object::Name(ps.as_bytes().to_vec()),
        "Flags" => 32,
        "FontBBox" => vec![
            Object::Real(bb[0] as f32 * scale), Object::Real(bb[1] as f32 * scale),
            Object::Real(bb[2] as f32 * scale), Object::Real(bb[3] as f32 * scale),
        ],
        "ItalicAngle" => 0,
        "Ascent" => Object::Real(f.ascent() * 1000.0),
        "Descent" => Object::Real(-f.descent() * 1000.0),
        "CapHeight" => Object::Real(f.cap_height() * 1000.0),
        "StemV" => 80,
        "FontFile2" => Object::Reference(file_id),
    };
    let desc_id = d.add_object(descriptor);
    let widths: Vec<Object> = (0..f.num_glyphs())
        .map(|g| Object::Real((f.advance_units(g) as f32 * scale * 1000.0).round() / 1000.0))
        .collect();
    let cid = dictionary! {
        "Type" => "Font",
        "Subtype" => "CIDFontType2",
        "BaseFont" => Object::Name(ps.as_bytes().to_vec()),
        "CIDSystemInfo" => dictionary! {
            "Registry" => Object::String(b"Adobe".to_vec(), StringFormat::Literal),
            "Ordering" => Object::String(b"Identity".to_vec(), StringFormat::Literal),
            "Supplement" => 0,
        },
        "FontDescriptor" => Object::Reference(desc_id),
        "DW" => 1000,
        "W" => vec![Object::Integer(0), Object::Array(widths)],
        "CIDToGIDMap" => "Identity",
    };
    let cid_id = d.add_object(cid);
    let tu_id = d.add_object(Stream::new(Dictionary::new(), to_unicode_cmap(f)));
    d.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type0",
        "BaseFont" => Object::Name(ps.as_bytes().to_vec()),
        "Encoding" => "Identity-H",
        "DescendantFonts" => vec![Object::Reference(cid_id)],
        "ToUnicode" => Object::Reference(tu_id),
        "PdfrFont" => true,
    })
}

/// Glyph id -> character, for every Unicode mapping in the font.
pub fn gid_to_char(f: &Font) -> BTreeMap<u16, char> {
    let face = f.face();
    let mut map = BTreeMap::new();
    if let Some(cmap) = face.tables().cmap {
        for st in cmap.subtables {
            if !st.is_unicode() {
                continue;
            }
            st.codepoints(|cp| {
                if let (Some(c), Some(g)) = (char::from_u32(cp), st.glyph_index(cp)) {
                    let e = map.entry(g.0).or_insert(c);
                    if (c as u32) < (*e as u32) {
                        *e = c;
                    }
                }
            });
        }
    }
    map
}

fn to_unicode_cmap(f: &Font) -> Vec<u8> {
    let map = gid_to_char(f);
    let mut s = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
         1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let entries: Vec<(u16, char)> = map.into_iter().collect();
    for chunk in entries.chunks(100) {
        let _ = writeln!(s, "{} beginbfchar", chunk.len());
        for (g, c) in chunk {
            let mut buf = [0u16; 2];
            let units = c.encode_utf16(&mut buf);
            let hex: String = units.iter().map(|u| format!("{u:04X}")).collect();
            let _ = writeln!(s, "<{g:04X}> <{hex}>");
        }
        s.push_str("endbfchar\n");
    }
    s.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    s.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn number_format() {
        assert_eq!(num(1.0), "1");
        assert_eq!(num(1.25), "1.25");
        assert_eq!(num(-0.0001), "0");
        assert_eq!(num(12.3456), "12.346");
    }
    #[test]
    fn tounicode_covers_ascii_and_latin() {
        let f = crate::font::font();
        let m = gid_to_char(f);
        for c in ['A', 'z', '0', 'é', 'Å', 'ñ', '€'] {
            assert_eq!(m.get(&f.glyph_id(c)), Some(&c), "{c}");
        }
    }
}
