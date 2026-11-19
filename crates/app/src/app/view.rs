//! The document view: page geometry, zoom, scrolling, the scrollbar and
//! which page images are kept or requested from the render workers.

use super::*;

impl App {
    /// The colour of paper under the page images.
    pub(super) fn page_fill(&self) -> Color32 {
        if self.dark_pages() {
            Color32::from_gray(pdfr_core::render::DARK_PAPER)
        } else {
            Color32::WHITE
        }
    }

    /// Colour of what is written on the page (typed text, ticks).
    pub(super) fn page_ink(&self) -> Color32 {
        if self.dark_pages() {
            Color32::from_gray(pdfr_core::render::DARK_INK)
        } else {
            Color32::BLACK
        }
    }

    pub(super) fn dark_pages(&self) -> bool {
        self.assist.a.store.settings.dark_pages
    }

    /// Switches pages between light and dark (screen only).
    pub(super) fn set_dark_pages(&mut self, on: bool) {
        self.assist.a.store.settings.dark_pages = on;
        self.assist.a.save();
    }

    /// Makes the page images follow the light/dark setting (it can also
    /// change in Settings).
    pub(super) fn follow_dark_pages(&mut self) {
        let on = self.dark_pages();
        if self.dark_applied == on {
            return;
        }
        self.dark_applied = on;
        let generation = self.worker.set_dark(on);
        if let Some(doc) = &mut self.doc {
            doc.generation = generation;
            doc.tex.clear();
            doc.thumbs.clear();
            doc.pending.clear();
        }
        self.thumbs_asked.clear();
    }

    pub(super) fn max_page_width(&self) -> f32 {
        self.doc
            .as_ref()
            .map_or(612.0, |d| d.sizes.iter().map(|s| s.0).fold(1.0, f32::max))
    }

    pub(super) fn fit_width_zoom(&self) -> f32 {
        let w = if self.viewport.is_positive() {
            self.viewport.width()
        } else {
            1000.0
        };
        let (n, gap) = if self.two_up { (2.0, GAP) } else { (1.0, 0.0) };
        ((w - 2.0 * PAD - SCROLLBAR_W - gap) / (n * self.max_page_width())).max(0.1)
    }

    /// Pages by row: one per row, or side by side in pairs like an open
    /// book (the first page alone, as a cover).
    pub(super) fn page_rows(&self) -> Vec<Vec<usize>> {
        let n = self.doc.as_ref().map_or(0, |d| d.sizes.len());
        if !self.two_up {
            return (0..n).map(|i| vec![i]).collect();
        }
        let mut rows = vec![];
        if n > 0 {
            rows.push(vec![0]);
        }
        let mut i = 1;
        while i < n {
            rows.push((i..(i + 2).min(n)).collect());
            i += 2;
        }
        rows
    }

    /// The first page of the row after page `i`'s (the last page if none).
    pub(super) fn row_after(&self, i: usize) -> usize {
        let rows = self.page_rows();
        let r = rows.iter().position(|r| r.contains(&i)).unwrap_or(0);
        rows.get(r + 1).or(rows.last()).map_or(0, |r| r[0])
    }

    /// The first page of the row before page `i`'s.
    pub(super) fn row_before(&self, i: usize) -> usize {
        let rows = self.page_rows();
        let r = rows.iter().position(|r| r.contains(&i)).unwrap_or(0);
        rows.get(r.saturating_sub(1)).map_or(0, |r| r[0])
    }

    pub(super) fn content_size(&self) -> Vec2 {
        let Some(doc) = &self.doc else {
            return Vec2::ZERO;
        };
        let rows = self.page_rows();
        let (mut w, mut h) = (0.0f32, 0.0f32);
        for r in &rows {
            let rw: f32 = r.iter().map(|&i| doc.sizes[i].0 * self.zoom).sum::<f32>()
                + GAP * (r.len() as f32 - 1.0);
            let rh = r.iter().map(|&i| doc.sizes[i].1).fold(0.0, f32::max) * self.zoom;
            w = w.max(rw);
            h += rh + GAP;
        }
        vec2(w + 2.0 * PAD, h - GAP + 2.0 * PAD)
    }

