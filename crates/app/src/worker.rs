//! Background rendering and detection. The UI thread never renders a page.
//!
//! A small pool of threads shares one queue. Each thread parses its own copy
//! of the document (hayro's render cache borrows the parsed file, so the file
//! and cache live together on the thread's stack). The queue keeps only the
//! newest render request per page and serves renders before detection.

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use pdfr_core::field::Field;
use pdfr_core::render::{RenderCache, page_geom, render_page};
use std::collections::{HashMap, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};

/// Largest texture side we render (GPU limits and memory).
pub const MAX_TEX_SIDE: f32 = 8192.0;
/// Width of a sidebar thumbnail image, in pixels.
pub const THUMB_PX: f32 = 240.0;

pub enum Msg {
    /// `requested`: the scale asked for; `scale`: the one used (capped by
    /// the texture size limit).
    Rendered {
        generation: u64,
        page: usize,
        requested: f32,
        scale: f32,
        tex: TextureHandle,
    },
    /// A render could not be made; the page is no longer pending.
    RenderFailed { generation: u64, page: usize },
    /// A small image of a page for the sidebar (`None` if it failed).
    Thumb {
        generation: u64,
        page: usize,
        tex: Option<TextureHandle>,
    },
    Detected {
        generation: u64,
        page: usize,
        fields: Vec<Field>,
        text: Vec<pdfr_core::text::TextRun>,
    },
    /// A page's text, for documents without detection (search, copy).
    Text {
        generation: u64,
        page: usize,
        text: Vec<pdfr_core::text::TextRun>,
    },
}

#[derive(Default)]
struct State {
    generation: u64,
    bytes: Option<Arc<Vec<u8>>>,
    /// page -> (scale, priority); lower priority value is served first.
    renders: HashMap<usize, (f32, f32)>,
    detect: VecDeque<usize>,
    /// Pages whose text is wanted without detection.
    text: VecDeque<usize>,
    /// Sidebar thumbnails wanted, most recent request first.
    thumbs: VecDeque<usize>,
    /// Pages are shown dark (lightness inverted).
    dark: bool,
    quit: bool,
}

struct Shared {
    state: Mutex<State>,
    cv: Condvar,
}

pub struct Worker {
    shared: Arc<Shared>,
    pub rx: Receiver<Msg>,
}

enum Job {
    Render(usize, f32),
    Detect(usize),
    Text(usize),
    Thumb(usize),
}

