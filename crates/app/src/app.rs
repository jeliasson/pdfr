//! The window: document view, scrolling, zoom, mouse and keyboard, toolbar.

use crate::draw::{self, PageXf};
use crate::theme::{self, Palette};
use crate::worker::{Msg, Worker};
use eframe::egui::{
    self, Color32, CursorIcon, Event, EventFilter, Id, Key, Modifiers, Pos2, Rect, Sense, Stroke,
    TextureHandle, Vec2, pos2, vec2,
};
use pdfr_core::edit::Editor;
use pdfr_core::field::{Field, FieldKind, Origin, TextMode};
use pdfr_core::forms::FormDoc;
use pdfr_core::geom::Rect as PRect;
use pdfr_core::layout::layout;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

/// Gap between pages and around them, in screen points.
const GAP: f32 = 14.0;
const PAD: f32 = 20.0;
const SCROLLBAR_W: f32 = 10.0;
pub const EDITOR_ID: &str = "pdfr-field-editor";

#[derive(Clone, Debug, Default)]
pub struct CliOptions {
    pub file: Option<PathBuf>,
    pub demo: bool,
    pub save: Option<PathBuf>,
    pub screenshot: Option<PathBuf>,
    pub zoom: Option<f32>,
    pub no_ai: bool,
    /// Hidden: where the value memory lives (tests).
    pub data_dir: Option<PathBuf>,
    /// Hidden: open the settings panel at this section (screenshots).
    pub settings: Option<String>,
    /// Hidden: show the remember dialog after loading (screenshots).
    pub ask_remember: bool,
    /// Hidden: show the signing dialog with this name typed (screenshots).
    pub sign: Option<String>,
    /// Hidden: act as if the pointer were at this page-0 point (screenshots).
    pub hover: Option<(f32, f32)>,
    /// Hidden: record a scripted demo scene's frames into this folder.
    pub record: Option<PathBuf>,
    /// Hidden: the scene to record (see `record::script`).
    pub script: Option<String>,
    /// Hidden: values to type at page positions (screenshots).
    pub values: Option<PathBuf>,
}

/// One field plus app-level state, with a stable id.
#[derive(Clone, Debug)]
pub struct Entry {
    pub id: u64,
    pub f: Field,
    /// The user marked it "don't remember".
    pub no_memory: bool,
}

struct PageTex {
    tex: TextureHandle,
    scale: f32,
}

pub struct Doc {
    pub path: PathBuf,
    pub name: String,
    pub form: Arc<FormDoc>,
    pub entries: Vec<Entry>,
    next_id: u64,
    pub order: Vec<u64>,
    sizes: Vec<(f32, f32)>,
    tex: HashMap<usize, PageTex>,
    /// page -> scale requested and not yet delivered.
    pending: HashMap<usize, f32>,
    generation: u64,
    pub detected: Vec<bool>,
    pub save_path: Option<PathBuf>,
    pub dirty: bool,
    opened_at: Instant,
    pub first_page_ms: Option<f64>,
    pub detect_done_ms: Option<f64>,
    /// Text runs per page (for labels), filled by the worker.
    pub text: HashMap<usize, Vec<pdfr_core::text::TextRun>>,
}

impl Doc {
    pub fn fields(&self) -> Vec<Field> {
        self.entries.iter().map(|e| e.f.clone()).collect()
    }
    pub fn index_of(&self, id: u64) -> Option<usize> {
        self.entries.iter().position(|e| e.id == id)
    }
    fn add(&mut self, f: Field) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.entries.push(Entry {
            id,
            f,
            no_memory: false,
        });
        id
    }
    /// Sets entry `i`'s value and that of the other widgets of the same real
    /// field (they share one value).
    pub fn set_value_at(&mut self, i: usize, v: String) {
        if let Origin::AcroForm { field, .. } = self.entries[i].f.origin {
            for e in self.entries.iter_mut() {
                if matches!(e.f.origin, Origin::AcroForm { field: g, .. } if g == field)
                    && !e.f.kind.is_toggle()
                {
                    e.f.value = v.clone();
                }
            }
        }
        self.entries[i].f.value = v;
    }

    pub fn reorder(&mut self) {
        let mut items: Vec<(u64, usize, PRect)> = self
            .entries
            .iter()
            .map(|e| (e.id, e.f.page, e.f.rect))
            .collect();
        pdfr_core::detect::reading_order_by(&mut items, |t| (t.1, t.2));
        self.order = items.into_iter().map(|t| t.0).collect();
    }
}

