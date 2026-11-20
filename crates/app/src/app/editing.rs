//! The focused field: keyboard input, the caret editor, Tab order, ghost
//! suggestions, and every write of a field value.

use super::*;

/// The keys inside a focused box, for the F1 cheatsheet. Keep in step with
/// `key()` below.
pub(super) const BOX_KEYS: &[(&str, &str)] = &[
    ("Next / previous box", "Tab / Shift+Tab"),
    ("Take the suggestion", "Tab, Enter or →"),
    ("Browse suggestions", "↑ / ↓"),
    ("Tick a checkbox", "Space"),
    ("Remove a selected signature", "Delete"),
    ("Open a drop-down", "Space, Enter or Alt+↓"),
    ("Pick in a drop-down", "↑ / ↓ or a letter, Enter"),
    ("Delete a word", "Ctrl+Backspace"),
    ("Select everything", "Ctrl+A"),
    ("Leave the box", "Esc"),
];

impl App {
    /// The keyboard belongs to something else: another widget (a settings
    /// text box) has focus, or a dialog is open.
    pub(super) fn keyboard_elsewhere(&self, ctx: &egui::Context) -> bool {
        let other = ctx.memory(|m| m.focused().is_some_and(|f| f != Id::unique(EDITOR_ID)));
        other
            || self.confirm.is_some()
            || self.assist.modal_open()
            || self.license.modal_open()
            || self.overlay.is_some()
            || self.hints.is_some()
            || self.sign.is_some()
            || self.password.is_some()
    }

    pub fn focus_field(&mut self, id: u64, caret_end: bool, ctx: &egui::Context) {
        let Some(doc) = &self.doc else { return };
        let Some(i) = doc.index_of(id) else { return };
        let len = doc.entries[i].f.value.chars().count();
        self.focus = Some(Focus {
            id,
            ed: Editor::at(if caret_end { len } else { 0 }),
            ghost: None,
            ghost_dismissed: false,
        });
        self.typing_run = None;
        self.choice_open = None;
        self.assist.close_list();
        ctx.memory_mut(|m| m.request_focus(Id::unique(EDITOR_ID)));
        self.refresh_ghost();
        self.scroll_to_field(id);
    }

    pub(super) fn refresh_ghost(&mut self) {
        let (Some(doc), Some(f)) = (&self.doc, &mut self.focus) else {
            return;
        };
        let Some(i) = doc.index_of(f.id) else { return };
        let e = &doc.entries[i];
        f.ghost =
            if e.f.value.is_empty() && !f.ghost_dismissed && e.f.kind.is_text() && !e.f.read_only {
                self.assist.best(doc, i)
            } else {
                None
            };
    }

    pub(super) fn editable_ids(&self) -> Vec<u64> {
        let Some(doc) = &self.doc else { return vec![] };
        doc.order
            .iter()
            .copied()
            .filter(|id| {
                doc.index_of(*id)
                    .is_some_and(|i| !doc.entries[i].f.read_only)
            })
            .collect()
    }

    /// Moves focus to the next (or previous) field in reading order.
    pub(super) fn move_focus(&mut self, back: bool, ctx: &egui::Context) {
        let ids = self.editable_ids();
        if ids.is_empty() {
            return;
        }
        let cur = self
            .focus
            .as_ref()
            .and_then(|f| ids.iter().position(|&i| i == f.id));
        let next = match (cur, back) {
            (None, false) => 0,
            (None, true) => ids.len() - 1,
            (Some(c), false) => (c + 1) % ids.len(),
            (Some(c), true) => (c + ids.len() - 1) % ids.len(),
        };
        self.focus_field(ids[next], back, ctx);
    }

    /// The comb field directly after (or before) `id` on the same line, if any.
    pub(super) fn adjacent_comb(&self, id: u64, after: bool) -> Option<u64> {
        let doc = self.doc.as_ref()?;
        let ids = self.editable_ids();
        let pos = ids.iter().position(|&i| i == id)?;
        let other = if after {
            *ids.get(pos + 1)?
        } else {
            *ids.get(pos.checked_sub(1)?)?
        };
        let a = &doc.entries[doc.index_of(id)?].f;
        let b = &doc.entries[doc.index_of(other)?].f;
        let same_line = a.page == b.page
            && (a.rect.center().1 - b.rect.center().1).abs()
                < a.rect.height().min(b.rect.height()) * 0.5;
        let ordered = if after {
            b.rect.x0 >= a.rect.x1 - 1.0
        } else {
            b.rect.x1 <= a.rect.x0 + 1.0
        };
        (same_line && ordered && b.kind.is_comb()).then_some(other)
    }

