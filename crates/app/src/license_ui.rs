//! Registering pdfr: the licence key, the "please register" reminder for
//! unregistered copies (WinRAR style: always closable straight away), the
//! register dialog, the License section of Settings and the daily online
//! check. Keys are verified offline by `pdfr-license`; see
//! docs/license-keys.md.

mod online;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::assist_ui::widgets::*;
use crate::icons::{self, Icon};
use crate::theme::Palette;
use eframe::egui::{self, Align, Id, Layout, RichText, Ui, vec2};
use pdfr_license::{self as lic, License, Plan, State};

/// No reminder for the first week.
pub const GRACE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// While pdfr stays open, the reminder comes back this often.
pub const EVERY: Duration = Duration::from_secs(30 * 60);
/// And on every this many saves.
pub const SAVES: u32 = 5;
/// The online check runs at most this often.
const CHECK_EVERY: u64 = 24 * 60 * 60;
/// …and at launch only if the last one is older than this.
const CHECK_AT_LAUNCH: u64 = 60 * 60;

const MESSAGE: &str = "Using pdfr is free, but it may not always be. Sign up and register your copy for a free lifetime license.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dialog {
    /// The reminder.
    Nag,
    /// Paste a key.
    Register,
    /// Something the online check said (revoked, too many computers).
    Notice(String),
}

