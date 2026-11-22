//! The autocomplete interface: ghost text, the alternatives list, "Fill all",
//! the AI consent prompt and the settings window. All AI work runs on a
//! background thread; nothing here blocks typing, scrolling or saving.

mod remember;
mod settings;
mod system_ui;
pub(crate) mod widgets;

use crate::app::Doc;
use eframe::egui::{self, Id, Rect};
use pdfr_assist::memory::Remember;
use pdfr_assist::{AiOutcome, Analysis, Assistant, LearnPlan, PeopleRequest};
use pdfr_core::field::Field;
use std::collections::HashSet;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

struct List {
    field: u64,
    all: Vec<String>,
    items: Vec<String>,
    sel: usize,
}

/// What an AI call would be about, waiting for consent.
enum AiAsk {
    Labels(Vec<String>),
    People(PeopleRequest),
}

impl AiAsk {
    fn len(&self) -> usize {
        match self {
            AiAsk::Labels(l) => l.len(),
            AiAsk::People(r) => r.sigs.len(),
        }
    }
}

pub struct AssistState {
    pub a: Assistant,
    ctx: egui::Context,
    an: Analysis,
    an_ids: Vec<u64>,
    an_at: Option<Instant>,
    stale: bool,
    ai_rx: Option<Receiver<AiOutcome>>,
    attempted: HashSet<String>,
    consent: Option<AiAsk>,
    /// Forms whose people grouping was already asked for this session.
    people_attempted: HashSet<String>,
    /// Shown after saving: what to remember, chosen by the user.
    pending_learn: Option<(LearnPlan, Analysis, Vec<Field>)>,
    dont_ask_again: bool,
    /// Automated runs remember everything without asking.
    auto_remember: bool,
    /// A save made by an agent: "ask what to remember" neither asks nor
    /// remembers (the user is not the one saving).
    pub quiet_save: bool,
    ai_done: bool,
    declined_this_session: bool,
    pub settings_open: bool,
    pub settings_ui: settings::SettingsUi,
    /// Whether the hovered field has suggestions: (field, when checked, answer).
    hover_suggestions: Option<(u64, Instant, bool)>,
    list: Option<List>,
    no_ai: bool,
    last_ai: Option<String>,
    sys: system_ui::SystemUi,
    /// Where the text reader's models are kept (Settings shows whether
    /// they are there).
    ocr_dir: Option<std::path::PathBuf>,
}

impl AssistState {
    pub fn new(
        ctx: egui::Context,
        no_ai: bool,
        data_dir: Option<std::path::PathBuf>,
        auto_remember: bool,
    ) -> AssistState {
        let ocr_dir = pdfr_license::data_dir(data_dir.as_deref()).map(|d| pdfr_ocr::model_dir(&d));
        let a = match data_dir {
            Some(d) => Assistant::open_at(Some(d.join("memory.json"))),
            None => Assistant::open_default(),
        };
        AssistState {
            a,
            ctx,
            an: Analysis::default(),
            an_ids: vec![],
            an_at: None,
            stale: true,
            ai_rx: None,
            attempted: HashSet::new(),
            consent: None,
            people_attempted: HashSet::new(),
            pending_learn: None,
            dont_ask_again: false,
            auto_remember,
            quiet_save: false,
            ai_done: false,
            declined_this_session: false,
            settings_open: false,
            settings_ui: settings::SettingsUi::default(),
            hover_suggestions: None,
            list: None,
            no_ai,
            last_ai: None,
            sys: Default::default(),
            ocr_dir,
        }
    }

    /// Re-labels the document if its fields changed.
    fn ensure(&mut self, doc: &mut Doc, force: bool) {
        let ids: Vec<u64> = doc.entries.iter().map(|e| e.id).collect();
        if !force && !self.stale && ids == self.an_ids {
            return;
        }
        let fields = doc.fields();
        self.an = self.a.analyse(&fields, &doc.text, &doc.form.pages);
        self.an_ids = ids;
        self.stale = false;
        self.an_at = Some(Instant::now());
        self.hover_suggestions = None;
        for (e, inf) in doc.entries.iter_mut().zip(&self.an.info) {
            e.f.label = inf.label.clone();
        }
    }

