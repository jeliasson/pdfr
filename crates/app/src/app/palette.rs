//! The command palette (Ctrl+Shift+P / Ctrl+Space), search (Ctrl+F: the
//! document's text and boxes by label), "find a box" (Ctrl+Shift+F),
//! "go to page" (Ctrl+G) and the F1 cheatsheet: overlays in the Settings
//! design, built from the command registry so they never fall out of date.

use super::commands::Command;
use super::*;
use crate::assist_ui::widgets::{Colors, panel_frame};
use eframe::egui::{KeyboardShortcut, RichText, TextEdit};

/// What the palette searches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Commands,
    Boxes,
    Page,
    /// The document's text, plus boxes by label (Ctrl+F).
    Find,
}

pub struct Finder {
    pub scope: Scope,
    pub query: String,
    pub sel: usize,
    /// The search hit last scrolled to while browsing the list.
    previewed: Option<usize>,
}

pub enum Overlay {
    Palette(Finder),
    Cheatsheet,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Item {
    Cmd(Command),
    Box(u64),
    Page(usize),
    /// A text search hit (index into the search's hits).
    Hit(usize),
    /// A line of information; never runs.
    Info,
}

/// One row: what it does, a weak detail on the right, whether it can run.
struct Row {
    item: Item,
    title: String,
    detail: String,
    enabled: bool,
}

const QUERY_ID: &str = "pdfr-palette-query";
const WIDTH: f32 = 560.0;
const ROW_H: f32 = 34.0;
const MAX_ROWS: usize = 10;

impl App {
    pub(super) fn open_palette(&mut self, scope: Scope) {
        self.choice_open = None;
        self.assist.close_list();
        // Search starts from the last query, its highlights back on.
        let query = match (scope, &mut self.search) {
            (Scope::Find, Some(s)) => {
                s.shown = true;
                s.query.clone()
            }
            _ => String::new(),
        };
        self.overlay = Some(Overlay::Palette(Finder {
            scope,
            query,
            sel: 0,
            previewed: None,
        }));
    }

    pub(super) fn overlay_ui(&mut self, ctx: &egui::Context) {
        match &self.overlay {
            None => {}
            Some(Overlay::Cheatsheet) => self.cheatsheet_ui(ctx),
            Some(Overlay::Palette(_)) => self.palette_ui(ctx),
        }
    }

    fn rows(&self, f: &Finder) -> Vec<Row> {
        let q = f.query.trim();
        match f.scope {
            Scope::Commands => {
                let mut rows: Vec<(i32, Row)> = Command::ALL
                    .iter()
                    .filter(|&&c| c != Command::Palette)
                    .filter_map(|&c| {
                        let s = c.spec();
                        let score = fuzzy(q, s.label)?;
                        let keys: Vec<String> = s
                            .shortcuts
                            .iter()
                            .map(|k| super::commands::key_text(&self.egui_ctx, k))
                            .collect();
                        let enabled = self.can(c);
                        Some((
                            score + if enabled { 1000 } else { 0 },
                            Row {
                                item: Item::Cmd(c),
                                title: if self.is_on(c) && c == Command::ToggleRemember {
                                    "Remember this box".into()
                                } else {
                                    s.label.into()
                                },
                                detail: keys.join("  "),
                                enabled,
                            },
                        ))
                    })
                    .collect();
                // Stable: equal scores keep the registry's order.
                rows.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
                rows.into_iter().map(|(_, r)| r).collect()
            }
            Scope::Boxes => self.box_rows(q),
            Scope::Find => {
                let mut rows = Vec::new();
                if !q.is_empty() {
                    // The best few boxes by label first.
                    rows.extend(self.box_rows(q).into_iter().take(5).map(|mut r| {
                        r.detail = format!("box \u{b7} {}", r.detail);
                        r
                    }));
                }
                if let Some(s) = self.search.as_ref().filter(|s| s.query == q) {
                    rows.extend(s.hits.iter().enumerate().map(|(i, h)| Row {
                        item: Item::Hit(i),
                        title: h.snippet.clone(),
                        detail: format!("p. {}", h.page + 1),
                        enabled: true,
                    }));
                    if s.hits.len() >= super::search::MAX_HITS {
                        rows.push(Row {
                            item: Item::Info,
                            title: format!("Showing the first {}", super::search::MAX_HITS),
                            detail: String::new(),
                            enabled: false,
                        });
                    }
                }
                let (read, n) = self.text_progress();
                if !q.is_empty() && !rows.is_empty() && read < n {
                    rows.push(Row {
                        item: Item::Info,
                        title: format!("Still reading pages\u{2026} {read} of {n}"),
                        detail: String::new(),
                        enabled: false,
                    });
                }
                rows
            }
            Scope::Page => {
                let n = self.doc.as_ref().map_or(0, |d| d.sizes.len());
                match q.parse::<usize>() {
                    Ok(p) if (1..=n).contains(&p) => vec![Row {
                        item: Item::Page(p - 1),
                        title: format!("Go to page {p}"),
                        detail: format!("of {n}"),
                        enabled: true,
                    }],
                    _ => vec![],
                }
            }
        }
    }