impl Worker {
    pub fn new(ctx: egui::Context) -> Worker {
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            cv: Condvar::new(),
        });
        let (tx, rx) = channel();
        let threads = std::thread::available_parallelism()
            .map(|n| n.get().saturating_sub(1))
            .unwrap_or(1)
            .clamp(1, 6);
        for i in 0..threads {
            let (shared, tx, ctx) = (shared.clone(), tx.clone(), ctx.clone());
            // With more than one thread, the first never detects: a page
            // scrolled into view must not wait for a detection to finish
            // on every thread.
            let pictures_only = i == 0 && threads > 1;
            std::thread::Builder::new()
                .name(format!("render-{i}"))
                .spawn(move || run(shared, tx, ctx, pictures_only))
                .expect("spawn render thread");
        }
        Worker { shared, rx }
    }

    /// Switches to a new document. Returns its generation number.
    pub fn set_document(&self, bytes: Arc<Vec<u8>>, pages: usize, detect: bool) -> u64 {
        let mut s = self.shared.state.lock().unwrap();
        s.generation += 1;
        s.bytes = Some(bytes);
        s.renders.clear();
        s.thumbs.clear();
        // Detection reads each page's text too; without it, read only the text.
        (s.detect, s.text) = if detect {
            ((0..pages).collect(), VecDeque::new())
        } else {
            (VecDeque::new(), (0..pages).collect())
        };
        self.shared.cv.notify_all();
        s.generation
    }

    /// Switches to a changed version of the same document (pages turned):
    /// only `pages` are detected (or read for text) again. Returns the new
    /// generation number.
    pub fn replace_document(&self, bytes: Arc<Vec<u8>>, pages: &[usize], detect: bool) -> u64 {
        let mut s = self.shared.state.lock().unwrap();
        s.generation += 1;
        s.bytes = Some(bytes);
        s.renders.clear();
        s.thumbs.clear();
        let mut redo: VecDeque<usize> = pages.iter().copied().collect();
        // Pages still waiting from before keep waiting.
        let waiting: Vec<usize> = std::mem::take(&mut s.detect)
            .into_iter()
            .chain(std::mem::take(&mut s.text))
            .collect();
        for p in waiting {
            if !redo.contains(&p) {
                redo.push_back(p);
            }
        }
        (s.detect, s.text) = if detect {
            (redo, VecDeque::new())
        } else {
            (VecDeque::new(), redo)
        };
        self.shared.cv.notify_all();
        s.generation
    }

    /// Requests a render of `page` at `scale` (pixels per point). Replaces any
    /// queued request for that page.
    pub fn request(&self, page: usize, scale: f32, priority: f32) {
        let mut s = self.shared.state.lock().unwrap();
        s.renders.insert(page, (scale, priority));
        self.shared.cv.notify_one();
    }

    /// Shows pages dark (or light again). Images in the making are dropped:
    /// returns the new generation number. Detection carries on.
    pub fn set_dark(&self, dark: bool) -> u64 {
        let mut s = self.shared.state.lock().unwrap();
        s.dark = dark;
        s.generation += 1;
        s.renders.clear();
        s.thumbs.clear();
        self.shared.cv.notify_all();
        s.generation
    }

    /// Asks for sidebar thumbnails of these pages (only these: older
    /// requests are dropped, as they scrolled out of the sidebar).
    pub fn request_thumbs(&self, pages: &[usize]) {
        let mut s = self.shared.state.lock().unwrap();
        s.thumbs = pages.iter().copied().collect();
        if !pages.is_empty() {
            self.shared.cv.notify_all();
        }
    }

    /// Drops queued renders for pages that are no longer wanted.
    pub fn retain(&self, keep: impl Fn(usize) -> bool) {
        let mut s = self.shared.state.lock().unwrap();
        s.renders.retain(|p, _| keep(*p));
    }

    /// Moves detection of these pages to the front of the queue.
    pub fn prioritise_detection(&self, pages: &[usize]) {
        let mut s = self.shared.state.lock().unwrap();
        for &p in pages.iter().rev() {
            if let Some(i) = s.detect.iter().position(|&q| q == p) {
                s.detect.remove(i);
                s.detect.push_front(p);
            }
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if let Ok(mut s) = self.shared.state.lock() {
            s.quit = true;
        }
        self.shared.cv.notify_all();
    }
}

fn run(shared: Arc<Shared>, tx: Sender<Msg>, ctx: egui::Context, pictures_only: bool) {
    let mut generation = 0;
    loop {
        let bytes = {
            let mut s = shared.state.lock().unwrap();
            while !s.quit && (s.bytes.is_none() || s.generation == generation) {
                s = shared.cv.wait(s).unwrap();
            }
            if s.quit {
                return;
            }
            generation = s.generation;
            s.bytes.clone().unwrap()
        };
        serve(&shared, &tx, &ctx, generation, &bytes, pictures_only);
    }
}

fn dark(shared: &Shared) -> bool {
    shared.state.lock().map(|s| s.dark).unwrap_or(false)
}

/// The next job: renders first, then thumbnails, then detection and text
/// (not for a `pictures_only` thread).
fn next_job(shared: &Shared, generation: u64, pictures_only: bool) -> Option<Job> {
    let mut s = shared.state.lock().unwrap();
    loop {
        if s.quit || s.generation != generation {
            return None;
        }
        if let Some((&page, &(scale, _))) = s.renders.iter().min_by(|a, b| a.1.1.total_cmp(&b.1.1))
        {
            s.renders.remove(&page);
            return Some(Job::Render(page, scale));
        }
        if let Some(p) = s.thumbs.pop_front() {
            return Some(Job::Thumb(p));
        }
        if !pictures_only {
            if let Some(p) = s.detect.pop_front() {
                return Some(Job::Detect(p));
            }
            if let Some(p) = s.text.pop_front() {
                return Some(Job::Text(p));
            }
        }
        s = shared.cv.wait(s).unwrap();
    }
}

