//! Building blocks shared by pdfr's panels and dialogs (Settings, the
//! remember-on-save dialog): theme-derived colours, rounded cards, rows with
//! a title and description, switches, checkboxes and icon buttons. New
//! dialogs are built from these so the app looks the same everywhere.

use crate::icons::{self, Icon};
use crate::theme::{Palette, mix};
use eframe::egui::{
    self, Align, Color32, CornerRadius, Frame, Layout, Margin, RichText, Sense, Stroke, Ui, vec2,
};

pub(crate) const DANGER: Color32 = Color32::from_rgb(0xd0, 0x3e, 0x3e);

/// Colours derived from the theme palette.
pub(crate) struct Colors {
    pub(crate) text: Color32,
    pub(crate) weak: Color32,
    pub(crate) accent: Color32,
    pub(crate) card: Color32,
    pub(crate) border: Color32,
    pub(crate) hover: Color32,
    pub(crate) sidebar: Color32,
}

impl Colors {
    pub(crate) fn new(p: &Palette) -> Colors {
        Colors {
            text: p.text,
            weak: p.weak_text,
            accent: p.accent,
            card: mix(p.window, p.text, if p.dark { 0.05 } else { 0.03 }),
            border: mix(p.window, p.text, 0.12),
            hover: mix(p.window, p.text, 0.08),
            sidebar: mix(p.window, p.text, if p.dark { 0.025 } else { 0.015 }),
        }
    }
}

/// A rounded group of rows, with an optional small heading above it.
pub(crate) fn card(ui: &mut Ui, c: &Colors, heading: Option<&str>, body: impl FnOnce(&mut Ui)) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        if let Some(h) = heading {
            ui.label(RichText::new(h).size(12.0).strong().color(c.weak));
        }
        Frame::new()
            .fill(c.card)
            .stroke(Stroke::new(1.0, c.border))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::symmetric(16, 12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 8.0;
                body(ui);
            });
    });
}

pub(crate) fn row_divider(ui: &mut Ui, c: &Colors) {
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(vec2(w, 1.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, c.border);
}

/// A title and description on the left, a control on the right.
pub(crate) fn setting_row(
    ui: &mut Ui,
    c: &Colors,
    title: &str,
    sub: &str,
    control: impl FnOnce(&mut Ui),
) {
    ui.horizontal(|ui| {
        let w = ui.available_width();
        ui.vertical(|ui| {
            ui.set_max_width(w * 0.62);
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(RichText::new(title).color(c.text));
            ui.add(egui::Label::new(RichText::new(sub).size(12.0).color(c.weak)).wrap());
        });
        ui.with_layout(Layout::right_to_left(Align::Center), control);
    });
}

/// A radio-style row: the whole row is clickable. Returns true when clicked.
pub(crate) fn choice_row(ui: &mut Ui, c: &Colors, selected: bool, title: &str, sub: &str) -> bool {
    let resp = ui
        .horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
            let center = r.center();
            if selected {
                ui.painter().circle_filled(center, 8.0, c.accent);
                ui.painter().circle_filled(center, 3.2, Color32::WHITE);
            } else {
                ui.painter()
                    .circle_stroke(center, 7.5, Stroke::new(1.5, c.weak));
            }
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new(title).color(c.text));
                ui.label(RichText::new(sub).size(12.0).color(c.weak));
            });
            ui.allocate_space(vec2(ui.available_width(), 0.0));
        })
        .response;
    ui.interact(
        resp.rect,
        ui.scope_id().with(("choice", title)),
        Sense::click(),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
    .clicked()
}

/// An on/off switch.
pub(crate) fn toggle(ui: &mut Ui, c: &Colors, on: &mut bool) -> egui::Response {
    let size = vec2(40.0, 22.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool(resp.id, *on);
    let off = mix(c.card, c.text, 0.25);
    let bg = mix(off, c.accent, t);
    let r = rect.height() / 2.0;
    ui.painter().rect_filled(rect, r, bg);
    let x = egui::lerp((rect.left() + r)..=(rect.right() - r), t);
    ui.painter()
        .circle_filled(egui::pos2(x, rect.center().y), r - 3.0, Color32::WHITE);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A small borderless icon button that shows a background on hover.
pub(crate) fn icon_button(ui: &mut Ui, c: &Colors, icon: Icon, tip: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 6.0, c.hover);
    }
    let color = if resp.hovered() { c.text } else { c.weak };
    icons::paint(
        ui.painter(),
        egui::Rect::from_center_size(rect.center(), vec2(16.0, 16.0)),
        icon,
        color,
    );
    resp.on_hover_text(tip)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(crate) fn note(ui: &mut Ui, c: &Colors, text: &str) {
    ui.horizontal(|ui| {
        ui.add_space(2.0);
        ui.add(egui::Label::new(RichText::new(text).size(12.0).color(c.weak)).wrap());
    });
}

pub(crate) fn avatar(ui: &mut Ui, c: &Colors, name: &str) {
    let (r, _) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::hover());
    ui.painter()
        .circle_filled(r.center(), 17.0, c.accent.gamma_multiply(0.85));
    let initials: String = name
        .split_whitespace()
        .filter_map(|w| w.chars().next())
        .filter(|ch| ch.is_alphanumeric())
        .take(2)
        .collect::<String>()
        .to_uppercase();
    ui.painter().text(
        r.center(),
        egui::Align2::CENTER_CENTER,
        initials,
        egui::FontId::proportional(13.0),
        Color32::WHITE,
    );
}