    /// Boxes whose label matches `q`, best first.
    fn box_rows(&self, q: &str) -> Vec<Row> {
        let Some(doc) = &self.doc else { return vec![] };
        let mut rows: Vec<(i32, Row)> = self
            .editable_ids()
            .into_iter()
            .enumerate()
            .filter_map(|(n, id)| {
                let e = &doc.entries[doc.index_of(id)?];
                let title = box_name(e, n);
                let score = fuzzy(q, &title)?;
                let detail = if e.f.kind.is_toggle() {
                    if e.f.checked {
                        "ticked".into()
                    } else {
                        String::new()
                    }
                } else if e.f.kind.is_password() || e.f.kind.is_signature() {
                    String::new()
                } else {
                    e.f.shown_text().chars().take(40).collect()
                };
                Some((
                    score,
                    Row {
                        item: Item::Box(id),
                        title,
                        detail: format!("{detail}   p. {}", e.f.page + 1)
                            .trim()
                            .to_string(),
                        enabled: true,
                    },
                ))
            })
            .collect();
        rows.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
        rows.into_iter().map(|(_, r)| r).collect()
    }

    fn palette_ui(&mut self, ctx: &egui::Context) {
        if let Some(Overlay::Palette(f)) = &self.overlay
            && f.scope == Scope::Find
        {
            let q = f.query.trim().to_string();
            self.update_search(&q);
        }
        let Some(Overlay::Palette(f)) = &self.overlay else {
            return;
        };
        let rows = self.rows(f);
        let scope = f.scope;
        let mut sel = f.sel.min(rows.len().saturating_sub(1));
        let prev_sel = sel;

        // Keys first, before the text box sees them.
        let (close, enter, moves) = ctx.input_mut(|i| {
            let close = i.consume_key(Modifiers::NONE, Key::Escape);
            let enter = i.consume_key(Modifiers::NONE, Key::Enter);
            let shift_tab = i.modifiers.shift && i.key_pressed(Key::Tab);
            let mut moves = 0i32;
            for (m, k, d) in [
                (Modifiers::NONE, Key::ArrowDown, 1),
                (Modifiers::NONE, Key::ArrowUp, -1),
                (Modifiers::COMMAND, Key::J, 1),
                (Modifiers::COMMAND, Key::K, -1),
                (Modifiers::COMMAND, Key::N, 1),
                (Modifiers::COMMAND, Key::P, -1),
            ] {
                moves += d * i.count_and_consume_key(m, k) as i32;
            }
            if i.consume_key(Modifiers::NONE, Key::Tab) {
                moves += if shift_tab { -1 } else { 1 };
            }
            (close, enter, moves)
        });
        if !rows.is_empty() {
            let n = rows.len() as i32;
            sel = (sel as i32 + moves).rem_euclid(n) as usize;
        }

        let pal = self.palette.clone();
        let c = Colors::new(&pal);
        let mut run: Option<Item> = if enter {
            rows.get(sel).filter(|r| r.enabled).map(|r| r.item)
        } else {
            None
        };
        let mut query = match &self.overlay {
            Some(Overlay::Palette(f)) => f.query.clone(),
            _ => String::new(),
        };
        let (placeholder, empty_text) = match scope {
            Scope::Commands => ("Type a command…", "No command matches".to_string()),
            Scope::Boxes => ("Find a box by its label…", "No box matches".to_string()),
            Scope::Find => {
                let (read, n) = self.text_progress();
                let none = if query.trim().is_empty() {
                    "Type a word to find it in the document".to_string()
                } else if read < n {
                    format!("Nothing yet \u{2014} still reading pages\u{2026} {read} of {n}")
                } else if read > 0 && self.doc.as_ref().is_some_and(|d| d.text.values().all(|t| t.is_empty())) {
                    "This document has no text to search (a scan?)".to_string()
                } else {
                    "Not found".to_string()
                };
                ("Search the document\u{2026}", none)
            }
            Scope::Page => {
                let n = self.doc.as_ref().map_or(0, |d| d.sizes.len());
                ("Page number…", format!("Type a page number from 1 to {n}"))
            }
        };
        let top = ctx.content_rect().top() + 80.0;
        let resp = egui::Modal::new(Id::unique("pdfr-palette"))
            .frame(panel_frame(ctx, &pal, &c, 10))
            .area(
                egui::Modal::default_area(Id::unique("pdfr-palette"))
                    .anchor(egui::Align2::CENTER_TOP, vec2(0.0, top)),
            )
            .show(ctx, |ui| {
                ui.set_width(WIDTH);
                ui.spacing_mut().item_spacing.y = 6.0;
                let te = ui.add(
                    TextEdit::singleline(&mut query)
                        .id(Id::unique(QUERY_ID))
                        .hint_text(placeholder)
                        .font(egui::FontId::proportional(17.0))
                        .frame(egui::Frame::NONE)
                        .desired_width(f32::INFINITY)
                        .margin(egui::Margin::symmetric(8, 6)),
                );
                if !te.has_focus() {
                    te.request_focus();
                }
                let sep = ui.allocate_exact_size(vec2(WIDTH, 1.0), Sense::hover()).0;
                ui.painter().rect_filled(sep, 0.0, c.border);
                if rows.is_empty() {
                    ui.add_space(4.0);
                    ui.label(RichText::new(&empty_text).color(c.weak));
                    ui.add_space(4.0);
                    return;
                }
                egui::ScrollArea::vertical()
                    .max_height(ROW_H * MAX_ROWS as f32 + 8.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        for (k, r) in rows.iter().enumerate() {
                            let (row, rresp) =
                                ui.allocate_exact_size(vec2(WIDTH, ROW_H), Sense::click());
                            let fill = if k == sel {
                                c.accent.gamma_multiply(0.25)
                            } else if rresp.hovered() {
                                c.hover
                            } else {
                                Color32::TRANSPARENT
                            };
                            ui.painter().rect_filled(row, 6.0, fill);
                            let text = if r.enabled {
                                c.text
                            } else {
                                c.weak.gamma_multiply(0.7)
                            };
                            let mut x = row.left() + 10.0;
                            if let Item::Cmd(cmd) = r.item {
                                let ir = Rect::from_center_size(
                                    pos2(x + 9.0, row.center().y),
                                    vec2(18.0, 18.0),
                                );
                                crate::icons::paint(ui.painter(), ir, cmd.spec().icon, text);
                                x += 28.0;
                            }
                            ui.painter().text(
                                pos2(x, row.center().y),
                                egui::Align2::LEFT_CENTER,
                                &r.title,
                                egui::FontId::proportional(14.0),
                                text,
                            );
                            ui.painter().text(
                                row.right_center() - vec2(10.0, 0.0),
                                egui::Align2::RIGHT_CENTER,
                                &r.detail,
                                egui::FontId::proportional(12.5),
                                c.weak,
                            );
                            if k == sel && sel != prev_sel {
                                rresp.scroll_to_me(None);
                            }
                            if r.enabled
                                && rresp.on_hover_cursor(CursorIcon::PointingHand).clicked()
                            {
                                run = Some(r.item);
                            }
                        }
                    });
            });
        let backdrop = resp.backdrop_response.clicked();
        // Browsing search hits shows each one on the page.
        let mut preview = None;
        if let Some(Overlay::Palette(f)) = &mut self.overlay {
            if f.query != query {
                f.query = query;
                sel = 0;
                ctx.request_repaint();
            } else if let Some(Row {
                item: Item::Hit(i), ..
            }) = rows.get(sel)
                && f.previewed != Some(*i)
            {
                f.previewed = Some(*i);
                preview = Some(*i);
            }
            f.sel = sel;
        }
        if let Some(i) = preview {
            self.show_hit(i);
        }
        if close || backdrop || run.is_some() {
            self.overlay = None;
            ctx.memory_mut(|m| m.surrender_focus(Id::unique(QUERY_ID)));
            if scope == Scope::Find
                && !matches!(run, Some(Item::Hit(_)))
                && let Some(s) = &mut self.search
            {
                s.shown = false;
            }
        }
        match run {
            Some(Item::Cmd(c)) => self.run(c),
            Some(Item::Box(id)) => self.focus_field(id, true, ctx),
            Some(Item::Page(p)) => self.scroll_to_page(p),
            Some(Item::Hit(i)) => {
                self.show_hit(i);
                let n = self.search.as_ref().map_or(0, |s| s.hits.len());
                self.status = format!("Match {} of {n}", i + 1);
            }
            Some(Item::Info) | None => {}
        }
    }