fn serve(
    shared: &Shared,
    tx: &Sender<Msg>,
    ctx: &egui::Context,
    generation: u64,
    bytes: &Arc<Vec<u8>>,
    pictures_only: bool,
) {
    let Ok(pdf) = hayro::hayro_syntax::Pdf::new(bytes.clone()) else {
        // Unparseable for rendering: answer every job (nothing to show, no
        // fields found) so nothing waits forever.
        while let Some(job) = next_job(shared, generation, pictures_only) {
            let msg = match job {
                Job::Render(page, _) => Msg::RenderFailed { generation, page },
                Job::Detect(page) => Msg::Detected {
                    generation,
                    page,
                    fields: vec![],
                    text: vec![],
                },
                Job::Text(page) => Msg::Text {
                    generation,
                    page,
                    text: vec![],
                },
                Job::Thumb(page) => Msg::Thumb {
                    generation,
                    page,
                    tex: None,
                },
            };
            if tx.send(msg).is_err() {
                return;
            }
            ctx.request_repaint();
        }
        return;
    };
    let mut cache = RenderCache::new();
    let text_cache = hayro::hayro_interpret::InterpreterCache::new();
    let pages = pdf.pages();
    while let Some(job) = next_job(shared, generation, pictures_only) {
        match job {
            Job::Render(page, requested) => {
                let failed =
                    |tx: &Sender<Msg>| tx.send(Msg::RenderFailed { generation, page }).is_ok();
                let Some(p) = pages.get(page) else {
                    if !failed(tx) {
                        return;
                    }
                    continue;
                };
                let (w, h) = page_geom(p).display_size();
                let scale = requested.min(MAX_TEX_SIDE / w.max(h)).max(0.05);
                let res = catch_unwind(AssertUnwindSafe(|| render_page(p, &cache, scale)));
                let Ok(mut img) = res else {
                    cache = RenderCache::new();
                    if !failed(tx) {
                        return;
                    }
                    continue;
                };
                if dark(shared) {
                    img.invert_lightness();
                }
                let image = ColorImage::from_rgba_premultiplied(
                    [img.width as usize, img.height as usize],
                    &img.rgba,
                );
                let tex = ctx.load_texture(format!("page-{page}"), image, TextureOptions::LINEAR);
                if tx
                    .send(Msg::Rendered {
                        generation,
                        page,
                        requested,
                        scale,
                        tex,
                    })
                    .is_err()
                {
                    return;
                }
                ctx.request_repaint();
            }
            Job::Thumb(page) => {
                let tex = pages.get(page).and_then(|p| {
                    let (w, h) = page_geom(p).display_size();
                    let scale = (THUMB_PX / w.max(1.0)).min(THUMB_PX * 2.0 / h.max(1.0));
                    let mut img =
                        catch_unwind(AssertUnwindSafe(|| render_page(p, &cache, scale))).ok()?;
                    if dark(shared) {
                        img.invert_lightness();
                    }
                    let image = ColorImage::from_rgba_premultiplied(
                        [img.width as usize, img.height as usize],
                        &img.rgba,
                    );
                    Some(ctx.load_texture(format!("thumb-{page}"), image, TextureOptions::LINEAR))
                });
                if tx
                    .send(Msg::Thumb {
                        generation,
                        page,
                        tex,
                    })
                    .is_err()
                {
                    return;
                }
                ctx.request_repaint();
            }
            Job::Text(page) => {
                let text = pages
                    .get(page)
                    .and_then(|p| {
                        catch_unwind(AssertUnwindSafe(|| {
                            pdfr_core::text::extract(p, &text_cache)
                        }))
                        .ok()
                    })
                    .unwrap_or_default();
                if tx
                    .send(Msg::Text {
                        generation,
                        page,
                        text,
                    })
                    .is_err()
                {
                    return;
                }
                ctx.request_repaint();
            }
            Job::Detect(page) => {
                let Some(p) = pages.get(page) else {
                    if tx
                        .send(Msg::Detected {
                            generation,
                            page,
                            fields: vec![],
                            text: vec![],
                        })
                        .is_err()
                    {
                        return;
                    }
                    continue;
                };
                let res = catch_unwind(AssertUnwindSafe(|| {
                    pdfr_core::detect::detect_page(p, &cache, page)
                }));
                let fields = res.unwrap_or_else(|_| {
                    cache = RenderCache::new();
                    vec![]
                });
                let text = catch_unwind(AssertUnwindSafe(|| {
                    pdfr_core::text::extract(p, &text_cache)
                }))
                .unwrap_or_default();
                if tx
                    .send(Msg::Detected {
                        generation,
                        page,
                        fields,
                        text,
                    })
                    .is_err()
                {
                    return;
                }
                ctx.request_repaint();
            }
        }
    }
}
