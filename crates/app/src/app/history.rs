//! Undo and redo. Snapshots of the fields; typing in one field coalesces
//! into one step, and fields found by detection are never undone.

use super::*;

/// One undo step: the fields, and the document as it was (pages can be
/// turned; the document is shared, so keeping it costs nothing).
pub(super) struct Snap {
    pub entries: Vec<Entry>,
    pub form: Arc<FormDoc>,
}

impl App {
    pub(super) fn snapshot(&mut self) {
        if let Some(doc) = &self.doc {
            self.undo.push(Snap {
                entries: doc.entries.clone(),
                form: doc.form.clone(),
            });
            if self.undo.len() > 200 {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }

    pub(super) fn undo(&mut self, redo: bool) {
        let Some(doc) = &mut self.doc else { return };
        let (from, to) = if redo {
            (&mut self.redo, &mut self.undo)
        } else {
            (&mut self.undo, &mut self.redo)
        };
        if let Some(prev) = from.pop() {
            to.push(Snap {
                entries: std::mem::replace(&mut doc.entries, prev.entries),
                form: doc.form.clone(),
            });
            if !Arc::ptr_eq(&prev.form, &doc.form) {
                // Pages were turned in this step: show them as they were.
                let changed: Vec<usize> = (0..prev.form.pages.len())
                    .filter(|&i| {
                        doc.form.pages.get(i).map(|g| g.rotate) != Some(prev.form.pages[i].rotate)
                    })
                    .collect();
                self.apply_form(prev.form, &changed);
            }
            let doc = self.doc.as_mut().unwrap();
            doc.reorder();
            doc.dirty = true;
            if let Some(f) = &mut self.focus {
                if let Some(i) = doc.index_of(f.id) {
                    let len = doc.entries[i].f.value.chars().count();
                    f.ed = Editor::at(len);
                    f.ghost = None;
                    f.ghost_dismissed = false;
                } else {
                    self.focus = None;
                }
            }
        }
        self.typing_run = None;
        self.choice_open = None;
        self.assist.close_list();
        self.assist.invalidate();
        self.refresh_ghost();
    }

    /// Records an undo point before an edit of field `id` (typing in one field coalesces).
    pub(super) fn before_edit(&mut self, id: u64) {
        if self.typing_run != Some(id) {
            self.snapshot();
            self.typing_run = Some(id);
        }
        if let Some(doc) = &mut self.doc {
            doc.dirty = true;
        }
    }
}
