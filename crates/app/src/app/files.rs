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
    Open(Option<PathBuf>),
    /// Save target, for the document opened as `generation`.
    Save(Option<PathBuf>, u64, AfterSave),
    /// Print result: `Ok(true)` printed, `Ok(false)` cancelled.
    Printed(Result<bool, String>),
}

/// What to do once a save has succeeded.
#[derive(Clone)]
pub(super) enum AfterSave {
    Nothing,
    Close,
    Open(PathBuf),
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

    pub fn open(&mut self, path: PathBuf) {
        self.status = format!("Opening {}…", path.display());
        let tx = self.open_tx.clone();
        let started = Instant::now();
        std::thread::spawn(move || {
            let res = std::fs::read(&path)
                .map_err(|e| e.to_string())
                .and_then(|b| FormDoc::open(b).map_err(|e| e.to_string()))
                .map(|form| {
                    let render = form.render_bytes();
                    Opened { form, render }
                });
            let _ = tx.send((path, res, started));
        });
    }

    pub(super) fn finish_open(
        &mut self,
        path: PathBuf,
        opened: Opened,
        started: Instant,
        ctx: &egui::Context,
    ) {
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
        self.doc = Some(doc);
        self.zoom = self
            .cli
            .zoom
            .unwrap_or_else(|| self.fit_width_zoom().clamp(0.5, 1.6));
        self.status = match &form.read_only_reason {
            Some(r) => format!("Read-only: {r}."),
            None => format!("{} page{}.", n, if n == 1 { "" } else { "s" }),
        };
        self.assist.on_open(self.doc.as_mut().unwrap());
        ctx.request_repaint();
    }

    /// Whether a file dialog is open (the app keeps running meanwhile).
    pub(super) fn dialog_open(&self) -> bool {
        self.dialog_rx.is_some()
    }

    pub(super) fn pick_and_open(&mut self) {
        if self.dialog_open() {
            return;
        }
        let task = self
            .file_dialog()
            .add_filter("PDF", &["pdf", "PDF"])
            .pick_file();
        self.wait_for_dialog(async move {
            DialogResult::Open(task.await.map(|h| h.path().to_path_buf()))
        });
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
        match res {
            DialogResult::Open(Some(p)) => self.request_open(p),
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
            DialogResult::Open(None) | DialogResult::Save(None, ..) => {}
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
                        self.status = format!("Saved {}.", target.display());
                        self.assist.on_save(doc);
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
