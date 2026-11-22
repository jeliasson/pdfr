//! Following links: the PDF's own (web addresses, jumps within the
//! document) and addresses printed as text. Boxes take clicks first. A jump
//! can be undone with Go back (Ctrl+Shift+G).

use super::*;
use pdfr_core::links::{Link, Target};

/// Most positions kept for Go back.
const BACK_MAX: usize = 50;

/// A web or e-mail address waiting for "Open this link?".
pub(super) struct LinkAsk {
    pub url: String,
    /// Its site (lowercase), for "Always open links to …"; `None` for an
    /// address without one.
    pub host: Option<String>,
    /// Where the pointer was, to show the question there.
    pub at: Option<Pos2>,
}

/// Whether pdfr hands this address to the desktop at all: web pages and
/// e-mail only, never files, scripts or other programs.
fn openable(u: &str) -> bool {
    let lower = u.to_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:")
}

/// The site of a web address, lowercase: "https://user@Example.org:8080/x"
/// gives "example.org". `None` for an address without one (e-mail).
pub(super) fn link_host(u: &str) -> Option<String> {
    let lower = u.trim().to_lowercase();
    let rest = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))?;
    let authority = rest.split(['/', '?', '#', '\\']).next()?;
    // Whatever is before the last "@" is a user name, not the site.
    let host_port = authority.rsplit('@').next()?;
    let host = if let Some(v6) = host_port.strip_prefix('[') {
        format!("[{}]", v6.split(']').next()?)
    } else {
        host_port.split(':').next()?.to_string()
    };
    let host = host.trim_end_matches('.').to_string();
    (!host.is_empty()).then_some(host)
}

/// Opens an address with the desktop. Tests record it instead.
fn launch(u: &str) -> Result<(), String> {
    #[cfg(test)]
    {
        LAUNCHED.with(|l| l.borrow_mut().push(u.to_string()));
        Ok(())
    }
    #[cfg(not(test))]
    crate::system::open_url(u)
}

#[cfg(test)]
thread_local! {
    /// Addresses tests "opened".
    pub(super) static LAUNCHED: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

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

    /// Follows a link: jumps within the document, or asks before a web
    /// or e-mail address opens (unless its site is always allowed).
    pub(super) fn follow_link(&mut self, l: &Link) {
        self.follow_link_with(l, false);
    }

    /// Follows a link; `now` (Ctrl+click) opens an address without asking.
    pub(super) fn follow_link_with(&mut self, l: &Link, now: bool) {
        match &l.target {
            Target::Uri(u) => {
                if !openable(u) {
                    // Files, scripts and other schemes are never opened.
                    self.status = format!("Not opened (not a web address): {u}");
                    return;
                }
                let host = link_host(u);
                let allowed = host.as_ref().is_some_and(|h| {
                    self.assist
                        .a
                        .store
                        .settings
                        .link_hosts
                        .iter()
                        .any(|a| a == h)
                });
                if now || allowed {
                    self.open_address(u);
                } else {
                    let at = self.egui_ctx.input(|i| i.pointer.latest_pos());
                    self.link_ask = Some(LinkAsk {
                        url: u.clone(),
                        host,
                        at,
                    });
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

    /// Hands an address to the desktop (the browser or mail app).
    fn open_address(&mut self, u: &str) {
        self.status = match launch(u) {
            Ok(()) => format!("Opening {u}"),
            Err(e) => format!("Could not open {u}: {e}"),
        };
    }

    /// The small "Open this link?" card by the pointer: the whole address,
    /// Open, Cancel and "Always open links to <site>". Esc or a click
    /// elsewhere closes it; nothing else waits for it.
    pub(super) fn link_window(&mut self, ctx: &egui::Context) {
        use crate::assist_ui::widgets::{
            Colors, link, panel_frame, primary_button, secondary_button,
        };
        use egui::RichText;
        let Some(ask) = &self.link_ask else { return };
        let pal = self.palette.clone();
        let c = Colors::new(&pal);
        let (mut open, mut cancel) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        let mut always = false;
        let at = ask
            .at
            .unwrap_or_else(|| self.viewport.center() - vec2(160.0, 60.0))
            + vec2(0.0, 12.0);
        let mail = ask.url.to_lowercase().starts_with("mailto:");
        let area = egui::Area::new(Id::unique("pdfr-link-ask"))
            .fixed_pos(at)
            .constrain(true)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                panel_frame(ctx, &pal, &c, 14).show(ui, |ui| {
                    ui.set_max_width(340.0);
                    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                    ui.label(
                        RichText::new(if mail {
                            "Write an e-mail?"
                        } else {
                            "Open this link?"
                        })
                        .strong()
                        .color(c.text),
                    );
                    // The whole address, so a look-alike shows for what it is.
                    let shown = ask.url.strip_prefix("mailto:").unwrap_or(&ask.url);
                    ui.add(
                        egui::Label::new(RichText::new(shown).monospace().size(12.0).color(c.weak))
                            .wrap_mode(egui::TextWrapMode::Wrap),
                    );
                    ui.horizontal(|ui| {
                        if let Some(h) = &ask.host
                            && !mail
                        {
                            always = link(ui, &c, &format!("Always open links to {h}")).clicked();
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            open |= primary_button(ui, &c, "Open")
                                .on_hover_text("Enter. Ctrl+click a link to open it at once.")
                                .clicked();
                            cancel |= secondary_button(ui, &c, "Cancel")
                                .on_hover_text("Esc")
                                .clicked();
                        });
                    });
                });
            });
        let outside = ctx.input(|i| {
            i.pointer.any_pressed()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|p| !area.response.rect.contains(p))
        });
        if always && let Some(h) = ask.host.clone() {
            let hosts = &mut self.assist.a.store.settings.link_hosts;
            if !hosts.contains(&h) {
                hosts.push(h);
                self.assist.a.save();
            }
            open = true;
        }
        if open {
            let url = self.link_ask.take().unwrap().url;
            self.open_address(&url);
        } else if cancel || outside {
            self.link_ask = None;
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

#[cfg(test)]
mod tests {
    use super::link_host;

    #[test]
    fn hosts_of_addresses() {
        let h = |u: &str| link_host(u);
        assert_eq!(
            h("https://Example.org/form-help").as_deref(),
            Some("example.org")
        );
        assert_eq!(
            h("http://example.org:8080?q=1").as_deref(),
            Some("example.org")
        );
        // The part before "@" is a user name: the site is the one after.
        assert_eq!(
            h("https://bank.com@evil.example/x").as_deref(),
            Some("evil.example")
        );
        assert_eq!(h("https://example.org./").as_deref(), Some("example.org"));
        assert_eq!(h("https://[::1]:80/").as_deref(), Some("[::1]"));
        assert_eq!(h("https:///nothing"), None);
        assert_eq!(h("mailto:anna@example.org"), None);
        assert_eq!(h("file:///etc/passwd"), None);
    }
}