struct Focus {
    id: u64,
    ed: Editor,
    /// Index into the suggestion list being previewed as ghost text.
    ghost: Option<String>,
    ghost_dismissed: bool,
}

/// A file read and prepared on a background thread.
pub struct Opened {
    form: FormDoc,
    /// The bytes to render (field values blanked; drawn by the app instead).
    render: Arc<Vec<u8>>,
}

/// What an "unsaved changes" prompt is guarding.
#[derive(Clone, Debug)]
enum Confirm {
    Close,
    Open(PathBuf),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    None,
    AddText,
    AddCheckbox,
    /// Putting down the signature in `App::placing`.
    PlaceSignature,
}

pub struct App {
    pub cli: CliOptions,
    /// The main window, as the parent of file dialogs.
    parent: Option<ParentWindow>,
    pub doc: Option<Doc>,
    worker: Worker,
    pub zoom: f32,
    scroll: Vec2,
    viewport: Rect,
    focus: Option<Focus>,
    tool: Tool,
    drag_start: Option<(usize, (f32, f32))>,
    ctx_field: Option<u64>,
    choice_open: Option<u64>,
    /// The highlighted option of the open drop-down (keyboard).
    choice_sel: usize,
    /// The highlight last drawn, to scroll a moved one into view.
    choice_sel_shown: Option<usize>,
    /// The command palette or the F1 cheatsheet, when open.
    overlay: Option<palette::Overlay>,
    /// Ctrl+L jump labels, while shown.
    hints: Option<hints::Hints>,
    /// The signing dialog, while open.
    sign: Option<signature::SignDialog>,
    /// The signature being placed (with `Tool::PlaceSignature`).
    placing: Option<Arc<pdfr_core::signature::Ink>>,
    placing_initials: bool,
    /// A placed signature being moved or resized.
    sig_drag: Option<signature::SigDrag>,
    /// The text search (Ctrl+F): query, hits, highlights.
    search: Option<search::Search>,
    /// Page text selected with the mouse.
    text_sel: Option<select::TextSel>,
    /// The right-click menu of the page (not of a box) is open.
    page_menu_open: bool,
    pub palette: Palette,
    omarchy_active: bool,
    theme_rx: Receiver<Option<Palette>>,
    open_rx: Receiver<(PathBuf, Result<Opened, String>, Instant)>,
    open_tx: Sender<(PathBuf, Result<Opened, String>, Instant)>,
    pub status: String,
    undo: Vec<Vec<Entry>>,
    redo: Vec<Vec<Entry>>,
    typing_run: Option<u64>,
    /// Unsaved changes would be lost: ask first.
    confirm: Option<Confirm>,
    allow_close: bool,
    last_title: Option<String>,
    /// Close once the "remember?" dialog after saving is answered.
    close_after_remember: bool,
    // Automation (hidden CLI flags).
    auto_stage: u8,
    auto_frames: u32,
    screenshot_rx: Option<Receiver<Arc<egui::ColorImage>>>,
    pub assist: crate::assist_ui::AssistState,
    /// The licence key and the "please register" reminder.
    pub license: crate::license_ui::LicenseUi,
    applied_system_dark: Option<bool>,
    /// The open file dialog's answer arrives here (`Some` while one is open).
    dialog_rx: Option<Receiver<files::DialogResult>>,
    /// pdfr's own file dialog, where the desktop has none.
    builtin_dialog: Option<(egui_file_dialog::FileDialog, files::builtin::Purpose)>,
    /// pdfr's own print dialog (straight to CUPS), where the desktop has none.
    #[cfg(target_os = "linux")]
    cups_dialog: Option<print::CupsDialog>,
    /// Hidden demo recorder (`--record`).
    rec: Option<record::Recorder>,
    egui_ctx: egui::Context,
}

