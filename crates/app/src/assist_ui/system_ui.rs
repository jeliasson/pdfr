//! pdfr and the computer: Settings → General (default PDF app, app menu,
//! terminal command, updates) and the small notices in the corner that ask
//! once about the default app and update checks, and announce new versions.

use super::AssistState;
use super::widgets::*;
use crate::system::{self, DefaultApp, InstallKind};
use crate::theme::Palette;
use crate::updates;
use eframe::egui::{self, Align, Align2, Layout, RichText, Ui, vec2};
use pdfr_assist::memory::now;
use std::time::Instant;

pub struct SystemUi {
    install: InstallKind,
    /// Looked up when needed (it reads a few small files).
    default_app: Option<DefaultApp>,
    on_path: Option<bool>,
    /// The last action's outcome: (worked, message).
    msg: Option<(bool, String)>,
    checker: updates::Checker,
    check_error: Option<String>,
    /// "Check now" was pressed: report the answer in Settings.
    manual_check: bool,
    started: Instant,
    /// A notice was answered or closed this session.
    notice_closed: bool,
}

impl Default for SystemUi {
    fn default() -> Self {
        SystemUi {
            install: system::install_kind(),
            default_app: None,
            on_path: None,
            msg: None,
            checker: Default::default(),
            check_error: None,
            manual_check: false,
            started: Instant::now(),
            notice_closed: false,
        }
    }
}

/// Which corner notice to show.
enum Notice {
    /// Ask once: open PDFs with pdfr? (`register` first for archive installs.)
    DefaultApp { register: bool },
    /// Ask once (direct installs): look for new versions?
    UpdateOptIn,
    /// A newer version is out.
    NewVersion(String),
}

impl AssistState {
    fn default_app(&mut self) -> DefaultApp {
        self.sys
            .default_app
            .get_or_insert_with(system::default_pdf_app)
            .clone()
    }

    /// Forgets looked-up state so it is read again next time.
    pub(super) fn refresh_system(&mut self) {
        self.sys.default_app = None;
        self.sys.on_path = None;
    }

    fn make_default(&mut self, register: bool) {
        let mut res = Ok(String::new());
        if register {
            res = system::register();
        }
        if res.is_ok() {
            res = system::make_default();
        }
        self.sys.msg = Some(match res {
            Ok(m) => (true, m),
            Err(e) => (false, e),
        });
        self.refresh_system();
    }