    pub(super) fn handle_keys(&mut self, ctx: &egui::Context) {
        if self.focus.is_none() || self.keyboard_elsewhere(ctx) {
            return;
        }
        let events = ctx.input(|i| i.events.clone());
        for ev in events {
            // Focus may move during a batch (Tab, a full row of boxes): each
            // event goes to the field focused when it is handled.
            let Some(fid) = self.focus.as_ref().map(|f| f.id) else {
                return;
            };
            let Some(doc) = &self.doc else { return };
            let Some(i) = doc.index_of(fid) else {
                self.focus = None;
                return;
            };
            let field = doc.entries[i].f.clone();
            let editable = doc.form.editable() && !field.read_only;
            // A selected signature takes keys only (Delete removes it).
            if field.kind.is_signature() && !matches!(ev, Event::Key { .. }) {
                continue;
            }
            match ev {
                Event::Text(t) => {
                    if editable && field.kind.is_fixed_choice() {
                        self.choice_type_ahead(&t, &field);
                    } else if editable {
                        self.type_text(&t, ctx);
                    }
                }
                Event::Ime(egui::ImeEvent::Commit(t)) => {
                    if editable {
                        self.type_text(&t, ctx);
                    }
                }
                Event::Paste(t) => {
                    if editable {
                        self.type_text(&t, ctx);
                    }
                }
                Event::Copy | Event::Cut => {
                    let f = self.focus.as_ref().unwrap();
                    let sel = f.ed.selected_text(&field.value);
                    if !sel.is_empty() && !field.kind.is_password() {
                        ctx.copy_text(sel);
                    }
                    if matches!(ev, Event::Cut) && editable && f.ed.has_selection() {
                        self.before_edit(fid);
                        self.edit_value(|ed, v| {
                            ed.delete_selection(v);
                        });
                    }
                }
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => self.key(key, modifiers, &field, editable, ctx),
                _ => {}
            }
        }
    }

    pub(super) fn edit_value(&mut self, op: impl FnOnce(&mut Editor, &mut String)) {
        let (Some(doc), Some(f)) = (&mut self.doc, &mut self.focus) else {
            return;
        };
        let Some(i) = doc.index_of(f.id) else { return };
        let mut v = std::mem::take(&mut doc.entries[i].f.value);
        op(&mut f.ed, &mut v);
        doc.set_value_at(i, v);
    }

    pub(super) fn type_text(&mut self, t: &str, ctx: &egui::Context) {
        let Some(fid) = self.focus.as_ref().map(|f| f.id) else {
            return;
        };
        // Checkboxes take no text: Space toggles them in `key` and the " "
        // that arrives with it must neither edit nor move focus.
        if self
            .doc
            .as_ref()
            .and_then(|d| {
                d.index_of(fid).map(|i| {
                    let k = &d.entries[i].f.kind;
                    k.is_toggle() || k.is_signature()
                })
            })
            .unwrap_or(false)
        {
            return;
        }
        self.before_edit(fid);
        if let Some(f) = &mut self.focus {
            f.ghost = None;
        }
        let doc = self.doc.as_ref().unwrap();
        let field = doc.entries[doc.index_of(fid).unwrap()].f.clone();
        let mut outcome = Default::default();
        self.edit_value(|ed, v| outcome = ed.insert(&field, v, t));
        let outcome: pdfr_core::edit::InsertOutcome = outcome;
        if outcome.row_full {
            if let Some(next) = self.adjacent_comb(fid, true) {
                self.focus_field(next, false, ctx);
                if !outcome.rest.is_empty() {
                    self.type_text(&outcome.rest, ctx);
                }
            }
        } else if !outcome.rest.is_empty()
            && field.kind.is_comb()
            && let Some(next) = self.adjacent_comb(fid, true)
        {
            self.focus_field(next, false, ctx);
            self.type_text(&outcome.rest, ctx);
        }
        self.assist.on_typing(self.doc.as_ref().unwrap(), fid);
    }

