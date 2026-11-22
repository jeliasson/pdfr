//! Signing with a certificate (digital signature): choosing the
//! certificate, the look, where it goes, then signing and saving on a
//! background thread; and checking the signatures of an opened file.
//!
//! pdfr never stores a PIN or password; it remembers only which
//! certificate was used (its fingerprint) and the paths of .p12 files.

use super::signature::{SignTab, Target};
use super::*;
use crate::assist_ui::widgets::{
    Colors, panel_frame, primary_button, row_divider, secondary_button, segmented,
};
use egui::{Align, Layout, RichText};
use pdfr_core::digital::{DEFAULT_RESERVE, SignRequest};
use pdfr_core::signature::Ink;
use pdfr_sign::Signer;
use pdfr_sign::ltv::Level;
use pdfr_sign::revocation::{Cache, Fetch, Online, Revocation, Source, State};
use pdfr_sign::signer::{CertChoice, PinTries};
use pdfr_sign::trust::TrustStore;
use pdfr_sign::verify::{After, Report, Status};
use std::sync::OnceLock;
use std::sync::mpsc::Receiver;
use zeroize::Zeroizing;

#[cfg(target_os = "macos")]
const NO_CERTS: &str = "No certificates found. Add yours to the Keychain (double-click the .p12 file), \
     insert your ID card, or choose a certificate file.";
#[cfg(not(target_os = "macos"))]
const NO_CERTS: &str = "No certificates found. Install yours in Chrome, Chromium or Firefox, \
     insert your ID card or token, or choose a certificate file.";

/// The PIN or password field, by id: its state is dropped when the
/// dialog closes.
const PIN_FIELD: &str = "pdfr-sign-pin";

fn forget_pin(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove::<egui::text_edit::TextEditState>(Id::unique(PIN_FIELD)));
}

/// The roots signatures are checked against, loaded once.
fn trust() -> &'static TrustStore {
    static T: OnceLock<TrustStore> = OnceLock::new();
    T.get_or_init(TrustStore::standard)
}

/// Issuers' revocation answers, kept for a day.
fn revocation_cache() -> &'static Cache {
    static C: OnceLock<Cache> = OnceLock::new();
    C.get_or_init(Cache::default)
}

/// Asks certificate issuers (OCSP responders, CRLs) over plain HTTP(S),
/// giving up quickly so a check never waits long. Never this computer or
/// the local network, and no redirects (which could lead there).
struct NetFetch(ureq::Agent);

impl NetFetch {
    fn new() -> NetFetch {
        NetFetch(crate::updates::agent_with_timeout(6))
    }
}

/// Refuses an issuer address on this computer or the local network,
/// as written or as its name resolves.
fn public_address(url: &str) -> Result<(), String> {
    use std::net::ToSocketAddrs;
    const LOCAL: &str = "the issuer's address is on this computer or the local network";
    if !pdfr_sign::revocation::public_url(url) {
        return Err(LOCAL.into());
    }
    let uri: ureq::http::Uri = url
        .parse()
        .map_err(|_| "the issuer's address can't be read".to_string())?;
    let host = uri
        .host()
        .ok_or("the issuer's address can't be read")?
        .trim_start_matches('[')
        .trim_end_matches(']');
    let port = uri
        .port_u16()
        .unwrap_or(if uri.scheme_str() == Some("https") {
            443
        } else {
            80
        });
    let addrs: Vec<_> = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("could not reach the issuer ({e})"))?
        .collect();
    if addrs.is_empty()
        || addrs
            .iter()
            .any(|a| !pdfr_sign::revocation::is_public_ip(a.ip()))
    {
        return Err(LOCAL.into());
    }
    Ok(())
}

impl Fetch for NetFetch {
    fn post(&self, url: &str, content_type: &str, body: &[u8]) -> Result<Vec<u8>, String> {
        public_address(url)?;
        let resp = self
            .0
            .post(url)
            .config()
            .max_redirects(0)
            .build()
            .header("Content-Type", content_type)
            .send(body)
            .map_err(|e| format!("could not reach the issuer ({e})"))?;
        read_body(resp, 1 << 20)
    }

    fn get(&self, url: &str) -> Result<Vec<u8>, String> {
        public_address(url)?;
        let resp = self
            .0
            .get(url)
            .config()
            .max_redirects(0)
            .build()
            .call()
            .map_err(|e| format!("could not reach the issuer ({e})"))?;
        read_body(resp, 32 << 20)
    }
}

