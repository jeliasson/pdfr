//! Opening and saving files, and the native file dialogs (attached to the
//! app window).

use super::*;

/// The app window's raw handles, so file dialogs open attached to it (on
/// tiling compositors an unparented portal dialog opens as an unrelated
/// window, behind a floating one).
#[derive(Clone, Copy)]
pub(crate) struct ParentWindow {
    pub(crate) window: raw_window_handle::RawWindowHandle,
    pub(crate) display: raw_window_handle::RawDisplayHandle,
}

// SAFETY: the handles are only used to attach a portal dialog to the window
// (exporting a Wayland surface handle or passing an X11 id), which the
// display libraries allow from any thread; the window outlives the app.
unsafe impl Send for ParentWindow {}

impl raw_window_handle::HasWindowHandle for ParentWindow {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        // SAFETY: the handle is the app's own window, which lives as long as
        // the app; dialogs are created while it exists and rfd only uses the
        // handle to attach the dialog when it opens.
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(self.window) })
    }
}

impl raw_window_handle::HasDisplayHandle for ParentWindow {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        // SAFETY: as above, the display connection lives as long as the app.
        Ok(unsafe { raw_window_handle::DisplayHandle::borrow_raw(self.display) })
    }
}

/// What a file dialog answered.
pub(super) enum DialogResult {
    /// The file picked, and whether it opens in a new window.
    Open(Option<PathBuf>, bool),
    /// Save target, for the document opened as `generation`.
    Save(Option<PathBuf>, u64, AfterSave),
    /// Print result: `Ok(true)` printed, `Ok(false)` cancelled. (macOS
    /// prints from a sheet on the window instead.)
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    Printed(Result<bool, String>),
}

/// What to do once a save has succeeded.
#[derive(Clone)]
pub(super) enum AfterSave {
    Nothing,
    Close,
    Open(PathBuf),
}

/// Why a file did not open.
#[derive(Debug)]
pub enum OpenFail {
    /// It needs a password (`wrong`: the one given did not work).
    Password { wrong: bool },
    Other(String),
}

/// Asking for a protected file's password.
pub(super) struct PasswordAsk {
    pub path: PathBuf,
    pub input: String,
    pub wrong: bool,
    focused: bool,
}

impl PasswordAsk {
    pub fn new(path: PathBuf, wrong: bool) -> PasswordAsk {
        PasswordAsk {
            path,
            input: String::new(),
            wrong,
            focused: false,
        }
    }
}

impl App {
    /// Opens a file, asking first if the current one has unsaved changes.
    pub fn request_open(&mut self, path: PathBuf) {
        if self.doc.as_ref().is_some_and(|d| d.dirty) {
            self.confirm = Some(Confirm::Open(path));
        } else {
            self.open(path);
        }
    }