    /// Answers that look wrong and empty boxes: (entry index, kind, message).
    pub fn check_doc(
        &mut self,
        doc: &mut Doc,
    ) -> Vec<(usize, pdfr_assist::check::Severity, String)> {
        self.ensure(doc, false);
        pdfr_assist::check::check(&self.an, &doc.fields())
            .into_iter()
            .map(|f| (f.field, f.severity, f.message))
            .collect()
    }

    /// A request to explain the form, when AI may be used.
    pub fn explain_job(
        &mut self,
        doc: &mut Doc,
    ) -> Option<(std::sync::Arc<dyn pdfr_assist::ai::Backend>, String)> {
        self.ensure(doc, false);
        self.a.explain_job(&self.an, &doc.fields())
    }

    /// Position of entry `id` in the analysis, only while the analysis
    /// matches the document's fields exactly (same ids, same order).
    fn index(&self, doc: &Doc, id: u64) -> Option<usize> {
        let same = self.an_ids.len() == doc.entries.len()
            && self
                .an_ids
                .iter()
                .zip(&doc.entries)
                .all(|(a, e)| *a == e.id);
        same.then(|| self.an_ids.iter().position(|&x| x == id))
            .flatten()
    }

    pub fn on_open(&mut self, doc: &mut Doc) {
        self.list = None;
        self.attempted.clear();
        self.consent = None;
        self.ensure(doc, true);
    }

    pub fn on_detected(&mut self, doc: &mut Doc) {
        // Re-labelling is cheap but not free: at most every 250 ms while
        // detection streams in, and once more when it finishes.
        let done = doc.detected.iter().all(|d| *d);
        let due = self
            .an_at
            .is_none_or(|t| t.elapsed() > Duration::from_millis(250));
        if done || due {
            self.ensure(doc, true);
        } else {
            self.stale = true;
        }
        if done {
            self.maybe_ask_ai(doc);
        }
    }

    fn ai_off(&self) -> bool {
        self.no_ai || !self.a.store.settings.ai_enabled
    }

    /// One call per document, only for labels nothing cheaper resolved;
    /// then, for forms about several people, one call to group the fields
    /// into people (labels and section headings only).
    fn maybe_ask_ai(&mut self, doc: &Doc) {
        if self.ai_off() || self.ai_rx.is_some() || !doc.form.editable() {
            return;
        }
        let labels: Vec<String> = self
            .a
            .pending_labels(&self.an)
            .into_iter()
            .filter(|l| !self.attempted.contains(&pdfr_assist::meaning::normalise(l)))
            .collect();
        let ask = if !labels.is_empty() {
            AiAsk::Labels(labels)
        } else {
            let fields = doc.fields();
            match self.a.people_request(&self.an, &fields) {
                Some(r) if !self.people_attempted.contains(&r.fingerprint) => AiAsk::People(r),
                _ => return,
            }
        };
        if self.a.needs_consent() {
            if !self.declined_this_session {
                self.consent = Some(ask);
            }
            return;
        }
        self.start_ai(ask);
    }

    fn start_ai(&mut self, ask: AiAsk) {
        let n = ask.len();
        let (job, what) = match ask {
            AiAsk::Labels(labels) => {
                let Some(job) = self.a.make_job(labels.clone()) else {
                    return;
                };
                for l in &labels {
                    self.attempted.insert(pdfr_assist::meaning::normalise(l));
                }
                (
                    job,
                    format!(
                        "Asking AI about {n} label{}…",
                        if n == 1 { "" } else { "s" }
                    ),
                )
            }
            AiAsk::People(r) => {
                self.people_attempted.insert(r.fingerprint.clone());
                let Some(job) = self.a.make_people_job(r) else {
                    return;
                };
                (
                    job,
                    "Asking AI which fields belong to which person…".to_string(),
                )
            }
        };
        let (tx, rx) = channel();
        let ctx = self.ctx.clone();
        std::thread::Builder::new()
            .name("ai".into())
            .spawn(move || {
                let out = job.run();
                let _ = tx.send(out);
                ctx.request_repaint();
            })
            .ok();
        self.ai_rx = Some(rx);
        self.last_ai = Some(what);
    }