fn read_body(resp: ureq::http::Response<ureq::Body>, limit: u64) -> Result<Vec<u8>, String> {
    resp.into_body()
        .with_config()
        .limit(limit)
        .read_to_vec()
        .map_err(|e| e.to_string())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Look {
    /// A saved signature (by index into the saved list) and the details.
    Saved(usize),
    /// The certificate's name in plain type beside the details, as Adobe
    /// Acrobat shows it.
    Formal,
    /// Only who signed and when.
    Details,
    /// Nothing on the page.
    Invisible,
}

/// Where a certificate comes from in the dialog.
enum CertItem {
    Found(Box<CertChoice>),
    /// A .p12 / .pfx file (path; read when signing).
    File(PathBuf),
}

pub(super) struct DigitalDialog {
    found_rx: Option<Receiver<(Vec<CertChoice>, Option<String>)>>,
    /// Why no ID card shows up, when no card certificate was found.
    no_card: Option<String>,
    items: Vec<CertItem>,
    selected: Option<usize>,
    look: Look,
    reason: String,
    location: String,
    /// PIN or password, wiped when the dialog closes.
    pin: Zeroizing<String>,
    error: Option<String>,
    target: Option<Target>,
}

/// A signature ready to go once its place (and file) is chosen.
pub(super) struct Pending {
    signer: Box<dyn Signer>,
    req: SignRequest,
    /// Waiting for a click on the page.
    placing: bool,
}

#[derive(Default)]
pub(super) struct DigitalState {
    pub dialog: Option<DigitalDialog>,
    pub pending: Option<Pending>,
    job_rx: Option<Receiver<Result<(PathBuf, String), String>>>,
    /// Checked signatures for the document opened as `.0`.
    pub reports: Option<(u64, Vec<Report>)>,
    verify_rx: Option<(u64, Receiver<Vec<Report>>)>,
    /// A PIN error from the last signing, shown when the dialog reopens.
    retry_error: Option<String>,
}

impl DigitalState {
    pub fn busy(&self) -> bool {
        self.job_rx.is_some()
    }
}

impl App {
    /// Opens the "sign with a certificate" dialog.
    pub(super) fn open_digital(&mut self, target: Option<Target>) {
        if self.digital.busy() {
            return;
        }
        let ctx = self.egui_ctx.clone();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let found = pdfr_sign::signer::discover();
            let any_card = found
                .iter()
                .any(|c| matches!(c.source, pdfr_sign::signer::Source::Token(_)));
            #[cfg(not(target_os = "macos"))]
            let no_card = (!any_card).then(pdfr_sign::signer::diagnose).flatten();
            #[cfg(target_os = "macos")]
            let no_card = {
                let _ = any_card;
                None
            };
            let _ = tx.send((found, no_card));
            ctx.request_repaint();
        });
        let st = &self.assist.a.store.settings;
        let items = st
            .cert_files
            .iter()
            .map(|p| CertItem::File(PathBuf::from(p)))
            .filter(|i| matches!(i, CertItem::File(p) if p.is_file()))
            .collect();
        self.tool = Tool::None;
        self.placing = None;
        self.digital.pending = None;
        self.digital.dialog = Some(DigitalDialog {
            found_rx: Some(rx),
            no_card: None,
            items,
            selected: None,
            look: Look::Formal,
            reason: st.sign_reason.clone(),
            location: st.sign_location.clone(),
            pin: Zeroizing::new(String::new()),
            // After a wrong PIN: the same certificate comes back selected
            // (it was remembered as the last one), with why.
            error: self.digital.retry_error.take(),
            target,
        });
    }

    /// Saved (full) signatures, most recently used first.
    fn saved_signature_inks(&self) -> Vec<Arc<Ink>> {
        let mut saved: Vec<_> = self
            .assist
            .a
            .store
            .signatures
            .iter()
            .filter(|s| !s.initials)
            .collect();
        saved.sort_by_key(|s| std::cmp::Reverse(s.last_used));
        saved
            .iter()
            .filter_map(|s| Ink::decode(&s.ink).map(Arc::new))
            .collect()
    }

    /// A .p12 / .pfx file was picked in the dialog.
    pub(super) fn digital_file_picked(&mut self, path: PathBuf) {
        let Some(dlg) = self.digital.dialog.as_mut() else {
            return;
        };
        let i = dlg
            .items
            .iter()
            .position(|it| matches!(it, CertItem::File(p) if *p == path))
            .unwrap_or_else(|| {
                dlg.items.push(CertItem::File(path));
                dlg.items.len() - 1
            });
        dlg.selected = Some(i);
        dlg.error = None;
    }

    /// The dialog, while open.
    pub(super) fn digital_window(&mut self, ctx: &egui::Context) {
        self.digital_poll();
        let saved = if self.digital.dialog.is_some() {
            self.saved_signature_inks()
        } else {
            // Closed, however: what egui keeps of the PIN field (its
            // text in the undo history) goes too.
            forget_pin(ctx);
            return;
        };
        let pal = self.palette.clone();
        let c = Colors::new(&pal);
        let last_cert = self.assist.a.store.settings.last_cert.clone();
        let now = pdfr_sign::now();
        let mut pick_file = false;
        let mut sign = false;
        let mut to_ink: Option<SignTab> = None;
        let mut cancel = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
        let dlg = self.digital.dialog.as_mut().unwrap();
        if let Some(rx) = &dlg.found_rx
            && let Ok((found, no_card)) = rx.try_recv()
        {
            dlg.found_rx = None;
            dlg.no_card = no_card;
            let mut found: Vec<CertItem> = found
                .into_iter()
                .map(|c| CertItem::Found(Box::new(c)))
                .collect();
            let shift = found.len();
            found.append(&mut dlg.items);
            dlg.items = found;
            if dlg.selected.is_none() {
                dlg.selected = dlg
                    .items
                    .iter()
                    .position(
                        |i| matches!(i, CertItem::Found(c) if c.cert.fingerprint() == last_cert),
                    )
                    .or_else(|| {
                        dlg.items
                            .iter()
                            .position(|i| matches!(i, CertItem::Found(c) if c.problem().is_none()))
                    });
            } else {
                // A file picked meanwhile moved down behind the found ones.
                dlg.selected = dlg.selected.map(|s| s + shift);
            }
        }
        // The Certificate tab of the signing dialog: the same window (same
        // id, so switching tabs doesn't fade it in again) and size.
        egui::Modal::new(Id::unique("pdfr-sign"))
            .area(super::signature::sign_area(ctx))
            .frame(panel_frame(ctx, &pal, &c, 20))
            .show(ctx, |ui| {
                ui.set_width(super::signature::PAD_W);
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                ui.label(RichText::new("Sign").size(20.0).strong().color(c.text));
                let tabs = [
                    (Some(SignTab::Draw), "Draw"),
                    (Some(SignTab::Type), "Type"),
                    (Some(SignTab::Image), "Image"),
                    (None, "Certificate"),
                ];
                if let Some(Some(t)) = segmented(ui, &c, &tabs, None) {
                    to_ink = Some(t);
                }
                ui.label(
                    RichText::new(
                        "A digital signature shows who signed, and that the document has not changed since.",
                    )
                    .color(c.weak),
                );
                ui.label(RichText::new("Certificate").size(12.0).color(c.weak));
                egui::ScrollArea::vertical()
                    .max_height(200.0)
                    .id_salt("pdfr-certs")
                    .show(ui, |ui| {
                        if dlg.items.is_empty() && dlg.found_rx.is_none() {
                            ui.label(RichText::new(NO_CERTS).color(c.weak));
                        }
                        let found: Vec<&CertChoice> = dlg
                            .items
                            .iter()
                            .filter_map(|i| match i {
                                CertItem::Found(c) => Some(&**c),
                                CertItem::File(_) => None,
                            })
                            .collect();
                        for (i, it) in dlg.items.iter().enumerate() {
                            if i > 0 {
                                row_divider(ui, &c);
                            }
                            let (title, sub, problem) = match it {
                                CertItem::Found(ch) => (
                                    if ch.login_only(&found) {
                                        format!("{} (for logging in)", ch.cert.name())
                                    } else {
                                        ch.cert.name()
                                    },
                                    format!(
                                        "Issued by {} · valid until {} · {}",
                                        ch.cert.issuer_name(),
                                        day(ch.cert.not_after()),
                                        ch.source.describe()
                                    ),
                                    ch.cert.sign_problem(now),
                                ),
                                CertItem::File(p) => (
                                    p.file_name()
                                        .map_or("Certificate file".into(), |n| n.to_string_lossy().into_owned()),
                                    format!("Certificate file · {}", p.display()),
                                    None,
                                ),
                            };
                            let sub = match &problem {
                                Some(p) => format!("Can't sign: {p}. {sub}"),
                                None => sub,
                            };
                            let clicked = ui
                                .add_enabled_ui(problem.is_none(), |ui| {
                                    crate::assist_ui::widgets::choice_row(
                                        ui,
                                        &c,
                                        dlg.selected == Some(i),
                                        &title,
                                        &sub,
                                    )
                                })
                                .inner;
                            if clicked && problem.is_none() {
                                dlg.selected = Some(i);
                                dlg.pin.clear();
                                dlg.error = None;
                            }
                        }
                        if dlg.found_rx.is_some() {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(RichText::new("Looking for certificates…").color(c.weak));
                            });
                        }
                    });
                if let Some(why) = &dlg.no_card {
                    ui.label(RichText::new(format!("No ID card or token found. {why}")).size(12.0).color(c.weak));
                    ui.hyperlink_to(
                        RichText::new("How to check your card").size(12.0),
                        "https://pdfr.app/sign-pdf-with-id-card",
                    );
                }
                if secondary_button(ui, &c, "Use a certificate file (.p12, .pfx)…").clicked() {
                    pick_file = true;
                }
                row_divider(ui, &c);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Looks like").color(c.weak));
                    let name = |l: Look| match l {
                        Look::Formal => "My name and the details".to_string(),
                        Look::Saved(0) => "My drawn signature, with the details".to_string(),
                        Look::Saved(i) => format!("Drawn signature {}, with the details", i + 1),
                        Look::Details => "Only the details (who and when)".into(),
                        Look::Invisible => "Invisible (nothing on the page)".into(),
                    };
                    egui::ComboBox::from_id_salt("pdfr-digital-look")
                        .selected_text(name(dlg.look))
                        .width(320.0)
                        .show_ui(ui, |ui| {
                            let mut looks = vec![Look::Formal, Look::Details];
                            looks.extend((0..saved.len()).map(Look::Saved));
                            looks.push(Look::Invisible);
                            for l in looks {
                                if ui.selectable_label(dlg.look == l, name(l)).clicked() {
                                    dlg.look = l;
                                }
                            }
                        });
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Reason").color(c.weak));
                    ui.add(
                        egui::TextEdit::singleline(&mut dlg.reason)
                            .hint_text("Optional, e.g. I agree to the terms")
                            .desired_width(180.0),
                    );
                    ui.label(RichText::new("Place").color(c.weak));
                    ui.add(
                        egui::TextEdit::singleline(&mut dlg.location)
                            .hint_text("Optional")
                            .desired_width(120.0),
                    );
                });
                let needs_secret = match dlg.selected.and_then(|i| dlg.items.get(i)) {
                    Some(CertItem::File(_)) => Some("Password of the file"),
                    Some(CertItem::Found(ch)) if ch.needs_pin && !ch.pin_pad => Some("PIN"),
                    _ => None,
                };
                if let Some(label) = needs_secret {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(label).color(c.weak));
                        let r = ui.add(
                            egui::TextEdit::singleline(&mut *dlg.pin)
                                .id(Id::unique(PIN_FIELD))
                                .password(true)
                                .desired_width(200.0),
                        );
                        if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                            sign = true;
                        }
                    });
                    ui.label(
                        RichText::new("Used for this signature only; pdfr never stores it.")
                            .size(11.0)
                            .color(c.weak),
                    );
                }
                // What the card says about wrong PINs so far, before the
                // next try rather than after the card has locked.
                let tries = match dlg.selected.and_then(|i| dlg.items.get(i)) {
                    Some(CertItem::Found(ch)) if ch.needs_pin => ch.pin_tries,
                    _ => PinTries::Ok,
                };
                let warn = Color32::from_rgb(200, 60, 60);
                match tries {
                    PinTries::Ok => {}
                    PinTries::Low => {
                        ui.label(RichText::new("A wrong PIN was entered before: only a few tries are left.").color(warn));
                    }
                    PinTries::FinalTry => {
                        ui.label(
                            RichText::new("Last try: one more wrong PIN locks the card.")
                                .strong()
                                .color(warn),
                        );
                    }
                    PinTries::Locked => {
                        ui.label(RichText::new(
                            "This card's PIN is locked. Unlock it with your PUK, using the tool from whoever issued the card.",
                        ).color(warn));
                    }
                }
                if let Some(e) = &dlg.error {
                    ui.label(RichText::new(e).color(Color32::from_rgb(200, 60, 60)));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let ok = dlg.selected.is_some() && tries != PinTries::Locked;
                    let label = match (dlg.look, dlg.target.is_some()) {
                        (Look::Invisible, _) | (_, true) => "Sign and save…",
                        _ => "Sign…",
                    };
                    if ui
                        .add_enabled_ui(ok, |ui| primary_button(ui, &c, label))
                        .inner
                        .clicked()
                    {
                        sign = true;
                    }
                    if secondary_button(ui, &c, "Cancel").on_hover_text("Esc").clicked() {
                        cancel = true;
                    }
                });
            });
        if cancel {
            self.digital.dialog = None;
            return;
        }
        if let Some(t) = to_ink {
            let target = self.digital.dialog.take().and_then(|d| d.target);
            self.open_sign(false, target);
            if let Some(d) = self.sign.as_mut() {
                d.show_tab(t);
            }
            return;
        }
        if pick_file {
            self.pick_cert_file();
            return;
        }
        if sign {
            self.digital_prepare(&saved);
        }
    }

    /// Makes the signer and the request; then asks where the signature
    /// goes (or straight away for the file to save).
    fn digital_prepare(&mut self, saved: &[Arc<Ink>]) {
        let Some(dlg) = self.digital.dialog.as_mut() else {
            return;
        };
        let Some(item) = dlg.selected.and_then(|i| dlg.items.get(i)) else {
            return;
        };
        let pin = (!dlg.pin.is_empty()).then(|| Zeroizing::new(dlg.pin.as_str().to_string()));
        let signer: Result<Box<dyn Signer>, String> = match item {
            CertItem::Found(ch) => ch.signer(pin),
            CertItem::File(p) => std::fs::read(p)
                .map_err(|e| format!("could not read the file: {e}"))
                .and_then(|b| {
                    pdfr_sign::signer::p12::open(&b, dlg.pin.as_str())
                        .map(|s| Box::new(s) as Box<dyn Signer>)
                }),
        };
        let signer = match signer {
            Ok(s) => s,
            Err(e) => {
                dlg.error = Some(capitalise(&e));
                return;
            }
        };
        if let Some(p) = signer.cert().sign_problem(pdfr_sign::now()) {
            dlg.error = Some(format!("This certificate can't sign: {p}."));
            return;
        }
        let name = signer.cert().name();
        let ink = match dlg.look {
            Look::Saved(i) => saved.get(i).cloned(),
            Look::Formal | Look::Details | Look::Invisible => None,
        };
        let invisible = dlg.look == Look::Invisible;
        let req = SignRequest {
            spot: None,
            ink,
            name_beside: (dlg.look == Look::Formal).then(|| display_name(&name)),
            signer: name,
            reason: Some(dlg.reason.trim().to_string()).filter(|s| !s.is_empty()),
            location: Some(dlg.location.trim().to_string()).filter(|s| !s.is_empty()),
            time: 0,
            reserve: DEFAULT_RESERVE,
        };
        // Remember the choices (never the PIN or password).
        let st = &mut self.assist.a.store.settings;
        st.last_cert = signer.cert().fingerprint();
        st.sign_reason = dlg.reason.trim().to_string();
        st.sign_location = dlg.location.trim().to_string();
        if let CertItem::File(p) = item {
            let p = p.to_string_lossy().into_owned();
            st.cert_files.retain(|x| *x != p);
            st.cert_files.insert(0, p);
            st.cert_files.truncate(5);
        }
        self.assist.a.save();
        let target = dlg.target;
        self.digital.dialog = None;
        let mut pending = Pending {
            signer,
            req,
            placing: false,
        };
        if invisible {
            self.digital.pending = Some(pending);
            self.ask_signed_file();
            return;
        }
        let preview = Arc::new(preview_ink(&pending.req));
        if let Some(t) = target {
            let rect = if t.on_line {
                t.rect_for(&preview)
            } else {
                t.rect
            };
            pending.req.spot = Some((t.page, rect));
            self.digital.pending = Some(pending);
            self.ask_signed_file();
            return;
        }
        pending.placing = true;
        self.digital.pending = Some(pending);
        self.placing_initials = false;
        self.placing = Some(preview);
        self.tool = Tool::PlaceSignature;
        self.status =
            "Click where the signature goes, or on a \"Sign here\" box. Esc cancels.".into();
    }

    /// Called by the place tool: whether the click placed a digital
    /// signature (and the file dialog is next).
    pub(super) fn place_digital(&mut self, page: usize, rect: PRect) -> bool {
        let Some(p) = self.digital.pending.as_mut().filter(|p| p.placing) else {
            return false;
        };
        p.placing = false;
        p.req.spot = Some((page, rect));
        self.tool = Tool::None;
        self.placing = None;
        self.ask_signed_file();
        true
    }

    /// Asks where the signed copy goes.
    fn ask_signed_file(&mut self) {
        let Some(doc) = &self.doc else { return };
        if self.dialog_open() {
            self.digital.pending = None;
            return;
        }
        let stem = doc
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "document".into());
        let name = format!("{stem} (signed).pdf");
        // The document's id, not its render generation: switching pages to
        // dark mode while the dialog is open must not cancel the signing.
        let generation = doc.id;
        if !super::files::native_dialogs() {
            let mut d = super::files::builtin::dialog(doc.path.parent(), Some(&name));
            d.save_file();
            self.builtin_dialog = Some((d, super::files::builtin::Purpose::SignedSave(generation)));
            return;
        }
        let mut dlg = self
            .file_dialog()
            .add_filter("PDF", &["pdf"])
            .set_file_name(name);
        if let Some(d) = doc.path.parent() {
            dlg = dlg.set_directory(d);
        }
        let task = dlg.save_file();
        self.status = "Choose where to save the signed copy.".into();
        self.wait_for_dialog(async move {
            super::files::DialogResult::SignedSave(
                task.await.map(|h| h.path().to_path_buf()),
                generation,
            )
        });
    }

    /// The file for the signed copy was chosen (or not): sign and save on
    /// a background thread.
    pub(super) fn digital_save(&mut self, target: Option<PathBuf>, generation: u64) {
        // Set going by an agent: the user says yes first.
        if let Some(t) = &target
            && self.agent_hold_signing(t, generation)
        {
            return;
        }
        let Some(mut pending) = self.digital.pending.take() else {
            return;
        };
        let Some(target) = target else {
            self.status = "Not signed.".into();
            return;
        };
        let Some(doc) = self.doc.as_ref().filter(|d| d.id == generation) else {
            self.status = "Not signed: another document was opened meanwhile.".into();
            return;
        };
        let form = doc.form.clone();
        let fields = doc.fields();
        let marks = doc.mark_list();
        let st = &self.assist.a.store.settings;
        let tsa = st.sign_timestamp.then(|| {
            if st.tsa_url.trim().is_empty() {
                pdfr_sign::tsa::DEFAULT_TSA.to_string()
            } else {
                st.tsa_url.trim().to_string()
            }
        });
        let ltv = st.check_revocation;
        let ctx = self.egui_ctx.clone();
        let (tx, rx) = channel();
        self.status = "Signing…".into();
        std::thread::spawn(move || {
            pending.req.time = pdfr_sign::now();
            let res = sign_job(
                &form,
                &fields,
                &marks,
                &pending,
                tsa.as_deref(),
                ltv,
                &target,
            );
            let _ = tx.send(res.map(|note| (target, note)));
            ctx.request_repaint();
        });
        self.digital.job_rx = Some(rx);
    }

    /// Takes in signing results and signature checks; starts checking an
    /// opened file's signatures.
    fn digital_poll(&mut self) {
        // A placement abandoned (Esc, another tool) drops the signature.
        if self
            .digital
            .pending
            .as_ref()
            .is_some_and(|p| p.placing && self.tool != Tool::PlaceSignature)
        {
            self.digital.pending = None;
        }
        if let Some(rx) = &self.digital.job_rx
            && let Ok(res) = rx.try_recv()
        {
            self.digital.job_rx = None;
            match res {
                Ok((path, note)) => {
                    self.open(path.clone());
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    self.status = format!("Signed and saved as {name}.{note}");
                }
                Err(e) if pdfr_sign::signer::pkcs11::is_pin_error(&e) => {
                    // Back to the dialog, same certificate, the PIN asked
                    // again by the person — never retried by pdfr.
                    self.digital.retry_error = Some(format!("Not signed: {}", capitalise(&e)));
                    self.status = "Not signed: the PIN was not accepted.".into();
                    self.open_digital(None);
                }
                Err(e) => self.status = format!("Not signed: {e}"),
            }
        }
        let Some(doc) = &self.doc else { return };
        let g = doc.generation;
        if let Some((vg, rx)) = &self.digital.verify_rx
            && let Ok(reports) = rx.try_recv()
        {
            if *vg == g {
                self.status = summary(&reports);
                self.side_tab = super::sidebar::SideTab::Signatures;
                self.digital.reports = Some((g, reports));
            }
            self.digital.verify_rx = None;
        }
        let checked = self
            .digital
            .reports
            .as_ref()
            .is_some_and(|(rg, _)| *rg == g);
        let running = self
            .digital
            .verify_rx
            .as_ref()
            .is_some_and(|(vg, _)| *vg == g);
        if doc.form.digital.is_empty() || checked || running {
            return;
        }
        let form = doc.form.clone();
        let online = self.assist.a.store.settings.check_revocation;
        let ctx = self.egui_ctx.clone();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let net = NetFetch::new();
            let online = online.then_some(Online {
                fetch: &net,
                cache: revocation_cache(),
            });
            let r = pdfr_sign::verify::verify_all_with(&form.bytes, &form.digital, trust(), online);
            let _ = tx.send(r);
            ctx.request_repaint();
        });
        self.digital.verify_rx = Some((g, rx));
    }

    /// The checked signatures of the open document, if checked.
    pub(super) fn signature_reports(&self) -> Option<&[Report]> {
        let g = self.doc.as_ref()?.generation;
        self.digital
            .reports
            .as_ref()
            .filter(|(rg, _)| *rg == g)
            .map(|(_, r)| r.as_slice())
    }

    /// The sidebar's Signatures tab: one card per signature, in the style
    /// of pdfr's dialogs.
    pub(super) fn signatures_panel(&mut self, ui: &mut egui::Ui, c: &Colors) {
        let Some(reports) = self.signature_reports().map(<[Report]>::to_vec) else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(
                    RichText::new("Checking signatures…")
                        .size(12.0)
                        .color(c.weak),
                );
            });
            return;
        };
        let mut go_to = None;
        egui::ScrollArea::vertical()
            .id_salt("pdfr-sig-list")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 10.0;
                for r in &reports {
                    if signature_card(ui, c, r) {
                        go_to = r.page;
                    }
                }
            });
        if let Some(p) = go_to {
            self.scroll_to_page(p);
        }
    }
}