    /// A file the system asked us to open (macOS Finder): here if this
    /// window is empty, otherwise in a new pdfr window.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(super) fn open_from_system(&mut self, path: PathBuf) {
        self.open_in_new_window(path);
    }

    pub fn open(&mut self, path: PathBuf) {
        self.open_with(path, None);
    }

    /// Opens a file in the background, with its password if it has one.
    pub(super) fn open_with(&mut self, path: PathBuf, password: Option<String>) {
        self.status = format!("Opening {}…", path.display());
        let tx = self.open_tx.clone();
        let started = Instant::now();
        std::thread::spawn(move || {
            use pdfr_core::forms::OpenError;
            let res = std::fs::read(&path)
                .map_err(|e| OpenFail::Other(e.to_string()))
                .and_then(|b| {
                    FormDoc::open_with_password(b, password.as_deref()).map_err(|e| match e {
                        OpenError::NeedsPassword => OpenFail::Password { wrong: false },
                        OpenError::WrongPassword => OpenFail::Password { wrong: true },
                        e => OpenFail::Other(e.to_string()),
                    })
                })
                .map(|form| {
                    let render = form.render_bytes();
                    Opened { form, render }
                });
            let _ = tx.send((path, res, started));
        });
    }

    /// The question for a protected file's password, while open.
    pub(super) fn password_window(&mut self, ctx: &egui::Context) {
        use crate::assist_ui::widgets::{Colors, panel_frame, primary_button, secondary_button};
        let Some(ask) = self.password.as_mut() else { return };
        let pal = self.palette.clone();
        let c = Colors::new(&pal);
        let (mut open, mut cancel) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        let name = ask
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        egui::Modal::new(Id::unique("pdfr-password"))
            .frame(panel_frame(ctx, &pal, &c, 20))
            .show(ctx, |ui| {
                ui.set_width(380.0);
                ui.spacing_mut().item_spacing = vec2(8.0, 10.0);
                ui.label(egui::RichText::new("Password needed").size(18.0).strong().color(c.text));
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(format!("\u{201c}{name}\u{201d} is protected. Enter its password to open it."))
                            .color(c.weak),
                    )
                    .wrap(),
                );
                let te = ui.add(
                    egui::TextEdit::singleline(&mut ask.input)
                        .password(true)
                        .hint_text("Password")
                        .desired_width(f32::INFINITY),
                );
                if !te.has_focus() && !ask.focused {
                    te.request_focus();
                    ask.focused = true;
                }
                if ask.wrong {
                    ui.label(
                        egui::RichText::new("That password doesn't open it. Try again.")
                            .color(crate::assist_ui::widgets::DANGER),
                    );
                }
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        open |= primary_button(ui, &c, "Open").clicked();
                        cancel |= secondary_button(ui, &c, "Cancel").on_hover_text("Esc").clicked();
                    });
                });
            });
        if cancel {
            self.password = None;
            self.status = format!("Not opened: {name} needs its password.");
        } else if open && !ask.input.is_empty() {
            let ask = self.password.take().unwrap();
            // The password is used once to read the file and not kept.
            self.open_with(ask.path, Some(ask.input));
        }
    }

    pub(super) fn finish_open(
        &mut self,
        path: PathBuf,
        opened: Opened,
        started: Instant,
        ctx: &egui::Context,
    ) {
        // The file being left keeps its place in the recent list.
        self.remember_position();
        let Opened { form, render } = opened;
        let sizes: Vec<(f32, f32)> = form.pages.iter().map(|g| g.display_size()).collect();
        let n = sizes.len();
        let editable = form.editable();
        let form = Arc::new(form);
        let generation = self.worker.set_document(render, n, editable);
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut doc = Doc {
            path,
            name,
            form: form.clone(),
            entries: vec![],
            next_id: 1,
            order: vec![],
            sizes,
            tex: HashMap::new(),
            pending: HashMap::new(),
            generation,
            detected: vec![!editable; n],
            save_path: None,
            dirty: false,
            opened_at: started,
            first_page_ms: None,
            detect_done_ms: None,
            text: HashMap::new(),
            text_links: HashMap::new(),
            thumbs: HashMap::new(),
        };
        for f in &form.fields {
            doc.add(f.clone());
        }
        doc.reorder();
        self.focus = None;
        self.undo.clear();
        self.redo.clear();
        self.scroll = Vec2::ZERO;
        // Per-document UI state: ids restart with every document.
        self.choice_open = None;
        self.ctx_field = None;
        self.typing_run = None;
        self.drag_start = None;
        self.tool = Tool::None;
        self.assist.close_list();
        self.text_sel = None;
        self.page_menu_open = false;
        self.back.clear();
        self.side_follow = None;
        self.thumbs_asked.clear();
        self.outline_open.clear();
        if let Some(s) = &mut self.search {
            let q = std::mem::take(&mut s.query);
            *s = Default::default();
            s.query = q;
            s.shown = false;
        }
        self.doc = Some(doc);
        self.zoom = self
            .cli
            .zoom
            .unwrap_or_else(|| self.fit_width_zoom().clamp(0.5, 1.6));
        self.status = match &form.read_only_reason {
            Some(r) => format!("Read-only: {r}."),
            None => format!("{} page{}.", n, if n == 1 { "" } else { "s" }),
        };
        self.restore = None;
        let p = self.doc.as_ref().unwrap().path.clone();
        self.opened_recent(&p);
        self.assist.on_open(self.doc.as_mut().unwrap());
        ctx.request_repaint();
    }

    /// Whether a file dialog is open (the app keeps running meanwhile).
    pub(super) fn dialog_open(&self) -> bool {
        let cups = {
            #[cfg(target_os = "linux")]
            {
                self.cups_dialog.is_some()
            }
            #[cfg(not(target_os = "linux"))]
            {
                false
            }
        };
        self.dialog_rx.is_some() || self.builtin_dialog.is_some() || cups
    }

    /// Draws pdfr's own file dialog (desktops without a file chooser
    /// portal) and acts on its answer.
    pub(super) fn builtin_dialog_ui(&mut self, ctx: &egui::Context) {
        let Some((d, _)) = &mut self.builtin_dialog else {
            return;
        };
        d.update(ctx);
        let state = d.state();
        let picked = match state {
            egui_file_dialog::DialogState::Open => return,
            egui_file_dialog::DialogState::Picked(p) => Some(p.clone()),
            _ => None,
        };
        let (_, purpose) = self.builtin_dialog.take().unwrap();
        let res = match purpose {
            builtin::Purpose::Open(new_window) => DialogResult::Open(picked, new_window),
            builtin::Purpose::Save(g, after) => DialogResult::Save(picked, g, after),
        };
        self.handle_dialog(res);
    }

    /// Asks for a file to open, here or in a new window.
    pub(super) fn pick_and_open(&mut self, new_window: bool) {
        if self.dialog_open() {
            return;
        }
        if !native_dialogs() {
            let mut d = builtin::dialog(None, None);
            d.pick_file();
            self.builtin_dialog = Some((d, builtin::Purpose::Open(new_window)));
            return;
        }
        let task = self
            .file_dialog()
            .add_filter("PDF", &["pdf", "PDF"])
            .pick_file();
        self.wait_for_dialog(async move {
            DialogResult::Open(task.await.map(|h| h.path().to_path_buf()), new_window)
        });
    }

    /// Opens a file in a new pdfr window (this window if it is empty).
    pub(super) fn open_in_new_window(&mut self, path: PathBuf) {
        if self.doc.is_none() {
            self.open(path);
            return;
        }
        let Ok(exe) = std::env::current_exe() else {
            self.status = "Could not start another pdfr window.".into();
            return;
        };
        let mut cmd = std::process::Command::new(exe);
        if let Some(d) = &self.cli.data_dir {
            cmd.arg("--data-dir").arg(d);
        }
        // pdfr itself, never another program; the file is the last argument.
        match cmd.arg(&path).spawn() {
            Ok(_) => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.status = format!("Opened {name} in a new window.");
            }
            Err(e) => self.status = format!("Could not open {}: {e}", path.display()),
        }
    }

    /// Waits for a dialog on a background thread, so the window keeps
    /// drawing and answering the compositor (a blocked event loop gets
    /// reported as "not responding"). The dialog itself is created on the
    /// UI thread, which macOS requires.
    pub(super) fn wait_for_dialog(
        &mut self,
        task: impl std::future::Future<Output = DialogResult> + Send + 'static,
    ) {
        self.dialog_in_background(move || pollster::block_on(task));
    }

    /// Runs a dialog to completion on a background thread; its answer
    /// arrives in [`Self::poll_dialog`].
    pub(super) fn dialog_in_background(
        &mut self,
        job: impl FnOnce() -> DialogResult + Send + 'static,
    ) {
        let (tx, rx) = channel();
        let ctx = self.egui_ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(job());
            ctx.request_repaint();
        });
        self.dialog_rx = Some(rx);
    }

    /// Handles a file dialog's answer, if it has arrived.
    pub(super) fn poll_dialog(&mut self) {
        let Some(rx) = &self.dialog_rx else { return };
        let res = match rx.try_recv() {
            Ok(r) => r,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.dialog_rx = None;
                return;
            }
        };
        self.dialog_rx = None;
        self.handle_dialog(res);
    }

    /// Acts on what a file or print dialog answered.
    pub(super) fn handle_dialog(&mut self, res: DialogResult) {
        match res {
            DialogResult::Open(Some(p), false) => self.request_open(p),
            DialogResult::Open(Some(p), true) => self.open_in_new_window(p),
            DialogResult::Save(Some(p), generation, after) => {
                if self
                    .doc
                    .as_ref()
                    .is_some_and(|d| d.generation == generation)
                {
                    if self.save_to(p) {
                        self.after_save(after);
                    }
                } else {
                    self.status = "Not saved: another document was opened meanwhile.".into();
                }
            }
            DialogResult::Printed(Ok(true)) => self.status = "Sent to the printer.".into(),
            DialogResult::Printed(Ok(false)) => self.status = "Printing cancelled.".into(),
            DialogResult::Printed(Err(e)) => self.status = format!("Could not print: {e}"),
            DialogResult::Open(None, _) | DialogResult::Save(None, ..) => {}
        }
    }

    fn after_save(&mut self, after: AfterSave) {
        match after {
            AfterSave::Nothing => {}
            // Closing waits for the "remember?" answer, if one is asked.
            AfterSave::Close => self.close_after_remember = true,
            AfterSave::Open(p) => self.open(p),
        }
    }

    pub(super) fn suggested_name(doc: &Doc) -> String {
        let stem = doc
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "document".into());
        format!("{stem}-filled.pdf")
    }

    pub fn save(&mut self, save_as: bool) {
        self.save_then(save_as, AfterSave::Nothing);
    }

    /// Saves (asking where first if needed), then does `after` if it worked.
    pub(super) fn save_then(&mut self, save_as: bool, after: AfterSave) {
        let Some(doc) = &self.doc else { return };
        if !doc.form.editable() {
            self.status = "This file is read-only; it cannot be saved with fields.".into();
            return;
        }
        if let (false, Some(p)) = (save_as, doc.save_path.clone()) {
            if self.save_to(p) {
                self.after_save(after);
            }
            return;
        }
        if self.dialog_open() {
            return;
        }
        let generation = doc.generation;
        if !native_dialogs() {
            let mut d = builtin::dialog(doc.path.parent(), Some(&Self::suggested_name(doc)));
            d.save_file();
            self.builtin_dialog = Some((d, builtin::Purpose::Save(generation, after)));
            return;
        }
        let mut dlg = self
            .file_dialog()
            .add_filter("PDF", &["pdf"])
            .set_file_name(Self::suggested_name(doc));
        if let Some(d) = doc.path.parent() {
            dlg = dlg.set_directory(d);
        }
        let task = dlg.save_file();
        self.wait_for_dialog(async move {
            DialogResult::Save(
                task.await.map(|h| h.path().to_path_buf()),
                generation,
                after,
            )
        });
    }

    pub fn save_to(&mut self, target: PathBuf) -> bool {
        let Some(doc) = &mut self.doc else {
            return false;
        };
        let fields = doc.fields();
        match pdfr_core::save::save(&doc.form, &fields) {
            Ok(bytes) => {
                let tmp = target.with_extension("pdf.pdfr-tmp");
                let res = std::fs::write(&tmp, &bytes).and_then(|_| std::fs::rename(&tmp, &target));
                match res {
                    Ok(()) => {
                        doc.save_path = Some(target.clone());
                        doc.dirty = false;
                        self.status = if doc.form.was_protected {
                            format!(
                                "Saved {}. The saved copy is not password-protected.",
                                target.display()
                            )
                        } else {
                            format!("Saved {}.", target.display())
                        };
                        self.assist.on_save(doc);
                        self.license.on_save();
                        true
                    }
                    Err(e) => {
                        let _ = std::fs::remove_file(&tmp);
                        self.status = format!("Could not save: {e}");
                        false
                    }
                }
            }
            Err(e) => {
                self.status = format!("Could not save: {e}");
                false
            }
        }
    }

    pub(super) fn file_dialog(&self) -> rfd::AsyncFileDialog {
        let d = rfd::AsyncFileDialog::new();
        match &self.parent {
            Some(p) => d.set_parent(p),
            None => d,
        }
    }
}

