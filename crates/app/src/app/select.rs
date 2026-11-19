//! Selecting the page's own text with the mouse and copying it: drag over
//! text, double-click a word, triple-click a line, then Ctrl+C or the
//! right-click menu. Boxes still take clicks first.

use super::*;
use pdfr_core::text::{TextPos, line_at, pos_at, selected_text, selection, word_at};

pub(super) struct TextSel {
    pub page: usize,
    pub anchor: TextPos,
    pub head: TextPos,
    /// The mouse button is still down.
    pub dragging: bool,
}

impl App {
    /// The page under a screen point and the point in page space.
    fn page_point(&self, rects: &[Rect], p: Pos2) -> Option<(usize, (f32, f32))> {
        let page = rects.iter().position(|r| r.contains(p))?;
        let xf = PageXf {
            origin: rects[page].min,
            zoom: self.zoom,
        };
        Some((page, xf.to_page(p)))
    }

    /// Whether the pointer is over a phrase of the page's text.
    pub(super) fn over_text(&self, rects: &[Rect], p: Pos2) -> bool {
        let Some((page, (x, y))) = self.page_point(rects, p) else {
            return false;
        };
        self.doc
            .as_ref()
            .and_then(|d| d.text.get(&page))
            .is_some_and(|runs| runs.iter().any(|r| r.rect.inset(-1.0).contains(x, y)))
    }

    /// Mouse on the page's text (with no tool, not on a box). Returns
    /// whether it handled the pointer.
    pub(super) fn select_mouse(
        &mut self,
        resp: &egui::Response,
        rects: &[Rect],
        p: Pos2,
        ctx: &egui::Context,
    ) -> bool {
        if let Some(sel) = &mut self.text_sel
            && sel.dragging
        {
            if resp.dragged()
                && let (Some(pr), Some(doc)) = (rects.get(sel.page), &self.doc)
                && let Some(runs) = doc.text.get(&sel.page)
            {
                let (x, y) = PageXf {
                    origin: pr.min,
                    zoom: self.zoom,
                }
                .to_page(p);
                if let Some(h) = pos_at(runs, x, y) {
                    sel.head = h;
                }
            }
            if resp.drag_stopped() || !resp.dragged() {
                sel.dragging = false;
            }
            return true;
        }
        let real_click = ctx.input(|i| i.pointer.any_released());
        let press = ctx.input(|i| i.pointer.press_origin()).unwrap_or(p);
        let start = resp.drag_started();
        let double = resp.double_clicked() && real_click;
        let triple = resp.triple_clicked() && real_click;
        if !(start || double || triple) {
            return false;
        }
        let at = if start { press } else { p };
        if self.field_at(rects, at).is_some() {
            return false;
        }
        let Some((page, (x, y))) = self.page_point(rects, at) else {
            return false;
        };
        let Some(runs) = self.doc.as_ref().and_then(|d| d.text.get(&page)) else {
            return false;
        };
        if (double || triple) && !self.over_text(rects, at) {
            return false;
        }
        let Some(pos) = pos_at(runs, x, y) else {
            return false;
        };
        let (anchor, head) = if triple {
            line_at(runs, pos.run)
        } else if double {
            let ch = runs[pos.run].char_at(x).0;
            word_at(runs, pos.run, ch)
        } else {
            (pos, pos)
        };
        self.text_sel = Some(TextSel {
            page,
            anchor,
            head,
            dragging: start,
        });
        self.focus = None;
        self.choice_open = None;
        self.assist.close_list();
        true
    }

    /// The selected text, if anything is selected.
    pub(super) fn selection_text(&self) -> Option<String> {
        let sel = self.text_sel.as_ref()?;
        let runs = self.doc.as_ref()?.text.get(&sel.page)?;
        let t = selected_text(runs, &selection(runs, sel.anchor, sel.head));
        (!t.trim().is_empty()).then_some(t)
    }

    pub(super) fn copy_selection(&mut self, ctx: &egui::Context) {
        if let Some(t) = self.selection_text() {
            let n = t.chars().count();
            ctx.copy_text(t);
            self.status = format!("Copied {n} character{}.", if n == 1 { "" } else { "s" });
        }
    }

    /// Paints the selection over the page's text.
    pub(super) fn draw_text_selection(&self, painter: &egui::Painter, rects: &[Rect]) {
        let Some(sel) = &self.text_sel else { return };
        let (Some(pr), Some(doc)) = (rects.get(sel.page), &self.doc) else {
            return;
        };
        let Some(runs) = doc.text.get(&sel.page) else {
            return;
        };
        let xf = PageXf {
            origin: pr.min,
            zoom: self.zoom,
        };
        let fill = self.palette.selection.gamma_multiply(0.55);
        for (ri, from, to) in selection(runs, sel.anchor, sel.head) {
            let r = &runs[ri];
            let a = r.char_rect(from);
            let b = r.char_rect(to - 1);
            let span = PRect::new(a.x0, r.rect.y0, b.x1, r.rect.y1);
            painter.rect_filled(xf.rect(&span), 1.0, fill);
        }
    }

    /// The right-click menu on the page itself (not on a box).
    pub(super) fn page_menu(&mut self, resp: &egui::Response, ctx: &egui::Context) {
        if !self.page_menu_open {
            return;
        }
        let text = self.selection_text();
        let mut copy = false;
        let mut find = false;
        let shown = resp.context_menu(|ui| {
            if ui
                .add_enabled(text.is_some(), egui::Button::new("Copy"))
                .clicked()
            {
                copy = true;
                ui.close();
            }
            if let Some(t) = text.as_ref().filter(|t| t.chars().count() <= 60) {
                let short: String = t.split_whitespace().collect::<Vec<_>>().join(" ");
                if ui.button(format!("Search for \u{201c}{short}\u{201d}")).clicked() {
                    find = true;
                    ui.close();
                }
            }
        });
        if copy {
            self.copy_selection(ctx);
        }
        if find && let Some(t) = text {
            let q = t.split_whitespace().collect::<Vec<_>>().join(" ");
            self.update_search(&q);
            self.open_palette(super::palette::Scope::Find);
        }
        if shown.is_none() || copy || find {
            self.page_menu_open = false;
        }
    }
}