/// One signature as a card: who signed and the verdict on top, then the
/// details as small label/value pairs. Returns true when "Show on page" is
/// clicked.
fn signature_card(ui: &mut egui::Ui, c: &Colors, r: &Report) -> bool {
    let (mark, color, verdict) = verdict(r);
    let mut clicked = false;
    egui::Frame::new()
        .fill(c.card)
        .stroke(egui::Stroke::new(1.0, c.border))
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                status_badge(ui, mark, color);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.add(
                        egui::Label::new(RichText::new(&r.signer).strong().color(c.text)).wrap(),
                    );
                    ui.add(egui::Label::new(RichText::new(verdict).size(12.0).color(color)).wrap());
                    if let Some(p) = &r.problem {
                        ui.add(
                            egui::Label::new(RichText::new(capitalise(p)).size(12.0).color(color))
                                .wrap(),
                        );
                    }
                });
            });
            row_divider(ui, c);
            ui.spacing_mut().item_spacing.y = 8.0;

            let date = pdfr_core::digital::display_date;
            if let Some(t) = r.timestamp {
                detail(ui, c, "Timestamped", &date(t), None);
            } else if let Some(t) = r.claimed_time {
                detail(
                    ui,
                    c,
                    "Signed",
                    &date(t),
                    Some("Time from the signer's computer"),
                );
            }
            if let Some(p) = &r.timestamp_problem {
                detail(ui, c, "Timestamp", &format!("Not valid: {p}"), None);
            }
            if let Some(iss) = &r.issuer {
                let trust = match r.anchor {
                    Some(pdfr_sign::trust::Anchor::EuQualified) => Some("On the EU Trusted List"),
                    Some(pdfr_sign::trust::Anchor::System) => Some("Trusted by your system"),
                    None => None,
                };
                detail(ui, c, "Certificate", iss, trust);
            }
            if let Some(x) = &r.reason {
                detail(ui, c, "Reason", x, None);
            }
            if let Some(x) = &r.location {
                detail(ui, c, "Place", x, None);
            }
            if let After::Changes { what, .. } = &r.after {
                detail(ui, c, "Since signed", &what.join(", "), None);
            }
            let revocation = r
                .revocation
                .as_ref()
                .map_or("Revocation not checked".into(), |rev| {
                    capitalise(&revocation_line(r, rev))
                });
            detail(
                ui,
                c,
                "Format",
                &format!("{} · {}", r.kind, r.digest),
                Some(&revocation),
            );
            if let Some(p) = r.page {
                ui.add_space(2.0);
                clicked =
                    crate::assist_ui::widgets::link(ui, c, &format!("Show on page {}", p + 1))
                        .clicked();
            }
        });
    clicked
}

