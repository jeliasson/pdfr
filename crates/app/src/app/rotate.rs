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
        self.apply_form(form);
        self.assist.invalidate();
        let n = pages.len();
        self.status = if n == 1 {
            format!("Turned page {}. Ctrl+Z turns it back.", pages[0] + 1)
        } else {
            format!("Turned {n} pages. Ctrl+Z turns them back.")
        };
    }

    /// Shows `form`, a changed version of the open document (pages turned,
    /// moved, deleted or added). Pages are matched by their PDF objects:
    /// what was made for a page that is still there (pictures, text, found
    /// boxes) follows it to its new place; new and turned pages are drawn,
    /// detected and read again. Fields are the caller's to update.
    pub(super) fn apply_form(&mut self, form: Arc<FormDoc>) {
        let Some(doc) = &mut self.doc else { return };
        let map = page_map(&doc.form, &form);
        let changed: Vec<usize> = (0..form.pages.len())
            .filter(|&i| map[i].is_none_or(|o| doc.form.pages[o] != form.pages[i]))
            .collect();
        let mut old_to_new = vec![None; doc.form.pages.len()];
        for (i, o) in map.iter().enumerate() {
            if let Some(o) = *o
                && !changed.contains(&i)
            {
                old_to_new[o] = Some(i);
            }
        }
        let moved =
            map.len() != old_to_new.len() || map.iter().enumerate().any(|(i, o)| *o != Some(i));
        let render = form.render_bytes();
        let editable = form.editable();
        doc.sizes = form.pages.iter().map(|g| g.display_size()).collect();
        doc.detected = (0..form.pages.len())
            .map(|i| match map[i] {
                Some(o) if !changed.contains(&i) => {
                    doc.detected.get(o).copied().unwrap_or(!editable)
                }
                _ => !editable,
            })
            .collect();
        doc.form = form;
        doc.generation = self
            .worker
            .replace_document(render, &changed, editable, &old_to_new);
        doc.pending.clear();
        remap_keys(&mut doc.tex, &old_to_new);
        remap_keys(&mut doc.thumbs, &old_to_new);
        remap_keys(&mut doc.text, &old_to_new);
        remap_keys(&mut doc.text_links, &old_to_new);
        for links in doc.text_links.values_mut() {
            for l in links.iter_mut() {
                if let Some(Some(n)) = old_to_new.get(l.page) {
                    l.page = *n;
                }
            }
        }
        doc.reorder();
        self.thumbs_asked.clear();
        self.side_follow = None;
        if self
            .text_sel
            .as_ref()
            .is_some_and(|s| moved || changed.contains(&s.page))
        {
            self.text_sel = None;
        }
        if let Some(s) = &mut self.search {
            s.shown = false;
            if moved {
                s.reset_hits();
            }
        }
        self.clamp_scroll();
    }
}

/// Where each page of `new` was in `old` (pages matched by object; a
/// read-only document, which cannot change, maps to itself).
fn page_map(old: &FormDoc, new: &FormDoc) -> Vec<Option<usize>> {
    if new.page_ids.is_empty() {
        return (0..new.pages.len())
            .map(|i| (i < old.pages.len()).then_some(i))
            .collect();
    }
    pdfr_core::pages::page_map(old, new)
}

/// Moves values keyed by old page index to their new index, dropping those
/// whose page is gone or changed.
fn remap_keys<T>(m: &mut HashMap<usize, T>, old_to_new: &[Option<usize>]) {
    *m = std::mem::take(m)
        .into_iter()
        .filter_map(|(k, v)| Some((old_to_new.get(k).copied().flatten()?, v)))
        .collect();
}