pub struct LicenseUi {
    dir: Option<PathBuf>,
    /// Public keys trusted (tests swap in the test key).
    pub keys: Vec<(u8, [u8; 32])>,
    pub license: Option<License>,
    pub state: State,
    pub dialog: Option<Dialog>,
    /// Automation runs and tests: no reminder, no network.
    pub quiet: bool,
    /// Whether the online check may run (off in tests).
    pub online: bool,
    paste: String,
    confirm_remove: bool,
    /// When the reminder last closed in this run.
    shown_at: Option<Instant>,
    launch_done: bool,
    checker: online::Checker,
    checked_at: Option<Instant>,
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl LicenseUi {
    pub fn new(data_dir: Option<&std::path::Path>, quiet: bool) -> LicenseUi {
        let dir = lic::data_dir(data_dir);
        let mut ui = LicenseUi {
            dir,
            keys: lic::trusted_keys(),
            license: None,
            state: State::default(),
            dialog: None,
            quiet,
            online: !cfg!(test) && !quiet,
            paste: String::new(),
            confirm_remove: false,
            shown_at: None,
            launch_done: false,
            checker: online::Checker::default(),
            checked_at: None,
        };
        ui.reload();
        ui
    }

    /// Reads the key and state from disk (also after tests change `keys`).
    pub fn reload(&mut self) {
        let Some(dir) = &self.dir else { return };
        self.state = State::load(dir);
        if self.state.first_run == 0 {
            self.state.first_run = now();
            let _ = self.state.save(dir);
        }
        self.license = lic::load_key(dir).and_then(|k| lic::verify(&k, &self.keys).ok());
    }

    fn save_state(&self) {
        if let Some(dir) = &self.dir {
            let _ = self.state.save(dir);
        }
    }

    pub fn registered(&self) -> bool {
        self.license.is_some()
    }

    pub fn modal_open(&self) -> bool {
        self.dialog.is_some()
    }

    fn in_grace(&self) -> bool {
        now().saturating_sub(self.state.first_run) < GRACE.as_secs()
    }

    /// Whether the reminder may show now.
    fn may_nag(&self) -> bool {
        !self.quiet && !self.registered() && !self.in_grace() && self.dialog.is_none()
    }

    fn nag(&mut self) {
        self.dialog = Some(Dialog::Nag);
        self.state.last_nag = now();
        self.state.saves = 0;
        self.save_state();
    }

    /// A document was saved.
    pub fn on_save(&mut self) {
        if self.registered() {
            return;
        }
        self.state.saves += 1;
        self.save_state();
        if self.state.saves >= SAVES && self.may_nag() {
            self.nag();
        }
    }

    pub fn open_register(&mut self) {
        self.paste.clear();
        self.dialog = Some(Dialog::Register);
    }

    pub fn open_reminder(&mut self) {
        self.dialog = Some(Dialog::Nag);
    }

    /// Saves a verified key and registers this copy.
    pub fn register(&mut self, l: License) -> Result<(), String> {
        let dir = self
            .dir
            .as_ref()
            .ok_or("No data folder to keep the key in.")?;
        lic::save_key(dir, &l.key).map_err(|e| format!("Couldn't save the key: {e}"))?;
        self.license = Some(l);
        self.state.last_check = 0;
        self.state.last_status = None;
        self.state.last_message = None;
        self.save_state();
        self.checked_at = None;
        Ok(())
    }

    pub fn unregister(&mut self) {
        if let Some(dir) = &self.dir {
            let _ = lic::remove_key(dir);
        }
        self.license = None;
        self.state.last_status = None;
        self.state.last_message = None;
        self.save_state();
    }

    /// Timers, the online check, and the reminder at launch. `blocked`: some
    /// other dialog is open, so no reminder now.
    pub fn tick(&mut self, ctx: &egui::Context, blocked: bool) {
        self.poll_check();
        if self.online && self.registered() && !self.checker.running() {
            let since = now().saturating_sub(self.state.last_check);
            let due = match self.checked_at {
                None => since >= CHECK_AT_LAUNCH,
                Some(t) => t.elapsed().as_secs() >= CHECK_EVERY,
            };
            if due && let (Some(l), Some(dir)) = (&self.license, &self.dir) {
                self.checker.start(ctx, l.id.clone(), lic::install_id(dir));
                self.checked_at = Some(Instant::now());
            }
        }
        if blocked || !self.may_nag() {
            return;
        }
        if !self.launch_done {
            self.launch_done = true;
            self.nag();
            return;
        }
        let since = self.shown_at.map(|t| t.elapsed()).unwrap_or(Duration::ZERO);
        if self.shown_at.is_some() && since >= EVERY {
            self.nag();
        } else if let Some(t) = self.shown_at {
            ctx.request_repaint_after(EVERY.saturating_sub(t.elapsed()));
        }
    }

    fn poll_check(&mut self) {
        let Some(r) = self.checker.poll() else { return };
        let Ok(a) = r else { return };
        self.state.last_check = now();
        self.state.last_status = Some(a.status.clone());
        self.state.last_message = a.message.clone();
        self.save_state();
        self.apply_answer(&a.status, a.message);
    }

    /// What pdfr.app said about the key.
    pub fn apply_answer(&mut self, status: &str, message: Option<String>) {
        match status {
            "revoked" => {
                self.unregister();
                self.state.last_status = Some("revoked".into());
                self.save_state();
                self.dialog = Some(Dialog::Notice(
                    message.unwrap_or_else(|| "This licence key was revoked.".into()),
                ));
            }
            "device_limit" => {
                self.dialog = Some(Dialog::Notice(message.unwrap_or_else(|| {
                    "This key is in use on too many computers. Free one at pdfr.app/account.".into()
                })));
            }
            _ => {}
        }
    }

    fn closed(&mut self) {
        self.dialog = None;
        self.shown_at = Some(Instant::now());
    }

    /* --------------------------------------------------------------------- */
    /* Drawing                                                                */

    pub fn windows(&mut self, ctx: &egui::Context, pal: &Palette) {
        let Some(d) = self.dialog.clone() else { return };
        let c = Colors::new(pal);
        let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        match d {
            Dialog::Nag => self.nag_window(ctx, pal, &c, esc),
            Dialog::Register => self.register_window(ctx, pal, &c, esc),
            Dialog::Notice(m) => self.notice_window(ctx, pal, &c, &m, esc),
        }
    }

    fn nag_window(&mut self, ctx: &egui::Context, pal: &Palette, c: &Colors, esc: bool) {
        let mut close = esc;
        egui::Modal::new(Id::unique("pdfr-register-reminder"))
            .frame(panel_frame(ctx, pal, c, 26))
            .show(ctx, |ui| {
                ui.set_width(440.0);
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(26.0, 26.0), egui::Sense::hover());
                    icons::paint(ui.painter(), r, Icon::Key, c.accent);
                    ui.label(
                        RichText::new("This copy of pdfr isn't registered")
                            .size(19.0)
                            .strong()
                            .color(c.text),
                    );
                });
                ui.add(egui::Label::new(RichText::new(MESSAGE).size(14.5).color(c.text)).wrap());
                ui.add(
                    egui::Label::new(
                        RichText::new("Using pdfr at work? Get a Business licence: $39, once.")
                            .color(c.weak),
                    )
                    .wrap(),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if primary_button(ui, c, "Get free license").clicked() {
                        let _ = crate::system::open_url(lic::GET_URL);
                    }
                    if secondary_button(ui, c, "Enter license key\u{2026}").clicked() {
                        self.open_register();
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if secondary_button(ui, c, "Not now").clicked() {
                            close = true;
                        }
                    });
                });
            });
        if close && self.dialog == Some(Dialog::Nag) {
            self.closed();
        }
    }

    fn register_window(&mut self, ctx: &egui::Context, pal: &Palette, c: &Colors, esc: bool) {
        let mut close = esc;
        let checked = (!self.paste.trim().is_empty()).then(|| lic::verify(&self.paste, &self.keys));
        let mut error: Option<String> = None;
        egui::Modal::new(Id::unique("pdfr-register"))
            .frame(panel_frame(ctx, pal, c, 26))
            .show(ctx, |ui| {
                ui.set_width(460.0);
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                ui.label(RichText::new("Register pdfr").size(19.0).strong().color(c.text));
                ui.add(
                    egui::Label::new(
                        RichText::new("Paste the licence key from pdfr.app. It's checked on this computer; no internet needed.")
                            .color(c.weak),
                    )
                    .wrap(),
                );
                let edit = egui::TextEdit::multiline(&mut self.paste)
                    .hint_text("PDFR1.\u{2026}")
                    .font(egui::TextStyle::Monospace)
                    .desired_rows(4)
                    .desired_width(f32::INFINITY);
                let r = ui.add(edit);
                if self.paste.is_empty() && !r.has_focus() {
                    r.request_focus();
                }
                match &checked {
                    Some(Ok(l)) => {
                        ui.horizontal(|ui| {
                            tick(ui, c.accent, 14.0);
                            ui.label(
                                RichText::new(format!("Registered to {} \u{b7} {}", l.name, l.plan.label()))
                                    .color(c.text),
                            );
                        });
                    }
                    Some(Err(e)) => {
                        ui.add(egui::Label::new(RichText::new(e.to_string()).color(DANGER)).wrap());
                    }
                    None => {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("No key yet?").color(c.weak));
                            if link(ui, c, "Get a free one at pdfr.app").clicked() {
                                let _ = crate::system::open_url(lic::GET_URL);
                            }
                        });
                    }
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let ok = matches!(checked, Some(Ok(_)));
                    let b = ui.add_enabled_ui(ok, |ui| primary_button(ui, c, "Register")).inner;
                    if b.clicked()
                        && let Some(Ok(l)) = checked.clone()
                    {
                        match self.register(l) {
                            Ok(()) => close = true,
                            Err(e) => error = Some(e),
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if secondary_button(ui, c, "Cancel").clicked() {
                            close = true;
                        }
                    });
                });
            });
        if let Some(e) = error {
            self.dialog = Some(Dialog::Notice(e));
        } else if close && self.dialog == Some(Dialog::Register) {
            self.closed();
        }
    }

    fn notice_window(
        &mut self,
        ctx: &egui::Context,
        pal: &Palette,
        c: &Colors,
        msg: &str,
        esc: bool,
    ) {
        let mut close = esc;
        egui::Modal::new(Id::unique("pdfr-license-notice"))
            .frame(panel_frame(ctx, pal, c, 26))
            .show(ctx, |ui| {
                ui.set_width(420.0);
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                ui.label(
                    RichText::new("Your pdfr licence")
                        .size(19.0)
                        .strong()
                        .color(c.text),
                );
                ui.add(egui::Label::new(RichText::new(msg).color(c.text)).wrap());
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if secondary_button(ui, c, "Manage computers").clicked() {
                        let _ = crate::system::open_url(lic::ACCOUNT_URL);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if primary_button(ui, c, "OK").clicked() {
                            close = true;
                        }
                    });
                });
            });
        if close {
            self.closed();
        }
    }

    /// Settings → License.
    pub fn settings_section(&mut self, ui: &mut Ui, c: &Colors) {
        match self.license.clone() {
            Some(l) => {
                card(ui, c, None, |ui| {
                    setting_row(ui, c, "Registered to", plan_note(&l.plan), |ui| {
                        done_label(ui, c, &l.name);
                    });
                    row_divider(ui, c);
                    setting_row(ui, c, "Licence", "Lifetime: every update included.", |ui| {
                        ui.label(
                            RichText::new(format!("{} \u{b7} issued {}", l.plan.label(), l.issued))
                                .color(c.text),
                        );
                    });
                    row_divider(ui, c);
                    let checked = match (self.state.last_check, self.state.last_status.as_deref()) {
                        (0, _) => {
                            "Not checked online yet. pdfr checks once a day when it has internet."
                                .to_string()
                        }
                        (_, Some("device_limit")) => {
                            "In use on too many computers. Free one on your account.".to_string()
                        }
                        _ => "Checked online. pdfr checks once a day when it has internet."
                            .to_string(),
                    };
                    setting_row(ui, c, "Computers", &checked, |ui| {
                        if secondary_button(ui, c, "Manage").clicked() {
                            let _ = crate::system::open_url(lic::ACCOUNT_URL);
                        }
                    });
                });
                card(ui, c, None, |ui| {
                    let confirming = self.confirm_remove;
                    setting_row(
                        ui,
                        c,
                        "Remove licence",
                        if confirming {
                            "Click again to remove the key from this computer."
                        } else {
                            "This computer becomes unregistered. The key stays on your account."
                        },
                        |ui| {
                            let label = if confirming {
                                "Remove"
                            } else {
                                "Remove\u{2026}"
                            };
                            if danger_button(ui, label, confirming).clicked() {
                                if confirming {
                                    self.unregister();
                                }
                                self.confirm_remove = !confirming;
                            }
                        },
                    );
                });
            }
            None => {
                card(ui, c, None, |ui| {
                    ui.add(egui::Label::new(RichText::new(MESSAGE).color(c.text)).wrap());
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if primary_button(ui, c, "Get free license").clicked() {
                            let _ = crate::system::open_url(lic::GET_URL);
                        }
                        if secondary_button(ui, c, "Enter license key\u{2026}").clicked() {
                            self.open_register();
                        }
                    });
                });
                if self.state.last_status.as_deref() == Some("revoked") {
                    note(ui, c, "The key on this computer was revoked by pdfr.app.");
                }
                card(ui, c, Some("Plans"), |ui| {
                    setting_row(
                        ui,
                        c,
                        "Personal",
                        "Free for life, one key per account. Non-commercial use.",
                        |ui| {
                            ui.label(RichText::new("Free").color(c.text));
                        },
                    );
                    row_divider(ui, c);
                    setting_row(
                        ui,
                        c,
                        "Business",
                        "Use pdfr at work. One key per person, up to 3 computers.",
                        |ui| {
                            ui.label(RichText::new("$39 once").color(c.text));
                        },
                    );
                    row_divider(ui, c);
                    setting_row(
                        ui,
                        c,
                        "Sponsor",
                        "100 keys to use, give away or resell, and your name on pdfr.app.",
                        |ui| {
                            ui.label(RichText::new("$499 once").color(c.text));
                        },
                    );
                });
                if link(ui, c, "Compare plans on pdfr.app").clicked() {
                    let _ = crate::system::open_url(lic::PRICING_URL);
                }
            }
        }
    }
}

