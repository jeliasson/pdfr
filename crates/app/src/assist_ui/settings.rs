//! The settings panel: a sidebar of sections on the left, the chosen
//! section's options grouped in cards on the right.

use super::AssistState;
use super::widgets::*;
use crate::icons::{self, Icon};
use crate::theme::{Palette, mix};
use eframe::egui::{
    self, Align, Color32, CornerRadius, Frame, Layout, Margin, RichText, Sense, Stroke, Ui, vec2,
};
use pdfr_assist::memory::Remember;
use std::collections::HashSet;

const WIDTH: f32 = 720.0;
const HEIGHT: f32 = 520.0;
const SIDEBAR: f32 = 190.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Section {
    #[default]
    General,
    License,
    /// AI agents controlling pdfr (`pdfr mcp`).
    Agents,
    Autocomplete,
    Ai,
    People,
    Values,
    Storage,
}

impl Section {
    const ALL: [Section; 8] = [
        Section::General,
        Section::License,
        Section::Agents,
        Section::Autocomplete,
        Section::People,
        Section::Values,
        Section::Ai,
        Section::Storage,
    ];

    /// The sidebar heading a section sits under: everything but General is
    /// part of autocomplete.
    fn group(self) -> Option<&'static str> {
        match self {
            Section::General | Section::License | Section::Agents => None,
            _ => Some("Autocomplete"),
        }
    }

    /// Also found by these names (`--settings autocomplete`).
    fn matches(self, name: &str) -> bool {
        // "ai" is the AI helper's section, not this one.
        if self == Section::Agents {
            return name == "agents" || name == "ai agents";
        }
        self.title().to_lowercase().contains(name)
            || (self == Section::Autocomplete && name == "autocomplete")
    }

    fn title(self) -> &'static str {
        match self {
            Section::General => "General",
            Section::License => "License",
            Section::Agents => "AI agents",
            Section::Autocomplete => "Behaviour",
            Section::Ai => "AI",
            Section::People => "People",
            Section::Values => "Memory",
            Section::Storage => "Storage",
        }
    }

    fn blurb(self) -> &'static str {
        match self {
            Section::General => "How pdfr fits into your computer.",
            Section::License => "Your licence key, and who this copy is registered to.",
            Section::Agents => "Let Claude and other AI agents fill in forms in pdfr for you.",
            Section::Autocomplete => "How pdfr learns from the forms you fill.",
            Section::Ai => "Optional help from an AI tool already installed on this computer.",
            Section::People => {
                "Each person's details are kept together, so a form about your child gets your child's details."
            }
            Section::Values => "Every value pdfr can suggest, grouped by what it is.",
            Section::Storage => "Where your data is kept, and how to remove it.",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Section::General => Icon::Settings,
            Section::License => Icon::Key,
            Section::Agents => Icon::Pointer,
            Section::Autocomplete => Icon::FillAll,
            Section::Ai => Icon::Sparkle,
            Section::People => Icon::Person,
            Section::Values => Icon::Brain,
            Section::Storage => Icon::Storage,
        }
    }
}

/// Panel state that lives only while the app runs.
#[derive(Default)]
pub struct SettingsUi {
    pub section: Section,
    people_open: HashSet<u64>,
    filter: String,
    confirm_clear: bool,
    /// Which "Copy" button of the AI agents section was just pressed.
    copied: Option<(usize, std::time::Instant)>,
}

/// What the user asked for this frame, applied after drawing.
#[derive(Default)]
struct Actions {
    changed: bool,
    forget_value: Option<(String, String)>,
    forget_person: Option<u64>,
    forget_signature: Option<u64>,
    clear: bool,
    close: bool,
}

impl AssistState {
    /// Opens the panel at a section named like "ai" or "people".
    pub fn open_settings(&mut self, name: &str) {
        let name = name.to_lowercase();
        if let Some(s) = Section::ALL.into_iter().find(|s| s.matches(&name)) {
            self.settings_ui.section = s;
        }
        self.settings_open = true;
    }

