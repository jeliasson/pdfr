//! Reading scanned text: keeping the workers' OCR in step with the
//! "Read scanned text" setting, the one-time download of its models, and
//! reading again the scans found before the models were there.
//!
//! The download fetches two fixed model files (pinned URLs and SHA-256
//! sums in `pdfr_ocr::MODELS`); nothing about the user or their files is
//! sent. Pages are read on this computer only.

use super::App;
use pdfr_core::field::Origin;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, channel};

/// The models' download, running on a background thread.
pub(super) struct Download {
    rx: Receiver<Result<(), String>>,
    /// Bytes fetched so far.
    done: Arc<AtomicU64>,
}

#[derive(Default)]
pub(super) struct OcrState {
    download: Option<Download>,
    /// Why the last download failed (not retried until the setting is
    /// switched off and on, or pdfr restarts).
    failed: Option<String>,
    /// The setting and whether the models are there, as last given to
    /// the workers.
    applied: Option<(bool, bool)>,
}

impl Download {
    fn start(dir: PathBuf, ctx: eframe::egui::Context) -> Download {
        let (tx, rx) = channel();
        let done = Arc::new(AtomicU64::new(0));
        let d = done.clone();
        std::thread::Builder::new()
            .name("pdfr-ocr-download".into())
            .spawn(move || {
                let _ = tx.send(fetch_models(&dir, &d));
                ctx.request_repaint();
            })
            .ok();
        Download { rx, done }
    }

    /// How far along, 0..=100.
    fn percent(&self) -> u64 {
        self.done.load(Ordering::Relaxed) * 100 / pdfr_ocr::download_size().max(1)
    }
}

/// Fetches the models that are missing into `dir`, checked before use.
fn fetch_models(dir: &std::path::Path, done: &AtomicU64) -> Result<(), String> {
    use std::io::Read;
    let agent = crate::updates::agent_with_timeout(120);
    for m in &pdfr_ocr::MODELS {
        if pdfr_ocr::models_present(dir) {
            break;
        }
        let mut resp = agent
            .get(m.url)
            .call()
            .map_err(|e| format!("could not reach the download server: {e}"))?;
        let mut body = resp.body_mut().with_config().limit(m.bytes + 1).reader();
        let mut bytes = Vec::with_capacity(m.bytes as usize);
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = body.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&buf[..n]);
            done.fetch_add(n as u64, Ordering::Relaxed);
        }
        pdfr_ocr::install(dir, m, &bytes)?;
    }
    Ok(())
}

impl App {
    /// Where the OCR models are kept.
    pub(super) fn ocr_dir(&self) -> Option<PathBuf> {
        pdfr_license::data_dir(self.cli.data_dir.as_deref()).map(|d| pdfr_ocr::model_dir(&d))
    }

    fn read_scans_on(&self) -> bool {
        self.assist.a.store.settings.read_scans
    }

    /// Follows the setting and the download: tells the workers whether
    /// scans can be read, and reads again the scans found meanwhile.
    pub(super) fn follow_ocr(&mut self) {
        if let Some(d) = &self.ocr.download
            && let Ok(res) = d.rx.try_recv()
        {
            self.ocr.download = None;
            match res {
                Ok(()) => self.status = "Scanned pages can now be read.".into(),
                Err(e) => {
                    self.status = format!("Could not download the text reader: {e}");
                    self.ocr.failed = Some(e);
                }
            }
            self.ocr.applied = None;
        }
        if self.ocr.download.is_some() {
            self.egui_ctx
                .request_repaint_after(std::time::Duration::from_millis(250));
        }
        let on = self.read_scans_on();
        let dir = self.ocr_dir().filter(|d| on && pdfr_ocr::models_present(d));
        let ready = dir.is_some();
        if self.ocr.applied == Some((on, ready)) {
            return;
        }
        self.ocr.applied = Some((on, ready));
        if !on {
            self.ocr.failed = None;
        }
        self.worker.set_ocr(dir);
        if ready {
            self.reread_scans();
        } else if on
            && self
                .doc
                .as_ref()
                .is_some_and(|d| !d.unread_scans.is_empty())
        {
            self.start_ocr_download();
        }
    }

    /// A scanned page could not be read: fetch the models if wanted.
    pub(super) fn on_unread_scan(&mut self) {
        if self.read_scans_on() {
            self.start_ocr_download();
        }
    }

    fn start_ocr_download(&mut self) {
        if self.ocr.download.is_some()
            || self.ocr.failed.is_some()
            || self.automated()
            || cfg!(test)
        {
            return;
        }
        let Some(dir) = self.ocr_dir() else { return };
        if pdfr_ocr::models_present(&dir) {
            self.ocr.applied = None;
            return;
        }
        self.ocr.download = Some(Download::start(dir, self.egui_ctx.clone()));
    }

    /// The status line's note about reading scanned text, if any.
    pub(super) fn ocr_status(&self) -> Option<String> {
        if let Some(d) = &self.ocr.download {
            let mb = pdfr_ocr::download_size().div_ceil(1_000_000);
            return Some(format!(
                "Reading scanned text needs a one-time {mb} MB download\u{2026} {}%",
                d.percent()
            ));
        }
        let doc = self.doc.as_ref()?;
        let left = doc.sizes.len().saturating_sub(doc.text.len());
        (doc.reading && left > 0).then(|| format!("Reading scanned text\u{2026} {left} pages left"))
    }

    /// Notes where a page's text came from. True for a scan not read.
    pub(super) fn note_scan(doc: &mut super::Doc, page: usize, scan: crate::worker::Scan) -> bool {
        if scan == crate::worker::Scan::Unread {
            doc.unread_scans.insert(page);
            true
        } else {
            doc.unread_scans.remove(&page);
            false
        }
    }

    /// Reads again the scanned pages found while scans could not be read:
    /// their found boxes (still empty) are found again, with labels.
    fn reread_scans(&mut self) {
        let Some(doc) = &mut self.doc else { return };
        if doc.unread_scans.is_empty() {
            return;
        }
        let pages: Vec<usize> = std::mem::take(&mut doc.unread_scans).into_iter().collect();
        let stale = |e: &super::Entry| {
            pages.contains(&e.f.page) && e.f.origin == Origin::Detected && e.f.is_empty()
        };
        doc.entries.retain(|e| !stale(e));
        for snap in self.undo.iter_mut().chain(self.redo.iter_mut()) {
            snap.entries.retain(|e| !stale(e));
        }
        if self
            .focus
            .as_ref()
            .is_some_and(|f| doc.index_of(f.id).is_none())
        {
            self.focus = None;
        }
        let editable = doc.form.editable();
        // Same pages, same places: these are read again, and so are pages
        // whose answer has not arrived (it would be dropped as out of date).
        let same: Vec<Option<usize>> = (0..doc.sizes.len()).map(Some).collect();
        for &p in &pages {
            doc.text.remove(&p);
            doc.text_links.remove(&p);
            if let Some(d) = doc.detected.get_mut(p) {
                *d = !editable;
            }
        }
        let redo: Vec<usize> = (0..doc.sizes.len())
            .filter(|&i| pages.contains(&i) || !doc.detected[i] || !doc.text.contains_key(&i))
            .collect();
        doc.generation =
            self.worker
                .replace_document(doc.form.render_bytes(), &redo, editable, &same);
        doc.content = doc.generation;
        doc.pending.clear();
        doc.reorder();
        self.assist.invalidate();
    }
}
