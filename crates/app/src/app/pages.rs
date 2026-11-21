//! Organising pages: moving (dragging thumbnails), deleting, inserting
//! blank pages, pages of another PDF or a picture, and extracting pages to a
//! new file. Like turning pages, each change makes a new document
//! (`pdfr_core::pages`) and is one undo step; everything kept per page
//! follows its page to its new place.
//!
//! A digitally signed file asks first: its signatures are removed, since
//! they could not stay valid once pages change.

use super::files::{DialogResult, builtin, native_dialogs};
use super::*;

/// A change to the document's pages.
#[derive(Clone, Debug)]
pub(super) enum PageOp {
    /// The pages in this order (old indices; pages left out are deleted),
    /// then page `show` (new index) is shown and `done` said.
    Arrange {
        order: Vec<usize>,
        show: usize,
        done: String,
    },
    /// A blank page at `at`, the size of page `like`.
    Blank { at: usize, like: usize },
    /// The pages of a PDF, or a picture, inserted at `at`.
    File { at: usize, path: PathBuf },
}

/// "page 3" or "3 pages".
fn pages_text(pages: &[usize]) -> String {
    match pages {
        [p] => format!("page {}", p + 1),
        _ => format!("{} pages", pages.len()),
    }
}

impl App {
    /// Changes the pages, asking first if that removes digital signatures.
    pub(super) fn page_op(&mut self, op: PageOp) {
        let Some(doc) = &self.doc else { return };
        if !doc.form.editable() {
            self.status = "This file cannot be edited, so its pages cannot be changed.".into();
            return;
        }
        if doc.form.is_digitally_signed() {
            self.page_confirm = Some(op);
            return;
        }
        self.run_page_op(op);
    }

