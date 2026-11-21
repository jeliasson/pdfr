//! The toolbar: icon buttons built from the command registry, each group in
//! a rounded capsule (like Preview and Safari on macOS), with tooltips
//! naming the shortcut. Tools that stay on are highlighted.

use super::App;
use super::commands::{Command, tooltip};
use crate::icons;
use crate::theme::mix;
use eframe::egui::{self, Color32, Rect, Sense, Shape, Stroke, vec2};

/// An icon button (pt) and the icon inside it.
const BUTTON: egui::Vec2 = egui::vec2(32.0, 28.0);
const ICON: f32 = 18.0;
/// Space between a capsule's edge and its buttons, and between capsules.
const INSET: f32 = 3.0;
const GAP: f32 = 10.0;

impl App {
    pub(crate) fn toolbar(&mut self, ui: &mut egui::Ui) {
        let mut clicked: Option<Command> = None;
        let mut anchor = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (gi, group) in Command::TOOLBAR.iter().enumerate() {
                if gi > 0 {
                    ui.add_space(GAP);
                }
                self.capsule(ui, |app, ui| {
                    for (i, &c) in group.iter().enumerate() {
                        if i > 0 {
                            app.divider(ui);
                        }
                        let (hit, rect) = app.icon_button(ui, c);
                        if hit {
                            clicked = Some(c);
                        }
                        if c == Command::Markup {
                            anchor = Some(rect);
                        }
                        // The zoom level sits between zoom out and zoom in.
                        if c == Command::ZoomOut && app.doc.is_some() {
                            app.divider(ui);
                            ui.add_space(6.0);
                            let pct = ui
                                .add(
                                    egui::Label::new(
                                        egui::RichText::new(format!("{:.0}%", app.zoom * 100.0))
                                            .color(app.palette.text),
                                    )
                                    .selectable(false)
                                    .sense(Sense::click()),
                                )
                                .on_hover_text("Click for 100%");
                            ui.add_space(6.0);
                            if pct.clicked() {
                                clicked = Some(Command::ActualSize);
                            }
                        }
                    }
                });
            }
            // Settings at the far right.
            ui.add_space((ui.available_width() - BUTTON.x - 2.0 * INSET).max(GAP));
            self.capsule(ui, |app, ui| {
                if app.icon_button(ui, Command::Settings).0 {
                    clicked = Some(Command::Settings);
                }
            });
        });
        if let Some(c) = clicked {
            self.run(c);
        }
        self.marks_ui.anchor = anchor;
        let ctx = ui.ctx().clone();
        self.markup_menu(&ctx);
    }

    /// The capsule's fill and outline, from the theme.
    fn capsule_colors(&self) -> (Color32, Color32) {
        let p = &self.palette;
        if p.dark {
            (mix(p.toolbar, p.text, 0.07), mix(p.toolbar, p.text, 0.13))
        } else {
            (
                mix(p.toolbar, Color32::WHITE, 0.75),
                mix(p.toolbar, p.text, 0.14),
            )
        }
    }

    /// A rounded capsule behind the buttons `body` adds.
    fn capsule(&self, ui: &mut egui::Ui, body: impl FnOnce(&Self, &mut egui::Ui)) {
        let bg = ui.painter().add(Shape::Noop);
        let h = BUTTON.y + 2.0 * INSET;
        let r = ui
            .allocate_ui_with_layout(
                vec2(0.0, h),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_height(h);
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.add_space(INSET);
                    body(self, ui);
                    ui.add_space(INSET);
                },
            )
            .response
            .rect;
        let (fill, line) = self.capsule_colors();
        ui.painter().set(
            bg,
            egui::epaint::RectShape::new(
                r,
                9.0,
                fill,
                Stroke::new(1.0, line),
                egui::StrokeKind::Inside,
            ),
        );
    }

    /// A thin line between two buttons in a capsule.
    fn divider(&self, ui: &mut egui::Ui) {
        let (r, _) = ui.allocate_exact_size(vec2(1.0, BUTTON.y), Sense::hover());
        let (_, line) = self.capsule_colors();
        ui.painter().rect_filled(
            Rect::from_center_size(r.center(), vec2(1.0, 16.0)),
            0.0,
            line,
        );
    }

    /// One icon button. Returns whether it was clicked, and where it is.
    fn icon_button(&self, ui: &mut egui::Ui, c: Command) -> (bool, Rect) {
        let enabled = self.can(c);
        let on = self.is_on(c);
        let (rect, resp) = ui.allocate_exact_size(
            BUTTON,
            if enabled {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        let pal = &self.palette;
        let painter = ui.painter();
        let inner = rect.shrink(2.0);
        if on {
            painter.rect_filled(inner, 6.0, pal.accent.gamma_multiply(0.22));
            painter.rect_stroke(
                inner,
                6.0,
                Stroke::new(1.0, pal.accent.gamma_multiply(0.7)),
                egui::StrokeKind::Inside,
            );
        } else if enabled && resp.hovered() {
            painter.rect_filled(inner, 6.0, mix(pal.toolbar, pal.text, 0.12));
        }
        let color = if enabled {
            pal.text
        } else {
            pal.weak_text.gamma_multiply(0.6)
        };
        icons::paint(
            painter,
            egui::Rect::from_center_size(rect.center(), vec2(ICON, ICON)),
            c.spec().icon,
            color,
        );
        let resp = resp.on_hover_text(tooltip(ui.ctx(), c));
        let resp = if enabled {
            resp.on_hover_cursor(egui::CursorIcon::PointingHand)
        } else {
            resp
        };
        (enabled && resp.clicked(), rect)
    }
}