    /// The General section of Settings. Returns true if a setting changed.
    pub(super) fn general_section(&mut self, ui: &mut Ui, c: &Colors) -> bool {
        let mut changed = false;
        let install = self.sys.install;
        let default = self.default_app();
        let direct = install == InstallKind::Direct;

        card(ui, c, Some("Opening PDFs"), |ui| {
            let sub = match &default {
                DefaultApp::Pdfr => "PDF files you open from your files or browser open in pdfr.".to_string(),
                DefaultApp::Other(name) => format!("PDF files open in {name} now."),
                DefaultApp::Unknown => "Choose the app that opens PDF files.".to_string(),
            };
            let mut clicked = false;
            setting_row(ui, c, "Default PDF app", &sub, |ui| {
                if default == DefaultApp::Pdfr {
                    ui.label(RichText::new("\u{2713} pdfr").color(c.text));
                } else if system::can_make_default() {
                    clicked = primary_button(ui, c, "Use pdfr").clicked();
                }
            });
            if clicked {
                // An archive install joins the app menu first.
                self.make_default(direct && !system::in_app_menu());
            }
            if cfg!(target_os = "linux") && direct {
                row_divider(ui, c);
                let mut add = false;
                setting_row(
                    ui,
                    c,
                    "App menu",
                    "Add pdfr to your app launcher and to \u{201c}Open with\u{201d}, and the pdfr command to the terminal.",
                    |ui| {
                        if system::in_app_menu() {
                            ui.label(RichText::new("\u{2713} Added").color(c.text));
                        } else {
                            add = secondary_button(ui, c, "Add").clicked();
                        }
                    },
                );
                if add {
                    self.sys.msg = Some(match system::register() {
                        Ok(m) => (true, m),
                        Err(e) => (false, e),
                    });
                    self.refresh_system();
                }
            }
            row_divider(ui, c);
            let on_path = *self.sys.on_path.get_or_insert_with(system::command_on_path);
            setting_row(
                ui,
                c,
                "Terminal command",
                "Open a file from the terminal with  pdfr form.pdf",
                |ui| {
                    ui.label(RichText::new(if on_path { "Installed" } else { "Not installed" }).color(
                        if on_path { c.text } else { c.weak },
                    ));
                },
            );
            if let Some((ok, m)) = &self.sys.msg {
                row_divider(ui, c);
                ui.label(RichText::new(m).size(12.0).color(if *ok { c.text } else { DANGER }));
            }
        });

        card(ui, c, Some("Updates"), |ui| {
            if let (true, Some(cmd)) = (install.managed(), install.update_hint()) {
                setting_row(
                    ui,
                    c,
                    "Updates",
                    "pdfr is updated with the rest of your system.",
                    |ui| {
                        ui.label(RichText::new(cmd).monospace().size(12.0).color(c.text));
                    },
                );
            } else if install.managed() {
                setting_row(
                    ui,
                    c,
                    "Updates",
                    "pdfr is updated by your package manager.",
                    |_| {},
                );
            } else if install == InstallKind::Dev {
                setting_row(
                    ui,
                    c,
                    "Development build",
                    "Run from a Cargo build folder, so pdfr doesn\u{2019}t check for new versions.",
                    |_| {},
                );
            } else {
                let mut on = self.a.store.settings.update_check == Some(true);
                setting_row(
                    ui,
                    c,
                    "Tell me about new versions",
                    "Once a day pdfr asks GitHub for the newest version number. Nothing else is sent, and nothing is installed.",
                    |ui| {
                        if toggle(ui, c, &mut on).changed() {
                            self.a.store.settings.update_check = Some(on);
                            changed = true;
                        }
                    },
                );
                row_divider(ui, c);
                let latest = self.a.store.settings.latest_version.clone();
                let status = if self.sys.checker.running() {
                    "Checking\u{2026}".to_string()
                } else if let Some(e) = &self.sys.check_error {
                    format!("Could not check: {e}")
                } else if !latest.is_empty() && updates::is_newer(&latest, updates::current()) {
                    format!("pdfr {latest} is available.")
                } else if self.a.store.settings.update_checked_at > 0 {
                    "You have the newest version.".to_string()
                } else {
                    "Not checked yet.".to_string()
                };
                let mut check = false;
                let mut download = false;
                setting_row(ui, c, &format!("Version {}", updates::current()), &status, |ui| {
                    if !latest.is_empty() && updates::is_newer(&latest, updates::current()) {
                        download = primary_button(ui, c, "Download").clicked();
                    } else {
                        check = ui
                            .add_enabled_ui(!self.sys.checker.running(), |ui| {
                                secondary_button(ui, c, "Check now")
                            })
                            .inner
                            .clicked();
                    }
                });
                if check {
                    self.sys.manual_check = true;
                    self.sys.check_error = None;
                    self.sys.checker.start(ui.ctx());
                }
                if download {
                    self.open_releases();
                }
            }
        });
        if install.managed() || install == InstallKind::Dev {
            note(ui, c, &format!("pdfr {}.", updates::current()));
        }
        changed
    }

    fn open_releases(&mut self) {
        if let Err(e) = system::open_url(system::RELEASES_URL) {
            self.sys.msg = Some((false, format!("{e}. The downloads are at {}", system::RELEASES_URL)));
        }
    }

    /// Starts the daily check when it is due; takes in its answer.
    pub fn poll_updates(&mut self, ctx: &egui::Context, automated: bool) {
        if let Some(r) = self.sys.checker.poll() {
            let s = &mut self.a.store.settings;
            s.update_checked_at = now();
            match r {
                Ok(v) => {
                    s.latest_version = v;
                    self.sys.check_error = None;
                }
                // 404 before the first release: nothing newer.
                Err(e) if e.contains("404") => self.sys.check_error = None,
                Err(e) => self.sys.check_error = self.sys.manual_check.then_some(e),
            }
            self.sys.manual_check = false;
            self.a.save();
        }
        let s = &self.a.store.settings;
        if !automated
            && s.update_check == Some(true)
            && self.sys.install == InstallKind::Direct
            && now().saturating_sub(s.update_checked_at) > updates::DAY
            && !self.sys.checker.running()
        {
            self.sys.checker.start(ctx);
        }
    }

