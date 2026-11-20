//! One-click stamps: today's date (Ctrl+Shift+D) into the focused box or
//! anywhere on the page, initials (Ctrl+Shift+I), and putting a signature
//! or initials on every page.

use super::*;
use pdfr_assist::dates;

/// Font size of a placed date.
const DATE_SIZE: f32 = 11.0;

impl App {
    /// Today's date written for the focused box (its format and parts as
    /// the label asks), or placed where the user clicks next.
    pub(super) fn stamp_date(&mut self) {
        let focused = self.focused_entry().cloned();
        if let Some(e) = focused.filter(|e| e.f.kind.is_text() && !e.f.read_only) {
            let doc = self.doc.as_ref().unwrap();
            let i = doc.index_of(e.id).unwrap();
            let part = self.assist.date_part(doc, i).unwrap_or(None);
            let label = e.f.label.clone().unwrap_or_default();
            let mut v = dates::format_for(dates::today(), part, &e.f, &label);
            if let Some(cap) = e.f.capacity() {
                v = v.chars().take(cap).collect();
            }
            self.replace_value(e.id, &e.f, &v);
            self.status = format!("Today's date: {v}");
            return;
        }
        self.tool = Tool::PlaceDate;
        self.status = "Click where today's date goes. Esc cancels.".into();
    }

    /// Today's date as written on a spot of its own.
    fn today_text(&self) -> String {
        let probe = Field::new_text(
            0,
            PRect::new(0.0, 0.0, 200.0, 20.0),
            TextMode::Single,
            Origin::Manual,
        );
        dates::format_for(dates::today(), None, &probe, "")
    }

    /// The box a placed date gets, its text starting at the click.
    pub(super) fn date_rect_at(&self, x: f32, y: f32) -> (PRect, String) {
        let v = self.today_text();
        let w = pdfr_core::font::font().text_width(&v, DATE_SIZE) + 6.0;
        let h = DATE_SIZE * 1.5;
        (
            PRect::new(x - 2.0, y - h / 2.0, x - 2.0 + w, y + h / 2.0),
            v,
        )
    }

    /// Places today's date at a page point, as one undo step.
    pub(super) fn place_date(&mut self, page: usize, x: f32, y: f32) {
        let (r, v) = self.date_rect_at(x, y);
        self.snapshot();
        let Some(doc) = self.doc.as_mut() else { return };
        let mut f = Field::new_text(page, r, TextMode::Single, Origin::Manual);
        f.value = v;
        f.font_size = Some(DATE_SIZE);
        doc.add(f);
        doc.reorder();
        doc.dirty = true;
        self.assist.invalidate();
        self.tool = Tool::None;
        self.status = "Date placed. Right-click it to remove it.".into();
    }

    /// Copies a signature to the same place on every other page (scaled
    /// to pages of another size), as one undo step.
    pub(super) fn sign_every_page(&mut self, id: u64) {
        let Some(doc) = &self.doc else { return };
        let Some(i) = doc.index_of(id) else { return };
        let src = doc.entries[i].f.clone();
        if !src.kind.is_signature() {
            return;
        }
        let (sw, sh) = doc.sizes[src.page];
        let mut add = Vec::new();
        for (p, &(w, h)) in doc.sizes.iter().enumerate() {
            if p == src.page {
                continue;
            }
            // The same place relative to the page; the same size.
            let (cx, cy) = src.rect.center();
            let (nx, ny) = (cx * w / sw.max(1.0), cy * h / sh.max(1.0));
            let (hw, hh) = (src.rect.width() / 2.0, src.rect.height() / 2.0);
            let r = PRect::new(nx - hw, ny - hh, nx + hw, ny + hh);
            let taken = doc.entries.iter().any(|e| {
                e.f.kind.is_signature() && e.f.page == p && e.f.rect.overlap_ratio(&r) > 0.3
            });
            if !taken {
                add.push(Field {
                    page: p,
                    rect: r,
                    ..src.clone()
                });
            }
        }
        if add.is_empty() {
            self.status = "Every page has it already.".into();
            return;
        }
        let n = add.len();
        self.snapshot();
        let doc = self.doc.as_mut().unwrap();
        for f in add {
            doc.add(f);
        }
        doc.reorder();
        doc.dirty = true;
        self.assist.invalidate();
        self.status = format!(
            "Added to {n} more page{}. Ctrl+Z takes them off.",
            if n == 1 { "" } else { "s" }
        );
    }
}