    pub fn settings_window(
        &mut self,
        ctx: &egui::Context,
        pal: &Palette,
        license: &mut crate::license_ui::LicenseUi,
    ) {
        if !self.settings_open {
            return;
        }
        let c = Colors::new(pal);
        let mut act = Actions::default();
        let frame = Frame::window(&ctx.global_style())
            .fill(pal.window)
            .stroke(Stroke::new(1.0, c.border))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(Margin::ZERO);
        egui::Window::new("Settings")
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .fixed_size(vec2(WIDTH, HEIGHT))
            .frame(frame)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    self.sidebar(ui, &c);
                    // Divider.
                    let (r, _) = ui.allocate_exact_size(vec2(1.0, HEIGHT), Sense::hover());
                    ui.painter().rect_filled(r, 0.0, c.border);
                    Frame::new()
                        .inner_margin(Margin {
                            left: 28,
                            right: 20,
                            top: 18,
                            bottom: 18,
                        })
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                ui.set_width(WIDTH - SIDEBAR - 1.0 - 48.0);
                                ui.set_height(HEIGHT - 36.0);
                                self.content(ui, &c, &mut act, license);
                            });
                        });
                });
            });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && ctx.memory(|m| m.focused().is_none())
        {
            act.close = true;
        }
        self.apply(act);
    }

    fn sidebar(&mut self, ui: &mut Ui, c: &Colors) {
        let r = ui.available_rect_before_wrap();
        let r = egui::Rect::from_min_size(r.min, vec2(SIDEBAR, HEIGHT));
        // Fill under the sidebar, rounded only on the window's left corners.
        ui.painter().rect_filled(
            r,
            CornerRadius {
                nw: 12,
                sw: 12,
                ne: 0,
                se: 0,
            },
            c.sidebar,
        );
        Frame::new()
            .inner_margin(Margin {
                left: 12,
                right: 12,
                top: 18,
                bottom: 12,
            })
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.set_width(SIDEBAR - 24.0);
                    ui.set_height(HEIGHT - 30.0);
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        ui.label(RichText::new("Settings").size(17.0).strong().color(c.text));
                    });
                    ui.add_space(14.0);
                    ui.spacing_mut().item_spacing.y = 2.0;
                    let mut last_group = None;
                    for s in Section::ALL {
                        if s.group() != last_group {
                            last_group = s.group();
                            if let Some(g) = last_group {
                                ui.add_space(12.0);
                                ui.horizontal(|ui| {
                                    ui.add_space(10.0);
                                    ui.label(
                                        RichText::new(g.to_uppercase())
                                            .size(11.0)
                                            .strong()
                                            .color(c.weak),
                                    );
                                });
                                ui.add_space(2.0);
                            }
                        }
                        let selected = self.settings_ui.section == s;
                        let (rect, resp) = ui
                            .allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
                        let fill = if selected {
                            c.accent.gamma_multiply(0.22)
                        } else if resp.hovered() {
                            c.hover
                        } else {
                            Color32::TRANSPARENT
                        };
                        ui.painter().rect_filled(rect, 7.0, fill);
                        let ic = egui::Rect::from_center_size(
                            egui::pos2(rect.left() + 20.0, rect.center().y),
                            vec2(17.0, 17.0),
                        );
                        icons::paint(
                            ui.painter(),
                            ic,
                            s.icon(),
                            if selected { c.text } else { c.weak },
                        );
                        ui.painter().text(
                            egui::pos2(rect.left() + 38.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            s.title(),
                            egui::FontId::proportional(14.0),
                            if selected {
                                c.text
                            } else {
                                mix(c.weak, c.text, 0.4)
                            },
                        );
                        if resp
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                            && !selected
                        {
                            self.settings_ui.section = s;
                            self.settings_ui.confirm_clear = false;
                        }
                    }
                });
            });
    }

    fn content(
        &mut self,
        ui: &mut Ui,
        c: &Colors,
        act: &mut Actions,
        license: &mut crate::license_ui::LicenseUi,
    ) {
        let s = self.settings_ui.section;
        ui.horizontal(|ui| {
            ui.label(RichText::new(s.title()).size(21.0).strong().color(c.text));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icon_button(ui, c, Icon::Close, "Close (Esc)").clicked() {
                    act.close = true;
                }
            });
        });
        ui.label(RichText::new(s.blurb()).color(c.weak));
        ui.add_space(10.0);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .id_salt(("pdfr-settings", s as u8))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 12.0;
                match s {
                    Section::General => act.changed |= self.general_section(ui, c),
                    Section::License => license.settings_section(ui, c),
                    Section::Agents => self.agents(ui, c, act),
                    Section::Autocomplete => self.autocomplete(ui, c, act),
                    Section::Ai => self.ai(ui, c, act),
                    Section::People => self.people(ui, c, act),
                    Section::Values => self.values(ui, c, act),
                    Section::Storage => self.storage(ui, c, act),
                }
                ui.add_space(4.0);
            });
    }

    fn autocomplete(&mut self, ui: &mut Ui, c: &Colors, act: &mut Actions) {
        card(ui, c, Some("When you save a filled form"), |ui| {
            let r = &mut self.a.store.settings.remember;
            let options = [
                (
                    Remember::Ask,
                    "Ask what to remember",
                    "Pick, person by person, what pdfr may offer again.",
                ),
                (
                    Remember::Always,
                    "Remember everything",
                    "Keep what you entered without asking.",
                ),
                (
                    Remember::Never,
                    "Remember nothing",
                    "Suggestions keep working from what is already saved.",
                ),
            ];
            for (i, (v, title, sub)) in options.into_iter().enumerate() {
                if i > 0 {
                    row_divider(ui, c);
                }
                if choice_row(ui, c, *r == v, title, sub) && *r != v {
                    *r = v;
                    act.changed = true;
                }
            }
        });
        note(
            ui,
            c,
            "Passwords, card numbers and fields you mark \u{201c}don't remember\u{201d} are never stored.",
        );
    }

    /// The AI agents section is showing. Agents' own clicks and keys are
    /// not let through meanwhile: these switches are the user's.
    pub fn agents_section_open(&self) -> bool {
        self.settings_open && self.settings_ui.section == Section::Agents
    }

    /// AI agents: whether they may control pdfr, what they must ask the
    /// user first, and how to connect one.
    fn agents(&mut self, ui: &mut Ui, c: &Colors, act: &mut Actions) {
        let mut st = self.a.store.settings.clone();
        card(ui, c, None, |ui| {
            setting_row(
                ui,
                c,
                "Allow AI agents",
                "An agent you connect can open PDFs in pdfr, read them and fill them in, in a window you can watch.",
                |ui| {
                    toggle(ui, c, &mut st.agents_enabled);
                },
            );
        });
        // How to connect: the command an agent runs to reach pdfr.
        let flatpak = std::path::Path::new("/.flatpak-info").exists();
        let program = if flatpak {
            "flatpak run --command=pdfr io.github.kjellberg.pdfr".to_string()
        } else if pdfr_assist::ai::which("pdfr").is_some() {
            "pdfr".to_string()
        } else {
            // Not on the PATH (run from an archive or an app bundle).
            std::env::current_exe()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "pdfr".into())
        };
        let (cmd, args): (&str, Vec<&str>) = match program.split_once(' ') {
            Some((cmd, rest)) => (cmd, rest.split(' ').chain(["mcp"]).collect()),
            None => (program.as_str(), vec!["mcp"]),
        };
        let json = serde_json::json!({"mcpServers": {"pdfr": {"command": cmd, "args": args}}});
        let rows = [
            (
                "Claude Code",
                "Run this once in a terminal.",
                format!("claude mcp add pdfr -- {program} mcp"),
            ),
            (
                "Claude Desktop, Cursor and others",
                "Add this to the agent's MCP settings.",
                json.to_string(),
            ),
        ];
        card(ui, c, Some("Connect your agent"), |ui| {
            for (i, (title, sub, text)) in rows.iter().enumerate() {
                if i > 0 {
                    row_divider(ui, c);
                }
                let copied = self
                    .settings_ui
                    .copied
                    .is_some_and(|(k, at)| k == i && at.elapsed().as_secs_f32() < 2.0);
                setting_row(ui, c, title, sub, |ui| {
                    let label = if copied { "Copied" } else { "Copy" };
                    if secondary_button(ui, c, label).clicked() {
                        ui.ctx().copy_text(text.clone());
                        self.settings_ui.copied = Some((i, std::time::Instant::now()));
                    }
                });
                ui.add(
                    egui::Label::new(RichText::new(text).monospace().size(12.0).color(c.weak))
                        .wrap(),
                );
            }
        });
        card(ui, c, Some("Ask me first"), |ui| {
            ui.add_enabled_ui(st.agents_enabled, |ui| {
                setting_row(
                    ui,
                    c,
                    "Signing digitally",
                    "Before an agent signs with your certificate or ID card.",
                    |ui| {
                        toggle(ui, c, &mut st.agent_ask_sign);
                    },
                );
                row_divider(ui, c);
                setting_row(
                    ui,
                    c,
                    "Saving over a file",
                    "Before an agent replaces a file that is already there. New files are saved without asking.",
                    |ui| {
                        toggle(ui, c, &mut st.agent_ask_overwrite);
                    },
                );
                row_divider(ui, c);
                setting_row(
                    ui,
                    c,
                    "Printing",
                    "Before an agent opens the print dialog.",
                    |ui| {
                        toggle(ui, c, &mut st.agent_ask_print);
                    },
                );
            });
        });
        card(ui, c, Some("Your details"), |ui| {
            ui.add_enabled_ui(st.agents_enabled, |ui| {
                setting_row(
                    ui,
                    c,
                    "Use remembered details",
                    "Agents may be told what pdfr remembers you usually write in a box. Off: they only type what they already know.",
                    |ui| {
                        toggle(ui, c, &mut st.agent_use_memory);
                    },
                );
            });
        });
        if st != self.a.store.settings {
            self.a.store.settings = st;
            act.changed = true;
        }
        note(
            ui,
            c,
            "Then ask your agent to fill in a form with pdfr. It works in a window you can see: click or type yourself and it pauses until you press Resume.",
        );
    }

    fn ai(&mut self, ui: &mut Ui, c: &Colors, act: &mut Actions) {
        let backend = self.a.backend_name();
        card(ui, c, None, |ui| {
            let mut on = self.a.store.settings.ai_enabled;
            setting_row(
                ui,
                c,
                "Use AI",
                "For labels pdfr doesn't recognise and for forms about several people.",
                |ui| {
                    if toggle(ui, c, &mut on).changed() {
                        self.a.store.settings.ai_enabled = on;
                        if on && self.a.store.settings.ai_consent == Some(false) {
                            self.a.store.settings.ai_consent = Some(true);
                        }
                        act.changed = true;
                    }
                },
            );
            row_divider(ui, c);
            let (tool, sub) = match &backend {
                // "Claude Code (claude, model haiku)" -> "Claude Code"; the model has its own row.
                Some(n) => (
                    n.split(" (").next().unwrap_or(n).to_string(),
                    "Found on this computer.",
                ),
                None => (
                    "Not found".to_string(),
                    if std::path::Path::new("/.flatpak-info").exists() {
                        "The Flatpak version can't use programs on your computer. Install pdfr from your distribution's packages to use AI."
                    } else {
                        "Install Claude Code to use AI. Everything else works without it."
                    },
                ),
            };
            setting_row(ui, c, "Tool", sub, |ui| {
                ui.label(RichText::new(tool).color(if backend.is_some() {
                    c.text
                } else {
                    c.weak
                }));
            });
            row_divider(ui, c);
            setting_row(
                ui,
                c,
                "Model",
                "Labels are short; the fastest model is usually enough.",
                |ui| {
                    if let Some(m) =
                        model_picker(ui, c, &self.a.store.settings.model, backend.is_some())
                    {
                        self.a.store.settings.model = m;
                        self.a.redetect_backend();
                        act.changed = true;
                    }
                },
            );
        });
        let u = &self.a.store.usage;
        card(ui, c, Some("Usage"), |ui| {
            ui.columns(3, |cols| {
                stat(&mut cols[0], c, "Calls", u.calls);
                stat(&mut cols[1], c, "Tokens sent", u.input_tokens);
                stat(&mut cols[2], c, "Tokens received", u.output_tokens);
            });
        });
        note(
            ui,
            c,
            "Only field labels and section headings are sent, never what you type, the page text or the PDF.",
        );
    }

    fn people(&mut self, ui: &mut Ui, c: &Colors, act: &mut Actions) {
        self.saved_signatures(ui, c, act);
        if self.a.store.profiles.is_empty() {
            empty(
                ui,
                c,
                Icon::Person,
                "No one yet",
                "People appear here when you save a form and choose to remember it.",
            );
            return;
        }
        let open = &mut self.settings_ui.people_open;
        card(ui, c, None, |ui| {
            for (i, p) in self.a.store.profiles.iter().enumerate() {
                if i > 0 {
                    row_divider(ui, c);
                }
                let title = p.title();
                let mut meta = Vec::new();
                if !p.roles.is_empty() {
                    meta.push(
                        p.roles
                            .iter()
                            .map(|r| capitalise(r))
                            .collect::<Vec<_>>()
                            .join(", "),
                    );
                }
                meta.push(plural(p.values.len(), "detail"));
                if p.count > 0 {
                    meta.push(format!("used {}", plural(p.count as usize, "time")));
                }
                let expanded = open.contains(&p.id);
                let mut toggle_open = false;
                ui.horizontal(|ui| {
                    let left = ui
                        .horizontal(|ui| {
                            avatar(ui, c, &title);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 1.0;
                                ui.label(RichText::new(&title).strong().color(c.text));
                                ui.label(
                                    RichText::new(meta.join(" \u{b7} "))
                                        .size(12.0)
                                        .color(c.weak),
                                );
                            });
                        })
                        .response;
                    // The name area toggles the details; the buttons sit outside it.
                    let hit = ui.interact(
                        left.rect,
                        ui.scope_id().with(("person", p.id)),
                        Sense::click(),
                    );
                    toggle_open |= hit
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked();
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if icon_button(ui, c, Icon::Trash, "Forget this person").clicked() {
                            act.forget_person = Some(p.id);
                        }
                        let more = ui.add(
                            egui::Label::new(
                                RichText::new(if expanded {
                                    "Hide details"
                                } else {
                                    "Show details"
                                })
                                .size(12.0)
                                .color(c.accent),
                            )
                            .sense(Sense::click()),
                        );
                        toggle_open |= more
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked();
                    });
                });
                if toggle_open {
                    if expanded {
                        open.remove(&p.id);
                    } else {
                        open.insert(p.id);
                    }
                }
                if expanded {
                    ui.add_space(2.0);
                    // Indented under the name (egui forbids spacing inside a grid).
                    ui.horizontal(|ui| {
                        ui.add_space(44.0);
                        egui::Grid::new(("pdfr-person", p.id))
                            .num_columns(2)
                            .spacing(vec2(16.0, 6.0))
                            .show(ui, |ui| {
                                for (k, v) in &p.values {
                                    ui.label(RichText::new(humanise(k)).color(c.weak));
                                    ui.label(RichText::new(v).color(c.text));
                                    ui.end_row();
                                }
                            });
                    });
                }
            }
        });
    }

    /// Signatures and initials kept for next time, each with its person.
    fn saved_signatures(&mut self, ui: &mut Ui, c: &Colors, act: &mut Actions) {
        if self.a.store.signatures.is_empty() {
            return;
        }
        let names: std::collections::HashMap<u64, String> = self
            .a
            .store
            .profiles
            .iter()
            .map(|p| (p.id, p.title()))
            .collect();
        card(ui, c, Some("Signatures"), |ui| {
            for (i, s) in self.a.store.signatures.iter().enumerate() {
                if i > 0 {
                    row_divider(ui, c);
                }
                let Some(ink) = pdfr_core::signature::Ink::decode(&s.ink) else {
                    continue;
                };
                let ink = std::sync::Arc::new(ink);
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(120.0, 40.0), Sense::hover());
                    ui.painter().rect_filled(r, 4.0, Color32::WHITE);
                    crate::ink::paint(ui.painter(), &ink, r.shrink(4.0), Color32::WHITE, false);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        let what = if s.initials { "Initials" } else { "Signature" };
                        ui.label(RichText::new(what).strong().color(c.text));
                        let who = s
                            .person
                            .and_then(|p| names.get(&p).cloned())
                            .unwrap_or_else(|| "Nobody in particular".into());
                        ui.label(RichText::new(who).size(12.0).color(c.weak));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if icon_button(ui, c, Icon::Trash, "Delete").clicked() {
                            act.forget_signature = Some(s.id);
                        }
                    });
                });
            }
        });
    }

    fn values(&mut self, ui: &mut Ui, c: &Colors, act: &mut Actions) {
        if self.a.store.values.is_empty() {
            empty(
                ui,
                c,
                Icon::List,
                "Nothing remembered yet",
                "Values appear here when you save a filled form.",
            );
            return;
        }
        search_box(ui, c, &mut self.settings_ui.filter);
        let q = self.settings_ui.filter.trim().to_lowercase();
        let mut shown = 0;
        for (k, list) in &self.a.store.values {
            let key_hit = q.is_empty()
                || k.to_lowercase().contains(&q)
                || humanise(k).to_lowercase().contains(&q);
            let items: Vec<_> = list
                .iter()
                .filter(|e| key_hit || e.value.to_lowercase().contains(&q))
                .collect();
            if items.is_empty() {
                continue;
            }
            shown += 1;
            card(ui, c, Some(&humanise(k)), |ui| {
                for (i, e) in items.iter().enumerate() {
                    if i > 0 {
                        row_divider(ui, c);
                    }
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&e.value).color(c.text));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if icon_button(ui, c, Icon::Trash, "Delete").clicked() {
                                act.forget_value = Some((k.clone(), e.value.clone()));
                            }
                            if e.count > 1 {
                                ui.label(
                                    RichText::new(format!("used {}\u{d7}", e.count))
                                        .size(12.0)
                                        .color(c.weak),
                                );
                            }
                        });
                    });
                }
            });
        }
        if shown == 0 {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| ui.label(RichText::new("No matches").color(c.weak)));
        }
    }

    fn storage(&mut self, ui: &mut Ui, c: &Colors, act: &mut Actions) {
        let st = &self.a.store;
        let values: usize = st.values.values().map(|l| l.len()).sum();
        card(ui, c, Some("On this computer"), |ui| {
            ui.columns(3, |cols| {
                stat(&mut cols[0], c, "People", st.profiles.len() as u64);
                stat(&mut cols[1], c, "Values", values as u64);
                stat(&mut cols[2], c, "Forms", st.forms.len() as u64);
            });
            if let Some(p) = self.a.path() {
                row_divider(ui, c);
                ui.label(RichText::new("Location").color(c.weak).size(12.0));
                ui.add(
                    egui::Label::new(
                        RichText::new(p.display().to_string())
                            .monospace()
                            .size(12.0)
                            .color(c.text),
                    )
                    .wrap(),
                );
                ui.label(
                    RichText::new("One file, readable only by you.")
                        .size(12.0)
                        .color(c.weak),
                );
            }
        });
        card(ui, c, Some("Danger zone"), |ui| {
            let confirming = self.settings_ui.confirm_clear;
            setting_row(
                ui,
                c,
                "Clear everything",
                if confirming {
                    "This cannot be undone. Click again to delete."
                } else {
                    "Deletes all remembered people, signatures, values and forms. Settings are kept."
                },
                |ui| {
                    let label = if confirming {
                        "Delete all"
                    } else {
                        "Clear\u{2026}"
                    };
                    if danger_button(ui, label, confirming).clicked() {
                        if confirming {
                            act.clear = true;
                            self.settings_ui.confirm_clear = false;
                        } else {
                            self.settings_ui.confirm_clear = true;
                        }
                    }
                },
            );
        });
    }

    fn apply(&mut self, act: Actions) {
        let mut changed = act.changed;
        if let Some((k, v)) = act.forget_value {
            self.a.store.forget_value(&k, &v);
            changed = true;
        }
        if let Some(id) = act.forget_person {
            self.a.store.forget_profile(id);
            self.settings_ui.people_open.remove(&id);
            changed = true;
        }
        if let Some(id) = act.forget_signature {
            self.a.store.forget_signature(id);
            changed = true;
        }
        if act.clear {
            self.a.store.clear_values();
            changed = true;
        }
        if changed {
            self.a.save();
            self.stale = true;
        }
        if act.close {
            self.settings_open = false;
            self.refresh_system();
            self.settings_ui.confirm_clear = false;
        }
    }
}

