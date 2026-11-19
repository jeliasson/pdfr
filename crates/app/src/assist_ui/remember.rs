//! The dialog after saving: what to remember, person by person, in the same
//! design as Settings (cards, rows, accent checkboxes).

use super::AssistState;
use super::widgets::*;
use crate::icons::{self, Icon};
use crate::theme::Palette;
use eframe::egui::{
    self, Align, CornerRadius, Frame, Id, Layout, Margin, RichText, Sense, Stroke, Ui, vec2,
};
use pdfr_assist::memory::Remember;
use pdfr_assist::{LearnGroup, LearnPlan};

const WIDTH: f32 = 520.0;

impl AssistState {
    pub fn remember_window(&mut self, ctx: &egui::Context, pal: &Palette) {
        let Some((plan, _, _)) = &mut self.pending_learn else {
            return;
        };
        let c = Colors::new(pal);
        let mut decision: Option<bool> = None;
        let dont_ask = &mut self.dont_ask_again;
        egui::Modal::new(Id::unique("pdfr-remember")).frame(panel_frame(ctx, pal, &c, 24)).show(ctx, |ui| {
            ui.set_width(WIDTH);
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            ui.label(RichText::new("Remember what you entered?").size(20.0).strong().color(c.text));
            ui.add(
                egui::Label::new(
                    RichText::new("pdfr can offer these again on other forms, each person's details together. They stay on this computer.")
                        .color(c.weak),
                )
                .wrap(),
            );
            ui.add_space(6.0);
            egui::ScrollArea::vertical().max_height(400.0).auto_shrink([false, true]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 12.0;
                for g in &mut plan.groups {
                    group_card(ui, &c, g);
                }
            });
            ui.add_space(4.0);
            row_divider(ui, &c);
            ui.add_space(4.0);
            let selected = count_selected(plan);
            ui.horizontal(|ui| {
                let cb = checkbox(ui, &c, *dont_ask);
                let text = ui.add(egui::Label::new(RichText::new("Don't ask again").color(c.weak)).sense(Sense::click()));
                if cb.clicked() || text.clicked() {
                    *dont_ask = !*dont_ask;
                }
                text.on_hover_text("Change this any time in Settings → Autocomplete");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let label = if selected == 0 { "Remember".to_string() } else { format!("Remember {selected}") };
                    if ui.add_enabled_ui(selected > 0, |ui| primary_button(ui, &c, &label)).inner.clicked() {
                        decision = Some(true);
                    }
                    if secondary_button(ui, &c, "Don't remember").clicked() {
                        decision = Some(false);
                    }
                });
            });
        });
        // Esc: the choice that stores nothing.
        if decision.is_none()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            decision = Some(false);
        }
        let Some(remember) = decision else { return };
        let (plan, an, fields) = self.pending_learn.take().unwrap();
        if self.dont_ask_again {
            self.a.store.settings.remember = if remember {
                Remember::Always
            } else {
                Remember::Never
            };
            self.a.save();
        }
        if remember {
            self.a.commit(&an, &fields, &plan);
        }
        self.stale = true;
    }
}

fn count_selected(plan: &LearnPlan) -> usize {
    plan.groups
        .iter()
        .flat_map(|g| &g.items)
        .filter(|i| i.selected)
        .count()
}

/// One person (or "Other details"): a header with select all / none, then
/// one row per value. A click anywhere on a row toggles it.
fn group_card(ui: &mut Ui, c: &Colors, g: &mut LearnGroup) {
    Frame::new()
        .fill(c.card)
        .stroke(Stroke::new(1.0, c.border))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 8.0;
            let n = g.items.len();
            let on = g.items.iter().filter(|i| i.selected).count();
            ui.horizontal(|ui| {
                if g.person.is_some() {
                    avatar(ui, c, &g.title);
                } else {
                    let (r, _) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::hover());
                    icons::paint(ui.painter(), r.shrink(8.0), Icon::List, c.weak);
                }
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.label(RichText::new(&g.title).strong().color(c.text));
                    ui.label(
                        RichText::new(format!("{on} of {n} selected"))
                            .size(12.0)
                            .color(c.weak),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let all = on == n;
                    if link(ui, c, if all { "Select none" } else { "Select all" }).clicked() {
                        for it in &mut g.items {
                            it.selected = !all;
                        }
                    }
                });
            });
            for (i, it) in g.items.iter_mut().enumerate() {
                row_divider(ui, c);
                let row = ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    let cb = checkbox(ui, c, it.selected);
                    ui.add_space(6.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        ui.add(egui::Label::new(RichText::new(&it.value).color(c.text)).wrap());
                        // A value typed in several fields is one row.
                        let label = match it.also.len() {
                            0 => capitalised(&it.label),
                            n => format!("{} \u{b7} {} fields", capitalised(&it.label), n + 1),
                        };
                        ui.label(RichText::new(label).size(12.0).color(c.weak));
                    });
                    ui.allocate_space(vec2(ui.available_width(), 0.0));
                    cb.clicked()
                });
                let hit = ui.interact(
                    row.response.rect,
                    ui.scope_id().with(("remember-row", i, it.field)),
                    Sense::click(),
                );
                if row.inner
                    || hit
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                {
                    it.selected = !it.selected;
                }
            }
        });
}

/// "con D.N.I. nº" -> "Con D.N.I. nº" (labels taken from inside a sentence).
fn capitalised(s: &str) -> String {
    let mut ch = s.chars();
    match ch.next() {
        Some(f) => f.to_uppercase().chain(ch).collect(),
        None => String::new(),
    }
}