    /// Screen rect of every page at the current zoom and scroll.
    pub(crate) fn page_rects(&self) -> Vec<Rect> {
        let Some(doc) = &self.doc else { return vec![] };
        let content = self.content_size();
        let vp = self.viewport;
        let avail_w = vp.width() - SCROLLBAR_W;
        let cx = if content.x <= avail_w {
            vp.left() + avail_w / 2.0
        } else {
            vp.left() - self.scroll.x + content.x / 2.0
        };
        let mut out = vec![Rect::NOTHING; doc.sizes.len()];
        let mut y = vp.top() - self.scroll.y + PAD;
        for row in self.page_rows() {
            let rw: f32 = row.iter().map(|&i| doc.sizes[i].0 * self.zoom).sum::<f32>()
                + GAP * (row.len() as f32 - 1.0);
            let mut x = cx - rw / 2.0;
            let mut rh = 0.0f32;
            for &i in &row {
                let (w, h) = doc.sizes[i];
                out[i] = Rect::from_min_size(
                    pos2(x.round(), y.round()),
                    vec2(w * self.zoom, h * self.zoom),
                );
                x += w * self.zoom + GAP;
                rh = rh.max(h * self.zoom);
            }
            y += rh + GAP;
        }
        out
    }

    pub(super) fn clamp_scroll(&mut self) {
        let c = self.content_size();
        let vp = self.viewport;
        self.scroll.x = self
            .scroll
            .x
            .clamp(0.0, (c.x - (vp.width() - SCROLLBAR_W)).max(0.0));
        self.scroll.y = self.scroll.y.clamp(0.0, (c.y - vp.height()).max(0.0));
    }

    pub(super) fn scroll_by(&mut self, dy: f32) {
        self.scroll.y += dy;
        self.clamp_scroll();
    }

    /// The page under the middle of the window (the status bar's "Page n").
    pub(super) fn current_page(&self) -> usize {
        self.page_rects()
            .iter()
            .position(|r| r.bottom() > self.viewport.center().y)
            .unwrap_or(0)
    }

    /// Scrolls so page `i` (clamped to the last) starts at the top.
    pub(super) fn scroll_to_page(&mut self, i: usize) {
        let rects = self.page_rects();
        let Some(r) = rects.get(i.min(rects.len().saturating_sub(1))) else {
            return;
        };
        self.scroll.y += r.top() - (self.viewport.top() + PAD);
        self.clamp_scroll();
    }

    /// Zooms to `z`, keeping the document point under `anchor` fixed.
    pub(super) fn zoom_to(&mut self, z: f32, anchor: Pos2) {
        let z = z.clamp(0.1, 8.0);
        let before = self.page_rects();
        let hit = before
            .iter()
            .position(|r| r.top() <= anchor.y && anchor.y <= r.bottom() + GAP);
        let old = self.zoom;
        self.zoom = z;
        if let Some(i) = hit {
            let local = (anchor - before[i].min) / old;
            let after = self.page_rects();
            let now = after[i].min + local * z;
            self.scroll += now - anchor;
        } else {
            self.scroll *= z / old;
        }
        self.clamp_scroll();
    }

    pub(super) fn scroll_to_field(&mut self, id: u64) {
        let Some(doc) = &self.doc else { return };
        let Some(i) = doc.index_of(id) else { return };
        let (page, rect) = (doc.entries[i].f.page, doc.entries[i].f.rect);
        self.scroll_to_rect(page, rect);
    }

    /// Scrolls as little as needed to bring a rectangle of a page into view.
    pub(super) fn scroll_to_rect(&mut self, page: usize, rect: PRect) {
        let rects = self.page_rects();
        let Some(pr) = rects.get(page) else { return };
        let xf = PageXf {
            origin: pr.min,
            zoom: self.zoom,
        };
        let r = xf.rect(&rect);
        let vp = self.viewport.shrink(30.0);
        if r.top() < vp.top() {
            self.scroll.y -= vp.top() - r.top();
        } else if r.bottom() > vp.bottom() {
            self.scroll.y += r.bottom() - vp.bottom();
        }
        if r.left() < vp.left() {
            self.scroll.x -= vp.left() - r.left();
        } else if r.right() > vp.right() - SCROLLBAR_W {
            self.scroll.x += r.right() - (vp.right() - SCROLLBAR_W);
        }
        self.clamp_scroll();
    }

