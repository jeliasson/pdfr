//! Redacting (Ctrl+Shift+X): marking areas to black out, by dragging boxes,
//! from the selected text or from every match of a search, then writing a
//! new copy with what is under them removed for good (`pdfr_core::redact`).
//! The original file is never overwritten.

use super::*;
use crate::assist_ui::widgets::{
    Colors, DANGER, danger_button, note, panel_frame, primary_button, row_divider, secondary_button,
};
use egui::{Align, Align2, Layout, RichText};
use pdfr_core::redact::Mark;
use pdfr_core::text::selection;

/// An area marked for redaction, in display space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RedactMark {
    pub page: usize,
    pub rect: PRect,
}

/// What a finished redaction says: where it went and what to tell.
type JobResult = Result<(PathBuf, String), String>;

#[derive(Default)]
pub(super) struct RedactState {
    /// The "Search and redact" box.
    pub query: String,
    /// The "apply?" dialog is open.
    pub confirm: bool,
    /// The redaction being written.
    job: Option<Receiver<JobResult>>,
}

const RED: Color32 = Color32::from_rgb(0xd0, 0x2a, 0x2a);

impl App {
    /// Marks an area (one undo step). Tiny ones are ignored.
    pub(super) fn add_redactions(&mut self, marks: Vec<RedactMark>) -> usize {
        let marks: Vec<RedactMark> = marks
            .into_iter()
            .filter(|m| m.rect.width() >= 2.0 && m.rect.height() >= 2.0)
            .collect();
        if marks.is_empty() {
            return 0;
        }
        self.snapshot();
        let n = marks.len();
        if let Some(doc) = self.doc.as_mut() {
            doc.redactions.extend(marks);
        }
        n
    }

    /// The mark under a page point, newest first.
    fn redaction_at(&self, page: usize, x: f32, y: f32) -> Option<usize> {
        let doc = self.doc.as_ref()?;
        doc.redactions
            .iter()
            .rposition(|m| m.page == page && m.rect.contains(x, y))
    }

    /// Pointer with the redact tool: drag a box; click a box to remove it.
    pub(super) fn redact_mouse(&mut self, resp: &egui::Response, rects: &[Rect], p: Pos2) {
        let real_click = resp.ctx.input(|i| i.pointer.any_released());
        // Where the button went down: a drag starts a little later.
        let press = resp.ctx.input(|i| i.pointer.press_origin()).unwrap_or(p);
        if resp.drag_started()
            && let Some((page, pt)) = self.page_point(rects, press)
        {
            self.drag_start = Some((page, pt));
        }
        if resp.drag_stopped()
            && let Some((page, a)) = self.drag_start.take()
            && let Some(pr) = rects.get(page)
        {
            let b = PageXf {
                origin: pr.min,
                zoom: self.zoom,
            }
            .to_page(p);
            if self.add_redactions(vec![RedactMark {
                page,
                rect: PRect::from_points(a, b),
            }]) > 0
            {
                self.status = "Marked. Click a box to take it away.".into();
            }
        } else if resp.clicked()
            && real_click
            && let Some((page, (x, y))) = self.page_point(rects, p)
            && let Some(i) = self.redaction_at(page, x, y)
        {
            self.snapshot();
            if let Some(doc) = self.doc.as_mut() {
                doc.redactions.remove(i);
            }
            self.status = "Mark removed.".into();
        }
    }

    /// Marks the selected page text. Returns how many areas were marked.
    pub(super) fn redact_selection(&mut self) -> usize {
        let Some(sel) = &self.text_sel else { return 0 };
        let Some(runs) = self.doc.as_ref().and_then(|d| d.text.get(&sel.page)) else {
            return 0;
        };
        let marks: Vec<RedactMark> = selection(runs, sel.anchor, sel.head)
            .into_iter()
            .map(|(ri, from, to)| {
                let r = &runs[ri];
                let (a, b) = (r.char_rect(from), r.char_rect(to - 1));
                RedactMark {
                    page: sel.page,
                    rect: PRect::new(a.x0, r.rect.y0, b.x1, r.rect.y1).inset(-0.5),
                }
            })
            .collect();
        let n = self.add_redactions(marks);
        if n > 0 {
            self.text_sel = None;
            self.status = "The selected text is marked.".into();
        }
        n
    }

