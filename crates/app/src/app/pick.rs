//! Selecting boxes pdfr or the user added (Select tool, Ctrl+Shift+B, or
//! Ctrl+click): click one, Ctrl/Shift+click more, or drag a frame around
//! several. Delete removes them and arrows or dragging move them, each as
//! one undo step. Boxes that came with the PDF can't be selected.

use super::*;

/// What a drag with the Select tool is doing.
#[derive(Clone, Copy, Debug)]
pub(super) enum PickDrag {
    /// Framing boxes on `page` from `start` (page points); `add` keeps the
    /// boxes already selected.
    Frame {
        page: usize,
        start: (f32, f32),
        add: bool,
    },
    /// Moving the selected boxes; the pointer when they were last moved
    /// (from the press, so the drag threshold isn't lost).
    Move { last: Pos2 },
}

impl App {
    /// The smallest added box under a screen point.
    pub(super) fn pickable_at(&self, rects: &[Rect], p: Pos2) -> Option<u64> {
        let doc = self.doc.as_ref()?;
        let (page, (x, y)) = self.page_point(rects, p)?;
        doc.entries
            .iter()
            .filter(|e| !e.f.is_real() && e.f.page == page && e.f.rect.contains(x, y))
            .min_by(|a, b| a.f.rect.area().total_cmp(&b.f.rect.area()))
            .map(|e| e.id)
    }

    /// Adds `id` to the selection, or takes it out if already in.
    pub(super) fn toggle_pick(&mut self, id: u64) {
        if let Some(i) = self.picks.iter().position(|p| *p == id) {
            self.picks.remove(i);
        } else {
            self.picks.push(id);
        }
        self.focus = None;
        self.choice_open = None;
        self.text_sel = None;
        self.pick_status();
    }

    fn pick_status(&mut self) {
        self.status = match self.picks.len() {
            0 => String::new(),
            1 => "1 box selected. Delete removes it; drag or the arrow keys move it.".into(),
            n => format!(
                "{n} boxes selected. Delete removes them; drag or the arrow keys move them."
            ),
        };
    }

    /// Selects or puts down the Select tool.
    pub(super) fn toggle_select_tool(&mut self) {
        if self.tool == Tool::Select {
            self.tool = Tool::None;
            self.pick_drag = None;
            return;
        }
        self.toggle_tool(Tool::Select);
        self.focus = None;
        self.choice_open = None;
        self.text_sel = None;
        self.marks_ui.sel = None;
        self.status = "Click the boxes to select, or drag a frame around several. \
                       Delete removes them. Esc puts the tool down."
            .into();
    }

    /// Selects every added box on the current page.
    pub(super) fn pick_all_on_page(&mut self) {
        let page = self.current_page();
        if let Some(doc) = &self.doc {
            self.picks = doc
                .entries
                .iter()
                .filter(|e| !e.f.is_real() && e.f.page == page)
                .map(|e| e.id)
                .collect();
        }
        self.pick_status();
    }

    /// Removes the selected boxes, as one undo step.
    pub(super) fn remove_picks(&mut self) {
        let Some(doc) = self.doc.as_ref() else { return };
        let ids: HashSet<u64> = self
            .picks
            .iter()
            .copied()
            .filter(|id| {
                doc.index_of(*id)
                    .is_some_and(|i| !doc.entries[i].f.is_real())
            })
            .collect();
        self.picks.clear();
        if ids.is_empty() || !doc.form.editable() {
            return;
        }
        self.snapshot();
        let doc = self.doc.as_mut().unwrap();
        doc.entries.retain(|e| !ids.contains(&e.id));
        doc.reorder();
        doc.dirty = true;
        if self.focus.as_ref().is_some_and(|f| ids.contains(&f.id)) {
            self.focus = None;
        }
        self.ctx_field = None;
        self.assist.invalidate();
        self.status = match ids.len() {
            1 => "Removed 1 box. Ctrl+Z brings it back.".into(),
            n => format!("Removed {n} boxes. Ctrl+Z brings them back."),
        };
    }

    /// Moves the selected boxes by `dx`, `dy` points (no undo step: the
    /// caller takes one).
    fn shift_picks(&mut self, dx: f32, dy: f32) {
        let Some(doc) = self.doc.as_mut() else { return };
        for id in &self.picks {
            if let Some(i) = doc.index_of(*id) {
                doc.entries[i].f.shift(dx, dy);
            }
        }
        doc.dirty = true;
    }

    /// Mouse input while the Select tool is held.
    pub(super) fn pick_mouse(&mut self, resp: &egui::Response, rects: &[Rect], p: Pos2) {
        let (add, real_click) = self.egui_ctx.input(|i| {
            (
                i.modifiers.shift || i.modifiers.command,
                i.pointer.any_released(),
            )
        });
        let editable = self.doc.as_ref().is_some_and(|d| d.form.editable());
        if resp.clicked() && real_click {
            match self.pickable_at(rects, p) {
                Some(id) if add => self.toggle_pick(id),
                Some(id) => {
                    self.picks = vec![id];
                    self.pick_status();
                }
                None if !add => {
                    self.picks.clear();
                    self.status.clear();
                }
                None => {}
            }
            return;
        }
        if resp.drag_started() {
            let press = self
                .egui_ctx
                .input(|i| i.pointer.press_origin())
                .unwrap_or(p);
            match self.pickable_at(rects, press) {
                Some(id) if editable => {
                    if !self.picks.contains(&id) {
                        if !add {
                            self.picks.clear();
                        }
                        self.picks.push(id);
                    }
                    self.snapshot();
                    self.pick_drag = Some(PickDrag::Move { last: press });
                    self.pick_status();
                }
                Some(_) => {}
                None => {
                    if let Some((page, start)) = self.page_point(rects, press) {
                        self.pick_drag = Some(PickDrag::Frame { page, start, add });
                    }
                }
            }
        }
        match self.pick_drag {
            Some(PickDrag::Move { last }) if resp.dragged() => {
                let d = (p - last) / self.zoom;
                if d != Vec2::ZERO {
                    self.shift_picks(d.x, d.y);
                    self.pick_drag = Some(PickDrag::Move { last: p });
                }
            }
            _ => {}
        }
        if resp.drag_stopped() {
            match self.pick_drag.take() {
                Some(PickDrag::Move { .. }) => {
                    if let Some(doc) = self.doc.as_mut() {
                        doc.reorder();
                    }
                    self.assist.invalidate();
                }
                Some(PickDrag::Frame { page, start, add }) => {
                    let end = self.frame_end(rects, page, p);
                    self.pick_framed(page, PRect::from_points(start, end), add);
                }
                None => {}
            }
        }
    }