/// The frame of panels and dialogs: the window colour, a thin border,
/// rounded corners and the window shadow.
pub(crate) fn panel_frame(ctx: &egui::Context, pal: &Palette, c: &Colors, margin: i8) -> Frame {
    Frame::window(&ctx.global_style())
        .fill(pal.window)
        .stroke(Stroke::new(1.0, c.border))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(margin))
}

/// A rounded checkbox drawn in the accent colour. Returns the response of
/// its square; the caller decides what a click on a whole row means.
pub(crate) fn checkbox(ui: &mut Ui, c: &Colors, checked: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
    let p = ui.painter();
    if checked {
        p.rect_filled(rect, 4.0, c.accent);
        let s = Stroke::new(2.0, Color32::WHITE);
        let pt = |x: f32, y: f32| rect.min + vec2(x * rect.width(), y * rect.height());
        p.line_segment([pt(0.24, 0.52), pt(0.43, 0.7)], s);
        p.line_segment([pt(0.43, 0.7), pt(0.77, 0.3)], s);
    } else {
        let s = Stroke::new(1.5, if resp.hovered() { c.text } else { c.weak });
        p.rect_stroke(rect.shrink(0.75), 4.0, s, egui::StrokeKind::Inside);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A button with its own fill and outline whose size never changes. egui
/// makes room for the theme's hover outline by shrinking the button's inner
/// margin; overriding the outline with `Button::stroke` keeps that shrink, so
/// the button got smaller on hover. Here every state gets the same outline
/// (and no expansion), and hovering only tints the fill.
pub(crate) fn framed_button(
    ui: &mut Ui,
    text: RichText,
    fill: Color32,
    hover_fill: Color32,
    stroke: Stroke,
    min_width: f32,
) -> egui::Response {
    ui.scope(|ui| {
        let w = &mut ui.visuals_mut().widgets;
        for (v, f) in [
            (&mut w.inactive, fill),
            (&mut w.hovered, hover_fill),
            (&mut w.active, hover_fill),
        ] {
            v.weak_bg_fill = f;
            v.bg_fill = f;
            v.bg_stroke = stroke;
            v.expansion = 0.0;
            v.corner_radius = CornerRadius::same(7);
        }
        ui.add(egui::Button::new(text).min_size(vec2(min_width, 32.0)))
    })
    .inner
    .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The main action of a dialog: filled with the accent colour.
pub(crate) fn primary_button(ui: &mut Ui, c: &Colors, label: &str) -> egui::Response {
    framed_button(
        ui,
        RichText::new(label).color(Color32::WHITE).strong(),
        c.accent,
        mix(c.accent, Color32::WHITE, 0.12),
        Stroke::new(1.0, c.accent),
        0.0,
    )
}

/// A secondary action: outlined.
pub(crate) fn secondary_button(ui: &mut Ui, c: &Colors, label: &str) -> egui::Response {
    framed_button(
        ui,
        RichText::new(label).color(c.text),
        Color32::TRANSPARENT,
        c.hover,
        Stroke::new(1.0, c.border),
        0.0,
    )
}

/// A destructive action with a two-click confirm: outlined in red, filled
/// red once `confirming`.
pub(crate) fn danger_button(ui: &mut Ui, label: &str, confirming: bool) -> egui::Response {
    let (text, fill) = if confirming {
        (Color32::WHITE, DANGER)
    } else {
        (DANGER, Color32::TRANSPARENT)
    };
    framed_button(
        ui,
        RichText::new(label).color(text),
        fill,
        if confirming { mix(DANGER, Color32::WHITE, 0.12) } else { DANGER.gamma_multiply(0.12) },
        Stroke::new(1.0, DANGER),
        96.0,
    )
}

/// A small text link in the accent colour.
pub(crate) fn link(ui: &mut Ui, c: &Colors, label: &str) -> egui::Response {
    ui.add(egui::Label::new(RichText::new(label).size(12.0).color(c.accent)).sense(Sense::click()))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;

    /// The outer bounds of every filled or outlined rectangle drawn.
    fn painted(h: &Harness<'_>) -> egui::Rect {
        let mut r = egui::Rect::NOTHING;
        for clip in &h.output().shapes {
            if let egui::Shape::Rect(rs) = &clip.shape {
                r = r.union(rs.rect.expand(rs.stroke.width / 2.0));
            }
        }
        r
    }

    fn check(name: &str, mut draw: impl FnMut(&mut Ui, &Colors) -> egui::Response) -> bool {
        let mut h = Harness::new_ui(move |ui| {
            let c = Colors::new(&Palette::builtin(false));
            draw(ui, &c);
        });
        h.run();
        let before = painted(&h);
        h.hover_at(before.center());
        h.run();
        let after = painted(&h);
        eprintln!("{name}: {before:?} -> {after:?}");
        before == after
    }

    /// Hovering a button must not change the size of what is drawn.
    #[test]
    fn buttons_keep_their_size_on_hover() {
        assert!(check("primary", |ui, c| primary_button(ui, c, "Get free license")));
        assert!(check("secondary", |ui, c| secondary_button(ui, c, "Enter license key\u{2026}")));
        assert!(check("danger", |ui, _| danger_button(ui, "Remove\u{2026}", false)));
    }

    /// The old way (overriding the stroke) does shrink: the test can see it.
    #[test]
    fn stroke_override_shrinks_on_hover() {
        assert!(!check("override", |ui, c| ui.add(
            egui::Button::new("Enter license key")
                .fill(Color32::TRANSPARENT)
                .stroke(Stroke::new(1.0, c.border))
                .corner_radius(7.0)
        )));
    }
}