    fn notice(&mut self) -> Option<Notice> {
        if self.sys.notice_closed
            || self.sys.install == InstallKind::Dev
            || self.sys.started.elapsed().as_secs_f32() < 2.0
        {
            return None;
        }
        let s = &self.a.store.settings;
        let direct = self.sys.install == InstallKind::Direct;
        let latest = s.latest_version.clone();
        if !latest.is_empty()
            && updates::is_newer(&latest, updates::current())
            && latest != s.dismissed_version
        {
            return Some(Notice::NewVersion(latest));
        }
        if !s.default_app_asked && system::can_make_default() {
            let default = self.default_app();
            if default != DefaultApp::Pdfr {
                // An archive install that isn't in the app menu yet.
                let register = direct && !system::in_app_menu();
                return Some(Notice::DefaultApp { register });
            }
        }
        if direct && self.a.store.settings.update_check.is_none() {
            return Some(Notice::UpdateOptIn);
        }
        None
    }

    /// The corner notice, if one is due. Never during automated runs or
    /// while another dialog is open.
    pub fn notices(&mut self, ctx: &egui::Context, pal: &Palette, automated: bool) {
        // Headless UI tests never see them either.
        if automated || cfg!(test) || self.settings_open || self.modal_open() {
            return;
        }
        let Some(notice) = self.notice() else { return };
        let c = Colors::new(pal);
        let (title, body, yes, no) = match &notice {
            Notice::DefaultApp { register: true } => (
                "Add pdfr to your apps?".to_string(),
                "Find pdfr in your app launcher, and open PDF files with it.".to_string(),
                "Add and use pdfr",
                "Not now",
            ),
            Notice::DefaultApp { register: false } => (
                "Open PDF files with pdfr?".to_string(),
                "Make pdfr the app that opens PDFs from your files and browser.".to_string(),
                "Use pdfr",
                "Not now",
            ),
            Notice::UpdateOptIn => (
                "Tell you about new versions?".to_string(),
                "Once a day pdfr asks GitHub for the newest version number. Nothing else is sent.".to_string(),
                "Yes",
                "No thanks",
            ),
            Notice::NewVersion(v) => (
                format!("pdfr {v} is available"),
                match self.sys.install.update_hint() {
                    Some(cmd) => format!("Update with:  {cmd}"),
                    None => format!("You have {}.", updates::current()),
                },
                if self.sys.install.managed() { "Got it" } else { "Download" },
                "Later",
            ),
        };
        let mut answer: Option<bool> = None;
        egui::Area::new(egui::Id::unique("pdfr-notice"))
            .anchor(Align2::RIGHT_BOTTOM, vec2(-16.0, -40.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                panel_frame(ctx, pal, &c, 16).show(ui, |ui| {
                    ui.set_width(330.0);
                    ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
                    ui.label(RichText::new(&title).size(15.0).strong().color(c.text));
                    ui.add(egui::Label::new(RichText::new(&body).color(c.weak)).wrap());
                    ui.add_space(4.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if primary_button(ui, &c, yes).clicked() {
                            answer = Some(true);
                        }
                        if secondary_button(ui, &c, no).clicked() {
                            answer = Some(false);
                        }
                    });
                });
            });
        let Some(yes) = answer else { return };
        self.sys.notice_closed = true;
        match notice {
            Notice::DefaultApp { register } => {
                self.a.store.settings.default_app_asked = true;
                if yes {
                    self.make_default(register);
                }
            }
            Notice::UpdateOptIn => {
                self.a.store.settings.update_check = Some(yes);
                if yes {
                    self.sys.checker.start(ctx);
                }
            }
            Notice::NewVersion(v) => {
                if yes && !self.sys.install.managed() {
                    self.open_releases();
                }
                self.a.store.settings.dismissed_version = v;
            }
        }
        self.a.save();
    }

    /// The outcome of the last action, for the status bar.
    pub fn take_system_message(&mut self) -> Option<String> {
        if self.settings_open {
            return None;
        }
        self.sys.msg.take().map(|(_, m)| m)
    }
}