/// A drop-down of the AI tool's models. `current` is the stored setting
/// ("" = default); a custom value from an older version stays listed.
/// Returns the new setting when the user picks another model.
fn model_picker(ui: &mut Ui, c: &Colors, current: &str, enabled: bool) -> Option<String> {
    use pdfr_assist::ai::CLAUDE_MODELS;
    let selected = if current.is_empty() {
        CLAUDE_MODELS[0].0
    } else {
        current
    };
    let name_of = |id: &str| {
        CLAUDE_MODELS
            .iter()
            .find(|m| m.0 == id)
            .map_or(id.to_string(), |m| m.1.to_string())
    };
    let mut picked: Option<String> = None;
    ui.add_enabled_ui(enabled, |ui| {
        egui::ComboBox::from_id_salt("pdfr-model")
            .selected_text(RichText::new(name_of(selected)).color(c.text))
            .width(170.0)
            .show_ui(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                let custom = !CLAUDE_MODELS.iter().any(|m| m.0 == selected);
                let mut rows: Vec<(String, String, String)> = CLAUDE_MODELS
                    .iter()
                    .map(|(id, name, note)| (id.to_string(), name.to_string(), note.to_string()))
                    .collect();
                if custom {
                    rows.push((
                        selected.to_string(),
                        selected.to_string(),
                        "Set earlier".into(),
                    ));
                }
                for (i, (id, name, note)) in rows.iter().enumerate() {
                    let note = if i == 0 {
                        format!("{note} \u{b7} default")
                    } else {
                        note.clone()
                    };
                    let mut job = egui::text::LayoutJob::default();
                    let fmt = |size: f32, color: Color32| egui::TextFormat {
                        font_id: egui::FontId::proportional(size),
                        color,
                        ..Default::default()
                    };
                    job.append(name, 0.0, fmt(14.0, c.text));
                    job.append(&format!("\n{note}"), 0.0, fmt(12.0, c.weak));
                    let r = ui.add(
                        egui::Button::selectable(id == selected, job).min_size(vec2(200.0, 0.0)),
                    );
                    if r.clicked() && id != selected {
                        // The default model is stored as "" so it follows future defaults.
                        picked = Some(if i == 0 { String::new() } else { id.clone() });
                    }
                }
            });
    });
    picked
}