/// The desktop's light/dark preference: the portal's answer where there is
/// one (any Linux desktop), otherwise what the window system reports.
fn system_dark(ctx: &egui::Context) -> bool {
    crate::system::prefers_dark().unwrap_or(ctx.system_theme() == Some(egui::Theme::Dark))
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, cli: CliOptions) -> App {
        let ctx = cc.egui_ctx.clone();
        draw::install_font(&ctx);
        ctx.options_mut(|o| o.zoom_with_keyboard = false);
        let (theme_tx, theme_rx) = channel();
        theme::watch(theme_tx, ctx.clone());
        crate::system::watch_color_scheme(&ctx);
        crate::system::set_wake(&ctx);
        let omarchy = theme::load_omarchy();
        let omarchy_active = omarchy.is_some();
        let palette = omarchy
            .unwrap_or_else(|| Palette::builtin(system_dark(&ctx)));
        ctx.set_visuals(palette.visuals());
        let (open_tx, open_rx) = channel();
        let auto_remember = cli.save.is_some() || cli.demo || cli.screenshot.is_some();
        let assist = crate::assist_ui::AssistState::new(
            ctx.clone(),
            cli.no_ai,
            cli.data_dir.clone(),
            auto_remember,
        );
        let quiet = cli.demo || cli.save.is_some() || cli.screenshot.is_some() || cli.record.is_some();
        let license = crate::license_ui::LicenseUi::new(cli.data_dir.as_deref(), quiet);
        let mut app = App {
            parent: None,
            worker: Worker::new(ctx.clone()),
            cli: cli.clone(),
            doc: None,
            zoom: 1.0,
            scroll: Vec2::ZERO,
            viewport: Rect::NOTHING,
            focus: None,
            tool: Tool::None,
            drag_start: None,
            ctx_field: None,
            choice_open: None,
            choice_sel: 0,
            choice_sel_shown: None,
            overlay: None,
            hints: None,
            sign: None,
            placing: None,
            placing_initials: false,
            sig_drag: None,
            search: None,
            text_sel: None,
            page_menu_open: false,
            palette,
            omarchy_active,
            theme_rx,
            open_rx,
            open_tx,
            status: "Open a PDF (Ctrl+O) or drop one here. F1 shows every shortcut.".into(),
            undo: vec![],
            redo: vec![],
            typing_run: None,
            confirm: None,
            allow_close: false,
            last_title: None,
            close_after_remember: false,
            auto_stage: 0,
            auto_frames: 0,
            screenshot_rx: None,
            assist,
            license,
            applied_system_dark: None,
            dialog_rx: None,
            builtin_dialog: None,
            #[cfg(target_os = "linux")]
            cups_dialog: None,
            rec: match (&cli.record, &cli.script) {
                (Some(dir), Some(name)) => {
                    record::script(name).map(|s| record::Recorder::new(s, dir.clone()))
                }
                _ => None,
            },
            egui_ctx: ctx.clone(),
        };
        if let Some(s) = &cli.settings {
            app.assist.open_settings(s);
        }
        if let Some(p) = cli.file {
            app.open(p);
        }
        app
    }

    fn poll(&mut self, ctx: &egui::Context) {
        self.poll_dialog();
        for p in crate::system::take_opened() {
            self.open_from_system(p);
        }
        while let Ok((path, res, started)) = self.open_rx.try_recv() {
            match res {
                Ok(form) => self.finish_open(path, form, started, ctx),
                Err(e) => self.status = format!("Could not open {}: {e}", path.display()),
            }
        }
        while let Ok(p) = self.theme_rx.try_recv() {
            self.omarchy_active = p.is_some();
            self.palette = p
                .unwrap_or_else(|| Palette::builtin(system_dark(ctx)));
            ctx.set_visuals(self.palette.visuals());
        }
        if !self.omarchy_active {
            let dark = system_dark(ctx);
            if self.applied_system_dark != Some(dark) {
                self.applied_system_dark = Some(dark);
                self.palette = Palette::builtin(dark);
                ctx.set_visuals(self.palette.visuals());
            }
        }
        let mut detected_any = false;
        while let Ok(msg) = self.worker.rx.try_recv() {
            let Some(doc) = &mut self.doc else { continue };
            match msg {
                Msg::RenderFailed { generation, page } if generation == doc.generation => {
                    doc.pending.remove(&page);
                }
                Msg::Rendered {
                    generation,
                    page,
                    requested,
                    scale,
                    tex,
                } if generation == doc.generation => {
                    let wanted = doc.pending.get(&page).copied();
                    if wanted.is_some_and(|s| (s - requested).abs() < 1e-3) {
                        doc.pending.remove(&page);
                    } else if wanted.is_some() && doc.tex.contains_key(&page) {
                        // An older render finishing late: keep the current
                        // image until the wanted one arrives.
                        continue;
                    }
                    doc.tex.insert(page, PageTex { tex, scale });
                    if doc.first_page_ms.is_none() {
                        doc.first_page_ms = Some(doc.opened_at.elapsed().as_secs_f64() * 1e3);
                    }
                }
                Msg::Text {
                    generation,
                    page,
                    text,
                } if generation == doc.generation => {
                    doc.text.insert(page, text);
                }
                Msg::Detected {
                    generation,
                    page,
                    fields,
                    text,
                } if generation == doc.generation => {
                    doc.detected[page] = true;
                    doc.text.insert(page, text);
                    let existing: Vec<Field> = doc.entries.iter().map(|e| e.f.clone()).collect();
                    for f in fields {
                        if !pdfr_core::forms::overlaps_any(&existing, page, &f.rect, 0.3) {
                            let id = doc.add(f);
                            // Found fields are not an edit: every undo/redo
                            // state gets them too, so undo never removes them.
                            let e = doc.entries.last().cloned().unwrap();
                            for snap in self.undo.iter_mut().chain(self.redo.iter_mut()) {
                                if !snap.iter().any(|x| x.id == id) {
                                    snap.push(e.clone());
                                }
                            }
                        }
                    }
                    detected_any = true;
                    if doc.detect_done_ms.is_none() && doc.detected.iter().all(|d| *d) {
                        doc.detect_done_ms = Some(doc.opened_at.elapsed().as_secs_f64() * 1e3);
                    }
                }
                _ => {}
            }
        }
        if detected_any {
            let doc = self.doc.as_mut().unwrap();
            doc.reorder();
            self.assist.on_detected(doc);
            self.refresh_ghost();
        }
        if self.assist.poll(self.doc.as_mut()) {
            self.refresh_ghost();
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(&self.status);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !self.license.registered() {
                    let r = ui
                        .add(
                            egui::Label::new(
                                egui::RichText::new("Unregistered: get a free license")
                                    .color(self.palette.accent),
                            )
                            .sense(egui::Sense::click()),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    if r.clicked() {
                        self.license.open_reminder();
                    }
                    ui.separator();
                }
                if let Some(doc) = &self.doc {
                    let n = doc.sizes.len();
                    let cur = self.current_page();
                    ui.label(format!("Page {} of {n}", cur + 1));
                    let left = doc.detected.iter().filter(|d| !**d).count();
                    if left > 0 {
                        ui.label(format!("Finding fields… {left} pages left"));
                    }
                    if let Some(s) = self.assist.status_text() {
                        ui.label(s);
                    }
                }
                ui.label(egui::RichText::new(&self.palette.name).weak());
            });
        });
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if !self.keyboard_elsewhere(ctx) {
            self.command_shortcuts(ctx);
        }
        // Drag and drop.
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if let Some(p) = dropped
            .into_iter()
            .find(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")))
        {
            self.request_open(p);
        }
    }

    fn title(&self) -> String {
        match &self.doc {
            Some(d) => format!("{}{} — pdfr", d.name, if d.dirty { " •" } else { "" }),
            None => "pdfr".into(),
        }
    }
}