    fn run_page_op(&mut self, op: PageOp) {
        let Some(doc) = &self.doc else { return };
        let form = &doc.form;
        let before = form.pages.len();
        let (res, show, done) = match op {
            PageOp::Arrange { order, show, done } => (form.arranged(&order), show, done),
            PageOp::Blank { at, like } => (
                form.with_blank_page(at, like),
                at,
                format!("Added a blank page {}.", at + 1),
            ),
            PageOp::File { at, path } => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let res = std::fs::read(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|b| {
                        if b.starts_with(b"\x89PNG") || b.starts_with(&[0xFF, 0xD8]) {
                            form.with_image(at, &b)
                        } else if b.windows(5).take(1024).any(|w| w == b"%PDF-") {
                            form.with_pdf(at, &b)
                        } else {
                            Err("pick a PDF, or a JPEG or PNG picture".into())
                        }
                    });
                let added = res.as_ref().map_or(0, |f| f.pages.len() - before);
                let what = if added == 1 {
                    "a page".to_string()
                } else {
                    format!("{added} pages")
                };
                (res, at, format!("Added {what} from {name}."))
            }
        };
        match res {
            Ok(form) => self.change_pages(Arc::new(form), show, done),
            Err(e) => self.status = format!("Could not change the pages: {e}"),
        }
    }

    /// Shows `form` (the document with its pages changed) as one undo step.
    fn change_pages(&mut self, form: Arc<FormDoc>, show: usize, done: String) {
        let Some(doc) = &self.doc else { return };
        let signed = doc.form.is_digitally_signed();
        self.snapshot();
        self.typing_run = None;
        let doc = self.doc.as_mut().unwrap();
        let map = pdfr_core::pages::page_map(&doc.form, &form);
        let mut old_to_new = vec![None; doc.form.pages.len()];
        for (i, o) in map.iter().enumerate() {
            if let Some(o) = *o {
                old_to_new[o] = Some(i);
            }
        }
        let new_page = |p: usize| old_to_new.get(p).copied().flatten();
        // Boxes follow their page; those of deleted pages go with them.
        doc.entries.retain_mut(|e| match new_page(e.f.page) {
            Some(n) => {
                e.f.page = n;
                true
            }
            None => false,
        });
        // Comments and redaction marks follow their page too.
        doc.marks.retain_mut(|e| match new_page(e.m.page) {
            Some(n) => {
                e.m.page = n;
                true
            }
            None => false,
        });
        doc.redactions.retain_mut(|r| match new_page(r.page) {
            Some(n) => {
                r.page = n;
                true
            }
            None => false,
        });
        // Scans not read yet keep waiting at their new place.
        doc.unread_scans = doc
            .unread_scans
            .iter()
            .filter_map(|&p| new_page(p))
            .collect();
        // The comments of inserted pages.
        for m in form
            .markups
            .iter()
            .filter(|m| map.get(m.page) == Some(&None))
        {
            doc.add_mark(m.clone());
        }
        // The fields of inserted pages.
        for f in form
            .fields
            .iter()
            .filter(|f| map.get(f.page) == Some(&None))
        {
            doc.add(f.clone());
        }
        doc.dirty = true;
        if let Some(f) = &self.focus
            && doc.index_of(f.id).is_none()
        {
            self.focus = None;
        }
        self.side_sel = self.side_sel.iter().filter_map(|&p| new_page(p)).collect();
        self.side_anchor = self.side_anchor.and_then(new_page);
        self.ctx_field = None;
        self.choice_open = None;
        self.apply_form(form);
        self.assist.invalidate();
        self.scroll_to_page(show);
        self.status = if signed {
            format!("{done} The digital signatures were removed. Ctrl+Z undoes it.")
        } else {
            format!("{done} Ctrl+Z undoes it.")
        };
    }

    /// Deletes `pages` (at least one page stays).
    pub(super) fn delete_pages(&mut self, pages: &[usize]) {
        let n = self.page_count();
        let order: Vec<usize> = (0..n).filter(|p| !pages.contains(p)).collect();
        if order.is_empty() {
            self.status = "A PDF needs at least one page.".into();
            return;
        }
        let first = pages.iter().copied().min().unwrap_or(0);
        self.page_op(PageOp::Arrange {
            show: first.min(order.len() - 1),
            order,
            done: format!("Deleted {}.", pages_text(pages)),
        });
    }

    /// Moves `pages` (in page order) to before page `slot` (`slot` = page
    /// count: to the end).
    pub(super) fn move_pages(&mut self, pages: &[usize], slot: usize) {
        let n = self.page_count();
        let mut moving: Vec<usize> = pages.iter().copied().filter(|&p| p < n).collect();
        moving.sort_unstable();
        moving.dedup();
        let mut order: Vec<usize> = (0..n).filter(|p| !moving.contains(p)).collect();
        let at = slot.min(n) - moving.iter().filter(|&&p| p < slot).count();
        order.splice(at..at, moving.iter().copied());
        if moving.is_empty() || order.iter().enumerate().all(|(i, &p)| i == p) {
            return;
        }
        let done = match moving.as_slice() {
            [p] => format!("Moved page {} to page {}.", p + 1, at + 1),
            _ => format!("Moved {} pages.", moving.len()),
        };
        self.page_op(PageOp::Arrange {
            order,
            show: at,
            done,
        });
    }

    pub(super) fn page_count(&self) -> usize {
        self.doc.as_ref().map_or(0, |d| d.sizes.len())
    }

    /// Asks for a PDF or picture to insert at page `at`.
    pub(super) fn pick_insert_file(&mut self, at: usize) {
        if self.dialog_open() {
            return;
        }
        const EXTS: [&str; 8] = ["pdf", "PDF", "png", "PNG", "jpg", "JPG", "jpeg", "JPEG"];
        if !native_dialogs() {
            let mut d = builtin::picker("PDFs and pictures", &EXTS);
            d.pick_file();
            self.builtin_dialog = Some((d, builtin::Purpose::InsertFile(at)));
            return;
        }
        let task = self
            .file_dialog()
            .add_filter("PDFs and pictures", &EXTS)
            .pick_file();
        self.wait_for_dialog(async move {
            DialogResult::InsertFile(task.await.map(|h| h.path().to_path_buf()), at)
        });
    }

    /// Asks where to save `pages` as a new PDF.
    pub(super) fn pick_extract(&mut self, pages: Vec<usize>) {
        let Some(doc) = &self.doc else { return };
        if self.dialog_open() || !doc.form.editable() {
            return;
        }
        let stem = doc
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "document".into());
        let name = match pages.as_slice() {
            [p] => format!("{stem}-page-{}.pdf", p + 1),
            _ => format!("{stem}-pages.pdf"),
        };
        let generation = doc.generation;
        if !native_dialogs() {
            let mut d = builtin::dialog(doc.path.parent(), Some(&name));
            d.save_file();
            self.builtin_dialog = Some((d, builtin::Purpose::Extract(pages, generation)));
            return;
        }
        let mut dlg = self
            .file_dialog()
            .add_filter("PDF", &["pdf"])
            .set_file_name(name);
        if let Some(d) = doc.path.parent() {
            dlg = dlg.set_directory(d);
        }
        let task = dlg.save_file();
        self.wait_for_dialog(async move {
            DialogResult::Extract(
                task.await.map(|h| h.path().to_path_buf()),
                pages,
                generation,
            )
        });
    }

    /// Writes `pages` (with what is filled in on them) to `target`.
    pub(super) fn extract_to(&mut self, target: PathBuf, pages: &[usize], generation: u64) {
        let Some(doc) = self.doc.as_ref().filter(|d| d.generation == generation) else {
            self.status = "Not extracted: the document changed meanwhile.".into();
            return;
        };
        let res = pdfr_core::pages::extract(&doc.form, &doc.fields(), &doc.mark_list(), pages)
            .and_then(|b| std::fs::write(&target, b).map_err(|e| e.to_string()));
        self.status = match res {
            Ok(()) => format!("Saved {} as {}.", pages_text(pages), target.display()),
            Err(e) => format!("Could not extract: {e}"),
        };
    }

    /// The question before page changes remove digital signatures.
    pub(super) fn page_confirm_window(&mut self, ctx: &egui::Context) {
        use crate::assist_ui::widgets::{Colors, danger_button, panel_frame, secondary_button};
        if self.page_confirm.is_none() {
            return;
        }
        let pal = self.palette.clone();
        let c = Colors::new(&pal);
        let mut cancel = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
        let mut go = false;
        egui::Modal::new(Id::unique("pdfr-pages-signed"))
            .frame(panel_frame(ctx, &pal, &c, 20))
            .show(ctx, |ui| {
                ui.set_width(420.0);
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                ui.label(
                    egui::RichText::new("Remove the digital signatures?")
                        .size(20.0)
                        .strong()
                        .color(c.text),
                );
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(
                            "This file is digitally signed. Changing its pages will remove the \
                             digital signatures, because they can't stay valid. Ctrl+Z puts \
                             them back until you save.",
                        )
                        .color(c.weak),
                    )
                    .wrap(),
                );
                ui.add_space(4.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    go = danger_button(ui, "Remove and change pages", false).clicked();
                    cancel |= secondary_button(ui, &c, "Cancel")
                        .on_hover_text("Esc")
                        .clicked();
                });
            });
        if cancel {
            self.page_confirm = None;
            self.status = "Pages not changed.".into();
        } else if go && let Some(op) = self.page_confirm.take() {
            self.run_page_op(op);
        }
    }
}
