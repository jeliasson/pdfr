//! The window: document view, scrolling, zoom, mouse and keyboard, toolbar.

use crate::draw::{self, PageXf};
use crate::theme::{self, Palette};
use crate::worker::{Msg, Scan, Worker};
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
    /// Started by `pdfr mcp` for an AI agent.
    pub agent: bool,
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
    /// Comments and markup, with stable ids.
    pub marks: Vec<markup::MarkEntry>,
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
    /// Web and e-mail addresses printed on each page, as links.
    pub text_links: HashMap<usize, Vec<pdfr_core::links::Link>>,
    /// Sidebar thumbnails.
    thumbs: HashMap<usize, TextureHandle>,
    /// Areas marked for redaction (not yet applied).
    pub redactions: Vec<redact::RedactMark>,
    /// Scanned pages whose text could not be read yet (reading is off or
    /// its models are not downloaded).
    pub unread_scans: std::collections::BTreeSet<usize>,
    /// A scanned page is being read: the status line says so.
    pub reading: bool,
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
    /// Putting today's date where the user clicks.
    PlaceDate,
    /// Marking areas to redact.
    Redact,
    /// Putting a note where the user clicks.
    Note,
    /// Dragging (or clicking) a text box onto the page.
    TextBox,
    /// Dragging a rectangle, ellipse or arrow.
    Shape(pdfr_core::markup::MarkKind),
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
    /// Check the form, while open.
    check: Option<check::CheckDialog>,
    /// The signature being placed (with `Tool::PlaceSignature`).
    placing: Option<Arc<pdfr_core::signature::Ink>>,
    placing_initials: bool,
    /// A placed signature being moved or resized.
    sig_drag: Option<signature::SigDrag>,
    /// Signing with a certificate, and the open file's signature checks.
    digital: digital_sign::DigitalState,
    /// The redaction tool's panel, confirm dialog and running job.
    redact: redact::RedactState,
    /// The text search (Ctrl+F): query, hits, highlights.
    search: Option<search::Search>,
    /// Page text selected with the mouse.
    text_sel: Option<select::TextSel>,
    /// Comments and markup: the selected one, dragging, editing, colour.
    marks_ui: markup::MarksUi,
    /// The right-click menu of the page (not of a box) is open.
    page_menu_open: bool,
    /// Where on which page it was opened (points).
    page_menu_at: Option<(usize, (f32, f32))>,
    /// Where link jumps started, for Go back.
    back: Vec<Vec2>,
    side_tab: sidebar::SideTab,
    /// The page the thumbnails last scrolled to.
    side_follow: Option<usize>,
    /// Thumbnails last asked of the workers.
    thumbs_asked: Vec<usize>,
    /// Pages selected in the sidebar (Ctrl/Shift+click), and where a Shift
    /// range starts.
    side_sel: Vec<usize>,
    side_anchor: Option<usize>,
    /// Pages being dragged in the sidebar.
    side_drag: Option<Vec<usize>>,
    /// Thumbnail rows drawn last frame, by page (tests drag them).
    side_rows: Vec<(usize, Rect)>,
    /// A page change waiting for "remove the digital signatures?".
    page_confirm: Option<pages::PageOp>,
    /// Outline entries opened or closed by the user.
    outline_open: HashMap<usize, bool>,
    /// Where to return to in a reopened file: page, offset, zoom.
    restore: Option<(usize, f32, f32)>,
    /// Whether the workers draw pages dark.
    dark_applied: bool,
    /// Pages side by side in pairs.
    two_up: bool,
    /// First-page pictures of recent files, for the start screen.
    start_pictures: start::Pictures,
    /// Presentation mode: the page being shown, and the zoom and scroll to
    /// go back to.
    present: Option<(usize, f32, Vec2)>,
    pub palette: Palette,
    omarchy_active: bool,
    theme_rx: Receiver<Option<Palette>>,
    open_rx: Receiver<(PathBuf, Result<Opened, files::OpenFail>, Instant)>,
    open_tx: Sender<(PathBuf, Result<Opened, files::OpenFail>, Instant)>,
    /// A protected file waiting for its password.
    password: Option<files::PasswordAsk>,
    pub status: String,
    undo: Vec<history::Snap>,
    redo: Vec<history::Snap>,
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
    /// Reading scanned text: the setting as applied, the model download.
    ocr: ocr::OcrState,
    egui_ctx: egui::Context,
    /// Files being read on a background thread.
    opens_in_flight: u32,
    /// AI agents controlling this window (`pdfr mcp`).
    agent: agent::AgentState,
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
        let palette = omarchy.unwrap_or_else(|| Palette::builtin(system_dark(&ctx)));
        ctx.set_visuals(palette.visuals());
        let (open_tx, open_rx) = channel();
        let auto_remember = cli.save.is_some() || cli.demo || cli.screenshot.is_some();
        let assist = crate::assist_ui::AssistState::new(
            ctx.clone(),
            cli.no_ai,
            cli.data_dir.clone(),
            auto_remember,
        );
        let quiet =
            cli.demo || cli.save.is_some() || cli.screenshot.is_some() || cli.record.is_some();
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
            check: None,
            placing: None,
            placing_initials: false,
            sig_drag: None,
            digital: Default::default(),
            redact: Default::default(),
            search: None,
            text_sel: None,
            marks_ui: Default::default(),
            page_menu_open: false,
            page_menu_at: None,
            back: vec![],
            side_tab: Default::default(),
            side_follow: None,
            thumbs_asked: vec![],
            side_sel: vec![],
            side_anchor: None,
            side_drag: None,
            side_rows: vec![],
            page_confirm: None,
            outline_open: HashMap::new(),
            restore: None,
            dark_applied: false,
            two_up: false,
            start_pictures: Default::default(),
            present: None,
            palette,
            omarchy_active,
            theme_rx,
            open_rx,
            open_tx,
            password: None,
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
            ocr: Default::default(),
            egui_ctx: ctx.clone(),
            opens_in_flight: 0,
            agent: Default::default(),
        };
        if let Some(s) = &cli.settings {
            app.assist.open_settings(s);
        }
        app.follow_dark_pages();
        app.follow_ocr();
        if let Some(p) = cli.file {
            app.open(p);
        }
        app
    }

    fn poll(&mut self, ctx: &egui::Context) {
        self.follow_dark_pages();
        self.follow_ocr();
        self.poll_dialog();
        for p in crate::system::take_opened() {
            self.open_from_system(p);
        }
        while let Ok((path, res, started)) = self.open_rx.try_recv() {
            self.opens_in_flight = self.opens_in_flight.saturating_sub(1);
            match res {
                Ok(form) => self.finish_open(path, form, started, ctx),
                // Automated runs (screenshots, --save) cannot ask: they stop.
                Err(e) if self.automated() => {
                    eprintln!("pdfr: could not open {}: {e:?}", path.display());
                    std::process::exit(1);
                }
                Err(files::OpenFail::Password { wrong }) => {
                    self.status = "This PDF is protected with a password.".into();
                    self.password = Some(files::PasswordAsk::new(path, wrong));
                }
                Err(files::OpenFail::Other(e)) => {
                    self.status = format!("Could not open {}: {e}", path.display())
                }
            }
        }
        while let Ok(p) = self.theme_rx.try_recv() {
            self.omarchy_active = p.is_some();
            self.palette = p.unwrap_or_else(|| Palette::builtin(system_dark(ctx)));
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
        let mut unread = false;
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
                Msg::Thumb {
                    generation,
                    page,
                    tex,
                } if generation == doc.generation => {
                    if let Some(t) = tex {
                        doc.thumbs.insert(page, t);
                    }
                    self.thumbs_asked.retain(|p| *p != page);
                }
                Msg::Reading { generation } if generation == doc.generation => {
                    doc.reading = true;
                }
                Msg::Text {
                    generation,
                    page,
                    text,
                    scan,
                } if generation == doc.generation => {
                    unread |= Self::note_scan(doc, page, scan);
                    doc.text.insert(page, text);
                    Self::add_text_links(doc, page);
                }
                Msg::Detected {
                    generation,
                    page,
                    fields,
                    text,
                    scan,
                } if generation == doc.generation => {
                    doc.detected[page] = true;
                    unread |= Self::note_scan(doc, page, scan);
                    // Panels that already hold printed answers start with
                    // them (not with words read from a scan: they may be
                    // misread).
                    let mut fields = fields;
                    if scan == Scan::Digital {
                        pdfr_core::detect::prefill_printed(&mut fields, &text);
                    }
                    doc.text.insert(page, text);
                    Self::add_text_links(doc, page);
                    let existing: Vec<Field> = doc.entries.iter().map(|e| e.f.clone()).collect();
                    for f in fields {
                        if !pdfr_core::forms::overlaps_any(&existing, page, &f.rect, 0.3) {
                            let id = doc.add(f);
                            // Found fields are not an edit: every undo/redo
                            // state gets them too, so undo never removes them.
                            // A state with pages in another order gets it on
                            // the same page; one without that page, not at all.
                            let e = doc.entries.last().cloned().unwrap();
                            let pid = doc.form.page_ids.get(page).copied();
                            for snap in self.undo.iter_mut().chain(self.redo.iter_mut()) {
                                let at = match pid {
                                    Some(p) => snap.form.page_ids.iter().position(|x| *x == p),
                                    None => Some(page),
                                };
                                if let Some(at) = at
                                    && !snap.entries.iter().any(|x| x.id == id)
                                {
                                    let mut e = e.clone();
                                    e.f.page = at;
                                    snap.entries.push(e);
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
        if unread {
            self.on_unread_scan();
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
                self.agent_banner(ui);
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
                    if let Some(s) = self.ocr_status() {
                        ui.label(s);
                    } else if left > 0 {
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
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.remember_position();
        // "Restart" after an update: start the new pdfr with this file.
        if let Some(exe) = self.assist.take_restart() {
            let file = self.doc.as_ref().map(|d| d.path.clone());
            crate::self_update::relaunch(&exe, file.as_deref());
        }
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.record_input(raw_input);
        self.agent_input(ctx, raw_input);
    }

    /// Runs before every frame, and also while the window is hidden (when
    /// nothing is drawn): background results and agents' requests are
    /// handled here, so an agent is answered whether or not the window shows.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
        self.agent_tick(ctx);
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
        self.hints_keys(&ctx);
        self.shortcuts(&ctx);
        self.handle_keys(&ctx);
        let title = self.title();
        if self.last_title.as_deref() != Some(title.as_str()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = Some(title);
        }

        // Cmd+W minimises on macOS (the window stays open, as with other
        // single-window Mac apps); quitting is Cmd+Q.
        #[cfg(target_os = "macos")]
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::W)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
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

        let presenting = self.presenting();
        if !presenting {
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
        }
        if self.sidebar_open() && !presenting {
            self.sidebar_panel(ui);
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(if presenting {
                Color32::BLACK
            } else {
                self.palette.canvas
            }))
            .show(ui, |ui| self.canvas(ui));

        self.builtin_dialog_ui(&ctx);
        #[cfg(target_os = "linux")]
        self.cups_dialog_ui(&ctx);
        self.assist
            .settings_window(&ctx, &self.palette, &mut self.license);
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
        self.digital_window(&ctx);
        self.check_window(&ctx);
        self.redact_windows(&ctx);
        self.password_window(&ctx);
        self.page_confirm_window(&ctx);
        self.agent_windows(&ctx);
        // The reminder waits while anything else asks for attention.
        let busy = self.confirm.is_some()
            || self.assist.modal_open()
            || self.assist.settings_open
            || self.overlay.is_some()
            || self.sign.is_some()
            || self.digital.dialog.is_some()
            || self.check.is_some()
            || self.password.is_some()
            || self.page_confirm.is_some()
            || self.dialog_open()
            // An agent is at work: the reminder is for the person.
            || self.cli.agent
            || crate::agent::active();
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
            let pal = self.palette.clone();
            let c = crate::assist_ui::widgets::Colors::new(&pal);
            let mut save = false;
            let mut discard = false;
            let mut dismiss = false;
            egui::Modal::new(Id::unique("pdfr-confirm"))
                .frame(crate::assist_ui::widgets::panel_frame(&ctx, &pal, &c, 20))
                .show(&ctx, |ui| {
                    use crate::assist_ui::widgets::{
                        danger_button, primary_button, secondary_button,
                    };
                    use egui::{Align, Layout, RichText};
                    ui.set_width(420.0);
                    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                    let name = self
                        .doc
                        .as_ref()
                        .map(|d| d.name.clone())
                        .unwrap_or_default();
                    ui.label(
                        RichText::new(format!("Save changes to {name}?"))
                            .size(20.0)
                            .strong()
                            .color(c.text),
                    );
                    ui.label(
                        RichText::new(match &what {
                            Confirm::Close => {
                                "Your changes will be lost if you close without saving.".to_string()
                            }
                            Confirm::Open(p) => format!(
                                "Your changes will be lost if you open {} without saving.",
                                p.file_name()
                                    .map(|n| n.to_string_lossy())
                                    .unwrap_or_default()
                            ),
                        })
                        .color(c.weak),
                    );
                    ui.add_space(4.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        save = primary_button(ui, &c, "Save")
                            .on_hover_text("Enter or Ctrl+S")
                            .clicked();
                        dismiss = secondary_button(ui, &c, "Cancel")
                            .on_hover_text("Esc")
                            .clicked();
                        discard = danger_button(ui, "Don't save", false).clicked();
                    });
                });
            if (save || save_key) && !cancel {
                // Closing or opening follows once the save succeeds
                // (after the file dialog, if one is needed).
                let after = match &what {
                    Confirm::Close => files::AfterSave::Close,
                    Confirm::Open(p) => files::AfterSave::Open(p.clone()),
                };
                self.save_then(false, after);
                self.confirm = None;
            } else if discard {
                proceed = true;
                self.confirm = None;
            } else if dismiss {
                self.confirm = None;
            }
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

mod agent;
mod agent_input;
mod automation;
pub(crate) mod canvas;
mod check;
use files::ParentWindow;
mod commands;
#[cfg(target_os = "linux")]
mod cups;
mod digital_sign;
mod editing;
mod files;
mod hints;
mod history;
mod links;
pub(crate) mod markup;
mod ocr;
mod pages;
mod palette;
mod present;
mod print;
mod recent;
mod record;
mod redact;
mod rotate;
mod search;
mod select;
mod sidebar;
mod signature;
mod stamps;
mod start;
mod toolbar;
mod view;

#[cfg(test)]
mod ui_tests;