    /// Marks every match of `query` in the pages read so far.
    pub(super) fn redact_matches(&mut self, query: &str) -> usize {
        self.update_search(query);
        let marks: Vec<RedactMark> = self
            .search
            .as_ref()
            .map(|s| {
                s.hits
                    .iter()
                    .flat_map(|h| {
                        h.rects.iter().map(|r| RedactMark {
                            page: h.page,
                            rect: r.inset(-0.5),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let hits = self.search.as_ref().map_or(0, |s| s.hits.len());
        if let Some(s) = self.search.as_mut() {
            s.shown = false;
        }
        self.add_redactions(marks);
        self.status = match hits {
            0 => format!(
                "\u{201c}{}\u{201d} isn't on the pages read so far.",
                query.trim()
            ),
            1 => "Marked 1 place.".into(),
            n => format!("Marked {n} places."),
        };
        hits
    }

    /// Paints the marked areas: a red outline with a light fill.
    pub(super) fn draw_redactions(
        &self,
        painter: &egui::Painter,
        rects: &[Rect],
        pointer: Option<Pos2>,
    ) {
        let Some(doc) = &self.doc else { return };
        for m in &doc.redactions {
            let Some(pr) = rects.get(m.page) else {
                continue;
            };
            let r = PageXf {
                origin: pr.min,
                zoom: self.zoom,
            }
            .rect(&m.rect);
            painter.rect_filled(r, 1.0, RED.gamma_multiply(0.18));
            painter.rect_stroke(r, 1.0, Stroke::new(1.5, RED), egui::StrokeKind::Outside);
        }
        if let (Tool::Redact, Some((page, start)), Some(p)) = (self.tool, self.drag_start, pointer)
            && let Some(pr) = rects.get(page)
        {
            let a = PageXf {
                origin: pr.min,
                zoom: self.zoom,
            }
            .pt(start.0, start.1);
            let r = Rect::from_two_pos(a, p);
            painter.rect_filled(r, 1.0, RED.gamma_multiply(0.12));
            painter.rect_stroke(r, 1.0, Stroke::new(1.5, RED), egui::StrokeKind::Outside);
        }
    }

    /// The tool's panel, the "apply?" dialog, and the running job.
    pub(super) fn redact_windows(&mut self, ctx: &egui::Context) {
        self.redact_poll();
        if self.redact.confirm {
            self.redact_confirm(ctx);
        } else if self.tool == Tool::Redact && self.doc.is_some() {
            self.redact_panel(ctx);
        }
    }

    fn redact_panel(&mut self, ctx: &egui::Context) {
        let pal = self.palette.clone();
        let c = Colors::new(&pal);
        let n = self.doc.as_ref().map_or(0, |d| d.redactions.len());
        let has_sel = self.text_sel.is_some();
        let busy = self.redact.job.is_some();
        let (mut mark_sel, mut mark_all, mut apply, mut close) = (false, false, false, false);
        let pos = self.viewport.right_top() + vec2(-16.0, 12.0);
        egui::Area::new(Id::unique("pdfr-redact-panel"))
            .fixed_pos(pos)
            .pivot(Align2::RIGHT_TOP)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                panel_frame(ctx, &pal, &c, 14).show(ui, |ui| {
                    ui.set_width(280.0);
                    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Redact").size(16.0).strong().color(c.text));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if crate::assist_ui::widgets::icon_button(ui, &c, crate::icons::Icon::Close, "Done (Esc)")
                                .clicked()
                            {
                                close = true;
                            }
                        });
                    });
                    note(
                        ui,
                        &c,
                        "Drag boxes over what should go. Click a box to take it away.",
                    );
                    let sel = ui
                        .add_enabled_ui(has_sel, |ui| secondary_button(ui, &c, "Mark the selected text"))
                        .inner;
                    if sel.clicked() {
                        mark_sel = true;
                    }
                    if !has_sel {
                        sel.on_disabled_hover_text("Select text first: put the tool down (Esc), drag over the text, then come back");
                    }
                    row_divider(ui, &c);
                    ui.label(RichText::new("Search and redact").size(12.0).color(c.weak));
                    ui.horizontal(|ui| {
                        let r = ui.add(
                            egui::TextEdit::singleline(&mut self.redact.query)
                                .hint_text("A word or number")
                                .desired_width(170.0),
                        );
                        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                        let ok = !self.redact.query.trim().is_empty();
                        if ui
                            .add_enabled_ui(ok, |ui| secondary_button(ui, &c, "Mark all"))
                            .inner
                            .clicked()
                            || (enter && ok)
                        {
                            mark_all = true;
                        }
                    });
                    row_divider(ui, &c);
                    ui.horizontal(|ui| {
                        let t = match n {
                            0 => "Nothing marked yet".to_string(),
                            1 => "1 area marked".to_string(),
                            n => format!("{n} areas marked"),
                        };
                        ui.label(RichText::new(t).color(c.weak));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let label = if busy { "Redacting…" } else { "Apply redactions…" };
                            if ui
                                .add_enabled_ui(n > 0 && !busy, |ui| primary_button(ui, &c, label))
                                .inner
                                .clicked()
                            {
                                apply = true;
                            }
                        });
                    });
                });
            });
        if close {
            self.tool = Tool::None;
        }
        if mark_sel {
            self.redact_selection();
        }
        if mark_all {
            let q = self.redact.query.clone();
            self.redact_matches(&q);
        }
        if apply {
            self.redact.confirm = true;
        }
    }

    /// "Apply redactions?": what happens, then where the copy goes.
    fn redact_confirm(&mut self, ctx: &egui::Context) {
        let pal = self.palette.clone();
        let c = Colors::new(&pal);
        let Some(doc) = &self.doc else {
            self.redact.confirm = false;
            return;
        };
        let n = doc.redactions.len();
        let signed = !doc.form.digital.is_empty();
        let mut cancel = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
        let mut go = false;
        egui::Modal::new(Id::unique("pdfr-redact-confirm"))
            .frame(panel_frame(ctx, &pal, &c, 20))
            .show(ctx, |ui| {
                ui.set_width(460.0);
                ui.spacing_mut().item_spacing = vec2(8.0, 10.0);
                ui.label(RichText::new("Apply redactions?").size(20.0).strong().color(c.text));
                let areas = if n == 1 { "the marked area".to_string() } else { format!("the {n} marked areas") };
                ui.label(
                    RichText::new(format!(
                        "Everything under {areas} is removed for good: text, pictures and form \
                         fields. It can't be brought back from the new copy."
                    ))
                    .color(c.text),
                );
                ui.label(
                    RichText::new(
                        "pdfr saves the result as a new file and opens it. The original stays as it is.",
                    )
                    .color(c.weak),
                );
                if signed {
                    ui.label(
                        RichText::new(
                            "This file is digitally signed. Redacting changes it, so its signatures \
                             will no longer be valid.",
                        )
                        .color(DANGER)
                        .strong(),
                    );
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if danger_button(ui, "Redact and save as…", true).clicked() {
                        go = true;
                    }
                    if secondary_button(ui, &c, "Cancel").on_hover_text("Esc").clicked() {
                        cancel = true;
                    }
                });
            });
        if cancel {
            self.redact.confirm = false;
        } else if go {
            self.redact.confirm = false;
            self.ask_redacted_file();
        }
    }

    /// Asks where the redacted copy goes: "<name> (redacted).pdf" by default.
    fn ask_redacted_file(&mut self) {
        let Some(doc) = &self.doc else { return };
        if self.dialog_open() {
            return;
        }
        let stem = doc
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "document".into());
        let name = format!("{stem} (redacted).pdf");
        let generation = doc.generation;
        if !super::files::native_dialogs() {
            let mut d = super::files::builtin::dialog(doc.path.parent(), Some(&name));
            d.save_file();
            self.builtin_dialog =
                Some((d, super::files::builtin::Purpose::RedactedSave(generation)));
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
        self.status = "Choose where to save the redacted copy.".into();
        self.wait_for_dialog(async move {
            super::files::DialogResult::RedactedSave(
                task.await.map(|h| h.path().to_path_buf()),
                generation,
            )
        });
    }

    /// The file for the redacted copy was chosen (or not): redact and save
    /// on a background thread, then open the copy.
    pub(super) fn redacted_save(&mut self, target: Option<PathBuf>, generation: u64) {
        let Some(target) = target else {
            self.status = "Not redacted.".into();
            return;
        };
        let Some(doc) = self.doc.as_ref().filter(|d| d.generation == generation) else {
            self.status = "Not redacted: another document was opened meanwhile.".into();
            return;
        };
        let same = |a: &std::path::Path, b: &std::path::Path| {
            a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y)
        };
        if same(&target, &doc.path) {
            self.status = "Not redacted: choose a new name, so the original stays as it is.".into();
            return;
        }
        let form = doc.form.clone();
        let fields = doc.fields();
        let marks: Vec<Mark> = doc
            .redactions
            .iter()
            .filter_map(|m| {
                Some(Mark {
                    page: m.page,
                    rect: form.pages.get(m.page)?.rect_to_user(&m.rect),
                })
            })
            .collect();
        let ctx = self.egui_ctx.clone();
        let (tx, rx) = channel();
        self.status = "Redacting…".into();
        std::thread::spawn(move || {
            let _ = tx.send(redact_job(&form, &fields, &marks, &target));
            ctx.request_repaint();
        });
        self.redact.job = Some(rx);
    }

    fn redact_poll(&mut self) {
        let Some(rx) = &self.redact.job else { return };
        let Ok(res) = rx.try_recv() else { return };
        self.redact.job = None;
        match res {
            Ok((path, msg)) => {
                self.tool = Tool::None;
                // The marks are done with; unsaved field changes of the
                // original are still asked about.
                if let Some(doc) = self.doc.as_mut() {
                    doc.redactions.clear();
                }
                self.request_open(path);
                self.status = msg;
            }
            Err(e) => self.status = format!("Could not redact: {e}"),
        }
    }
}

/// Saves the fields into the document, redacts it and writes the copy.
fn redact_job(
    form: &pdfr_core::forms::FormDoc,
    fields: &[Field],
    marks: &[Mark],
    target: &std::path::Path,
) -> JobResult {
    let bytes = pdfr_core::save::save(form, fields).map_err(|e| e.to_string())?;
    let out = pdfr_core::redact::redact(&bytes, marks)?;
    std::fs::write(target, &out.bytes).map_err(|e| e.to_string())?;
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut msg = format!("Redacted. Saved as {name}.");
    if out.signatures_broken {
        msg.push_str(" Its digital signatures are no longer valid.");
    }
    Ok((target.to_path_buf(), msg))
}