    /// Requests renders for pages near the viewport and drops far textures.
    pub(super) fn manage_textures(&mut self, ppp: f32) {
        let rects = self.page_rects();
        let vp = self.viewport;
        let scale = self.zoom * ppp;
        let Some(doc) = &mut self.doc else { return };
        let h = vp.height().max(1.0);
        let mut wanted = HashSet::new();
        for (i, r) in rects.iter().enumerate() {
            let dist = if r.bottom() < vp.top() {
                vp.top() - r.bottom()
            } else if r.top() > vp.bottom() {
                r.top() - vp.bottom()
            } else {
                0.0
            };
            if dist <= 1.5 * h {
                wanted.insert(i);
                let have = doc.tex.get(&i).map(|t| t.scale);
                let pend = doc.pending.get(&i).copied();
                let max_scale = crate::worker::MAX_TEX_SIDE / doc.sizes[i].0.max(doc.sizes[i].1);
                let target = scale.min(max_scale);
                let fine = |s: Option<f32>| s.is_some_and(|s| (s - target).abs() / target < 0.01);
                if !fine(have) && !fine(pend) {
                    // Visible pages first, then by distance.
                    doc.pending.insert(i, target);
                    self.worker.request(i, target, dist);
                }
            } else if dist > 4.0 * h {
                doc.tex.remove(&i);
                doc.pending.remove(&i);
            }
        }
        let pend: Vec<usize> = doc
            .pending
            .keys()
            .copied()
            .filter(|p| !wanted.contains(p))
            .collect();
        for p in pend {
            doc.pending.remove(&p);
        }
        self.worker.retain(|p| wanted.contains(&p));
        // Labels are best on the pages the user is looking at.
        let visible: Vec<usize> = rects
            .iter()
            .enumerate()
            .filter(|(_, r)| r.intersects(vp))
            .map(|(i, _)| i)
            .filter(|i| !doc.detected[*i])
            .collect();
        if !visible.is_empty() {
            self.worker.prioritise_detection(&visible);
        }
    }

    pub fn all_visible_rendered(&self, ppp: f32) -> bool {
        let Some(doc) = &self.doc else { return false };
        let target = self.zoom * ppp;
        self.page_rects()
            .iter()
            .enumerate()
            .filter(|(_, r)| r.intersects(self.viewport))
            .all(|(i, _)| {
                let max_scale = crate::worker::MAX_TEX_SIDE / doc.sizes[i].0.max(doc.sizes[i].1);
                doc.tex
                    .get(&i)
                    .is_some_and(|t| (t.scale - target.min(max_scale)).abs() / target < 0.01)
            })
    }

    pub(super) fn scrollbar(&mut self, ui: &mut egui::Ui, painter: &egui::Painter, full: Rect) {
        let content = self.content_size();
        let vh = full.height();
        if content.y <= vh {
            return;
        }
        let track = Rect::from_min_max(pos2(full.right() - SCROLLBAR_W, full.top()), full.max);
        let frac = vh / content.y;
        let th = (track.height() * frac).max(24.0);
        let max_scroll = content.y - vh;
        let ty = track.top() + (track.height() - th) * (self.scroll.y / max_scroll);
        let thumb = Rect::from_min_size(pos2(track.left() + 2.0, ty), vec2(SCROLLBAR_W - 4.0, th));
        let resp = ui.interact(track, Id::unique("pdfr-scrollbar"), Sense::click_and_drag());
        if resp.dragged() {
            self.scroll.y += resp.drag_delta().y * max_scroll / (track.height() - th);
            self.clamp_scroll();
        } else if resp.clicked()
            && let Some(p) = resp.interact_pointer_pos()
        {
            self.scroll.y = ((p.y - track.top() - th / 2.0) / (track.height() - th)) * max_scroll;
            self.clamp_scroll();
        }
        let c = if resp.hovered() || resp.dragged() {
            self.palette.text.gamma_multiply(0.6)
        } else {
            self.palette.text.gamma_multiply(0.35)
        };
        painter.rect_filled(thumb, 3.0, c);
    }
}
