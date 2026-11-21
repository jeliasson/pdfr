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
use std::time::{Duration, Instant};

/// How long to wait for an answer in macOS's "use pdfr?" dialog.
const AWAIT_DEFAULT: Duration = Duration::from_secs(60);
const LOOK_AGAIN: Duration = Duration::from_secs(1);

pub struct SystemUi {
    install: InstallKind,
    /// Looked up when needed (it reads a few small files).
    default_app: Option<DefaultApp>,
    on_path: Option<bool>,
    /// The last action's outcome: (worked, message).
    msg: Option<(bool, String)>,
    /// macOS: when pdfr asked the system to make it the default. The system
    /// answers in its own dialog, so the default app is looked up again
    /// until it is pdfr (and the "confirm" message goes) or a minute passes.
    awaiting_default: Option<Instant>,
    last_default_look: Option<Instant>,
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
            awaiting_default: None,
            last_default_look: None,
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
    /// Ask once: look for new versions?
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
        if res.is_ok() && cfg!(target_os = "macos") {
            self.sys.awaiting_default = Some(Instant::now());
        }
        self.sys.msg = Some(match res {
            Ok(m) => (true, m),
            Err(e) => (false, e),
        });
        self.refresh_system();
    }

    /// While the system's "use pdfr?" dialog may be open: look again once a
    /// second, and drop the "confirm" message once it's answered or stale.
    fn follow_default_dialog(&mut self, ctx: &eframe::egui::Context) {
        let Some(since) = self.sys.awaiting_default else {
            return;
        };
        if self.sys.default_app == Some(DefaultApp::Pdfr) || since.elapsed() > AWAIT_DEFAULT {
            self.sys.awaiting_default = None;
            self.sys.msg = None;
            return;
        }
        if self
            .sys
            .last_default_look
            .is_none_or(|t| t.elapsed() >= LOOK_AGAIN)
        {
            self.sys.default_app = None;
            self.sys.last_default_look = Some(Instant::now());
        }
        ctx.request_repaint_after(LOOK_AGAIN);
    }

    /// The General section of Settings. Returns true if a setting changed.
    pub(super) fn general_section(&mut self, ui: &mut Ui, c: &Colors) -> bool {
        let mut changed = false;
        self.follow_default_dialog(ui.ctx());
        let install = self.sys.install;
        let default = self.default_app();
        let direct = install == InstallKind::Direct;

        card(ui, c, Some("Opening PDFs"), |ui| {
            let sub = match &default {
                DefaultApp::Pdfr => {
                    "PDF files you open from your files or browser open in pdfr.".to_string()
                }
                DefaultApp::Other(name) => format!("PDF files open in {name} now."),
                DefaultApp::Unknown => "Choose the app that opens PDF files.".to_string(),
            };
            let mut clicked = false;
            setting_row(ui, c, "Default PDF app", &sub, |ui| {
                // Ticked while pdfr opens PDFs; click to switch either way.
                let on = default == DefaultApp::Pdfr;
                if on || system::can_make_default() {
                    let tip = if on {
                        "Stop opening PDF files in pdfr"
                    } else {
                        "Open PDF files in pdfr"
                    };
                    clicked = checkbox(ui, c, on).on_hover_text(tip).clicked();
                }
            });
            if clicked {
                if default == DefaultApp::Pdfr {
                    self.sys.msg = Some(match system::unset_default() {
                        Ok(m) => (true, m),
                        Err(e) => (false, e),
                    });
                    self.refresh_system();
                } else {
                    // An archive install joins the app menu first.
                    self.make_default(direct && !system::in_app_menu());
                }
            }
            if cfg!(target_os = "linux") && direct {
                row_divider(ui, c);
                let mut flip = false;
                let added = system::in_app_menu();
                setting_row(
                    ui,
                    c,
                    "App menu",
                    "Add pdfr to your app launcher and to \u{201c}Open with\u{201d}, and the pdfr command to the terminal.",
                    |ui| {
                        flip = checkbox(ui, c, added).clicked();
                    },
                );
                if flip {
                    let res = if added {
                        system::unregister()
                    } else {
                        system::register()
                    };
                    self.sys.msg = Some(match res {
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
                    ui.label(
                        RichText::new(if on_path {
                            "Installed"
                        } else {
                            "Not installed"
                        })
                        .color(if on_path { c.text } else { c.weak }),
                    );
                },
            );
            if let Some((ok, m)) = &self.sys.msg {
                row_divider(ui, c);
                ui.label(
                    RichText::new(m)
                        .size(12.0)
                        .color(if *ok { c.text } else { DANGER }),
                );
            }
        });

        card(ui, c, Some("Updates"), |ui| {
            if !install.checks_for_updates() {
                let (title, sub) = if install == InstallKind::Dev {
                    (
                        "Development build",
                        "Run from a Cargo build folder, so pdfr doesn\u{2019}t check for new versions.",
                    )
                } else {
                    (
                        "Updates",
                        "Your software centre installs new versions of pdfr, or run  flatpak update",
                    )
                };
                setting_row(ui, c, title, sub, |_| {});
                return;
            }
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
            let newer = !latest.is_empty() && updates::is_newer(&latest, updates::current());
            let status = if self.sys.checker.running() {
                "Checking\u{2026}".to_string()
            } else if let Some(e) = &self.sys.check_error {
                format!("Could not check: {e}")
            } else if newer {
                match install.update_hint() {
                    Some(cmd) => {
                        format!("pdfr {latest} is available. Update in a terminal with:  {cmd}")
                    }
                    None if install.managed() => {
                        format!("pdfr {latest} is available from your package manager.")
                    }
                    None => format!("pdfr {latest} is available."),
                }
            } else if self.a.store.settings.update_checked_at > 0 {
                "You have the newest version.".to_string()
            } else {
                "Not checked yet.".to_string()
            };
            let mut check = false;
            let mut download = false;
            setting_row(
                ui,
                c,
                &format!("Version {}", updates::current()),
                &status,
                |ui| {
                    if newer && let Some(cmd) = install.update_hint() {
                        if primary_button(ui, c, "Copy command").clicked() {
                            ui.ctx().copy_text(cmd.to_string());
                            self.sys.msg =
                                Some((true, "Copied. Paste it into a terminal to update.".into()));
                        }
                    } else if newer && !install.managed() {
                        download = primary_button(ui, c, "Download").clicked();
                    } else {
                        check = ui
                            .add_enabled_ui(!self.sys.checker.running(), |ui| {
                                secondary_button(ui, c, "Check now")
                            })
                            .inner
                            .clicked();
                    }
                },
            );
            if check {
                self.sys.manual_check = true;
                self.sys.check_error = None;
                self.sys.checker.start(ui.ctx());
            }
            if download {
                self.open_releases();
            }
        });
        if !install.checks_for_updates() {
            note(ui, c, &format!("pdfr {}.", updates::current()));
        }
        card(ui, c, Some("Pages"), |ui| {
            let mut on = self.a.store.settings.dark_pages;
            setting_row(
                ui,
                c,
                "Dark pages",
                "Show pages light on dark, for reading at night (Ctrl+M). Saving and printing are unchanged.",
                |ui| {
                    if toggle(ui, c, &mut on).changed() {
                        self.a.store.settings.dark_pages = on;
                        changed = true;
                    }
                },
            );
        });
        card(ui, c, Some("Recent files"), |ui| {
            let st = &mut self.a.store.settings;
            let mut on = st.remember_recent;
            setting_row(
                ui,
                c,
                "Remember recent files",
                "List the files you open (Ctrl+E) and reopen each where you left it.",
                |ui| {
                    if toggle(ui, c, &mut on).changed() {
                        st.remember_recent = on;
                        if !on {
                            st.recent.clear();
                        }
                        changed = true;
                    }
                },
            );
            if !st.recent.is_empty() {
                row_divider(ui, c);
                let n = st.recent.len();
                let mut clear = false;
                setting_row(
                    ui,
                    c,
                    "Clear the list",
                    &format!("{n} file{} listed.", if n == 1 { "" } else { "s" }),
                    |ui| clear = secondary_button(ui, c, "Clear").clicked(),
                );
                if clear {
                    st.recent.clear();
                    changed = true;
                }
            }
        });
        card(ui, c, Some("Signatures"), |ui| {
            let lock = &mut self.a.store.settings.lock_signatures;
            let options = [
                (
                    false,
                    "Keep them editable",
                    "Saved as a signature other PDF apps can still move or delete.",
                ),
                (
                    true,
                    "Lock them into the page",
                    "Saved as part of the page, so no one can move or remove them.",
                ),
            ];
            for (i, (v, title, sub)) in options.into_iter().enumerate() {
                if i > 0 {
                    row_divider(ui, c);
                }
                if choice_row(ui, c, *lock == v, title, sub) && *lock != v {
                    *lock = v;
                    changed = true;
                }
            }
        });
        note(
            ui,
            c,
            "For new drawn signatures. Right-click a signature to change it. To sign with a certificate, use Sign with certificate (Ctrl+Shift+C).",
        );
        card(ui, c, Some("Signing with a certificate"), |ui| {
            let st = &mut self.a.store.settings;
            let mut on = st.sign_timestamp;
            setting_row(
                ui,
                c,
                "Add a trusted timestamp",
                "Proves when you signed, and keeps the signature checkable after your certificate expires. Only a fingerprint of the signature is sent to the timestamp server, never the document.",
                |ui| {
                    if toggle(ui, c, &mut on).changed() {
                        st.sign_timestamp = on;
                        changed = true;
                    }
                },
            );
            if st.sign_timestamp {
                row_divider(ui, c);
                setting_row(
                    ui,
                    c,
                    "Timestamp server",
                    "Leave empty for pdfr's default.",
                    |ui| {
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut st.tsa_url)
                                    .hint_text(pdfr_sign::tsa::DEFAULT_TSA)
                                    .desired_width(240.0),
                            )
                            .changed()
                        {
                            changed = true;
                        }
                    },
                );
            }
            row_divider(ui, c);
            let mut on = st.check_revocation;
            setting_row(
                ui,
                c,
                "Check whether certificates were revoked (contacts the issuer)",
                "When a file doesn't include the answer, pdfr asks the certificate's issuer, which tells it which certificate is being checked. Files you sign also get that answer added, so their signatures stay checkable for years.",
                |ui| {
                    if toggle(ui, c, &mut on).changed() {
                        st.check_revocation = on;
                        changed = true;
                    }
                },
            );
            row_divider(ui, c);
            let nss = pdfr_sign::signer::pkcs11::nss_dir();
            let where_ = match &nss {
                Some(d) => format!(
                    "Certificates installed in your browser ({}), ID cards and tokens, and .p12 files you choose. pdfr never stores a PIN or password.",
                    d.display()
                ),
                None => "ID cards and tokens, and .p12 files you choose. No browser certificate store was found. pdfr never stores a PIN or password.".into(),
            };
            setting_row(ui, c, "Where certificates come from", &where_, |_| {});
            if !st.cert_files.is_empty() {
                row_divider(ui, c);
                let mut forget = false;
                setting_row(
                    ui,
                    c,
                    "Certificate files used before",
                    &format!("{} listed in the signing dialog.", st.cert_files.len()),
                    |ui| forget = secondary_button(ui, c, "Forget").clicked(),
                );
                if forget {
                    st.cert_files.clear();
                    st.last_cert.clear();
                    changed = true;
                }
            }
        });
        changed
    }

    fn open_releases(&mut self) {
        if let Err(e) = system::open_url(system::RELEASES_URL) {
            self.sys.msg = Some((
                false,
                format!("{e}. The downloads are at {}", system::RELEASES_URL),
            ));
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
            && self.sys.install.checks_for_updates()
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
        if self.sys.install.checks_for_updates() && self.a.store.settings.update_check.is_none() {
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
                "Once a day pdfr asks GitHub for the newest version number. Nothing else is sent."
                    .to_string(),
                "Yes",
                "No thanks",
            ),
            Notice::NewVersion(v) => (
                format!("pdfr {v} is available"),
                match self.sys.install.update_hint() {
                    Some(cmd) => format!("Update with:  {cmd}"),
                    None if self.sys.install.managed() => {
                        "Update it with your package manager.".to_string()
                    }
                    None => format!("You have {}.", updates::current()),
                },
                match self.sys.install.update_hint() {
                    Some(_) => "Copy command",
                    None if self.sys.install.managed() => "Got it",
                    None => "Download",
                },
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
                if yes {
                    match self.sys.install.update_hint() {
                        Some(cmd) => {
                            ctx.copy_text(cmd.to_string());
                            self.sys.msg =
                                Some((true, "Copied. Paste it into a terminal to update.".into()));
                        }
                        None if !self.sys.install.managed() => self.open_releases(),
                        None => {}
                    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn state(tag: &str) -> AssistState {
        let dir = std::env::temp_dir().join(format!("pdfr-sys-{tag}-{}", std::process::id()));
        AssistState::new(egui::Context::default(), true, Some(dir), false)
    }

    fn waiting(a: &mut AssistState, since: Instant) {
        a.sys.awaiting_default = Some(since);
        a.sys.msg = Some((true, "Confirm in the dialog macOS shows.".into()));
    }

    #[test]
    fn confirm_message_goes_once_pdfr_is_the_default() {
        let mut a = state("confirmed");
        waiting(&mut a, Instant::now());
        a.sys.default_app = Some(DefaultApp::Other("Preview".into()));
        a.sys.last_default_look = Some(Instant::now());
        a.follow_default_dialog(&egui::Context::default());
        assert!(a.sys.msg.is_some(), "still waiting for the answer");

        // The dialog was answered: the next look finds pdfr.
        a.sys.default_app = Some(DefaultApp::Pdfr);
        a.follow_default_dialog(&egui::Context::default());
        assert!(a.sys.msg.is_none());
        assert!(a.sys.awaiting_default.is_none());
    }

    #[test]
    fn looks_again_while_waiting_and_gives_up_after_a_minute() {
        let mut a = state("stale");
        waiting(&mut a, Instant::now());
        a.sys.default_app = Some(DefaultApp::Other("Preview".into()));
        a.follow_default_dialog(&egui::Context::default());
        assert!(
            a.sys.default_app.is_none(),
            "forgotten, so it is looked up again"
        );

        waiting(
            &mut a,
            Instant::now() - AWAIT_DEFAULT - Duration::from_secs(1),
        );
        a.sys.default_app = Some(DefaultApp::Other("Preview".into()));
        a.follow_default_dialog(&egui::Context::default());
        assert!(
            a.sys.msg.is_none(),
            "an unanswered dialog doesn't leave the message forever"
        );
    }
}
