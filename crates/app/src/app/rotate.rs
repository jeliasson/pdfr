//! Fit page (Ctrl+3) and turning pages (Ctrl+R, Ctrl+Shift+R). Turning
//! changes the page's `/Rotate`, so a sideways scan ends up upright on
//! screen, for box detection and in the saved file. It is one undo step.

use super::*;

impl App {
    /// The zoom at which the current page fits the window entirely.
    pub(super) fn fit_page_zoom(&self) -> f32 {
        let Some(doc) = &self.doc else { return 1.0 };
        let (w, h) = doc
            .sizes
            .get(self.current_page())
            .copied()
            .unwrap_or((612.0, 792.0));
        let vp = if self.viewport.is_positive() {
            self.viewport
        } else {
            Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 800.0))
        };
        let zw = (vp.width() - 2.0 * PAD - SCROLLBAR_W) / w.max(1.0);
        let zh = (vp.height() - 2.0 * PAD) / h.max(1.0);
        zw.min(zh).max(0.1)
    }

    pub(super) fn fit_page(&mut self) {
        let page = self.current_page();
        self.zoom = self.fit_page_zoom().clamp(0.1, 8.0);
        self.clamp_scroll();
        self.scroll_to_page(page);
    }

    /// Why `pages` cannot be turned, if they cannot.
    pub(super) fn cannot_rotate(&self, pages: &[usize]) -> Option<String> {
        let doc = self.doc.as_ref()?;
        if !doc.form.editable() {
            return Some("This file cannot be edited, so its pages cannot be turned.".into());
        }
        doc.entries
            .iter()
            .any(|e| e.f.is_real() && pages.contains(&e.f.page))
            .then(|| "Pages with the PDF's own form fields cannot be turned.".into())
    }

    /// Turns `pages` clockwise by `turns` quarter turns, as one undo step.
    pub(super) fn rotate_pages(&mut self, pages: Vec<usize>, turns: i32) {
        if let Some(why) = self.cannot_rotate(&pages) {
            self.status = why;
            return;
        }
        let Some(doc) = &self.doc else { return };
        let form = match doc.form.rotated(&pages, turns) {
            Ok(f) => Arc::new(f),
            Err(e) => {
                self.status = format!("Could not turn the page: {e}");
                return;
            }
        };
        let old = doc.form.clone();
        self.snapshot();
        let doc = self.doc.as_mut().unwrap();
        // Boxes found on a turned page are found again upright; the rest
        // (typed in, signed, added by hand) stay where they are on the paper.
        doc.entries.retain(|e| {
            !(pages.contains(&e.f.page) && e.f.origin == Origin::Detected && e.f.is_empty())
        });
        for e in doc.entries.iter_mut().filter(|e| pages.contains(&e.f.page)) {
            let (from, to) = (old.pages[e.f.page], form.pages[e.f.page]);
            let moved = from.move_to(&to, &e.f.rect);
            if let FieldKind::Text {
                mode: TextMode::Comb(cells),
                ..
            } = &mut e.f.kind
            {
                *cells = Field::equal_cells(&moved, cells.len());
            }
            e.f.rect = moved;
        }
        doc.dirty = true;
        self.apply_form(form, &pages);
        self.assist.invalidate();
        let n = pages.len();
        self.status = if n == 1 {
            format!("Turned page {}. Ctrl+Z turns it back.", pages[0] + 1)
        } else {
            format!("Turned {n} pages. Ctrl+Z turns them back.")
        };
    }

    /// Shows `form` (the same document with `changed` pages turned):
    /// those pages are drawn, detected and read again.
    pub(super) fn apply_form(&mut self, form: Arc<FormDoc>, changed: &[usize]) {
        let Some(doc) = &mut self.doc else { return };
        let render = form.render_bytes();
        let editable = form.editable();
        doc.sizes = form.pages.iter().map(|g| g.display_size()).collect();
        doc.form = form;
        doc.generation = self.worker.replace_document(render, changed, editable);
        doc.pending.clear();
        for &p in changed {
            doc.tex.remove(&p);
            doc.thumbs.remove(&p);
            doc.text.remove(&p);
            doc.text_links.remove(&p);
            if let Some(d) = doc.detected.get_mut(p) {
                *d = !editable;
            }
        }
        doc.reorder();
        self.thumbs_asked.clear();
        if self
            .text_sel
            .as_ref()
            .is_some_and(|s| changed.contains(&s.page))
        {
            self.text_sel = None;
        }
        if let Some(s) = &mut self.search {
            s.shown = false;
        }
        self.clamp_scroll();
    }
}