/// A round badge with the verdict's mark.
fn status_badge(ui: &mut egui::Ui, mark: &str, color: Color32) {
    let (r, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
    let p = ui.painter();
    p.circle_filled(r.center(), 11.0, color.gamma_multiply(0.2));
    let s = egui::Stroke::new(1.8, color);
    let pt = |x: f32, y: f32| r.center() + vec2(x, y);
    match mark {
        "✔" => {
            p.line_segment([pt(-4.5, 0.2), pt(-1.3, 3.4)], s);
            p.line_segment([pt(-1.3, 3.4), pt(4.6, -3.6)], s);
        }
        "✖" => {
            p.line_segment([pt(-3.8, -3.8), pt(3.8, 3.8)], s);
            p.line_segment([pt(-3.8, 3.8), pt(3.8, -3.8)], s);
        }
        _ => {
            p.text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                if mark == "?" { "?" } else { "!" },
                egui::FontId::proportional(14.0),
                color,
            );
        }
    }
}

/// A small caption over a value, with an optional note under it.
fn detail(ui: &mut egui::Ui, c: &Colors, label: &str, value: &str, sub: Option<&str>) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        ui.label(
            RichText::new(label.to_uppercase())
                .size(10.0)
                .strong()
                .color(c.weak),
        );
        ui.add(egui::Label::new(RichText::new(value).size(12.5).color(c.text)).wrap());
        if let Some(s) = sub {
            ui.add(egui::Label::new(RichText::new(s).size(11.5).color(c.weak)).wrap());
        }
    });
}

