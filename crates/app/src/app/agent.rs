//! What an AI agent's requests do to this window (see `crate::agent` for
//! how they get here). Each request is answered on the UI thread: at once
//! when it can be, otherwise when what it waits for has happened (the file
//! is loaded, the frame is drawn, the user has answered).
//!
//! Pages are numbered from 1 for agents; boxes are named `f<id>`.

use super::commands::Command;
use super::*;
use crate::agent::bridge::Failure;
use crate::agent::server::{Request, Server};
use serde_json::{Value, json};
use std::time::Duration;

/// How long loading may take before an agent is told the window is busy.
const LOAD_WAIT: Duration = Duration::from_secs(60);
/// How long the user gets to answer a question about what an agent wants.
const ANSWER_WAIT: Duration = Duration::from_secs(150);
/// How long a frame may take to be drawn (a hidden window may never be).
const DRAW_WAIT: Duration = Duration::from_secs(8);
/// The AI helper that names boxes is waited for this long, no longer.
const LABEL_WAIT: Duration = Duration::from_secs(12);
/// Frames drawn after a request before a screenshot shows its effect.
const SETTLE_PASSES: u64 = 3;

#[derive(Default)]
pub(super) struct AgentState {
    /// This window's socket, while agents are allowed.
    server: Option<Server>,
    /// Requests not answered yet.
    waiting: Vec<Waiting>,
    /// A question to the user about something an agent wants to do.
    pub(super) ask: Option<Ask>,
    next_ask: u64,
    /// The user took over: requests are refused until they resume.
    pub(super) paused: bool,
}

/// A question only the user can answer (agents' input is not let through
/// while it shows).
pub(super) struct Ask {
    id: u64,
    pub title: String,
    pub detail: String,
    pub answer: Option<bool>,
}

struct Waiting {
    reply: Sender<Value>,
    started: Instant,
    deadline: Instant,
    what: Wait,
}

pub(super) enum Wait {
    /// Loading has finished.
    Idle(Then),
    /// Loading has finished and the window has been drawn a few times
    /// since pass `from`: then a screenshot is taken.
    Drawn { from: u64 },
    /// The screenshot is on its way from the renderer.
    Shot(Receiver<Arc<egui::ColorImage>>),
    /// The user's answer to question `id`, and what to do on "allow".
    Answer(u64, Action),
}

pub(super) enum Then {
    State,
    /// The file at this path was asked to open.
    Opened(PathBuf),
}

pub(super) enum Action {
    SaveOver(PathBuf),
    SignDigitally,
    Print,
}

/// What a request comes to: an answer now, or something to wait for.
pub(super) enum Outcome {
    Done(Result<Value, Failure>),
    Wait(Wait, Duration),
}

fn fail(code: &str, message: impl Into<String>) -> Outcome {
    Outcome::Done(Err(Failure::new(code, message)))
}

fn ok(v: Value) -> Outcome {
    Outcome::Done(Ok(v))
}

/// `"f12"` → 12.
fn parse_ref(v: &Value) -> Option<u64> {
    let s = v.as_str()?;
    s.strip_prefix('f').unwrap_or(s).parse().ok()
}

fn round(v: f32) -> f64 {
    (v as f64 * 100.0).round() / 100.0
}

impl App {
    /// Whether agents may connect to this window.
    fn agents_allowed(&self) -> bool {
        self.assist.a.store.settings.agents_enabled && !self.automated() && !cfg!(test)
    }