/// A tick drawn with lines (the UI font has no check mark).
fn tick(ui: &mut Ui, color: egui::Color32, side: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(side, side), egui::Sense::hover());
    let s = egui::Stroke::new(2.0, color);
    let pt = |x: f32, y: f32| rect.min + vec2(x * rect.width(), y * rect.height());
    ui.painter()
        .line_segment([pt(0.18, 0.52), pt(0.42, 0.75)], s);
    ui.painter()
        .line_segment([pt(0.42, 0.75), pt(0.84, 0.25)], s);
}

/// Text with a tick before it, in a right-to-left control slot.
fn done_label(ui: &mut Ui, c: &Colors, text: &str) {
    ui.spacing_mut().item_spacing.x = 5.0;
    ui.label(RichText::new(text).color(c.text));
    tick(ui, c.accent, 14.0);
}

fn plan_note(p: &Plan) -> &'static str {
    if p.commercial() {
        "Thank you for supporting pdfr."
    } else {
        "Personal licence: non-commercial use."
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pdfr-license-ui-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn ui(d: &std::path::Path) -> LicenseUi {
        let mut u = LicenseUi::new(Some(d), false);
        u.keys = lic::test_keys();
        u.online = false;
        u.reload();
        u
    }

    fn age(u: &mut LicenseUi, d: &std::path::Path, secs: u64) {
        u.state.first_run = now() - secs;
        u.state.save(d).unwrap();
    }

    #[test]
    fn grace_then_reminders() {
        let d = dir("grace");
        let mut u = ui(&d);
        let ctx = egui::Context::default();
        u.tick(&ctx, false);
        assert_eq!(u.dialog, None, "no reminder in the first week");
        age(&mut u, &d, GRACE.as_secs() + 10);
        u.tick(&ctx, true);
        assert_eq!(u.dialog, None, "not over another dialog");
        u.tick(&ctx, false);
        assert_eq!(
            u.dialog,
            Some(Dialog::Nag),
            "at launch after the grace period"
        );
        u.closed();
        u.tick(&ctx, false);
        assert_eq!(u.dialog, None, "not again straight away");
        // Every fifth save.
        for _ in 0..SAVES - 1 {
            u.on_save();
        }
        assert_eq!(u.dialog, None);
        u.on_save();
        assert_eq!(u.dialog, Some(Dialog::Nag));
        // After the interval.
        u.closed();
        u.shown_at = Some(Instant::now() - EVERY - Duration::from_secs(1));
        u.tick(&ctx, false);
        assert_eq!(u.dialog, Some(Dialog::Nag));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn registered_copies_are_left_alone() {
        let d = dir("registered");
        let mut u = ui(&d);
        age(&mut u, &d, GRACE.as_secs() * 2);
        u.register(lic::verify(lic::TEST_KEY, &lic::test_keys()).unwrap())
            .unwrap();
        let ctx = egui::Context::default();
        u.tick(&ctx, false);
        for _ in 0..SAVES * 2 {
            u.on_save();
        }
        assert_eq!(u.dialog, None);
        // Still registered after a restart.
        let u2 = ui(&d);
        assert_eq!(u2.license.as_ref().unwrap().name, "Test Licensee");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn online_answers() {
        let d = dir("answers");
        let mut u = ui(&d);
        u.register(lic::verify(lic::TEST_KEY, &lic::test_keys()).unwrap())
            .unwrap();
        u.apply_answer("active", None);
        assert!(u.registered());
        u.apply_answer("device_limit", Some("Too many".into()));
        assert!(u.registered(), "too many computers keeps the key");
        assert_eq!(u.dialog, Some(Dialog::Notice("Too many".into())));
        u.apply_answer("revoked", Some("Refunded.".into()));
        assert!(!u.registered());
        assert!(lic::load_key(&d).is_none(), "a revoked key is removed");
        assert_eq!(u.dialog, Some(Dialog::Notice("Refunded.".into())));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn quiet_runs_never_remind() {
        let d = dir("quiet");
        let mut u = LicenseUi::new(Some(&d), true);
        age(&mut u, &d, GRACE.as_secs() * 2);
        u.tick(&egui::Context::default(), false);
        for _ in 0..SAVES {
            u.on_save();
        }
        assert_eq!(u.dialog, None);
        let _ = std::fs::remove_dir_all(&d);
    }
}