/// Signs, checks the result, writes it next to `target` and renames it
/// into place. Returns a note for the status line.
fn sign_job(
    form: &FormDoc,
    fields: &[Field],
    marks: &[pdfr_core::markup::Markup],
    p: &Pending,
    tsa: Option<&str>,
    ltv: bool,
    target: &std::path::Path,
) -> Result<String, String> {
    let post = |body: &[u8]| -> Result<Vec<u8>, String> {
        let url = tsa.unwrap_or_default();
        let resp = crate::updates::agent()
            .post(url)
            .header("Content-Type", "application/timestamp-query")
            .send(body)
            .map_err(|e| format!("could not reach the timestamp server ({e})"))?;
        let mut body = resp.into_body();
        body.with_config()
            .limit(256 * 1024)
            .read_to_vec()
            .map_err(|e| e.to_string())
    };
    let ts = |d: &[u8]| pdfr_sign::tsa::timestamp(d, &post);
    let mut ts_error = None;
    let bytes = pdfr_core::digital::sign_with(form, fields, marks, &p.req, |content| {
        let s = pdfr_sign::cades::sign(
            content,
            p.signer.as_ref(),
            tsa.is_some()
                .then_some(&ts as pdfr_sign::cades::Timestamper),
        )?;
        ts_error = s.timestamp_error;
        Ok(s.der)
    })
    .map_err(|e| e.to_string())?;
    // Long-term validation: only on top of a timestamp, and only if
    // asking issuers is allowed.
    let (bytes, lt) = if ltv && tsa.is_some() && ts_error.is_none() {
        let net = NetFetch::new();
        let fresh = Cache::default();
        let online = Online {
            fetch: &net,
            cache: &fresh,
        };
        let x = pdfr_sign::ltv::extend(
            bytes,
            trust(),
            online,
            Some(&ts as pdfr_sign::cades::Timestamper),
        );
        (x.bytes, Some((x.level, x.problem)))
    } else {
        (bytes, None)
    };
    // Check the file before it is written: it must open, and the new
    // signature must cover it and match.
    let back = FormDoc::open(bytes.clone())
        .map_err(|e| format!("the signed file did not open ({e:?})"))?;
    let reports = pdfr_sign::verify::verify_all(&bytes, &back.digital, trust());
    let mine = reports
        .iter()
        .rfind(|r| !r.doc_timestamp)
        .ok_or("the signature was not found in the signed file")?;
    let stamp_broken = reports
        .iter()
        .any(|r| r.doc_timestamp && matches!(r.status, Status::Broken | Status::Modified));
    if matches!(mine.status, Status::Broken | Status::Modified)
        || mine.after != After::Nothing
        || stamp_broken
    {
        return Err(format!(
            "the signed file did not pass its check ({})",
            mine.problem.clone().unwrap_or_default()
        ));
    }
    let tmp = target.with_extension("pdf.pdfr-tmp");
    std::fs::write(&tmp, &bytes)
        .and_then(|_| std::fs::rename(&tmp, target))
        .map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("could not save: {e}")
        })?;
    Ok(match (tsa, ts_error) {
        (None, _) => String::new(),
        (Some(_), Some(e)) => format!(" Without a timestamp: {e}."),
        (Some(_), None) => ltv_note(lt),
    })
}