    /// Picks up AI results. Returns true if suggestions may have changed.
    pub fn poll(&mut self, doc: Option<&mut Doc>) -> bool {
        let mut changed = false;
        if let Some(rx) = &self.ai_rx
            && let Ok(out) = rx.try_recv()
        {
            self.ai_rx = None;
            self.a.apply(&out);
            let (i, o): (u64, u64) = out.usage.iter().fold((0, 0), |a, u| (a.0 + u.1, a.1 + u.2));
            self.last_ai = Some(match &out.error {
                Some(e) if out.answers.is_empty() && out.people.is_none() => {
                    format!("AI unavailable ({e}); using memory and dictionary.")
                }
                _ if out.people.is_some() => {
                    format!("AI grouped the fields by person ({i} in / {o} out tokens).")
                }
                _ => format!(
                    "AI resolved {} label{} ({i} in / {o} out tokens).",
                    out.answers.len(),
                    if out.answers.len() == 1 { "" } else { "s" }
                ),
            });
            self.stale = true;
            self.ai_done = true;
            changed = true;
        }
        if let Some(doc) = doc {
            let ids_changed = self.an_ids.len() != doc.entries.len()
                || self
                    .an_ids
                    .iter()
                    .zip(&doc.entries)
                    .any(|(a, e)| *a != e.id);
            if ids_changed {
                self.stale = true;
            }
            if self.ai_done {
                // New meanings may reveal several people: maybe group them.
                self.ensure(doc, true);
                self.ai_done = false;
                self.maybe_ask_ai(doc);
            } else if self.stale
                && self
                    .an_at
                    .is_none_or(|t| t.elapsed() > Duration::from_millis(250))
            {
                self.ensure(doc, true);
                changed = true;
            }
        }
        changed
    }

    pub fn idle(&self) -> bool {
        self.ai_rx.is_none()
    }

    pub fn status_text(&self) -> Option<String> {
        self.last_ai.clone()
    }

    /// The confident suggestion for entry `i`, for ghost text.
    pub fn best(&self, doc: &Doc, i: usize) -> Option<String> {
        let id = doc.entries.get(i)?.id;
        let k = self.index(doc, id)?;
        let fields = doc.fields();
        self.a.confident(&self.an, &fields, k)
    }

    /// For a box that asks for a date: which part of it (day, month, year)
    /// or `None` for the whole date. Not a date box: `Err`.
    pub fn date_part(&self, doc: &Doc, i: usize) -> Result<Option<pdfr_assist::meaning::Part>, ()> {
        let id = doc.entries.get(i).ok_or(())?.id;
        let k = self.index(doc, id).ok_or(())?;
        match self.an.meanings.get(k).cloned().flatten() {
            Some(m) if m.is_date() => Ok(m.part),
            _ => Err(()),
        }
    }

    /// After saving: remember everything, nothing, or ask what to remember.
    pub fn on_save(&mut self, doc: &mut Doc) {
        self.ensure(doc, false);
        let fields = doc.fields();
        let skip: Vec<bool> = doc.entries.iter().map(|e| e.no_memory).collect();
        let plan = self.a.plan_learning(&self.an, &fields, &skip);
        if plan.is_empty() {
            return;
        }
        match self.a.store.settings.remember {
            Remember::Never => {}
            Remember::Always => self.a.commit(&self.an, &fields, &plan),
            Remember::Ask if self.auto_remember => self.a.commit(&self.an, &fields, &plan),
            Remember::Ask if self.quiet_save => {}
            Remember::Ask => {
                self.dont_ask_again = false;
                self.pending_learn = Some((plan, self.an.clone(), fields));
            }
        }
    }

    /// Hidden --ask-remember: shows the remember dialog as after a save
    /// (screenshots), whatever the setting.
    pub fn show_remember_dialog(&mut self, doc: &mut Doc) {
        let (auto, setting) = (self.auto_remember, self.a.store.settings.remember);
        self.auto_remember = false;
        self.a.store.settings.remember = Remember::Ask;
        self.on_save(doc);
        self.auto_remember = auto;
        self.a.store.settings.remember = setting;
    }

    /// The question whether AI may be used is showing.
    pub fn consent_open(&self) -> bool {
        self.consent.is_some()
    }

    /// Tests: shows the question whether AI may be used.
    #[cfg(test)]
    pub fn ask_consent(&mut self, labels: &[&str]) {
        self.consent = Some(AiAsk::Labels(
            labels.iter().map(|l| l.to_string()).collect(),
        ));
    }