    /// The pointer in `page`'s points, even outside the page.
    fn frame_end(&self, rects: &[Rect], page: usize, p: Pos2) -> (f32, f32) {
        rects.get(page).map_or((0.0, 0.0), |r| {
            PageXf {
                origin: r.min,
                zoom: self.zoom,
            }
            .to_page(p)
        })
    }

    /// Selects the added boxes at least half inside `frame`.
    fn pick_framed(&mut self, page: usize, frame: PRect, add: bool) {
        let Some(doc) = &self.doc else { return };
        let inside: Vec<u64> = doc
            .entries
            .iter()
            .filter(|e| !e.f.is_real() && e.f.page == page)
            .filter(|e| {
                let area = e.f.rect.area().max(1e-3);
                e.f.rect
                    .intersect(&frame)
                    .is_some_and(|i| i.area() / area >= 0.5)
            })
            .map(|e| e.id)
            .collect();
        if !add {
            self.picks.clear();
        }
        for id in inside {
            if !self.picks.contains(&id) {
                self.picks.push(id);
            }
        }
        self.pick_status();
    }

    /// Keys for the selection (no box focused): Delete removes, arrows
    /// move (Shift: 10 points), Esc lets go, Ctrl+A (Select tool) selects
    /// every added box on the page. True when a key was taken.
    pub(super) fn pick_keys(&mut self, ctx: &egui::Context) -> bool {
        if self.tool == Tool::Select && ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::A))
        {
            self.pick_all_on_page();
            return true;
        }
        if self.picks.is_empty() {
            return false;
        }
        let editable = self.doc.as_ref().is_some_and(|d| d.form.editable());
        let (del, esc, arrow) = ctx.input_mut(|i| {
            let del = i.consume_key(Modifiers::NONE, Key::Delete)
                || i.consume_key(Modifiers::NONE, Key::Backspace);
            let esc = i.consume_key(Modifiers::NONE, Key::Escape);
            let mut arrow = None;
            for (k, d) in [
                (Key::ArrowLeft, (-1.0, 0.0)),
                (Key::ArrowRight, (1.0, 0.0)),
                (Key::ArrowUp, (0.0, -1.0)),
                (Key::ArrowDown, (0.0, 1.0)),
            ] {
                let step = if i.consume_key(Modifiers::SHIFT, k) {
                    10.0
                } else if i.consume_key(Modifiers::NONE, k) {
                    1.0
                } else {
                    continue;
                };
                arrow = Some((d.0 * step, d.1 * step));
            }
            (del, esc, arrow)
        });
        if del && editable {
            self.remove_picks();
        } else if esc {
            self.picks.clear();
            self.status.clear();
        } else if let Some((dx, dy)) = arrow.filter(|_| editable) {
            // A run of nudges is one undo step.
            let now = Instant::now();
            if self
                .last_nudge
                .is_none_or(|t| now.duration_since(t).as_secs_f32() > 1.0)
            {
                self.snapshot();
            }
            self.last_nudge = Some(now);
            self.shift_picks(dx, dy);
            if let Some(doc) = self.doc.as_mut() {
                doc.reorder();
            }
        } else {
            return false;
        }
        true
    }

    /// Outlines the selected boxes and the frame being dragged.
    pub(super) fn draw_picks(
        &mut self,
        painter: &egui::Painter,
        rects: &[Rect],
        pointer: Option<Pos2>,
    ) {
        let Some(doc) = &self.doc else { return };
        self.picks.retain(|id| doc.index_of(*id).is_some());
        let accent = self.palette.accent;
        for id in &self.picks {
            let f = &doc.entries[doc.index_of(*id).unwrap()].f;
            let Some(pr) = rects.get(f.page) else {
                continue;
            };
            let r = PageXf {
                origin: pr.min,
                zoom: self.zoom,
            }
            .rect(&f.rect)
            .expand(2.0);
            painter.rect_filled(r, 2.0, accent.gamma_multiply(0.12));
            painter.rect_stroke(r, 2.0, Stroke::new(1.5, accent), egui::StrokeKind::Middle);
        }
        if let (Some(PickDrag::Frame { page, start, .. }), Some(p)) = (self.pick_drag, pointer)
            && let Some(pr) = rects.get(page)
        {
            let a = PageXf {
                origin: pr.min,
                zoom: self.zoom,
            }
            .pt(start.0, start.1);
            let r = Rect::from_two_pos(a, p);
            painter.rect_filled(r, 0.0, accent.gamma_multiply(0.08));
            painter.rect_stroke(r, 0.0, Stroke::new(1.0, accent), egui::StrokeKind::Middle);
        }
    }
}