/// The status-line note about a timestamped signature's level.
fn ltv_note(lt: Option<(Level, Option<String>)>) -> String {
    match lt {
        None => " Timestamped.".into(),
        Some((level @ (Level::Lt | Level::Lta), None)) => format!(
            " Timestamped, with proof it can be checked for years ({}).",
            level.name()
        ),
        Some((level @ Level::Lt, Some(why))) => format!(
            " Timestamped, with proof it can be checked for years ({}); {why}.",
            level.name()
        ),
        Some((level, why)) => format!(
            " Timestamped ({}); the long-term proof could not be added: {}.",
            level.name(),
            why.unwrap_or_default()
        ),
    }
}

/// What the page shows while placing: the real appearance as a picture.
fn preview_ink(req: &SignRequest) -> Ink {
    let aspect = pdfr_core::digital::appearance_aspect(req);
    let h = 120u32;
    let w = (h as f32 * aspect).round() as u32;
    let mut r = req.clone();
    r.time = pdfr_sign::now();
    let img = pdfr_core::digital::preview(&r, w, h);
    Ink::from_image(pdfr_core::signature::SigImage {
        width: img.width,
        height: img.height,
        rgba: img.rgba,
    })
}

/// "KJELLBERG RASMUS HARALD - Y7445098N" -> "Kjellberg Rasmus Harald": the
/// name part of a certificate name, in normal case, for the appearance.
fn display_name(cn: &str) -> String {
    let name = cn.split(" - ").next().unwrap_or(cn).trim();
    if name.chars().any(|c| c.is_lowercase()) {
        return name.to_string();
    }
    name.split_whitespace()
        .map(|w| {
            let mut cs = w.chars();
            cs.next()
                .map(|f| {
                    f.to_uppercase()
                        .chain(cs.flat_map(char::to_lowercase))
                        .collect()
                })
                .unwrap_or_default()
        })
        .collect::<Vec<String>>()
        .join(" ")
}