    /// A dialog of ours is open (it takes the keyboard).
    pub fn modal_open(&self) -> bool {
        self.consent.is_some() || self.pending_learn.is_some()
    }

    /// Fields were added, removed or restored: re-analyse.
    pub fn invalidate(&mut self) {
        self.stale = true;
    }

    pub fn remember_pending(&self) -> bool {
        self.pending_learn.is_some()
    }

    // ----------------------------------------------------------- alternatives list

    pub fn list_open(&self) -> bool {
        self.list.is_some()
    }

    pub fn close_list(&mut self) {
        self.list = None;
    }

    /// Whether field `fid` has anything to suggest (for its hover button).
    /// Checked at most twice a second per field: it runs while hovering.
    pub fn has_suggestions(&mut self, doc: &Doc, fid: u64) -> bool {
        if let Some((id, at, ans)) = self.hover_suggestions
            && id == fid
            && at.elapsed() < Duration::from_millis(500)
        {
            return ans;
        }
        // Not analysed yet: ask again next frame rather than remember "no".
        let Some(k) = self.index(doc, fid) else {
            return false;
        };
        let ans = !self.a.suggestions(&self.an, &doc.fields(), k).is_empty();
        self.hover_suggestions = Some((fid, Instant::now(), ans));
        ans
    }

    /// What is remembered for field `fid`, best first (for agents).
    pub fn suggestions_for(&mut self, doc: &mut Doc, fid: u64) -> Vec<String> {
        self.ensure(doc, false);
        match self.index(doc, fid) {
            Some(k) => self.a.suggestions(&self.an, &doc.fields(), k),
            None => vec![],
        }
    }

    /// The highlighted alternative of field `fid`'s open list (previewed in
    /// the field while browsing with Up/Down).
    pub fn list_choice(&self, fid: u64) -> Option<&str> {
        let l = self.list.as_ref().filter(|l| l.field == fid)?;
        l.items.get(l.sel).map(|s| s.as_str())
    }

    /// Opens the alternatives list of `fid`; `from_end` highlights the last
    /// one (Up), otherwise the first (Down, the hover button).
    pub fn open_list(&mut self, doc: &Doc, fid: u64, from_end: bool) -> bool {
        let Some(k) = self.index(doc, fid) else {
            return false;
        };
        let fields = doc.fields();
        let all = self.a.suggestions(&self.an, &fields, k);
        if all.is_empty() {
            return false;
        }
        let value = doc
            .entries
            .iter()
            .find(|e| e.id == fid)
            .map(|e| e.f.value.clone())
            .unwrap_or_default();
        let mut items = filter(&all, &value);
        let mut sel = if from_end {
            items.len().saturating_sub(1)
        } else {
            0
        };
        // The field holds one of the alternatives: browse all of them,
        // starting next to the current one.
        if items.is_empty()
            && let Some(cur) = all
                .iter()
                .position(|a| a.to_lowercase() == value.to_lowercase())
        {
            let n = all.len();
            sel = if from_end {
                (cur + n - 1) % n
            } else {
                (cur + 1) % n
            };
            items = all.clone();
        }
        if items.is_empty() || items.len() == 1 && items[0].eq_ignore_ascii_case(&value) {
            return false;
        }
        self.list = Some(List {
            field: fid,
            all,
            items,
            sel,
        });
        true
    }

    /// Up/Down while the list is open. Returns true if handled.
    pub fn handle_list_key(&mut self, down: bool, _field: &pdfr_core::field::Field) -> bool {
        let Some(l) = &mut self.list else {
            return false;
        };
        // Browsing wraps around; Escape closes the list.
        let n = l.items.len().max(1);
        l.sel = if down {
            (l.sel + 1) % n
        } else {
            (l.sel + n - 1) % n
        };
        true
    }

    /// The highlighted item, consuming the list (Enter / Tab).
    pub fn take_selected(&mut self, field: u64) -> Option<String> {
        let l = self.list.take()?;
        if l.field != field {
            return None;
        }
        l.items.get(l.sel).cloned()
    }