fn stat(ui: &mut Ui, c: &Colors, label: &str, n: u64) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.label(
            RichText::new(group_digits(n))
                .size(20.0)
                .strong()
                .color(c.text),
        );
        ui.label(RichText::new(label).size(12.0).color(c.weak));
    });
}

fn empty(ui: &mut Ui, c: &Colors, icon: Icon, title: &str, sub: &str) {
    ui.add_space(60.0);
    ui.vertical_centered(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
        icons::paint(ui.painter(), r, icon, c.weak);
        ui.add_space(6.0);
        ui.label(RichText::new(title).size(15.0).color(c.text));
        ui.label(RichText::new(sub).color(c.weak));
    });
}

fn search_box(ui: &mut Ui, c: &Colors, text: &mut String) {
    Frame::new()
        .fill(c.card)
        .stroke(Stroke::new(1.0, c.border))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                icons::paint(ui.painter(), r, Icon::Search, c.weak);
                ui.add(
                    egui::TextEdit::singleline(text)
                        .hint_text("Search")
                        .frame(egui::Frame::NONE)
                        .desired_width(f32::INFINITY),
                );
            });
        });
}

/// "given_name" -> "Given name".
fn humanise(key: &str) -> String {
    capitalise(&key.replace('_', " "))
}

fn capitalise(s: &str) -> String {
    let mut ch = s.chars();
    match ch.next() {
        Some(f) => f.to_uppercase().chain(ch).collect(),
        None => String::new(),
    }
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// 1234567 -> "1 234 567".
fn group_digits(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push('\u{2009}');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(humanise("date_of_birth"), "Date of birth");
        assert_eq!(group_digits(1234567), "1\u{2009}234\u{2009}567");
        assert_eq!(group_digits(12), "12");
        assert_eq!(plural(1, "detail"), "1 detail");
    }
}