    fn cheatsheet_ui(&mut self, ctx: &egui::Context) {
        let close = ctx.input_mut(|i| {
            i.consume_key(Modifiers::NONE, Key::Escape) | i.consume_key(Modifiers::NONE, Key::F1)
        });
        let pal = self.palette.clone();
        let c = Colors::new(&pal);
        let commands: Vec<(String, String)> = Command::ALL
            .iter()
            .filter(|c| !c.spec().shortcuts.is_empty())
            .map(|c| {
                let s = c.spec();
                (s.label.to_string(), shortcuts_text(ctx, s.shortcuts))
            })
            .collect();
        let half = commands.len().div_ceil(2);
        let max_h = (ctx.content_rect().height() - 160.0).max(200.0);
        let resp = egui::Modal::new(Id::unique("pdfr-cheatsheet"))
            .frame(panel_frame(ctx, &pal, &c, 22))
            .show(ctx, |ui| {
                ui.set_width(780.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Keyboard shortcuts")
                            .size(20.0)
                            .strong()
                            .color(c.text),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new("Esc to close").color(c.weak));
                    });
                });
                ui.add_space(8.0);
                egui::ScrollArea::vertical()
                    .max_height(max_h)
                    .min_scrolled_height(max_h)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.columns(2, |cols| {
                            keys_card(&mut cols[0], &c, "Commands", &commands[..half]);
                            keys_card(&mut cols[1], &c, "", &commands[half..]);
                            cols[0].add_space(10.0);
                            keys_card(
                                &mut cols[0],
                                &c,
                                "In a box",
                                &owned(super::editing::BOX_KEYS),
                            );
                            cols[1].add_space(10.0);
                            keys_card(&mut cols[1], &c, "On the page", &owned(PAGE_KEYS));
                        });
                    });
            });
        if close || resp.backdrop_response.clicked() {
            self.overlay = None;
        }
    }
}