    pub(super) fn key(
        &mut self,
        key: Key,
        m: Modifiers,
        field: &Field,
        editable: bool,
        ctx: &egui::Context,
    ) {
        let fid = self.focus.as_ref().unwrap().id;
        let shift = m.shift;
        let cmd = m.command;
        let value = field.value.clone();
        let has_ghost = self.focus.as_ref().unwrap().ghost.is_some();
        if matches!(key, Key::Enter | Key::Tab)
            && self.assist.list_open()
            && let Some(v) = self.assist.take_selected(fid)
        {
            if editable {
                self.replace_value(fid, field, &v);
            }
            if key == Key::Tab {
                self.move_focus(shift, ctx);
            }
            return;
        }
        if editable && self.choice_key(key, m, field) {
            return;
        }
        if field.kind.is_signature() {
            match key {
                Key::Delete | Key::Backspace if editable => self.remove_field(fid),
                Key::Tab | Key::Enter => self.move_focus(shift, ctx),
                Key::Escape => {
                    self.focus = None;
                    ctx.memory_mut(|mm| mm.surrender_focus(Id::unique(EDITOR_ID)));
                }
                _ => {}
            }
            return;
        }
        match key {
            Key::Tab => {
                if has_ghost && !shift && editable {
                    self.accept_ghost(ctx);
                }
                self.move_focus(shift, ctx);
            }
            Key::Enter => {
                if field.kind.is_multiline() && editable && !has_ghost {
                    self.type_text("\n", ctx);
                } else {
                    if has_ghost && editable {
                        self.accept_ghost(ctx);
                    }
                    self.move_focus(shift, ctx);
                }
            }
            Key::Escape => {
                if self.assist.list_open() {
                    self.assist.close_list();
                } else if has_ghost {
                    let f = self.focus.as_mut().unwrap();
                    f.ghost = None;
                    f.ghost_dismissed = true;
                } else {
                    self.focus = None;
                    self.choice_open = None;
                    ctx.memory_mut(|mm| mm.surrender_focus(Id::unique(EDITOR_ID)));
                }
            }
            Key::Backspace if editable => {
                let at_start = {
                    let f = self.focus.as_ref().unwrap();
                    f.ed.caret == 0 && !f.ed.has_selection()
                };
                if at_start && field.kind.is_comb() {
                    if let Some(prev) = self.adjacent_comb(fid, false) {
                        self.focus_field(prev, true, ctx);
                        self.before_edit(prev);
                        self.edit_value(|ed, v| {
                            ed.backspace(v);
                        });
                    }
                    return;
                }
                self.before_edit(fid);
                // Ctrl+Backspace (Alt on some desktops) deletes a word.
                let word = cmd || m.alt;
                self.edit_value(|ed, v| {
                    if word && !ed.has_selection() {
                        ed.word_left(v, true);
                    }
                    ed.backspace(v);
                });
                self.refresh_ghost_after_edit();
            }
            Key::Delete if editable => {
                self.before_edit(fid);
                self.edit_value(|ed, v| {
                    ed.delete(v);
                });
            }
            Key::ArrowLeft => {
                let f = self.focus.as_mut().unwrap();
                if cmd || m.alt {
                    f.ed.word_left(&value, shift)
                } else {
                    f.ed.left(shift)
                }
                self.typing_run = None;
            }
            Key::ArrowRight => {
                if has_ghost && editable && !shift {
                    self.accept_ghost(ctx);
                    return;
                }
                let f = self.focus.as_mut().unwrap();
                if cmd || m.alt {
                    f.ed.word_right(&value, shift)
                } else {
                    f.ed.right(&value, shift)
                }
                self.typing_run = None;
            }
            Key::ArrowUp | Key::ArrowDown => {
                let down = key == Key::ArrowDown;
                if self.assist.handle_list_key(down, field) {
                    return;
                }
                // Up or Down in a one-line field browses its alternatives.
                if !field.kind.is_multiline()
                    && self
                        .assist
                        .open_list(self.doc.as_ref().unwrap(), fid, !down)
                {
                    return;
                }
                let f = self.focus.as_mut().unwrap();
                if field.kind.is_multiline() {
                    f.ed.vertical(field, &value, down, shift);
                }
            }
            Key::Home => {
                let f = self.focus.as_mut().unwrap();
                f.ed.home(field, &value, shift);
            }
            Key::End => {
                let f = self.focus.as_mut().unwrap();
                f.ed.end(field, &value, shift);
            }
            Key::A if cmd => {
                let f = self.focus.as_mut().unwrap();
                f.ed.select_all(&value);
            }
            Key::Space if field.kind.is_toggle() && editable => {
                self.toggle(fid);
            }
            _ => {}
        }
    }

