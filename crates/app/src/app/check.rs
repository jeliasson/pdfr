//! Check the form (Ctrl+Shift+E): answers that look wrong and boxes still
//! empty, each with a way to go there; and, with AI on, a plain-words
//! explanation of the form made from its headings and labels only. Saving
//! asks first when an answer looks wrong (Settings → General).

use super::*;
use crate::assist_ui::widgets::{
    Colors, card, link, panel_frame, primary_button, row_divider, secondary_button,
};
use egui::{Align, Layout, RichText};
use pdfr_assist::check::Severity;
use std::sync::mpsc::Receiver;

/// Empty boxes listed before "and N more".
const EMPTY_SHOWN: usize = 6;

struct Item {
    id: u64,
    severity: Severity,
    what: String,
    message: String,
    page: usize,
}

enum Explain {
    Idle,
    Waiting(Receiver<Result<String, String>>),
    Done(String),
    Failed(String),
}

pub(super) struct CheckDialog {
    items: Vec<Item>,
    /// Opened by saving: `Some(save_as)`; the save goes ahead on "Save anyway".
    on_save: Option<bool>,
    explain: Explain,
}

impl App {
    fn findings(&mut self) -> Vec<Item> {
        let Some(doc) = self.doc.as_mut() else {
            return vec![];
        };
        self.assist
            .check_doc(doc)
            .into_iter()
            .filter_map(|(i, severity, message)| {
                let e = doc.entries.get(i)?;
                let what =
                    e.f.label
                        .clone()
                        .or_else(|| e.f.tooltip.clone())
                        .filter(|l| !l.trim().is_empty())
                        .unwrap_or_else(|| "A box".into());
                Some(Item {
                    id: e.id,
                    severity,
                    what,
                    message,
                    page: e.f.page,
                })
            })
            .collect()
    }

    /// Opens the check (from saving when `on_save` is set).
    pub(super) fn open_check(&mut self, on_save: Option<bool>) {
        let mut items = self.findings();
        // A date split over day, month and year boxes is one empty answer.
        items.dedup_by(|b, a| {
            a.severity == Severity::Empty
                && b.severity == Severity::Empty
                && a.what == b.what
                && a.page == b.page
        });
        self.check = Some(CheckDialog {
            items,
            on_save,
            explain: Explain::Idle,
        });
    }

    /// Saves, first pointing out answers that look wrong (if wanted).
    pub(super) fn save_checked(&mut self, save_as: bool) {
        let wanted = self.assist.a.store.settings.check_before_save;
        if wanted
            && self
                .findings()
                .iter()
                .any(|i| i.severity == Severity::Problem)
        {
            self.open_check(Some(save_as));
        } else {
            self.save(save_as);
        }
    }

    fn start_explain(&mut self) {
        let Some(doc) = self.doc.as_mut() else { return };
        let Some((backend, prompt)) = self.assist.explain_job(doc) else {
            return;
        };
        let (tx, rx) = channel();
        let ctx = self.egui_ctx.clone();
        std::thread::spawn(move || {
            let r = backend
                .run(pdfr_assist::check::EXPLAIN_INSTRUCTION, &prompt)
                .map(|r| r.text.trim().to_string());
            let _ = tx.send(r);
            ctx.request_repaint();
        });
        if let Some(d) = self.check.as_mut() {
            d.explain = Explain::Waiting(rx);
        }
    }