impl eframe::App for App {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.record_input(raw_input);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
        if self.parent.is_none()
            && let (Ok(w), Ok(d)) = (frame.window_handle(), frame.display_handle())
        {
            self.parent = Some(ParentWindow {
                window: w.as_raw(),
                display: d.as_raw(),
            });
        }
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        self.hints_keys(&ctx);
        self.shortcuts(&ctx);
        self.handle_keys(&ctx);
        let title = self.title();
        if self.last_title.as_deref() != Some(title.as_str()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = Some(title);
        }

        // Unsaved changes on close.
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            if self.doc.as_ref().is_some_and(|d| d.dirty) {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.confirm = Some(Confirm::Close);
            } else if self.assist.remember_pending() {
                // Let the user answer "remember?" first.
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.close_after_remember = true;
            }
        }
        if self.close_after_remember && !self.assist.remember_pending() {
            self.close_after_remember = false;
            self.allow_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        egui::Panel::top("toolbar")
            .frame(
                egui::Frame::new()
                    .fill(self.palette.toolbar)
                    .inner_margin(egui::Margin::symmetric(8, 5)),
            )
            .show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(self.palette.toolbar)
                    .inner_margin(egui::Margin::symmetric(8, 3)),
            )
            .show(ui, |ui| self.status_bar(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(self.palette.canvas))
            .show(ui, |ui| self.canvas(ui));

        self.builtin_dialog_ui(&ctx);
        #[cfg(target_os = "linux")]
        self.cups_dialog_ui(&ctx);
        self.assist.settings_window(&ctx, &self.palette, &mut self.license);
        self.assist.consent_window(&ctx);
        self.assist.remember_window(&ctx, &self.palette);
        let automated = self.cli.demo
            || self.cli.save.is_some()
            || self.cli.screenshot.is_some()
            || self.cli.record.is_some();
        self.assist.poll_updates(&ctx, automated);
        self.assist.notices(&ctx, &self.palette, automated);
        self.overlay_ui(&ctx);
        self.sign_window(&ctx);
        // The reminder waits while anything else asks for attention.
        let busy = self.confirm.is_some()
            || self.assist.modal_open()
            || self.assist.settings_open
            || self.overlay.is_some()
            || self.sign.is_some()
            || self.dialog_open();
        self.license.tick(&ctx, busy);
        self.license.windows(&ctx, &self.palette);
        if let Some(m) = self.assist.take_system_message() {
            self.status = m;
        }

        if let Some(what) = self.confirm.clone() {
            let mut proceed = false;
            // Keyboard: Esc cancels; Ctrl+S, or Enter while no button has
            // the keyboard focus, saves (the choice that never loses work).
            let free = ctx.memory(|m| m.focused().is_none_or(|f| f == Id::unique(EDITOR_ID)));
            let (cancel, save_key) = ctx.input_mut(|i| {
                (
                    i.consume_key(Modifiers::NONE, Key::Escape),
                    i.consume_key(Modifiers::COMMAND, Key::S)
                        || (free && i.consume_key(Modifiers::NONE, Key::Enter)),
                )
            });
            if cancel {
                self.confirm = None;
            }
            egui::Modal::new(Id::unique("pdfr-confirm")).show(&ctx, |ui| {
                ui.label(match &what {
                    Confirm::Close => "You have unsaved changes.".to_string(),
                    Confirm::Open(p) => format!(
                        "You have unsaved changes. Open {} anyway?",
                        p.file_name()
                            .map(|n| n.to_string_lossy())
                            .unwrap_or_default()
                    ),
                });
                ui.horizontal(|ui| {
                    if (ui.button("Save").on_hover_text("Enter or Ctrl+S").clicked() || save_key)
                        && !cancel
                    {
                        // Closing or opening follows once the save succeeds
                        // (after the file dialog, if one is needed).
                        let after = match &what {
                            Confirm::Close => files::AfterSave::Close,
                            Confirm::Open(p) => files::AfterSave::Open(p.clone()),
                        };
                        self.save_then(false, after);
                        self.confirm = None;
                    }
                    if ui.button("Discard").clicked() {
                        proceed = true;
                        self.confirm = None;
                    }
                    if ui.button("Cancel").on_hover_text("Esc").clicked() {
                        self.confirm = None;
                    }
                });
            });
            if proceed {
                match what {
                    // Closing waits for the "remember?" answer, if one is asked.
                    Confirm::Close => self.close_after_remember = true,
                    Confirm::Open(p) => self.open(p),
                }
            }
        }
        self.automation(&ctx);
        self.record_frame(&ctx);
    }
}

mod automation;
pub(crate) mod canvas;
use files::ParentWindow;
mod commands;
#[cfg(target_os = "linux")]
mod cups;
mod editing;
mod files;
mod hints;
mod history;
mod palette;
mod print;
mod record;
mod search;
mod select;
mod signature;
mod toolbar;
mod view;

#[cfg(test)]
mod ui_tests;