    /// Keys on a focused drop-down. Space, Enter or Alt+Down open a fixed
    /// list (only Alt+Down an editable one, where Space and Enter type and
    /// move on as in any box); while open, Up/Down move the highlight,
    /// Enter picks and Esc closes. Returns true if handled.
    fn choice_key(&mut self, key: Key, m: Modifiers, field: &Field) -> bool {
        let FieldKind::Choice { options, editable } = &field.kind else {
            return false;
        };
        let fid = self.focus.as_ref().unwrap().id;
        if self.choice_open == Some(fid) {
            match key {
                Key::ArrowDown | Key::ArrowUp => {
                    self.move_choice(key == Key::ArrowDown, options.len());
                    true
                }
                Key::Enter => {
                    if let Some((export, _)) = options.get(self.choice_sel) {
                        self.pick_choice(fid, export.clone());
                    }
                    true
                }
                Key::Escape => {
                    self.choice_open = None;
                    true
                }
                _ => false,
            }
        } else {
            let open = (key == Key::ArrowDown && m.alt)
                || (!editable && matches!(key, Key::Space | Key::Enter) && !m.shift);
            if open && !options.is_empty() {
                self.open_choice(fid, field);
            }
            open
        }
    }

    fn open_choice(&mut self, fid: u64, field: &Field) {
        let FieldKind::Choice { options, .. } = &field.kind else {
            return;
        };
        self.choice_open = Some(fid);
        self.choice_sel = options
            .iter()
            .position(|(e, _)| *e == field.value)
            .unwrap_or(0);
    }

    pub(super) fn move_choice(&mut self, down: bool, n: usize) {
        let n = n.max(1);
        self.choice_sel = if down {
            (self.choice_sel + 1) % n
        } else {
            (self.choice_sel + n - 1) % n
        };
    }

    /// Typing on a fixed drop-down highlights the next option starting with
    /// that letter (opening the list); Enter then picks it.
    fn choice_type_ahead(&mut self, t: &str, field: &Field) {
        let FieldKind::Choice { options, .. } = &field.kind else {
            return;
        };
        let Some(c) = t.chars().find(|c| !c.is_whitespace()) else {
            return;
        };
        let fid = self.focus.as_ref().unwrap().id;
        if self.choice_open != Some(fid) {
            self.open_choice(fid, field);
        }
        let lc = c.to_lowercase().to_string();
        let n = options.len();
        if let Some(k) = (1..=n)
            .map(|k| (self.choice_sel + k) % n)
            .find(|&k| options[k].1.to_lowercase().starts_with(&lc))
        {
            self.choice_sel = k;
        }
    }

    /// Sets a drop-down to option `export`, as one undo step, and closes it.
    pub(super) fn pick_choice(&mut self, id: u64, export: String) {
        self.snapshot();
        self.set_value(id, &export);
        self.choice_open = None;
        if let Some(fo) = self.focus.as_mut().filter(|f| f.id == id) {
            fo.ed = Editor::at(export.chars().count());
        }
    }

    /// Ctrl+J/K inside an open list: the suggestions or a drop-down.
    /// Returns false when no list is open (they scroll instead).
    pub(super) fn move_in_open_list(&mut self, down: bool) -> bool {
        let Some(fid) = self.focus.as_ref().map(|f| f.id) else {
            return false;
        };
        if self.choice_open == Some(fid) {
            let n = self
                .focused_entry()
                .map(|e| match &e.f.kind {
                    FieldKind::Choice { options, .. } => options.len(),
                    _ => 0,
                })
                .unwrap_or(0);
            self.move_choice(down, n);
            return true;
        }
        if self.assist.list_open() {
            let field = self.focused_entry().map(|e| e.f.clone());
            return field.is_some_and(|f| self.assist.handle_list_key(down, &f));
        }
        false
    }