    /// Called every frame, and while the window is hidden: takes agents'
    /// requests and answers those whose wait is over.
    pub(super) fn agent_tick(&mut self, ctx: &egui::Context) {
        match (self.agents_allowed(), self.agent.server.is_some()) {
            (true, false) => self.agent.server = Server::start(ctx.clone()),
            (false, true) => self.agent.server = None,
            _ => {}
        }
        let mut requests: Vec<Request> = vec![];
        if let Some(s) = &self.agent.server {
            while let Ok(r) = s.rx.try_recv() {
                requests.push(r);
            }
        }
        for r in requests {
            match self.agent_handle(&r.op, &r.args, ctx) {
                Outcome::Done(res) => self.agent_reply(&r.reply, res),
                Outcome::Wait(what, limit) => self.agent.waiting.push(Waiting {
                    reply: r.reply,
                    started: Instant::now(),
                    deadline: Instant::now() + limit,
                    what,
                }),
            }
        }
        for w in std::mem::take(&mut self.agent.waiting) {
            if let Some(w) = self.agent_progress(w, ctx) {
                self.agent.waiting.push(w);
            }
        }
        if !self.agent.waiting.is_empty() || self.agent.ask.is_some() {
            // Keep going until they are answered, drawing even a hidden
            // window (a screenshot needs a frame; a question, the user).
            ctx.request_repaint_after(Duration::from_millis(16));
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaintWhileHidden);
        }
    }

    fn agent_reply(&self, reply: &Sender<Value>, res: Result<Value, Failure>) {
        let _ = reply.send(self.agent_answer(res));
    }

    /// The reply as it goes over the socket.
    fn agent_answer(&self, res: Result<Value, Failure>) -> Value {
        match res {
            Ok(v) => json!({"ok": true, "result": v, "state": self.agent_state()}),
            Err(f) => json!({
                "ok": false,
                "error": {"code": f.code, "message": f.message},
                "state": self.agent_state(),
            }),
        }
    }

    /// Loading is over: the file is read, its boxes are found and named,
    /// and the window has been laid out once.
    fn agent_loaded(&self, waited: Duration) -> bool {
        self.opens_in_flight == 0
            && self
                .doc
                .as_ref()
                .is_none_or(|d| d.detected.iter().all(|x| *x))
            && (self.assist.idle() || waited > LABEL_WAIT)
            && self.viewport.is_positive()
    }

    /// Moves one waiting request on. `None`: it was answered.
    fn agent_progress(&mut self, mut w: Waiting, ctx: &egui::Context) -> Option<Waiting> {
        let waited = w.started.elapsed();
        let late = Instant::now() > w.deadline;
        match &mut w.what {
            Wait::Idle(then) => {
                if !self.agent_loaded(waited) {
                    if late {
                        self.agent_reply(
                            &w.reply,
                            Err(Failure::new(
                                "busy",
                                "pdfr is still loading. Try wait_idle.",
                            )),
                        );
                        return None;
                    }
                    return Some(w);
                }
                let res = match then {
                    Then::State => Ok(json!({})),
                    Then::Opened(path) => self.agent_opened(path),
                };
                self.agent_reply(&w.reply, res);
                None
            }
            Wait::Drawn { from } => {
                let ppp = ctx.pixels_per_point();
                let drawn = self.agent_loaded(waited)
                    && (self.doc.is_none() || self.all_visible_rendered(ppp))
                    && ctx.cumulative_pass_nr() >= *from + SETTLE_PASSES;
                if drawn {
                    let (tx, rx) = channel();
                    ctx.request_screenshot(move |img| {
                        let _ = tx.send(img);
                    });
                    w.what = Wait::Shot(rx);
                    w.deadline = Instant::now() + DRAW_WAIT;
                    return Some(w);
                }
                if late {
                    self.agent_reply(&w.reply, Err(not_drawing()));
                    return None;
                }
                Some(w)
            }
            Wait::Shot(rx) => match rx.try_recv() {
                Ok(img) => {
                    // Scaling and encoding take a moment: off the UI thread.
                    let state = self.agent_state();
                    let reply = w.reply.clone();
                    std::thread::spawn(move || {
                        let rgba: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
                        let (data, width, height) =
                            crate::agent::shot::encode(&rgba, img.size[0], img.size[1]);
                        let _ = reply.send(json!({
                            "ok": true,
                            "result": {"image": data, "width": width, "height": height},
                            "state": state,
                        }));
                    });
                    None
                }
                Err(_) if late => {
                    self.agent_reply(&w.reply, Err(not_drawing()));
                    None
                }
                Err(_) => Some(w),
            },
            Wait::Answer(id, _) => {
                let answer = match &self.agent.ask {
                    Some(a) if a.id == *id => a.answer,
                    // The question is gone (another took its place): no.
                    _ => Some(false),
                };
                match answer {
                    None if late => {
                        self.agent.ask = None;
                        self.agent_reply(
                            &w.reply,
                            Err(Failure::new(
                                "declined",
                                "The user did not answer in time. Ask them before trying again.",
                            )),
                        );
                        None
                    }
                    None => Some(w),
                    Some(allowed) => {
                        if self.agent.ask.as_ref().is_some_and(|a| a.id == *id) {
                            self.agent.ask = None;
                        }
                        let Wait::Answer(_, action) = w.what else {
                            unreachable!()
                        };
                        let res = if allowed {
                            self.agent_do(action)
                        } else {
                            Err(Failure::new(
                                "declined",
                                "The user did not allow this. Do not try again unless they ask.",
                            ))
                        };
                        self.agent_reply(&w.reply, res);
                        None
                    }
                }
            }
        }
    }

    /// The window as an agent needs to know it, sent with every answer.
    pub(super) fn agent_state(&self) -> Value {
        let mut s = json!({
            "file": null,
            "loading": !self.agent_loaded(LABEL_WAIT * 2),
            "dialog": self.agent_dialog(),
            "status": self.status,
        });
        if self.agent.paused {
            s["paused_by_user"] = json!(true);
        }
        if let Some(d) = &self.doc {
            s["file"] = json!(d.path);
            s["saved_as"] = json!(d.save_path);
            s["unsaved_changes"] = json!(d.dirty);
            s["pages"] = json!(d.sizes.len());
            s["boxes"] = json!(d.entries.len());
            s["zoom"] = json!(round(self.zoom));
            if self.viewport.is_positive() {
                s["page"] = json!(self.current_page() + 1);
            }
            if let Some(r) = &d.form.read_only_reason {
                s["read_only"] = json!(r);
            }
            if let Some(f) = &self.focus {
                s["focused"] = json!(format!("f{}", f.id));
            }
        }
        s
    }

    /// The dialog in front of the document, if any, by name.
    fn agent_dialog(&self) -> Option<&'static str> {
        Some(if self.agent.ask.is_some() {
            "asking_the_user"
        } else if self.confirm.is_some() {
            "unsaved_changes"
        } else if self.password.is_some() {
            "password"
        } else if self.dialog_open() {
            "file_or_print_dialog"
        } else if self.sign.is_some() {
            "signature"
        } else if self.digital.dialog.is_some() {
            "digital_signing"
        } else if self.check.is_some() {
            "check_form"
        } else if self.page_confirm.is_some() {
            "page_change"
        } else if self.assist.modal_open() {
            "remember"
        } else if self.license.modal_open() {
            "license"
        } else if self.assist.settings_open {
            "settings"
        } else if self.overlay.is_some() {
            "palette"
        } else {
            return None;
        })
    }

    /// Answers one request. Public to the tests, which call it directly.
    pub(super) fn agent_handle(&mut self, op: &str, args: &Value, ctx: &egui::Context) -> Outcome {
        if op == "state" {
            return ok(json!({}));
        }
        if self.agent.paused {
            return fail(
                "paused_by_user",
                "The user has taken over pdfr and paused agent control. Stop, and ask them to \
                 press Resume in pdfr when they want you to go on.",
            );
        }
        match op {
            "open" => self.agent_open(args),
            "close" => {
                if self.doc.as_ref().is_some_and(|d| d.dirty)
                    && args["discard"].as_bool() != Some(true)
                {
                    return fail(
                        "needs_confirmation",
                        "There are unsaved changes. Save first, or pass discard: true.",
                    );
                }
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                ok(json!({"closed": true}))
            }
            "wait_idle" => {
                let ms = args["timeout_ms"].as_u64().unwrap_or(30_000).min(120_000);
                Outcome::Wait(Wait::Idle(Then::State), Duration::from_millis(ms))
            }
            "screenshot" => Outcome::Wait(
                Wait::Drawn {
                    from: ctx.cumulative_pass_nr(),
                },
                DRAW_WAIT,
            ),
            "read_form" => self.agent_read_form(args, ctx),
            "fill" => self.agent_op_fill(args, ctx),
            "set_checked" => self.agent_set_checked(args, ctx),
            "choose" => self.agent_choose(args, ctx),
            "suggest" => self.agent_suggest(args),
            "read_text" => self.agent_read_text(args),
            "find" => self.agent_find(args),
            "check_form" => self.agent_check(),
            "view" => self.agent_view(args, ctx),
            "run_command" => self.agent_command(args),
            "save" => {
                let Some(doc) = &self.doc else {
                    return fail("no_document", "No document is open.");
                };
                match doc.save_path.clone() {
                    Some(p) => Outcome::Done(self.agent_save_to(p)),
                    None => fail(
                        "needs_path",
                        "This document has not been saved yet. Use save_as with a path \
                         (the original file is left as it is).",
                    ),
                }
            }
            "save_as" => self.agent_save_as(args),
            "print" => self.agent_print(),
            _ => fail("unknown", format!("pdfr has no agent operation \"{op}\".")),
        }
    }

    fn agent_open(&mut self, args: &Value) -> Outcome {
        let Some(path) = args["path"].as_str() else {
            return Outcome::Wait(Wait::Idle(Then::State), LOAD_WAIT);
        };
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            return fail("bad_request", "Give the PDF's absolute path.");
        }
        if !path.is_file() {
            return fail(
                "not_found",
                format!("There is no file at {}.", path.display()),
            );
        }
        if self.doc.as_ref().is_some_and(|d| d.dirty) && args["discard"].as_bool() != Some(true) {
            return fail(
                "needs_confirmation",
                "The open document has unsaved changes. Save it first, pass discard: true, \
                 or pass new_window: true.",
            );
        }
        self.confirm = None;
        self.password = None;
        self.open_with(path.clone(), args["password"].as_str().map(str::to_string));
        Outcome::Wait(Wait::Idle(Then::Opened(path)), LOAD_WAIT)
    }

    /// How opening `path` went, once loading is over.
    fn agent_opened(&mut self, path: &std::path::Path) -> Result<Value, Failure> {
        if let Some(ask) = self.password.take() {
            return Err(if ask.wrong {
                Failure::new("wrong_password", "That password does not open this PDF.")
            } else {
                Failure::new(
                    "password_required",
                    "This PDF is protected. Ask the user for its password and pass it as password.",
                )
            });
        }
        match &self.doc {
            Some(d) if d.path == path => Ok(json!({
                "opened": d.name,
                "pages": d.sizes.len(),
                "boxes": d.entries.len(),
                "fillable": d.form.editable(),
            })),
            _ => Err(Failure::new("failed", self.status.clone())),
        }
    }

    /// The factor from window points to screenshot pixels.
    fn agent_px(&self, ctx: &egui::Context) -> f32 {
        let ppp = ctx.pixels_per_point();
        let size = ctx.viewport_rect().size() * ppp;
        ppp * crate::agent::shot::scale_for(size.x, size.y)
    }

    /// One box as an agent sees it.
    fn agent_field(&self, e: &Entry, rects: &[Rect], px: f32) -> Value {
        let f = &e.f;
        let kind = match &f.kind {
            FieldKind::Text { mode, .. } => match mode {
                TextMode::Single => "text",
                TextMode::OnLine => "line",
                TextMode::Multi => "multiline",
                TextMode::Comb(_) => "boxes",
            },
            FieldKind::Checkbox { .. } => "checkbox",
            FieldKind::Radio { .. } => "radio",
            FieldKind::Choice { .. } => "choice",
            FieldKind::Signature { .. } => "signature",
        };
        let mut v = json!({
            "ref": format!("f{}", e.id),
            "page": f.page + 1,
            "kind": kind,
        });
        let label = f
            .label
            .clone()
            .or_else(|| f.tooltip.clone())
            .filter(|l| !l.trim().is_empty());
        if let Some(l) = label {
            v["label"] = json!(l);
        }
        if let Origin::AcroForm { name, .. } = &f.origin
            && !name.is_empty()
        {
            v["name"] = json!(name);
        }
        match &f.kind {
            FieldKind::Checkbox { .. } => v["checked"] = json!(f.checked),
            FieldKind::Radio { group, .. } => {
                v["checked"] = json!(f.checked);
                v["group"] = json!(group);
            }
            FieldKind::Signature { .. } => v["signed"] = json!(true),
            // What is typed in a password box is not passed on.
            FieldKind::Text { password: true, .. } => {
                v["password"] = json!(true);
                v["filled"] = json!(!f.value.is_empty());
            }
            FieldKind::Choice { options, editable } => {
                v["value"] = json!(f.shown_text());
                v["options"] = json!(options.iter().map(|(_, d)| d).collect::<Vec<_>>());
                if *editable {
                    v["other_text_allowed"] = json!(true);
                }
            }
            FieldKind::Text { .. } => v["value"] = json!(f.value),
        }
        if let Some(n) = f.capacity() {
            v["max_chars"] = json!(n);
        }
        if f.read_only {
            v["read_only"] = json!(true);
        }
        // Where it is in a screenshot, when it shows.
        if let Some(pr) = rects.get(f.page) {
            let r = PageXf {
                origin: pr.min,
                zoom: self.zoom,
            }
            .rect(&f.rect);
            if self.viewport.contains(r.center()) {
                let c = r.center() * px;
                v["at"] = json!([c.x.round(), c.y.round()]);
            }
        }
        v
    }

    fn agent_read_form(&mut self, args: &Value, ctx: &egui::Context) -> Outcome {
        let Some(doc) = &self.doc else {
            return fail("no_document", "No document is open. Use open first.");
        };
        let page = args["page"].as_u64().map(|p| p as usize);
        let only_empty = args["only_empty"].as_bool().unwrap_or(false);
        let filter = args["filter"].as_str().map(|s| s.to_lowercase());
        let rects = if self.viewport.is_positive() {
            self.page_rects()
        } else {
            vec![]
        };
        let px = self.agent_px(ctx);
        let mut rows = vec![];
        for id in &doc.order {
            let Some(i) = doc.index_of(*id) else { continue };
            let e = &doc.entries[i];
            if page.is_some_and(|p| e.f.page + 1 != p) || (only_empty && !e.f.is_empty()) {
                continue;
            }
            let row = self.agent_field(e, &rects, px);
            if let Some(q) = &filter {
                let hay = format!(
                    "{} {}",
                    row["label"].as_str().unwrap_or_default(),
                    row["name"].as_str().unwrap_or_default()
                )
                .to_lowercase();
                if !hay.contains(q) {
                    continue;
                }
            }
            rows.push(row);
        }
        let mut out = json!({"boxes": rows});
        if !doc.form.editable() {
            out["note"] = json!("This document is read-only: its boxes cannot be filled in.");
        } else if !doc.detected.iter().all(|d| *d) {
            out["note"] = json!(
                "pdfr is still finding boxes on some pages. Call wait_idle, then read again."
            );
        }
        ok(out)
    }

    /// The entry a request names with `ref`, if it can be changed.
    fn agent_target(&self, args: &Value) -> Result<(u64, Field), Failure> {
        let doc = self
            .doc
            .as_ref()
            .ok_or_else(|| Failure::new("no_document", "No document is open."))?;
        let id = parse_ref(&args["ref"])
            .ok_or_else(|| Failure::new("bad_request", "ref must be a box's ref, like \"f12\"."))?;
        let i = doc.index_of(id).ok_or_else(|| {
            Failure::new(
                "no_such_box",
                format!(
                    "There is no box f{id}. Refs change when a document is opened; call read_form."
                ),
            )
        })?;
        let f = doc.entries[i].f.clone();
        if !doc.form.editable() || f.read_only {
            return Err(Failure::new("read_only", "This box cannot be changed."));
        }
        Ok((id, f))
    }

    /// The box as it is after a change (and the caret in it, so the user
    /// sees where the agent is working).
    fn agent_changed(&mut self, id: u64, ctx: &egui::Context) -> Value {
        self.assist.invalidate();
        if self.viewport.is_positive() {
            self.focus_field(id, true, ctx);
        }
        ctx.request_repaint();
        let px = self.agent_px(ctx);
        let rects = if self.viewport.is_positive() {
            self.page_rects()
        } else {
            vec![]
        };
        let doc = self.doc.as_ref().unwrap();
        let i = doc.index_of(id).unwrap();
        self.agent_field(&doc.entries[i], &rects, px)
    }

    fn agent_op_fill(&mut self, args: &Value, ctx: &egui::Context) -> Outcome {
        let (id, f) = match self.agent_target(args) {
            Ok(t) => t,
            Err(e) => return Outcome::Done(Err(e)),
        };
        let Some(value) = args["value"].as_str() else {
            return fail("bad_request", "value must be text.");
        };
        if f.kind.is_toggle() {
            return fail(
                "wrong_kind",
                "This is a checkbox or radio button: use set_checked.",
            );
        }
        if f.kind.is_signature() {
            return fail("wrong_kind", "This is a signature, not a text box.");
        }
        if f.kind.is_fixed_choice() {
            return self.agent_choose(&json!({"ref": args["ref"], "option": value}), ctx);
        }
        let rest = self.agent_fill(id, value);
        let mut out = json!({"box": self.agent_changed(id, ctx)});
        if !rest.is_empty() {
            out["did_not_fit"] = json!(rest);
        }
        ok(out)
    }

    fn agent_set_checked(&mut self, args: &Value, ctx: &egui::Context) -> Outcome {
        let (id, f) = match self.agent_target(args) {
            Ok(t) => t,
            Err(e) => return Outcome::Done(Err(e)),
        };
        let Some(want) = args["checked"].as_bool() else {
            return fail("bad_request", "checked must be true or false.");
        };
        if !f.kind.is_toggle() {
            return fail(
                "wrong_kind",
                "This is not a checkbox or radio button: use fill.",
            );
        }
        if f.checked != want {
            self.toggle(id);
        }
        ok(json!({"box": self.agent_changed(id, ctx)}))
    }

    fn agent_choose(&mut self, args: &Value, ctx: &egui::Context) -> Outcome {
        let (id, f) = match self.agent_target(args) {
            Ok(t) => t,
            Err(e) => return Outcome::Done(Err(e)),
        };
        let Some(want) = args["option"].as_str() else {
            return fail("bad_request", "option must be text.");
        };
        let FieldKind::Choice { options, editable } = &f.kind else {
            return fail(
                "wrong_kind",
                "This is not a drop-down: use fill or set_checked.",
            );
        };
        let same = |a: &str| a.trim().eq_ignore_ascii_case(want.trim());
        let hit = options
            .iter()
            .find(|(e, d)| e == want || d == want)
            .or_else(|| options.iter().find(|(e, d)| same(e) || same(d)));
        match hit {
            Some((export, _)) => self.pick_choice(id, export.clone()),
            None if *editable => {
                self.agent_fill(id, want);
            }
            None => {
                let list: Vec<&str> = options.iter().map(|(_, d)| d.as_str()).collect();
                return fail(
                    "no_such_option",
                    format!("The options are: {}.", list.join(", ")),
                );
            }
        }
        if let Some(d) = &mut self.doc {
            d.dirty = true;
        }
        ok(json!({"box": self.agent_changed(id, ctx)}))
    }

    fn agent_suggest(&mut self, args: &Value) -> Outcome {
        let (id, _) = match self.agent_target(args) {
            Ok(t) => t,
            Err(e) => return Outcome::Done(Err(e)),
        };
        if !self.assist.a.store.settings.agent_use_memory {
            return ok(json!({
                "suggestions": [],
                "note": "The user has turned off remembered details for agents. Ask them for the value.",
            }));
        }
        let doc = self.doc.as_mut().unwrap();
        ok(json!({"suggestions": self.assist.suggestions_for(doc, id)}))
    }

    fn agent_read_text(&mut self, args: &Value) -> Outcome {
        let Some(doc) = &self.doc else {
            return fail("no_document", "No document is open.");
        };
        let n = doc.sizes.len();
        let Some(page) = args["page"]
            .as_u64()
            .map(|p| p as usize)
            .filter(|p| (1..=n).contains(p))
        else {
            return fail("bad_request", format!("page must be from 1 to {n}."));
        };
        let Some(runs) = doc.text.get(&(page - 1)) else {
            return ok(json!({
                "page": page,
                "text": "",
                "note": "This page's text has not been read yet (or it is a scan pdfr could not read).",
            }));
        };
        // Phrases in reading order, those on one line joined.
        let mut text = String::new();
        let mut last: Option<&pdfr_core::text::TextRun> = None;
        for i in pdfr_core::text::reading_order(runs) {
            let r = &runs[i];
            if let Some(l) = last {
                let same_line = (r.rect.center().1 - l.rect.center().1).abs()
                    < r.rect.height().min(l.rect.height()) * 0.5;
                text.push(if same_line { ' ' } else { '\n' });
            }
            text.push_str(r.text.trim());
            last = Some(r);
        }
        ok(json!({"page": page, "text": text}))
    }

    fn agent_find(&mut self, args: &Value) -> Outcome {
        if self.doc.is_none() {
            return fail("no_document", "No document is open.");
        }
        let Some(q) = args["query"].as_str().filter(|q| !q.trim().is_empty()) else {
            return fail("bad_request", "query must be some text.");
        };
        // The same search the user gets with Ctrl+F: the matches light up.
        self.update_search(q);
        let hits = self
            .search
            .as_ref()
            .map(|s| s.hits.as_slice())
            .unwrap_or(&[]);
        let shown: Vec<Value> = hits
            .iter()
            .take(50)
            .map(|h| json!({"page": h.page + 1, "text": h.snippet}))
            .collect();
        ok(json!({"matches": hits.len(), "shown": shown}))
    }

    fn agent_check(&mut self) -> Outcome {
        let Some(doc) = self.doc.as_mut() else {
            return fail("no_document", "No document is open.");
        };
        let mut problems = vec![];
        let mut empty = vec![];
        for (i, severity, message) in self.assist.check_doc(doc) {
            let Some(e) = doc.entries.get(i) else {
                continue;
            };
            let mut row = json!({"ref": format!("f{}", e.id), "page": e.f.page + 1});
            if let Some(l) = e.f.label.clone().or_else(|| e.f.tooltip.clone()) {
                row["label"] = json!(l);
            }
            match severity {
                pdfr_assist::check::Severity::Problem => {
                    row["problem"] = json!(message);
                    problems.push(row);
                }
                pdfr_assist::check::Severity::Empty => empty.push(row),
            }
        }
        ok(json!({"problems": problems, "empty": empty}))
    }

    fn agent_view(&mut self, args: &Value, ctx: &egui::Context) -> Outcome {
        let Some(doc) = &self.doc else {
            return fail("no_document", "No document is open.");
        };
        if !self.viewport.is_positive() {
            return fail(
                "busy",
                "The window has not been drawn yet. Call wait_idle first.",
            );
        }
        let n = doc.sizes.len() as u64;
        let page = args["page"].as_u64();
        if page.is_some_and(|p| !(1..=n).contains(&p)) {
            return fail("bad_request", format!("page must be from 1 to {n}."));
        }
        let target = if args["ref"].is_null() {
            None
        } else {
            match parse_ref(&args["ref"]).filter(|id| doc.index_of(*id).is_some()) {
                Some(id) => Some(id),
                None => return fail("no_such_box", "There is no such box. Call read_form."),
            }
        };
        if let Some(z) = args["zoom"].as_f64() {
            self.zoom_to(z as f32, self.viewport.center());
        }
        if let Some(p) = page {
            self.scroll_to_page(p as usize - 1);
        }
        if let Some(id) = target {
            self.focus_field(id, true, ctx);
        }
        ctx.request_repaint();
        ok(json!({}))
    }

    fn agent_command(&mut self, args: &Value) -> Outcome {
        let name = |c: Command| format!("{c:?}");
        let Some(want) = args["name"].as_str() else {
            let list: Vec<Value> = Command::ALL
                .iter()
                .filter(|c| self.can(**c))
                .map(|c| {
                    let s = c.spec();
                    let mut v = json!({"name": name(*c), "label": s.label});
                    if !s.hint.is_empty() {
                        v["what"] = json!(s.hint);
                    }
                    v
                })
                .collect();
            return ok(json!({"commands": list}));
        };
        let Some(c) = Command::ALL.iter().copied().find(|c| {
            name(*c).eq_ignore_ascii_case(want) || c.spec().label.eq_ignore_ascii_case(want)
        }) else {
            return fail(
                "no_such_command",
                "pdfr has no such command. Call run_command without a name for the list.",
            );
        };
        if !self.can(c) {
            return fail(
                "not_now",
                format!("{} cannot be used right now.", c.spec().label),
            );
        }
        match c {
            // These have rules of their own for agents.
            Command::Print => return self.agent_print(),
            Command::SignDigitally if self.assist.a.store.settings.agent_ask_sign => {
                return self.agent_ask(
                    "Let the AI agent sign this document digitally?",
                    "A digital signature is made with your certificate or ID card and counts \
                     as yours."
                        .into(),
                    Action::SignDigitally,
                );
            }
            Command::Save => {
                return self.agent_handle("save", &json!({}), &self.egui_ctx.clone());
            }
            _ => {}
        }
        self.run(c);
        self.egui_ctx.request_repaint();
        ok(json!({"ran": name(c)}))
    }

    /// Puts a question to the user; the request waits for the answer.
    fn agent_ask(&mut self, title: &str, detail: String, action: Action) -> Outcome {
        if self.agent.ask.is_some() {
            return fail(
                "busy",
                "pdfr is waiting for the user to answer an earlier question.",
            );
        }
        self.agent.next_ask += 1;
        let id = self.agent.next_ask;
        self.agent.ask = Some(Ask {
            id,
            title: title.into(),
            detail,
            answer: None,
        });
        // The user may be looking elsewhere.
        self.egui_ctx
            .send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                egui::UserAttentionType::Informational,
            ));
        Outcome::Wait(Wait::Answer(id, action), ANSWER_WAIT)
    }

    /// Does what the user allowed.
    fn agent_do(&mut self, action: Action) -> Result<Value, Failure> {
        match action {
            Action::SaveOver(p) => self.agent_save_to(p),
            Action::SignDigitally => {
                self.run(Command::SignDigitally);
                Ok(json!({"ran": "SignDigitally"}))
            }
            Action::Print => {
                self.print();
                Ok(json!({"printing": self.status}))
            }
        }
    }

    fn agent_save_as(&mut self, args: &Value) -> Outcome {
        let Some(doc) = &self.doc else {
            return fail("no_document", "No document is open.");
        };
        let Some(path) = args["path"].as_str().map(PathBuf::from) else {
            return fail("bad_request", "path must be where to save the PDF.");
        };
        if !path.is_absolute() {
            return fail("bad_request", "Give an absolute path.");
        }
        if !path.parent().is_some_and(|d| d.is_dir()) {
            return fail("not_found", "The folder to save in does not exist.");
        }
        // A file this window wrote itself is its own to write again.
        let ours = doc.save_path.as_deref() == Some(path.as_path());
        if path.exists() && !ours {
            if args["overwrite"].as_bool() != Some(true) {
                return fail(
                    "exists",
                    format!(
                        "{} is already there. Choose another name, or pass overwrite: true \
                         if the user wants it replaced.",
                        path.display()
                    ),
                );
            }
            if self.assist.a.store.settings.agent_ask_overwrite {
                let detail = format!("{} will be replaced.", path.display());
                return self.agent_ask(
                    "Let the AI agent save over this file?",
                    detail,
                    Action::SaveOver(path),
                );
            }
        }
        Outcome::Done(self.agent_save_to(path))
    }

    /// Saves for an agent: nothing is asked of the user about remembering.
    fn agent_save_to(&mut self, path: PathBuf) -> Result<Value, Failure> {
        let Some(doc) = &self.doc else {
            return Err(Failure::new("no_document", "No document is open."));
        };
        if !doc.form.editable() {
            return Err(Failure::new(
                "read_only",
                "This file is read-only; it cannot be saved with boxes filled in.",
            ));
        }
        self.assist.quiet_save = true;
        let saved = self.save_to(path.clone());
        self.assist.quiet_save = false;
        if saved {
            Ok(json!({"saved": path}))
        } else {
            Err(Failure::new("failed", self.status.clone()))
        }
    }

    fn agent_print(&mut self) -> Outcome {
        if !self.can(Command::Print) {
            return fail("not_now", "Printing is not possible right now.");
        }
        if self.assist.a.store.settings.agent_ask_print {
            return self.agent_ask(
                "Let the AI agent print this document?",
                "The print dialog opens next.".into(),
                Action::Print,
            );
        }
        Outcome::Done(self.agent_do(Action::Print))
    }

    /// The question an agent's request raised, for the user to answer.
    pub(super) fn agent_windows(&mut self, ctx: &egui::Context) {
        let Some(ask) = &mut self.agent.ask else {
            return;
        };
        if ask.answer.is_some() {
            return;
        }
        let pal = self.palette.clone();
        let c = crate::assist_ui::widgets::Colors::new(&pal);
        let mut answer = None;
        egui::Modal::new(Id::unique("pdfr-agent-ask"))
            .frame(crate::assist_ui::widgets::panel_frame(ctx, &pal, &c, 20))
            .show(ctx, |ui| {
                use crate::assist_ui::widgets::{primary_button, secondary_button};
                use egui::{Align, Layout, RichText};
                ui.set_width(420.0);
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                ui.label(RichText::new(&ask.title).size(20.0).strong().color(c.text));
                ui.label(RichText::new(&ask.detail).color(c.weak));
                ui.add_space(4.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if primary_button(ui, &c, "Allow").clicked() {
                        answer = Some(true);
                    }
                    if secondary_button(ui, &c, "Don't allow").clicked() {
                        answer = Some(false);
                    }
                });
            });
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            answer = Some(false);
        }
        if answer.is_some() {
            ask.answer = answer;
            ctx.request_repaint();
        }
    }

    /// Answers the question on screen (tests).
    #[cfg(test)]
    pub(super) fn agent_answer_ask(&mut self, allow: bool) {
        if let Some(a) = &mut self.agent.ask {
            a.answer = Some(allow);
        }
    }

    /// Takes a request as the socket would (tests, which have none): its
    /// answer, or where the answer will arrive once its wait is over.
    #[cfg(test)]
    pub(super) fn agent_request(
        &mut self,
        op: &str,
        args: &Value,
        ctx: &egui::Context,
    ) -> Result<Value, Receiver<Value>> {
        match self.agent_handle(op, args, ctx) {
            Outcome::Done(res) => Ok(self.agent_answer(res)),
            Outcome::Wait(what, limit) => {
                let (reply, rx) = channel();
                self.agent.waiting.push(Waiting {
                    reply,
                    started: Instant::now(),
                    deadline: Instant::now() + limit,
                    what,
                });
                Err(rx)
            }
        }
    }
}

fn not_drawing() -> Failure {
    Failure::new(
        "window_not_rendering",
        "The pdfr window is not being drawn (minimised, or on another workspace). \
         The other tools still work; for a screenshot the window must be shown.",
    )
}
