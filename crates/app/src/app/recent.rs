//! Recent files (Ctrl+E) and reopening a file where it was left: the page,
//! the place on it and the zoom. Kept in pdfr's settings on this computer;
//! Settings → General turns it off or clears the list.

use super::*;
use pdfr_assist::memory::{RECENT_MAX, RecentFile, now};

impl App {
    fn automated(&self) -> bool {
        self.cli.demo || self.cli.save.is_some() || self.cli.screenshot.is_some() || self.cli.record.is_some()
    }

    /// The page at the top of the window and how far down it (0..1).
    fn top_position(&self) -> (usize, f32) {
        let rects = self.page_rects();
        let top = self.viewport.top() + PAD;
        for (i, r) in rects.iter().enumerate() {
            if r.bottom() > top {
                let off = ((top - r.top()) / r.height().max(1.0)).clamp(0.0, 1.0);
                return (i, off);
            }
        }
        (0, 0.0)
    }

    /// Notes the open document's place in the recent list.
    pub(super) fn remember_position(&mut self) {
        if self.automated() || !self.assist.a.store.settings.remember_recent {
            return;
        }
        let Some(doc) = &self.doc else { return };
        let path = key(&doc.path);
        let (page, offset) = self.top_position();
        let entry = RecentFile {
            path: path.clone(),
            page,
            offset,
            zoom: self.zoom,
            opened_at: now(),
        };
        let list = &mut self.assist.a.store.settings.recent;
        let opened_at = list
            .iter()
            .find(|r| r.path == path)
            .map_or(entry.opened_at, |r| r.opened_at);
        list.retain(|r| r.path != path);
        list.insert(0, RecentFile { opened_at, ..entry });
        list.truncate(RECENT_MAX);
        self.assist.a.save();
    }

    /// On opening: moves the file to the top of the list and, if it was
    /// open before, plans the return to where it was left.
    pub(super) fn opened_recent(&mut self, path: &std::path::Path) {
        if self.automated() || !self.assist.a.store.settings.remember_recent {
            return;
        }
        let p = key(path);
        let list = &mut self.assist.a.store.settings.recent;
        let before = list.iter().position(|r| r.path == p).map(|i| list.remove(i));
        if let Some(r) = &before
            && r.page + (r.offset > 0.02) as usize > 0
        {
            self.restore = Some((r.page, r.offset, r.zoom));
        }
        let mut entry = before.unwrap_or(RecentFile {
            path: p,
            page: 0,
            offset: 0.0,
            zoom: 0.0,
            opened_at: 0,
        });
        entry.opened_at = now();
        list.insert(0, entry);
        list.truncate(RECENT_MAX);
        self.assist.a.save();
    }

    /// Goes back to where the file was left, once the window has its size.
    pub(super) fn apply_restore(&mut self) {
        if !self.viewport.is_positive() {
            return;
        }
        let Some((page, offset, zoom)) = self.restore.take() else {
            return;
        };
        let n = self.doc.as_ref().map_or(0, |d| d.sizes.len());
        if page >= n {
            return;
        }
        if zoom > 0.05 {
            self.zoom = zoom.clamp(0.1, 8.0);
        }
        self.scroll_to_page(page);
        if let Some(r) = self.page_rects().get(page) {
            self.scroll.y += offset * r.height();
            self.clamp_scroll();
        }
        self.status = format!(
            "Back where you left off, page {} of {n}. Home goes to the top.",
            page + 1
        );
    }

    /// The recent files that are not the open one.
    pub(super) fn recent_files(&self) -> Vec<RecentFile> {
        let open = self.doc.as_ref().map(|d| key(&d.path));
        self.assist
            .a
            .store
            .settings
            .recent
            .iter()
            .filter(|r| Some(&r.path) != open.as_ref())
            .cloned()
            .collect()
    }

    /// The list of recent files in the empty window.
    pub(super) fn recent_start(&mut self, ui: &mut egui::Ui, area: Rect) {
        let files: Vec<RecentFile> = self.recent_files().into_iter().take(8).collect();
        if files.is_empty() {
            return;
        }
        let top = area.center().y + 30.0;
        let w = 420.0f32.min(area.width() - 40.0);
        let rect = Rect::from_min_size(pos2(area.center().x - w / 2.0, top), vec2(w, area.bottom() - top));
        let mut open = None;
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            ui.label(egui::RichText::new("Recent").size(12.0).color(self.palette.weak_text));
            for r in &files {
                let path = std::path::Path::new(&r.path);
                let name = path.file_name().map_or(r.path.clone(), |n| n.to_string_lossy().into_owned());
                let exists = path.exists();
                let text = egui::RichText::new(format!("{name}    {}", when(r.opened_at))).color(if exists {
                    self.palette.text
                } else {
                    self.palette.weak_text
                });
                let resp = ui
                    .add_enabled(exists, egui::Button::new(text).frame(false))
                    .on_hover_text(&r.path);
                if resp.clicked() {
                    open = Some(path.to_path_buf());
                }
            }
        });
        if let Some(p) = open {
            self.request_open(p);
        }
    }
}

/// How a file is listed: its full path, so the same file opened from
/// different folders is one entry.
fn key(path: &std::path::Path) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

/// "today", "yesterday", "3 days ago", or the date.
pub(super) fn when(t: u64) -> String {
    match now().saturating_sub(t) / 86_400 {
        0 => "today".into(),
        1 => "yesterday".into(),
        d @ 2..=13 => format!("{d} days ago"),
        _ => ymd(t),
    }
}

/// A Unix time as a calendar date, `2026-11-19` (UTC).
fn ymd(t: u64) -> String {
    let z = (t / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + (m <= 2) as i64;
    format!("{y}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn dates() {
        assert_eq!(super::ymd(0), "1970-01-01");
        assert_eq!(super::ymd(1_795_046_400), "2026-11-19");
        assert_eq!(super::ymd(951_782_400), "2000-02-29");
        assert_eq!(super::when(pdfr_assist::memory::now()), "today");
    }
}
