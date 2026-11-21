//! Whole-document loading without UI: open, detect every page, extract text.
//! The app does the same work on its background threads; tests and dev
//! tools use this.

use crate::detect::PageGray;
use crate::field::Field;
use crate::forms::{FormDoc, OpenError, overlaps_any};
use crate::text::TextRun;
use std::collections::HashMap;

pub struct Loaded {
    pub form: FormDoc,
    /// Real fields followed by detected ones (overlaps with real fields skipped).
    pub fields: Vec<Field>,
    pub text: HashMap<usize, Vec<TextRun>>,
}

/// Reads the text on a grey page render (OCR), in page points.
pub type Ocr<'a> = &'a dyn Fn(&PageGray) -> Vec<TextRun>;

pub fn load(bytes: Vec<u8>) -> Result<Loaded, OpenError> {
    load_with(bytes, None)
}

/// Like [`load`], reading the text of scanned pages with `ocr`.
pub fn load_with(bytes: Vec<u8>, ocr: Option<Ocr>) -> Result<Loaded, OpenError> {
    let form = FormDoc::open(bytes)?;
    let mut fields = form.fields.clone();
    let mut text = HashMap::new();
    let render = form.render_bytes();
    if let Some(pdf) = crate::render::open((*render).clone()) {
        let cache = crate::render::RenderCache::new();
        let tcache = hayro::hayro_interpret::InterpreterCache::new();
        for (i, page) in pdf.pages().iter().enumerate() {
            let mut runs = crate::text::extract(page, &tcache);
            if form.editable() {
                let (mut found, gray) = crate::detect::detect_page_gray(page, &cache, i);
                let read = ocr.is_some_and(|o| read_scan(&mut found, &mut runs, &gray, o));
                if !read {
                    crate::detect::prefill_printed(&mut found, &runs);
                }
                for d in found {
                    if !overlaps_any(&form.fields, i, &d.rect, 0.3) {
                        fields.push(d);
                    }
                }
            }
            text.insert(i, runs);
        }
    }
    Ok(Loaded { form, fields, text })
}

/// On a page without a text layer, reads its text from the picture: the
/// words become the page's text, and found boxes that are printed text are
/// dropped. Returns whether the page was read this way.
pub fn read_scan(
    fields: &mut Vec<Field>,
    runs: &mut Vec<TextRun>,
    gray: &PageGray,
    ocr: Ocr,
) -> bool {
    if !crate::detect::lacks_text(runs) {
        return false;
    }
    *runs = ocr(gray);
    crate::detect::fit_scan_words(fields, runs);
    true
}