fn capitalise(s: &str) -> String {
    let mut cs = s.chars();
    match cs.next() {
        Some(f) => f.to_uppercase().chain(cs).collect(),
        None => String::new(),
    }
}

fn day(t: u64) -> String {
    pdfr_core::digital::display_date(t)
        .split(' ')
        .next()
        .unwrap_or_default()
        .to_string()
}

fn verdict(r: &Report) -> (&'static str, Color32, String) {
    let green = Color32::from_rgb(40, 150, 80);
    let amber = Color32::from_rgb(200, 140, 30);
    let red = Color32::from_rgb(200, 60, 60);
    match r.status {
        Status::Valid if r.doc_timestamp => (
            "✔",
            green,
            "Valid timestamp: proves the document existed, as it is, at this time.".into(),
        ),
        Status::Valid => match &r.after {
            After::Nothing => (
                "✔",
                green,
                "Valid: the document is as it was signed.".into(),
            ),
            After::Changes { .. } => (
                "✔",
                green,
                "Valid: the signed version is intact; allowed changes came after.".into(),
            ),
        },
        Status::ValidUnchecked => ("!", amber, "Valid, revocation could not be checked.".into()),
        Status::Untrusted => (
            "?",
            amber,
            "Intact, but the signer's identity could not be confirmed.".into(),
        ),
        Status::Modified => ("⚠", amber, "Changed after signing.".into()),
        Status::Broken => ("✖", red, "Not valid.".into()),
    }
}

