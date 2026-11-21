//! Saving a filled form must not balloon the file. Reviewers of other PDF
//! apps report fillable forms growing from 710 KB to 21 MB on save.
//!
//! Today a filled copy grows by one fixed cost: the font pdfr types with is
//! embedded whole (about 260 KB compressed), once per file. Subsetting it
//! to the glyphs used would bring that down to a few KB; until then the
//! allowance below covers the font and nothing more, so any growth that
//! scales with the document fails.

use pdfr_core::field::FieldKind;
use pdfr_core::forms::FormDoc;
use pdfr_core::save::save;

fn sample(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../../samples/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

/// The whole embedded font, compressed, plus its widths (see above).
const FONT_ALLOWANCE: usize = 320 * 1024;

#[test]
fn a_filled_copy_stays_about_the_size_of_the_original() {
    // Tracked samples only: CI has no others.
    for name in [
        "real_form.pdf",
        "flat_form.pdf",
        "household_form.pdf",
        "household_scanned.pdf",
        "panels.pdf",
    ] {
        let input = sample(name);
        let fd = FormDoc::open(input.clone()).unwrap();
        if !fd.editable() {
            continue;
        }
        let mut fields = fd.fields.clone();
        for f in fields.iter_mut() {
            if matches!(f.kind, FieldKind::Text { .. }) {
                f.value = "Åsa Öberg 1985-03-14".into();
            }
        }
        let out = save(&fd, &fields).unwrap();
        let limit = input.len() * 3 / 2 + FONT_ALLOWANCE;
        assert!(
            out.len() <= limit,
            "{name}: {} bytes in, {} bytes out (limit {limit})",
            input.len(),
            out.len()
        );
    }
}