    /// Focuses the next (or previous) box that still needs filling, from
    /// the focused one, or from the page in view when none is focused.
    pub(super) fn focus_empty(&mut self, back: bool) {
        let ids = self.editable_ids();
        let Some(doc) = &self.doc else { return };
        let empty = |id: &u64| {
            doc.index_of(*id).is_some_and(|i| {
                let f = &doc.entries[i].f;
                if f.kind.is_toggle() {
                    // A radio group is filled once any of its buttons is.
                    match &f.kind {
                        FieldKind::Radio { group, .. } => !doc.entries.iter().any(|e| {
                            e.f.checked
                                && matches!(&e.f.kind, FieldKind::Radio { group: g, .. } if g == group)
                        }),
                        _ => !f.checked,
                    }
                } else {
                    f.value.is_empty()
                }
            })
        };
        let n = ids.len();
        if n == 0 {
            return;
        }
        let start = match self
            .focus
            .as_ref()
            .and_then(|f| ids.iter().position(|&i| i == f.id))
        {
            Some(c) => c,
            None => {
                // Just before the first box on the page in view.
                let page = self.current_page();
                let first = ids
                    .iter()
                    .position(|id| {
                        doc.index_of(*id)
                            .is_some_and(|i| doc.entries[i].f.page >= page)
                    })
                    .unwrap_or(0);
                if back { first } else { (first + n - 1) % n }
            }
        };
        let found = (1..=n)
            .map(|k| {
                if back {
                    (start + n * 2 - k) % n
                } else {
                    (start + k) % n
                }
            })
            .map(|k| ids[k])
            .find(|id| empty(id));
        match found {
            Some(id) => {
                let ctx = self.egui_ctx.clone();
                self.focus_field(id, false, &ctx);
            }
            None => self.status = "All boxes are filled.".into(),
        }
    }

    pub(super) fn refresh_ghost_after_edit(&mut self) {
        let empty = self
            .doc
            .as_ref()
            .zip(self.focus.as_ref())
            .and_then(|(d, f)| d.index_of(f.id).map(|i| d.entries[i].f.value.is_empty()))
            .unwrap_or(false);
        if empty {
            self.refresh_ghost();
        }
    }

    pub fn accept_ghost(&mut self, ctx: &egui::Context) {
        let Some(g) = self.focus.as_mut().and_then(|f| f.ghost.take()) else {
            return;
        };
        let fid = self.focus.as_ref().unwrap().id;
        self.before_edit(fid);
        self.edit_value(|ed, v| {
            v.clear();
            *ed = Editor::at(0);
        });
        let doc = self.doc.as_ref().unwrap();
        let field = doc.entries[doc.index_of(fid).unwrap()].f.clone();
        self.edit_value(|ed, v| {
            ed.insert(&field, v, &g);
        });
        self.typing_run = None;
        let _ = ctx;
    }

    /// Replaces a field's whole value (accepting a suggestion), as one undo step.
    pub(super) fn replace_value(&mut self, fid: u64, field: &Field, v: &str) {
        self.before_edit(fid);
        self.edit_value(|ed, val| {
            val.clear();
            *ed = Editor::at(0);
        });
        self.edit_value(|ed, val| {
            ed.insert(field, val, v);
        });
        if let Some(fo) = &mut self.focus {
            fo.ghost = None;
        }
        self.typing_run = None;
    }

    pub fn set_value(&mut self, id: u64, value: &str) {
        let Some(doc) = &mut self.doc else { return };
        let Some(i) = doc.index_of(id) else { return };
        let f = doc.entries[i].f.clone();
        let mut v = String::new();
        Editor::default().insert(&f, &mut v, value);
        doc.set_value_at(i, v);
        doc.dirty = true;
    }

    pub(super) fn toggle(&mut self, id: u64) {
        self.snapshot();
        self.typing_run = None;
        let Some(doc) = &mut self.doc else { return };
        let Some(i) = doc.index_of(id) else { return };
        doc.dirty = true;
        match doc.entries[i].f.kind.clone() {
            FieldKind::Radio { group, .. } => {
                let now = !doc.entries[i].f.checked;
                for e in doc.entries.iter_mut() {
                    if matches!(&e.f.kind, FieldKind::Radio { group: g, .. } if *g == group) {
                        e.f.checked = false;
                    }
                }
                doc.entries[i].f.checked = now;
            }
            _ => doc.entries[i].f.checked = !doc.entries[i].f.checked,
        }
    }
}
