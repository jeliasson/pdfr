//! Two pages side by side, and presentation mode: one page at a time,
//! full screen, with nothing else on screen.

use super::*;

impl App {
    pub(super) fn toggle_two_up(&mut self) {
        let page = self.current_page();
        self.two_up = !self.two_up;
        self.clamp_scroll();
        self.scroll_to_page(page);
    }

    pub(super) fn presenting(&self) -> bool {
        self.present.is_some()
    }

    pub(super) fn start_presenting(&mut self, ctx: &egui::Context) {
        if self.doc.is_none() {
            return;
        }
        self.present = Some((self.current_page(), self.zoom, self.scroll));
        self.focus = None;
        self.tool = Tool::None;
        self.text_sel = None;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
        self.status = "Presenting. Arrow keys or a click turn pages; Esc ends.".into();
    }

    pub(super) fn stop_presenting(&mut self, ctx: &egui::Context) {
        if let Some((_, zoom, scroll)) = self.present.take() {
            self.zoom = zoom;
            self.scroll = scroll;
            self.clamp_scroll();
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
    }

    /// Each frame while presenting: page keys, and the page fitted.
    pub(super) fn present_frame(&mut self, ctx: &egui::Context, clicked: bool) {
        let Some((page, ..)) = self.present else { return };
        let n = self.doc.as_ref().map_or(0, |d| d.sizes.len());
        let (next, prev, first, last, stop) = ctx.input_mut(|i| {
            let mut k = |key| i.consume_key(Modifiers::NONE, key);
            (
                k(Key::ArrowRight) | k(Key::ArrowDown) | k(Key::Space) | k(Key::PageDown),
                k(Key::ArrowLeft) | k(Key::ArrowUp) | k(Key::Backspace) | k(Key::PageUp),
                k(Key::Home),
                k(Key::End),
                k(Key::Escape),
            )
        });
        if stop {
            self.stop_presenting(ctx);
            return;
        }
        let page = if first {
            0
        } else if last {
            n.saturating_sub(1)
        } else if next || clicked {
            (page + 1).min(n.saturating_sub(1))
        } else if prev {
            page.saturating_sub(1)
        } else {
            page
        };
        if let Some(p) = &mut self.present {
            p.0 = page;
        }
        // One page, as large as fits, centred.
        let (w, h) = self.doc.as_ref().map_or((612.0, 792.0), |d| d.sizes[page]);
        let vp = self.viewport;
        self.zoom = ((vp.width() - 2.0 * PAD) / w.max(1.0))
            .min((vp.height() - 2.0 * PAD) / h.max(1.0))
            .max(0.1);
        self.scroll_to_page(page);
        let ph = h * self.zoom;
        // Centre vertically where there is room above (clamping keeps the
        // first page at the top edge otherwise).
        self.scroll.y -= ((vp.height() - 2.0 * PAD - ph) / 2.0).max(0.0);
        self.clamp_scroll();
    }
}