/// The card's words for what is known about revocation.
fn revocation_line(r: &Report, rev: &Revocation) -> String {
    let date = pdfr_core::digital::display_date;
    match &rev.state {
        State::Good => {
            let how = match rev.source {
                Some(Source::Online) => "checked online",
                _ => "from the file",
            };
            match rev.at {
                Some(t) => format!("not revoked ({how}, {})", date(t)),
                None => format!("not revoked ({how})"),
            }
        }
        State::Revoked { at, reason } => {
            let who = if rev.ca.is_some() { "issuer " } else { "" };
            let why = reason
                .as_ref()
                .map(|x| format!(" ({x})"))
                .unwrap_or_default();
            let after = match r.timestamp {
                Some(ts) if *at > ts => ", after it was timestamped",
                _ => "",
            };
            format!("{who}revoked {}{why}{after}", date(*at))
        }
        State::Unknown(why) => format!("revocation not checked: {why}"),
    }
}

/// One line for the status bar about the document's signatures.
fn summary(reports: &[Report]) -> String {
    let n = reports.len();
    let worst = reports
        .iter()
        .map(|r| match r.status {
            Status::Broken => 4,
            Status::Modified => 3,
            Status::Untrusted => 2,
            Status::ValidUnchecked => 1,
            Status::Valid => 0,
        })
        .max()
        .unwrap_or(0);
    let people: Vec<&Report> = reports.iter().filter(|r| !r.doc_timestamp).collect();
    let n = if people.is_empty() { n } else { people.len() };
    let who = people
        .first()
        .or(reports.first().as_ref())
        .map(|r| r.signer.clone())
        .unwrap_or_default();
    let what = if n == 1 {
        format!("Digitally signed by {who}")
    } else {
        format!("{n} digital signatures")
    };
    let verdict = match worst {
        0 => "valid",
        1 => "valid, but revocation could not be checked",
        2 => "intact, but a signer could not be confirmed",
        3 => "changed after signing",
        _ => "NOT valid",
    };
    format!("{what}: {verdict}. The sidebar (Ctrl+B) has the details.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_for_handwriting() {
        assert_eq!(
            display_name("KJELLBERG RASMUS HARALD - Y7445098N"),
            "Kjellberg Rasmus Harald"
        );
        assert_eq!(display_name("Anna Ek"), "Anna Ek");
        assert_eq!(display_name("ÅSA ÖBERG"), "Åsa Öberg");
    }

    fn report(timestamp: Option<u64>) -> Report {
        Report {
            field: "Signature1".into(),
            page: None,
            status: Status::Valid,
            signer: "Anna Ek".into(),
            cert: None,
            issuer: None,
            claimed_time: None,
            timestamp,
            timestamp_problem: None,
            anchor: None,
            problem: None,
            after: After::Nothing,
            kind: "PAdES B-LT".into(),
            digest: "SHA-256".into(),
            reason: None,
            location: None,
            revocation: None,
            doc_timestamp: false,
        }
    }

    #[test]
    fn revocation_words() {
        let t = 1_795_182_310; // 2026-11-20 13:45 UTC
        let good = |source| Revocation {
            state: State::Good,
            source: Some(source),
            at: Some(t),
            ca: None,
        };
        assert_eq!(
            revocation_line(&report(None), &good(Source::File)),
            "not revoked (from the file, 2026-11-20 13:45 UTC)"
        );
        assert_eq!(
            revocation_line(&report(None), &good(Source::Online)),
            "not revoked (checked online, 2026-11-20 13:45 UTC)"
        );
        let revoked = Revocation {
            state: State::Revoked {
                at: t,
                reason: Some("the key was compromised".into()),
            },
            source: Some(Source::Online),
            at: Some(t),
            ca: None,
        };
        assert_eq!(
            revocation_line(&report(Some(t - 10)), &revoked),
            "revoked 2026-11-20 13:45 UTC (the key was compromised), after it was timestamped"
        );
        assert_eq!(
            revocation_line(&report(None), &Revocation::unknown("offline")),
            "revocation not checked: offline"
        );
    }

    #[test]
    fn status_notes_for_long_term_validation() {
        assert_eq!(ltv_note(None), " Timestamped.");
        assert!(ltv_note(Some((Level::Lta, None))).contains("PAdES B-LTA"));
        let n = ltv_note(Some((Level::T, Some("offline".into()))));
        assert!(n.contains("PAdES B-T") && n.contains("offline"), "{n}");
    }

    #[test]
    fn unchecked_revocation_is_amber() {
        let mut r = report(None);
        r.status = Status::ValidUnchecked;
        let (mark, color, words) = verdict(&r);
        assert_eq!(mark, "!");
        assert_eq!(words, "Valid, revocation could not be checked.");
        assert_ne!(color, verdict(&report(None)).1, "not the green of valid");
        let s = summary(&[report(None), r]);
        assert!(
            s.contains("valid, but revocation could not be checked"),
            "{s}"
        );
    }

    #[test]
    fn issuers_on_this_computer_are_not_asked() {
        for url in [
            "http://localhost/ocsp",
            "http://127.0.0.1:8080/",
            "http://[::1]/",
            "http://192.168.0.10/crl",
        ] {
            assert!(public_address(url).is_err(), "{url}");
        }
    }

    #[test]
    fn document_timestamps_are_not_counted_as_signers() {
        let mut stamp = report(Some(1));
        stamp.doc_timestamp = true;
        stamp.signer = "Some TSA".into();
        let s = summary(&[report(None), stamp]);
        assert!(s.starts_with("Digitally signed by Anna Ek: valid"), "{s}");
    }
}