    pub fn on_typing(&mut self, doc: &Doc, fid: u64) {
        let Some(l) = &mut self.list else { return };
        if l.field != fid {
            self.list = None;
            return;
        }
        let value = doc
            .entries
            .iter()
            .find(|e| e.id == fid)
            .map(|e| e.f.value.clone())
            .unwrap_or_default();
        l.items = filter(&l.all, &value);
        l.sel = 0;
        if l.items.is_empty() {
            self.list = None;
        }
    }

    /// The alternatives under field rect `r`, in the panel design. Returns
    /// the clicked item.
    pub fn list_popup(
        &mut self,
        ctx: &egui::Context,
        r: Rect,
        field: u64,
        pal: &crate::theme::Palette,
    ) -> Option<String> {
        let l = self.list.as_ref().filter(|l| l.field == field)?;
        let c = widgets::Colors::new(pal);
        let mut pick = None;
        egui::Area::new(Id::unique("pdfr-suggestions"))
            .fixed_pos(r.left_bottom() + egui::vec2(0.0, 4.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                widgets::panel_frame(ctx, pal, &c, 4)
                    .corner_radius(egui::CornerRadius::same(8))
                    .show(ui, |ui| {
                        let w = r.width().max(200.0);
                        ui.set_width(w);
                        ui.spacing_mut().item_spacing.y = 1.0;
                        for (i, it) in l.items.iter().enumerate() {
                            let (row, resp) =
                                ui.allocate_exact_size(egui::vec2(w, 28.0), egui::Sense::click());
                            let fill = if i == l.sel {
                                c.accent.gamma_multiply(0.25)
                            } else if resp.hovered() {
                                c.hover
                            } else {
                                egui::Color32::TRANSPARENT
                            };
                            ui.painter().rect_filled(row, 6.0, fill);
                            ui.painter().text(
                                row.left_center() + egui::vec2(10.0, 0.0),
                                egui::Align2::LEFT_CENTER,
                                it,
                                egui::FontId::proportional(14.0),
                                c.text,
                            );
                            if resp
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                pick = Some(it.clone());
                            }
                        }
                    });
            });
        if pick.is_some() {
            self.list = None;
        }
        pick
    }

    // ----------------------------------------------------------- windows

    pub fn consent_window(&mut self, ctx: &egui::Context) {
        let Some(ask) = &self.consent else { return };
        let tool = self.a.backend_name().unwrap_or_default();
        let what = match ask {
            AiAsk::Labels(l) => format!(
                "pdfr doesn't recognise {} field label{} on this form. It can ask {tool} what they mean, so it can offer your saved details there.",
                l.len(),
                if l.len() == 1 { "" } else { "s" }
            ),
            AiAsk::People(_) => format!(
                "This form asks about more than one person. pdfr can ask {tool} which fields belong to which person, so each person gets their own details."
            ),
        };
        let mut decided: Option<Option<bool>> = None;
        egui::Modal::new(Id::unique("pdfr-consent")).show(ctx, |ui| {
            ui.set_max_width(420.0);
            ui.heading("Recognise more fields with AI?");
            ui.label(what);
            ui.label(egui::RichText::new("It sends the field labels and section headings of this form, never what you type.").strong());
            ui.label("The answers are remembered on this computer, so each form is asked about once.");
            ui.horizontal(|ui| {
                if ui.button("Allow").clicked() {
                    decided = Some(Some(true));
                }
                if ui.button("Not now").clicked() {
                    decided = Some(None);
                }
                if ui.button("Never").clicked() {
                    decided = Some(Some(false));
                }
            });
        });
        // Esc: "Not now".
        if decided.is_none()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            decided = Some(None);
        }
        match decided {
            Some(Some(true)) => {
                self.a.give_consent();
                self.a.save();
                if let Some(ask) = self.consent.take() {
                    self.start_ai(ask);
                }
            }
            Some(Some(false)) => {
                self.a.store.settings.ai_consent = Some(false);
                self.a.store.settings.ai_consent_tool.clear();
                self.a.store.settings.ai_enabled = false;
                self.a.save();
                self.consent = None;
            }
            Some(None) => {
                self.declined_this_session = true;
                self.consent = None;
            }
            None => {}
        }
    }
}

fn filter(all: &[String], prefix: &str) -> Vec<String> {
    let p = prefix.to_lowercase();
    all.iter()
        .filter(|s| s.to_lowercase().starts_with(&p) && s.to_lowercase() != p)
        .cloned()
        .collect()
}
