//! The toolbar: icon buttons built from the command registry, grouped,
//! with tooltips naming the shortcut. Tools that stay on are highlighted.

use super::App;
use super::commands::{Command, tooltip};
use crate::icons;
use eframe::egui::{self, Sense, Stroke, vec2};

/// Side of an icon button (pt) and of the icon inside it.
const BUTTON: f32 = 30.0;
const ICON: f32 = 20.0;

impl App {
    pub(crate) fn toolbar(&mut self, ui: &mut egui::Ui) {
        let mut clicked: Option<Command> = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (gi, group) in Command::TOOLBAR.iter().enumerate() {
                if gi > 0 {
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);
                }
                for &c in *group {
                    if self.icon_button(ui, c) {
                        clicked = Some(c);
                    }
                    // The zoom level sits between zoom out and zoom in.
                    if c == Command::ZoomOut && self.doc.is_some() {
                        let pct = ui
                            .add(
                                egui::Label::new(
                                    egui::RichText::new(format!("{:.0}%", self.zoom * 100.0))
                                        .color(self.palette.text),
                                )
                                .sense(Sense::click()),
                            )
                            .on_hover_text("Click for 100%");
                        if pct.clicked() {
                            clicked = Some(Command::ActualSize);
                        }
                    }
                }
            }
            // Settings at the far right.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.icon_button(ui, Command::Settings) {
                    clicked = Some(Command::Settings);
                }
            });
        });
        if let Some(c) = clicked {
            self.run(c);
        }
    }

    /// One icon button. Returns true when clicked.
    fn icon_button(&self, ui: &mut egui::Ui, c: Command) -> bool {
        let enabled = self.can(c);
        let on = self.is_on(c);
        let (rect, resp) = ui.allocate_exact_size(
            vec2(BUTTON, BUTTON),
            if enabled {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        let pal = &self.palette;
        let painter = ui.painter();
        if on {
            painter.rect_filled(rect, 5.0, pal.accent.gamma_multiply(0.25));
            painter.rect_stroke(
                rect,
                5.0,
                Stroke::new(1.0, pal.accent),
                egui::StrokeKind::Inside,
            );
        } else if enabled && resp.hovered() {
            painter.rect_filled(rect, 5.0, pal.button);
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
        enabled && resp.clicked()
    }
}
