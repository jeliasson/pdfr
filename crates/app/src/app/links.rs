//! Following links: the PDF's own (web addresses, jumps within the
//! document) and addresses printed as text. Boxes take clicks first. A jump
//! can be undone with Go back (Ctrl+Shift+G).

use super::*;
use pdfr_core::links::{Link, Target};

/// Most positions kept for Go back.
const BACK_MAX: usize = 50;

impl App {
    /// A page's printed addresses become links when its text arrives,
    /// except where the PDF already has a link.
    pub(super) fn add_text_links(doc: &mut Doc, page: usize) {
        let Some(runs) = doc.text.get(&page) else {
            return;
        };
        let found: Vec<Link> = pdfr_core::links::text_links(page, runs)
            .into_iter()
            .filter(|l| {
                !doc.form
                    .links
                    .iter()
                    .any(|o| o.page == page && o.rect.overlap_ratio(&l.rect) > 0.3)
            })
            .collect();
        doc.text_links.insert(page, found);
    }

    /// The link under a screen point, if any.
    pub(super) fn link_at(&self, rects: &[Rect], p: Pos2) -> Option<Link> {
        let doc = self.doc.as_ref()?;
        let page = rects.iter().position(|r| r.contains(p))?;
        let (x, y) = PageXf {
            origin: rects[page].min,
            zoom: self.zoom,
        }
        .to_page(p);
        doc.form
            .links
            .iter()
            .chain(doc.text_links.get(&page).into_iter().flatten())
            .filter(|l| l.page == page && l.rect.contains(x, y))
            .min_by(|a, b| a.rect.area().total_cmp(&b.rect.area()))
            .cloned()
    }

    /// What a link does, for its tooltip.
    pub(super) fn link_text(&self, l: &Link) -> String {
        match &l.target {
            Target::Uri(u) => u
                .strip_prefix("mailto:")
                .map_or(u.clone(), |m| format!("E-mail {m}")),
            Target::Dest { page, .. } => format!("Go to page {}", page + 1),
            Target::Named(n) => match n.as_str() {
                "NextPage" => "Next page".into(),
                "PrevPage" => "Previous page".into(),
                "FirstPage" => "First page".into(),
                "LastPage" => "Last page".into(),
                other => other.into(),
            },
        }
    }

    /// Opens a web link with the desktop, or jumps within the document.
    pub(super) fn follow_link(&mut self, l: &Link) {
        match &l.target {
            Target::Uri(u) => {
                let lower = u.to_lowercase();
                if lower.starts_with("http://")
                    || lower.starts_with("https://")
                    || lower.starts_with("mailto:")
                {
                    self.status = match crate::system::open_url(u) {
                        Ok(()) => format!("Opening {u}"),
                        Err(e) => format!("Could not open {u}: {e}"),
                    };
                } else {
                    // Files, scripts and other schemes are never opened.
                    self.status = format!("Not opened (not a web address): {u}");
                }
            }
            Target::Dest { page, y } => {
                self.push_back();
                self.scroll_to_page(*page);
                if let Some(y) = y {
                    self.scroll.y += y * self.zoom;
                    self.clamp_scroll();
                }
            }
            Target::Named(n) => {
                let last = self
                    .doc
                    .as_ref()
                    .map_or(0, |d| d.sizes.len().saturating_sub(1));
                let cur = self.current_page();
                let to = match n.as_str() {
                    "NextPage" => Some((cur + 1).min(last)),
                    "PrevPage" => Some(cur.saturating_sub(1)),
                    "FirstPage" => Some(0),
                    "LastPage" => Some(last),
                    _ => None,
                };
                if let Some(p) = to {
                    self.push_back();
                    self.scroll_to_page(p);
                }
            }
        }
    }

    fn push_back(&mut self) {
        self.back.push(self.scroll);
        if self.back.len() > BACK_MAX {
            self.back.remove(0);
        }
    }

    /// Back to where the last link jump started.
    pub(super) fn go_back(&mut self) {
        if let Some(s) = self.back.pop() {
            self.scroll = s;
            self.clamp_scroll();
        }
    }
}
