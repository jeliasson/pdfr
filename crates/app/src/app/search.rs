//! Searching the document's text (Ctrl+F): the hits for the query, kept up
//! to date as pages' text arrives, highlighted on the pages, and stepping
//! from one to the next.

use super::*;
use pdfr_core::search::Hit;

/// Most hits listed (a common word in a long document has thousands).
pub(super) const MAX_HITS: usize = 2000;

#[derive(Default)]
pub(super) struct Search {
    pub query: String,
    pub hits: Vec<Hit>,
    /// The hit last jumped to.
    pub current: Option<usize>,
    /// Highlights are drawn (until Esc).
    pub shown: bool,
    /// How many pages had text when `hits` was made.
    pages_read: usize,
}

impl Search {
    /// Forgets the hits (pages moved): they are found again.
    pub fn reset_hits(&mut self) {
        self.hits.clear();
        self.current = None;
        self.pages_read = usize::MAX;
    }
}

impl App {
    /// Brings the hits up to date with `query` and the pages read so far.
    pub(super) fn update_search(&mut self, query: &str) {
        let Some(doc) = &self.doc else { return };
        let s = self.search.get_or_insert_with(Search::default);
        if s.query == query && s.pages_read == doc.text.len() {
            return;
        }
        let changed = s.query != query;
        s.query = query.to_string();
        s.pages_read = doc.text.len();
        let pages: Vec<(usize, &[pdfr_core::text::TextRun])> =
            doc.text.iter().map(|(p, r)| (*p, r.as_slice())).collect();
        let mut hits = pdfr_core::search::search(&pages, query);
        hits.truncate(MAX_HITS);
        s.hits = hits;
        if changed {
            s.current = None;
        }
        s.shown = true;
    }

    /// Pages whose text has been read, and the page count.
    pub(super) fn text_progress(&self) -> (usize, usize) {
        self.doc
            .as_ref()
            .map_or((0, 0), |d| (d.text.len(), d.sizes.len()))
    }

    /// Scrolls hit `i` into view and makes it the current one.
    pub(super) fn show_hit(&mut self, i: usize) {
        let Some(s) = &mut self.search else { return };
        let Some(h) = s.hits.get(i) else { return };
        s.current = Some(i);
        s.shown = true;
        let (page, rect) = (h.page, h.rects.first().copied());
        if let Some(r) = rect {
            self.scroll_to_rect(page, r);
        }
    }

    /// The next (or previous) hit after the current one.
    pub(super) fn step_hit(&mut self, back: bool) {
        let Some(s) = &self.search else { return };
        let n = s.hits.len();
        if n == 0 {
            return;
        }
        let next = match (s.current, back) {
            (None, false) => 0,
            (None, true) => n - 1,
            (Some(c), false) => (c + 1) % n,
            (Some(c), true) => (c + n - 1) % n,
        };
        self.show_hit(next);
        self.status = format!("Match {} of {n}", next + 1);
    }

    /// Draws the hits on the visible pages; the current one stronger.
    pub(super) fn draw_hits(&self, painter: &egui::Painter, rects: &[Rect]) {
        let Some(s) = self.search.as_ref().filter(|s| s.shown) else {
            return;
        };
        let vp = self.viewport;
        let accent = self.palette.accent;
        for (i, h) in s.hits.iter().enumerate() {
            let Some(pr) = rects.get(h.page) else {
                continue;
            };
            if !pr.intersects(vp) {
                continue;
            }
            let xf = PageXf {
                origin: pr.min,
                zoom: self.zoom,
            };
            let current = s.current == Some(i);
            for r in &h.rects {
                let sr = xf.rect(r).expand(1.0);
                if current {
                    painter.rect_filled(sr, 2.0, accent.gamma_multiply(0.45));
                    painter.rect_stroke(
                        sr,
                        2.0,
                        Stroke::new(1.5, accent),
                        egui::StrokeKind::Outside,
                    );
                } else {
                    painter.rect_filled(sr, 2.0, accent.gamma_multiply(0.22));
                }
            }
        }
    }
}