    pub(super) fn check_window(&mut self, ctx: &egui::Context) {
        let Some(dlg) = self.check.as_mut() else {
            return;
        };
        if let Explain::Waiting(rx) = &dlg.explain
            && let Ok(r) = rx.try_recv()
        {
            dlg.explain = match r {
                Ok(t) if !t.is_empty() => Explain::Done(t),
                Ok(_) => Explain::Failed("The AI gave no answer.".into()),
                Err(e) => Explain::Failed(format!("Couldn't ask the AI: {e}")),
            };
        }
        let pal = self.palette.clone();
        let c = Colors::new(&pal);
        let ai_ok = self.assist.a.ai_allowed();
        let mut close = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
        let mut go_to = None;
        let mut save_anyway = false;
        let mut explain = false;
        let dlg = self.check.as_ref().unwrap();
        let problems: Vec<&Item> = dlg
            .items
            .iter()
            .filter(|i| i.severity == Severity::Problem)
            .collect();
        let empty: Vec<&Item> = dlg
            .items
            .iter()
            .filter(|i| i.severity == Severity::Empty)
            .collect();
        egui::Modal::new(Id::unique("pdfr-check"))
            .frame(panel_frame(ctx, &pal, &c, 20))
            .show(ctx, |ui| {
                ui.set_width(super::signature::PAD_W);
                ui.spacing_mut().item_spacing = vec2(8.0, 10.0);
                ui.label(
                    RichText::new("Check the form")
                        .size(20.0)
                        .strong()
                        .color(c.text),
                );
                ui.label(RichText::new(summary(problems.len(), empty.len())).color(c.weak));
                egui::ScrollArea::vertical()
                    .max_height(380.0)
                    .id_salt("pdfr-check-list")
                    .show(ui, |ui| {
                        if !problems.is_empty() {
                            card(ui, &c, Some("Look again"), |ui| {
                                for (n, it) in problems.iter().enumerate() {
                                    if n > 0 {
                                        row_divider(ui, &c);
                                    }
                                    if item_row(ui, &c, it, true) {
                                        go_to = Some(it.id);
                                    }
                                }
                            });
                        }
                        if !empty.is_empty() {
                            card(ui, &c, Some("Still empty"), |ui| {
                                for (n, it) in empty.iter().take(EMPTY_SHOWN).enumerate() {
                                    if n > 0 {
                                        row_divider(ui, &c);
                                    }
                                    if item_row(ui, &c, it, false) {
                                        go_to = Some(it.id);
                                    }
                                }
                                if empty.len() > EMPTY_SHOWN {
                                    ui.label(
                                        RichText::new(format!(
                                            "and {} more (Ctrl+N goes from one to the next)",
                                            empty.len() - EMPTY_SHOWN
                                        ))
                                        .size(12.0)
                                        .color(c.weak),
                                    );
                                }
                            });
                        }
                        card(ui, &c, Some("About this form"), |ui| match &dlg.explain {
                            Explain::Idle if ai_ok => {
                                ui.label(
                                    RichText::new(
                                        "Have the AI explain what this form is for and what each part asks. \
                                         Only headings and box labels are sent, never your answers.",
                                    )
                                    .size(12.0)
                                    .color(c.weak),
                                );
                                explain = secondary_button(ui, &c, "Explain this form").clicked();
                            }
                            Explain::Idle => {
                                ui.label(
                                    RichText::new(
                                        "Turn on AI in Settings (AI section) to have the form explained in plain words. \
                                         Only headings and box labels are sent, never your answers.",
                                    )
                                    .size(12.0)
                                    .color(c.weak),
                                );
                            }
                            Explain::Waiting(_) => {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    ui.label(RichText::new("Reading the form…").color(c.weak));
                                });
                            }
                            Explain::Done(t) => {
                                ui.add(egui::Label::new(RichText::new(t).color(c.text)).wrap());
                            }
                            Explain::Failed(e) => {
                                ui.label(RichText::new(e).color(c.weak));
                            }
                        });
                    });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| match dlg.on_save {
                    Some(_) => {
                        close |= primary_button(ui, &c, "Fix first").clicked();
                        save_anyway = secondary_button(ui, &c, "Save anyway").clicked();
                    }
                    None => close |= primary_button(ui, &c, "Done").on_hover_text("Esc").clicked(),
                });
            });
        if explain {
            self.start_explain();
        }
        if let Some(id) = go_to {
            self.check = None;
            self.scroll_to_field(id);
            self.focus_field(id, true, ctx);
        } else if save_anyway {
            let save_as = self.check.take().and_then(|d| d.on_save).unwrap_or(false);
            self.save(save_as);
        } else if close {
            self.check = None;
        }
    }
}

/// "2 answers look wrong · 5 boxes are empty".
fn summary(problems: usize, empty: usize) -> String {
    let plural = |n: usize, one: &str, many: &str| {
        if n == 1 {
            format!("1 {one}")
        } else {
            format!("{n} {many}")
        }
    };
    match (problems, empty) {
        (0, 0) => "Everything is filled in and nothing looks wrong.".into(),
        (0, e) => format!(
            "Nothing looks wrong. {} still empty.",
            plural(e, "box is", "boxes are")
        ),
        (p, 0) => format!("{}.", plural(p, "answer looks wrong", "answers look wrong")),
        (p, e) => format!(
            "{} · {} still empty.",
            plural(p, "answer looks wrong", "answers look wrong"),
            plural(e, "box is", "boxes are")
        ),
    }
}

/// One finding: what the box is, what's wrong, and a link to it. True when
/// the link is clicked.
fn item_row(ui: &mut egui::Ui, c: &Colors, it: &Item, problem: bool) -> bool {
    let mut clicked = false;
    ui.horizontal(|ui| {
        let w = ui.available_width();
        ui.vertical(|ui| {
            ui.set_max_width(w - 80.0);
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.add(egui::Label::new(RichText::new(&it.what).color(c.text)).truncate());
            if problem {
                ui.add(
                    egui::Label::new(
                        RichText::new(&it.message)
                            .size(12.0)
                            .color(Color32::from_rgb(200, 140, 30)),
                    )
                    .wrap(),
                );
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            clicked = link(ui, c, &format!("Page {}", it.page + 1)).clicked();
        });
    });
    clicked
}
