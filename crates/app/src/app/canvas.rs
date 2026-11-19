//! The page area: drawing pages and fields, pointer input on the page, the
//! field context menu and the drop-down and suggestion popups.

use super::*;

impl App {
    pub(super) fn canvas(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let full = ui.available_rect_before_wrap();
        self.viewport = full;
        let id = Id::unique(EDITOR_ID);
        let resp = ui.interact(full, id, Sense::click_and_drag());
        let painter = ui.painter_at(full);
        painter.rect_filled(full, 0.0, self.palette.canvas);
        if self.doc.is_none() {
            painter.text(
                full.center(),
                egui::Align2::CENTER_CENTER,
                &self.status,
                egui::FontId::proportional(16.0),
                self.palette.text,
            );
            return;
        }
        // Keep egui from stealing tab, arrows and escape while a field is focused.
        if self.focus.is_some() && !self.keyboard_elsewhere(&ctx) {
            ctx.memory_mut(|m| {
                if !m.has_focus(id) {
                    m.request_focus(id);
                }
                m.set_focus_lock_filter(
                    id,
                    EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                );
            });
        }

        // Scrolling and zooming.
        let hovered = resp.hovered() || ui.rect_contains_pointer(full);
        let (scroll_delta, zoom_delta, pointer) =
            ctx.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.pointer.hover_pos()));
        if hovered {
            if (zoom_delta - 1.0).abs() > 1e-4 {
                let anchor = pointer.unwrap_or(full.center());
                self.zoom_to(self.zoom * zoom_delta, anchor);
            }
            if scroll_delta != Vec2::ZERO {
                self.scroll -= scroll_delta;
                self.clamp_scroll();
            }
        }
        self.clamp_scroll();

        let ppp = ctx.pixels_per_point();
        self.manage_textures(ppp);
        let rects = self.page_rects();
        let pointer = pointer.or_else(|| {
            let (x, y) = self.cli.hover?;
            let r = rects.first()?;
            Some(pos2(r.left() + x * self.zoom, r.top() + y * self.zoom))
        });
        let pal = self.palette.clone();
        let focus_id = self.focus.as_ref().map(|f| f.id);

        // Pages.
        let doc = self.doc.as_ref().unwrap();
        for (i, r) in rects.iter().enumerate() {
            if !r.intersects(full) {
                continue;
            }
            painter.rect_filled(
                r.translate(vec2(1.5, 2.0)),
                2.0,
                Color32::from_black_alpha(60),
            );
            painter.rect_filled(*r, 0.0, Color32::WHITE);
            if let Some(t) = doc.tex.get(&i) {
                // A stale texture (after a zoom) is shown stretched until the new one arrives.
                painter.image(
                    t.tex.id(),
                    *r,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
        }

        if doc.form.editable() {
            self.draw_sign_spots(&painter, &rects);
        }
        self.draw_hits(&painter, &rects);
        self.draw_text_selection(&painter, &rects);

        // Fields.
        let editable_doc = doc.form.editable();
        let hover_id = if self.tool == Tool::None {
            pointer.and_then(|p| self.field_at(&rects, p)).map(|h| h.0)
        } else {
            None
        };
        for e in &doc.entries {
            let f = &e.f;
            let Some(pr) = rects.get(f.page) else {
                continue;
            };
            if !pr.intersects(full) {
                continue;
            }
            let xf = PageXf {
                origin: pr.min,
                zoom: self.zoom,
            };
            let r = xf.rect(&f.rect);
            if !r.intersects(full) {
                continue;
            }
            let focused = focus_id == Some(e.id);
            if let FieldKind::Signature { .. } = &f.kind {
                let hovered = hover_id == Some(e.id) && editable_doc;
                self.draw_signature(&painter, &rects, f, r, focused, hovered);
                continue;
            }
            if editable_doc && !f.read_only {
                // Every fillable spot keeps its tint, filled or not, so the
                // fields stay visible. Screen only: never saved or printed.
                {
                    if let Some(TextMode::Comb(cells)) = f.kind.text_mode() {
                        for c in cells {
                            let cr = xf.rect(&PRect::new(
                                c.0 + 1.0,
                                f.rect.y0 + 1.0,
                                c.1 - 1.0,
                                f.rect.y1 - 1.0,
                            ));
                            painter.rect_filled(cr, 0.0, pal.empty_tint);
                        }
                    } else {
                        painter.rect_filled(r, 0.0, pal.empty_tint);
                    }
                }
            }
            match &f.kind {
                // Hovering an empty box previews the mark a click would make.
                FieldKind::Checkbox { .. } => {
                    if f.checked {
                        draw::check(&painter, &xf, f, Color32::BLACK);
                    } else if hover_id == Some(e.id) && editable_doc && !f.read_only {
                        draw::check(&painter, &xf, f, Color32::from_black_alpha(90));
                    }
                }
                FieldKind::Radio { .. } => {
                    if f.checked {
                        draw::radio_dot(&painter, &xf, f, Color32::BLACK);
                    } else if hover_id == Some(e.id) && editable_doc && !f.read_only {
                        draw::radio_dot(&painter, &xf, f, Color32::from_black_alpha(90));
                    }
                }
                _ => {
                    let l = layout(f, f.shown_text());
                    if focused {
                        let fo = self.focus.as_ref().unwrap();
                        if fo.ed.has_selection() {
                            let (a, b) = fo.ed.selection();
                            draw::selection(&painter, &xf, &l, a, b, pal.selection);
                        }
                    }
                    let pclip = painter.with_clip_rect(r.expand(2.0).intersect(full));
                    draw::text(&pclip, &xf, &l, Color32::BLACK);
                    if focused {
                        let fo = self.focus.as_ref().unwrap();
                        // Browsing alternatives previews the highlighted one.
                        let preview = self.assist.list_choice(e.id).filter(|_| f.value.is_empty());
                        if let Some(g) = preview.or(fo.ghost.as_deref()) {
                            let gl = layout(f, g);
                            draw::text(&pclip, &xf, &gl, Color32::from_gray(150));
                        }
                        let cr = draw::caret(&painter, &xf, &l, fo.ed.caret, pal.caret);
                        ctx.output_mut(|o| {
                            o.ime = Some(egui::output::IMEOutput {
                                purpose: if f.kind.is_password() {
                                    egui::IMEPurpose::Password
                                } else {
                                    egui::IMEPurpose::Normal
                                },
                                rect: r,
                                cursor_rect: cr,
                                should_interrupt_composition: false,
                            });
                        });
                    }
                    draw::dropdown_marker(&painter, &xf, f, Color32::from_gray(90));
                }
            }
        }

        // Drag preview for the add-text tool.
        if let (Tool::AddText, Some((page, start)), Some(p)) = (self.tool, self.drag_start, pointer)
            && let Some(pr) = rects.get(page)
        {
            let xf = PageXf {
                origin: pr.min,
                zoom: self.zoom,
            };
            let a = xf.pt(start.0, start.1);
            painter.rect_stroke(
                Rect::from_two_pos(a, p),
                0.0,
                Stroke::new(1.5, pal.accent),
                egui::StrokeKind::Middle,
            );
        }

        // The signature being placed follows the pointer.
        if self.tool == Tool::PlaceSignature
            && let (Some(p), Some(ink)) = (pointer, self.placing.clone())
            && let Some((page, pr)) = self.placement_at(&rects, p)
            && let Some(page_rect) = rects.get(page)
        {
            let r = PageXf {
                origin: page_rect.min,
                zoom: self.zoom,
            }
            .rect(&pr);
            crate::ink::paint(&painter, &ink, r, Color32::from_white_alpha(170));
            painter.rect_stroke(
                r.expand(2.0),
                2.0,
                Stroke::new(1.0, pal.accent.gamma_multiply(0.6)),
                egui::StrokeKind::Middle,
            );
        }

        // The hovered field's floating button (clear / suggestions).
        let on_button = hover_id.is_some_and(|hid| self.field_button(ui, &rects, hid, &ctx));

        // Ctrl+L jump labels, over everything on the page.
        self.draw_hints(&painter, &rects);

        // Scrollbar.
        self.scrollbar(ui, &painter, full);

        // Mouse.
        self.mouse(&resp, &rects, &ctx);
        if self.tool != Tool::None && resp.hovered() {
            ctx.set_cursor_icon(CursorIcon::Crosshair);
        } else if on_button {
            // The button sets its own pointer.
        } else if self.sig_drag.is_some() {
            ctx.set_cursor_icon(CursorIcon::Grabbing);
        } else if let Some(p) = pointer
            && let Some(e) = self
                .focused_entry()
                .filter(|e| e.f.kind.is_signature() && editable_doc)
            && self
                .sig_handle(&rects, &e.f)
                .is_some_and(|h| h.expand(3.0).contains(p))
        {
            ctx.set_cursor_icon(CursorIcon::ResizeNwSe);
        } else if let Some(p) = pointer
            && let Some((id, ..)) = self.field_at(&rects, p)
        {
            // A text caret only where text goes; boxes to tick get the plain
            // pointer; a signature can be picked up.
            let kind = self
                .doc
                .as_ref()
                .and_then(|d| d.entries.iter().find(|e| e.id == id))
                .map(|e| e.f.kind.clone());
            ctx.set_cursor_icon(match kind {
                Some(k) if k.is_toggle() => CursorIcon::Default,
                Some(k) if k.is_signature() && editable_doc => CursorIcon::Grab,
                Some(k) if k.is_signature() => CursorIcon::Default,
                _ => CursorIcon::Text,
            });
        } else if let Some(p) = pointer
            && editable_doc
            && self.sign_target_at(&rects, p).is_some_and(|t| !t.on_line)
        {
            ctx.set_cursor_icon(CursorIcon::PointingHand);
        } else if self.text_sel.as_ref().is_some_and(|s| s.dragging)
            || pointer.is_some_and(|p| self.over_text(&rects, p))
        {
            ctx.set_cursor_icon(CursorIcon::Text);
        }
        self.context_menu(&resp);
        self.page_menu(&resp, &ctx);
        self.choice_popup(&ctx, &rects);
        self.assist_list_popup(&ctx, &rects);

        // Keyboard: scrolling when no field has focus.
        if self.focus.is_none() && !self.keyboard_elsewhere(&ctx) {
            let vh = self.viewport.height();
            let dy = ctx.input(|i| {
                let mut dy = 0.0;
                if i.key_pressed(Key::PageDown) || (i.key_pressed(Key::Space) && !i.modifiers.shift)
                {
                    dy += vh * 0.9;
                }
                if i.key_pressed(Key::PageUp) || (i.key_pressed(Key::Space) && i.modifiers.shift) {
                    dy -= vh * 0.9;
                }
                if i.key_pressed(Key::ArrowDown) {
                    dy += 40.0;
                }
                if i.key_pressed(Key::ArrowUp) {
                    dy -= 40.0;
                }
                if i.key_pressed(Key::Home) {
                    dy = -1e9;
                }
                if i.key_pressed(Key::End) {
                    dy = 1e9;
                }
                dy
            });
            self.scroll.y += dy;
            // Copying selected page text (egui turns Ctrl+C into a Copy event).
            if self.text_sel.is_some() && ctx.input(|i| i.events.contains(&Event::Copy)) {
                self.copy_selection(&ctx);
            }
            // Esc puts down an add-box tool, else drops the selection, else
            // hides search highlights.
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                if self.tool != Tool::None {
                    self.tool = Tool::None;
                    self.placing = None;
                } else if self.text_sel.is_some() {
                    self.text_sel = None;
                } else if let Some(s) = self.search.as_mut().filter(|s| s.shown) {
                    s.shown = false;
                }
            }
            if ctx.input(|i| i.key_pressed(Key::Tab)) {
                let back = ctx.input(|i| i.modifiers.shift);
                self.move_focus(back, &ctx);
            }
            self.clamp_scroll();
        }
    }

    /// A round button floating at the right edge of the hovered text field:
    /// ✕ clears a filled field; the sparkle opens the suggestions of an
    /// empty one that has any. Never both. Drawn on screen only.
    /// Returns whether the pointer is on the button.
    fn field_button(
        &mut self,
        ui: &mut egui::Ui,
        rects: &[Rect],
        id: u64,
        ctx: &egui::Context,
    ) -> bool {
        let Some(doc) = &self.doc else { return false };
        if !doc.form.editable() {
            return false;
        }
        let Some(i) = doc.index_of(id) else {
            return false;
        };
        let f = doc.entries[i].f.clone();
        if f.read_only || f.kind.is_toggle() || matches!(f.kind, FieldKind::Choice { .. }) {
            return false;
        }
        let filled = !f.value.is_empty();
        if !filled && !self.assist.has_suggestions(self.doc.as_ref().unwrap(), id) {
            return false;
        }
        let Some(b) = self.field_button_rect(rects, &f) else {
            return false;
        };
        let side = b.width();
        let resp = ui.interact(b, Id::unique(("pdfr-field-button", id)), Sense::click());
        let hovered = resp.hovered();
        let painter = ui.painter();
        // Solid and lifted off the field so it is easy to spot: the accent
        // colour for suggestions, dark grey for clearing; darker on hover.
        let base = if filled {
            Color32::from_gray(70)
        } else {
            strong(self.palette.accent)
        };
        let bg = if hovered {
            crate::theme::mix(base, Color32::BLACK, 0.25)
        } else {
            base
        };
        let c = b.center();
        let rad = side * 0.5;
        painter.circle_filled(c + vec2(0.0, 1.5), rad + 1.0, Color32::from_black_alpha(55));
        painter.circle_filled(c, rad + 1.5, Color32::WHITE);
        painter.circle_filled(c, rad, bg);
        let icon = if filled {
            crate::icons::Icon::Close
        } else {
            crate::icons::Icon::Sparkle
        };
        crate::icons::paint(
            painter,
            b.shrink(side * if filled { 0.22 } else { 0.25 }),
            icon,
            Color32::WHITE,
        );
        let resp = resp
            .on_hover_text(if filled {
                "Clear"
            } else {
                "Suggestions (\u{2191}/\u{2193})"
            })
            .on_hover_cursor(CursorIcon::PointingHand);
        if resp.clicked() {
            self.focus_field(id, false, ctx);
            if filled {
                self.replace_value(id, &f, "");
                self.refresh_ghost();
            } else if let Some(doc) = &self.doc {
                self.assist.open_list(doc, id, false);
            }
        }
        hovered
    }

    /// Where a field's floating button sits on screen: at the right edge,
    /// centred on one-line fields, top-right on taller ones. None if the
    /// field is too small for one.
    pub(super) fn field_button_rect(&self, rects: &[Rect], f: &Field) -> Option<Rect> {
        let pr = rects.get(f.page)?;
        let r = PageXf {
            origin: pr.min,
            zoom: self.zoom,
        }
        .rect(&f.rect);
        // Small, with room around it: the padding is what makes it sit well.
        const PAD: f32 = 5.0;
        let side = (r.height() * 0.55).clamp(12.0, 16.0).min(r.width() * 0.4);
        if side < 10.0 {
            return None;
        }
        let cy = if r.height() > side * 2.5 {
            r.top() + side * 0.5 + PAD
        } else {
            r.center().y
        };
        Some(Rect::from_center_size(
            pos2(r.right() - side * 0.5 - PAD, cy),
            vec2(side, side),
        ))
    }

    /// The field under a screen point (smallest first).
    pub(super) fn field_at(&self, rects: &[Rect], p: Pos2) -> Option<(u64, usize, (f32, f32))> {
        let doc = self.doc.as_ref()?;
        let page = rects.iter().position(|r| r.contains(p))?;
        let xf = PageXf {
            origin: rects[page].min,
            zoom: self.zoom,
        };
        let (x, y) = xf.to_page(p);
        doc.entries
            .iter()
            .filter(|e| e.f.page == page && e.f.rect.contains(x, y))
            .min_by(|a, b| a.f.rect.area().total_cmp(&b.f.rect.area()))
            .map(|e| (e.id, page, (x, y)))
    }

    pub(super) fn mouse(&mut self, resp: &egui::Response, rects: &[Rect], ctx: &egui::Context) {
        let Some(p) = resp.interact_pointer_pos().or(resp.hover_pos()) else {
            return;
        };
        let shift = ctx.input(|i| i.modifiers.shift);
        let editable = self.doc.as_ref().is_some_and(|d| d.form.editable());
        // egui turns Space/Enter on the focused page widget into a "click";
        // only a real mouse button release counts here.
        let real_click = ctx.input(|i| i.pointer.any_released());
        let clicked = resp.clicked() && real_click;
        let double_clicked = resp.double_clicked() && real_click;
        match self.tool {
            Tool::AddText if editable => {
                if resp.drag_started()
                    && let Some(page) = rects.iter().position(|r| r.contains(p))
                {
                    let xf = PageXf {
                        origin: rects[page].min,
                        zoom: self.zoom,
                    };
                    self.drag_start = Some((page, xf.to_page(p)));
                }
                if resp.drag_stopped()
                    && let Some((page, a)) = self.drag_start.take()
                {
                    let xf = PageXf {
                        origin: rects[page].min,
                        zoom: self.zoom,
                    };
                    let b = xf.to_page(p);
                    let r = PRect::from_points(a, b);
                    if r.width() >= 8.0 && r.height() >= 6.0 {
                        self.snapshot();
                        let mode = if r.height() > 60.0 {
                            TextMode::Multi
                        } else {
                            TextMode::Single
                        };
                        let doc = self.doc.as_mut().unwrap();
                        let id = doc.add(Field::new_text(page, r, mode, Origin::Manual));
                        doc.reorder();
                        doc.dirty = true;
                        self.tool = Tool::None;
                        self.focus_field(id, false, ctx);
                    }
                }
                return;
            }
            Tool::AddCheckbox if editable => {
                if clicked && let Some(page) = rects.iter().position(|r| r.contains(p)) {
                    let xf = PageXf {
                        origin: rects[page].min,
                        zoom: self.zoom,
                    };
                    let (x, y) = xf.to_page(p);
                    self.snapshot();
                    let doc = self.doc.as_mut().unwrap();
                    let mut f = Field::new_checkbox(
                        page,
                        PRect::new(x - 6.0, y - 6.0, x + 6.0, y + 6.0),
                        Origin::Manual,
                    );
                    f.checked = true;
                    doc.add(f);
                    doc.reorder();
                    doc.dirty = true;
                    self.tool = Tool::None;
                }
                return;
            }
            Tool::PlaceSignature if editable => {
                if clicked
                    && let Some((page, r)) = self.placement_at(rects, p)
                    && let Some(ink) = self.placing.clone()
                {
                    self.place_signature(page, r, ink);
                }
                return;
            }
            _ => {}
        }
        if self.sig_mouse(resp, rects, p) {
            return;
        }
        if self.select_mouse(resp, rects, p, ctx) {
            return;
        }
        if resp.secondary_clicked() {
            self.ctx_field = self.field_at(rects, p).map(|h| h.0);
            self.page_menu_open = self.ctx_field.is_none();
        }
        if clicked
            || double_clicked
            || (resp.drag_started() && self.focus.is_some())
            || resp.dragged()
        {
            let hit = self.field_at(rects, p);
            if resp.dragged() && !resp.drag_started() {
                // Extend the selection while dragging inside the focused field.
                if let (Some(f), Some(doc)) = (&mut self.focus, &self.doc)
                    && let Some(i) = doc.index_of(f.id)
                {
                    let field = &doc.entries[i].f;
                    let xf = PageXf {
                        origin: rects[field.page].min,
                        zoom: self.zoom,
                    };
                    let (x, y) = xf.to_page(p);
                    f.ed.click(field, &field.value, x, y, true);
                }
                return;
            }
            match hit {
                Some((id, _, (x, y))) => {
                    let doc = self.doc.as_ref().unwrap();
                    let i = doc.index_of(id).unwrap();
                    let field = doc.entries[i].f.clone();
                    if field.kind.is_toggle() {
                        if clicked && editable && !field.read_only {
                            self.toggle(id);
                        }
                        self.focus = Some(Focus {
                            id,
                            ed: Editor::default(),
                            ghost: None,
                            ghost_dismissed: false,
                        });
                        return;
                    }
                    if matches!(field.kind, FieldKind::Choice { .. })
                        && clicked
                        && editable
                        && !field.read_only
                    {
                        if self.choice_open == Some(id) {
                            self.choice_open = None;
                        } else {
                            self.choice_open = Some(id);
                            if let FieldKind::Choice { options, .. } = &field.kind {
                                self.choice_sel = options
                                    .iter()
                                    .position(|(e, _)| *e == field.value)
                                    .unwrap_or(0);
                            }
                        }
                    }
                    if self.focus.as_ref().map(|f| f.id) != Some(id) {
                        self.focus_field(id, true, ctx);
                    }
                    let f = self.focus.as_mut().unwrap();
                    if double_clicked {
                        f.ed.select_word_at(&field, &field.value, x, y);
                    } else {
                        f.ed.click(&field, &field.value, x, y, shift);
                    }
                    self.typing_run = None;
                }
                None => {
                    if clicked {
                        self.text_sel = None;
                        self.focus = None;
                        self.choice_open = None;
                        self.assist.close_list();
                        // The PDF's own "sign here" spot opens the signing dialog.
                        if editable
                            && let Some(t) = self.sign_target_at(rects, p).filter(|t| !t.on_line)
                        {
                            self.open_sign(false, Some(t));
                        }
                    }
                }
            }
        }
    }

    pub(super) fn context_menu(&mut self, resp: &egui::Response) {
        let Some(id) = self.ctx_field else { return };
        let mut close = false;
        let mut remove = false;
        let mut clear = false;
        let mut toggle_mem = false;
        let info = self.doc.as_ref().and_then(|d| {
            d.index_of(id).map(|i| {
                (
                    d.entries[i].f.is_real(),
                    d.entries[i].no_memory,
                    d.entries[i].f.label.clone(),
                    d.form.editable() && !d.entries[i].f.read_only,
                    match &d.entries[i].f.kind {
                        FieldKind::Signature { lock, .. } => Some(*lock),
                        _ => None,
                    },
                )
            })
        });
        let Some((real, no_mem, label, editable, sig_lock)) = info else {
            return;
        };
        if let Some(locked) = sig_lock {
            let mut remove = false;
            let mut lock = false;
            let shown = resp.context_menu(|ui| {
                if editable && ui.button("Remove signature").clicked() {
                    remove = true;
                }
                if editable
                    && ui
                        .checkbox(&mut locked.clone(), "Lock into the page when saved")
                        .on_hover_text(
                            "Locked: part of the page, other PDF apps can't move or delete it.\nOff: a signature other PDF apps can still move.",
                        )
                        .clicked()
                {
                    lock = true;
                }
                if remove || lock {
                    ui.close();
                }
            });
            if remove {
                self.remove_field(id);
            } else if lock {
                self.toggle_lock(id);
            }
            if shown.is_none() || remove || lock {
                self.ctx_field = None;
            }
            return;
        }
        let shown = resp.context_menu(|ui| {
            if let Some(l) = &label {
                ui.label(egui::RichText::new(l).weak());
                ui.separator();
            }
            if !real && editable && ui.button("Remove this field").clicked() {
                remove = true;
                close = true;
            }
            if editable && ui.button("Clear").clicked() {
                clear = true;
                close = true;
            }
            if ui
                .checkbox(&mut no_mem.clone(), "Don't remember this field")
                .clicked()
            {
                toggle_mem = true;
                close = true;
            }
            if close {
                ui.close();
            }
        });
        if shown.is_none() && !close {
            self.ctx_field = None;
        }
        if remove {
            self.remove_field(id);
        } else if clear {
            self.clear_field(id);
        }
        if toggle_mem {
            self.toggle_memory(id);
        }
        if close {
            self.ctx_field = None;
        }
    }

    /// Empties a box (unticks a checkbox), as one undo step.
    pub(super) fn clear_field(&mut self, id: u64) {
        self.edit_entry(id, false);
    }

    /// Removes a box pdfr or the user added, as one undo step.
    pub(super) fn remove_field(&mut self, id: u64) {
        let real = self
            .doc
            .as_ref()
            .and_then(|d| d.index_of(id).map(|i| d.entries[i].f.is_real()));
        if real == Some(false) {
            self.edit_entry(id, true);
        }
    }

    fn edit_entry(&mut self, id: u64, remove: bool) {
        if self.doc.as_ref().and_then(|d| d.index_of(id)).is_none() {
            return;
        }
        self.snapshot();
        let doc = self.doc.as_mut().unwrap();
        let i = doc.index_of(id).unwrap();
        if remove {
            doc.entries.remove(i);
            if self.focus.as_ref().is_some_and(|f| f.id == id) {
                self.focus = None;
            }
        } else {
            doc.set_value_at(i, String::new());
            doc.entries[i].f.checked = false;
            if let Some(f) = self.focus.as_mut().filter(|f| f.id == id) {
                f.ed = Editor::at(0);
            }
        }
        doc.reorder();
        doc.dirty = true;
        self.assist.invalidate();
        if !remove {
            self.refresh_ghost();
        }
    }

    pub(super) fn toggle_memory(&mut self, id: u64) {
        if let Some(doc) = self.doc.as_mut()
            && let Some(i) = doc.index_of(id)
        {
            doc.entries[i].no_memory = !doc.entries[i].no_memory;
        }
    }

    pub(super) fn choice_popup(&mut self, ctx: &egui::Context, rects: &[Rect]) {
        let Some(id) = self.choice_open else { return };
        let Some(doc) = &self.doc else { return };
        let Some(i) = doc.index_of(id) else { return };
        let f = doc.entries[i].f.clone();
        let FieldKind::Choice { options, .. } = &f.kind else {
            return;
        };
        let Some(pr) = rects.get(f.page) else { return };
        let xf = PageXf {
            origin: pr.min,
            zoom: self.zoom,
        };
        let r = xf.rect(&f.rect);
        let mut pick = None;
        let sel = self.choice_sel.min(options.len().saturating_sub(1));
        let sel_moved = self.choice_sel_shown != Some(sel);
        egui::Area::new(Id::unique("pdfr-choice"))
            .fixed_pos(r.left_bottom())
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(r.width());
                    egui::ScrollArea::vertical()
                        .max_height(260.0)
                        .show(ui, |ui| {
                            for (k, (export, display)) in options.iter().enumerate() {
                                // The keyboard highlight, else the current value.
                                let on = k == sel;
                                let resp = ui.selectable_label(on, display);
                                if on && sel_moved {
                                    resp.scroll_to_me(None);
                                }
                                if resp.clicked() {
                                    pick = Some(export.clone());
                                }
                            }
                        });
                });
            });
        self.choice_sel_shown = Some(sel);
        if let Some(v) = pick {
            self.pick_choice(id, v);
        }
    }

    pub(super) fn assist_list_popup(&mut self, ctx: &egui::Context, rects: &[Rect]) {
        let Some(fid) = self.focus.as_ref().map(|f| f.id) else {
            return;
        };
        let Some(doc) = &self.doc else { return };
        let Some(i) = doc.index_of(fid) else { return };
        let f = doc.entries[i].f.clone();
        let Some(pr) = rects.get(f.page) else { return };
        let xf = PageXf {
            origin: pr.min,
            zoom: self.zoom,
        };
        let r = xf.rect(&f.rect);
        if let Some(v) = self.assist.list_popup(ctx, r, fid, &self.palette) {
            self.replace_value(fid, &f, &v);
        }
    }
}

/// `c` darkened (same hue) until a white icon on it has clear contrast, so
/// pale theme accents still make a button that stands out on paper.
pub(super) fn strong(c: Color32) -> Color32 {
    let mut c = c;
    for _ in 0..12 {
        if crate::theme::contrast(c, Color32::WHITE) >= 3.5 {
            break;
        }
        c = crate::theme::mix(c, Color32::BLACK, 0.12);
    }
    c
}
