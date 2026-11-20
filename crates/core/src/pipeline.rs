//! Whole-document loading without UI: open, detect every page, extract text.
//! The app does the same work on its background threads; tests and dev
//! tools use this.

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

pub fn load(bytes: Vec<u8>) -> Result<Loaded, OpenError> {
    let form = FormDoc::open(bytes)?;
    let mut fields = form.fields.clone();
    let mut text = HashMap::new();
    let render = form.render_bytes();
    if let Some(pdf) = crate::render::open((*render).clone()) {
        let cache = crate::render::RenderCache::new();
        let tcache = hayro::hayro_interpret::InterpreterCache::new();
        for (i, page) in pdf.pages().iter().enumerate() {
            let runs = crate::text::extract(page, &tcache);
            if form.editable() {
                let mut found = crate::detect::detect_page(page, &cache, i);
                crate::detect::prefill_printed(&mut found, &runs);
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