/// Whether the system's file dialogs can be used. On Linux they come from
/// the desktop portal, which minimal desktops may lack; then pdfr shows its
/// own (rfd would otherwise silently show nothing, or start another program).
fn native_dialogs() -> bool {
    #[cfg(target_os = "linux")]
    {
        crate::system::portal::has("FileChooser")
    }
    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}

pub(super) mod builtin {
    use super::AfterSave;
    use std::path::Path;

    pub enum Purpose {
        /// Open a file (in a new window when `true`).
        Open(bool),
        Save(u64, AfterSave),
    }

    pub fn dialog(dir: Option<&Path>, name: Option<&str>) -> egui_file_dialog::FileDialog {
        let mut d = egui_file_dialog::FileDialog::new()
            .add_file_filter_extensions("PDF documents", vec!["pdf", "PDF"])
            .default_file_filter("PDF documents")
            .add_save_extension("PDF document", "pdf")
            .default_save_extension("PDF document")
            .default_size([760.0, 500.0])
            .anchor(eframe::egui::Align2::CENTER_CENTER, [0.0, 0.0]);
        if let Some(dir) = dir {
            d = d.initial_directory(dir.to_path_buf());
        }
        if let Some(n) = name {
            d = d.default_file_name(n);
        }
        d
    }
}