/// Keys that work on the page when no box is focused (handled in canvas).
const PAGE_KEYS: &[(&str, &str)] = &[
    ("Scroll a screen", "Space / Shift+Space, PgDn / PgUp"),
    ("Scroll a little", "↑ / ↓"),
    ("Top / bottom", "Home / End"),
    ("First / last box", "Tab / Shift+Tab"),
    ("Put down a tool", "Esc"),
    ("Zoom", "Ctrl+scroll"),
];

fn owned(v: &[(&str, &str)]) -> Vec<(String, String)> {
    v.iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

fn shortcuts_text(ctx: &egui::Context, s: &[KeyboardShortcut]) -> String {
    s.iter()
        .map(|k| super::commands::key_text(ctx, k))
        .collect::<Vec<_>>()
        .join(" / ")
}

fn keys_card(ui: &mut egui::Ui, c: &Colors, heading: &str, rows: &[(String, String)]) {
    let h = (!heading.is_empty()).then_some(heading);
    crate::assist_ui::widgets::card(ui, c, h.or(Some(" ")), |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        for (what, keys) in rows {
            ui.horizontal(|ui| {
                ui.label(RichText::new(what).color(c.text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(keys).monospace().size(12.5).color(c.weak));
                });
            });
        }
    });
}

/// A box's name in lists: its label, else where it is.
fn box_name(e: &Entry, n: usize) -> String {
    match e.f.label.as_deref().map(str::trim) {
        Some(l) if !l.is_empty() => l.to_string(),
        _ => format!("Box {} on page {}", n + 1, e.f.page + 1),
    }
}

/// Case-insensitive match score: a substring (earlier and at a word start is
/// better), else the letters in order. None if it doesn't match at all.
pub(super) fn fuzzy(query: &str, text: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let q = query.to_lowercase();
    let t = text.to_lowercase();
    if let Some(pos) = t.find(&q) {
        let word_start = pos == 0 || !t[..pos].ends_with(char::is_alphanumeric);
        return Some(500 - pos.min(400) as i32 + if word_start { 100 } else { 0 });
    }
    let mut it = t.chars();
    q.chars()
        .filter(|c| !c.is_whitespace())
        .all(|qc| it.any(|tc| tc == qc))
        .then_some(10)
}

#[cfg(test)]
mod tests {
    use super::fuzzy;

    #[test]
    fn fuzzy_ranks_substrings_and_word_starts() {
        assert!(fuzzy("fit", "Fit width").unwrap() > fuzzy("fw", "Fit width").unwrap());
        assert!(fuzzy("width", "Fit width").unwrap() > fuzzy("idth", "Fit width").unwrap());
        assert_eq!(fuzzy("zz", "Fit width"), None);
        assert_eq!(fuzzy("", "anything"), Some(0));
        assert!(fuzzy("nxt empty", "Next empty box").is_some());
    }
}
