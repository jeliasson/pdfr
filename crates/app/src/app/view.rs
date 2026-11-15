//! The document view: page geometry, zoom, scrolling, the scrollbar and
//! which page images are kept or requested from the render workers.

use super::*;

impl App {
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
        ((w - 2.0 * PAD - SCROLLBAR_W) / self.max_page_width()).max(0.1)
    }

    pub(super) fn content_size(&self) -> Vec2 {
        let Some(doc) = &self.doc else {
            return Vec2::ZERO;
        };
        let h: f32 = doc.sizes.iter().map(|s| s.1 * self.zoom + GAP).sum::<f32>() - GAP + 2.0 * PAD;
        vec2(self.max_page_width() * self.zoom + 2.0 * PAD, h)
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
        let mut y = vp.top() - self.scroll.y + PAD;
        doc.sizes
            .iter()
            .map(|&(w, h)| {
                let r = Rect::from_min_size(
                    pos2((cx - w * self.zoom / 2.0).round(), y.round()),
                    vec2(w * self.zoom, h * self.zoom),
                );
                y += h * self.zoom + GAP;
                r
            })
            .collect()
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
        let f = &doc.entries[i].f;
        let rects = self.page_rects();
        let Some(pr) = rects.get(f.page) else { return };
        let xf = PageXf {
            origin: pr.min,
            zoom: self.zoom,
        };
        let r = xf.rect(&f.rect);
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
