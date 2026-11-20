//! The start screen (no document open): recent files as cards with a
//! picture of their first page, in a grid, plus Open. Pictures are drawn in
//! the background; files that can't be read get a plain page.

use super::*;
use crate::assist_ui::widgets::{Colors, ROSE, brand_button};
use egui::RichText;
use pdfr_assist::memory::RecentFile;
use std::sync::mpsc::{Receiver, Sender};

/// Most files shown.
const MAX_CARDS: usize = 12;
const CARD_W: f32 = 168.0;
const THUMB_H: f32 = 190.0;
const CARD_H: f32 = THUMB_H + 58.0;
const GAP: f32 = 16.0;
/// Width of a first-page picture, in pixels.
const PICTURE_PX: f32 = 300.0;

/// First-page pictures of recent files, made on background threads.
pub(super) struct Pictures {
    /// path -> picture (`None`: the file could not be drawn).
    tex: HashMap<String, Option<TextureHandle>>,
    /// Pictures being made.
    asked: HashSet<String>,
    tx: Sender<(String, Option<TextureHandle>)>,
    rx: Receiver<(String, Option<TextureHandle>)>,
    /// Drawn for dark pages.
    dark: bool,
}

impl Default for Pictures {
    fn default() -> Self {
        let (tx, rx) = channel();
        Pictures {
            tex: HashMap::new(),
            asked: HashSet::new(),
            tx,
            rx,
            dark: false,
        }
    }
}

impl Pictures {
    /// The picture of `path` once made (`Some(None)`: it could not be
    /// drawn); `None` while it is being made. Asks for it the first time.
    fn get(
        &mut self,
        ctx: &egui::Context,
        path: &str,
        dark: bool,
    ) -> Option<Option<TextureHandle>> {
        while let Ok((p, t)) = self.rx.try_recv() {
            self.asked.remove(&p);
            self.tex.insert(p, t);
        }
        if self.dark != dark {
            self.dark = dark;
            self.tex.clear();
            self.asked.clear();
        }
        if let Some(t) = self.tex.get(path) {
            return Some(t.clone());
        }
        if !self.asked.insert(path.to_string()) {
            return None;
        }
        let (tx, ctx, p) = (self.tx.clone(), ctx.clone(), path.to_string());
        std::thread::spawn(move || {
            let tex = first_page(&ctx, &p, dark);
            let _ = tx.send((p, tex));
            ctx.request_repaint();
        });
        None
    }
}

/// Draws the first page of a PDF small enough for a card.
fn first_page(ctx: &egui::Context, path: &str, dark: bool) -> Option<TextureHandle> {
    let bytes = std::fs::read(path).ok()?;
    let pdf = pdfr_core::render::open(bytes)?;
    let pages = pdf.pages();
    let page = pages.first()?;
    let (w, _) = pdfr_core::render::page_geom(page).display_size();
    let cache = pdfr_core::render::RenderCache::new();
    let mut img = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pdfr_core::render::render_page(page, &cache, PICTURE_PX / w.max(1.0))
    }))
    .ok()?;
    if dark {
        img.invert_lightness();
    }
    let image = egui::ColorImage::from_rgba_premultiplied(
        [img.width as usize, img.height as usize],
        &img.rgba,
    );
    Some(ctx.load_texture(
        format!("first-page-{path}"),
        image,
        egui::TextureOptions::LINEAR,
    ))
}

impl App {
    /// The start screen, while no document is open.
    pub(super) fn start_screen(&mut self, ui: &mut egui::Ui, area: Rect) {
        let c = Colors::new(&self.palette);
        let files: Vec<RecentFile> = self
            .recent_files()
            .into_iter()
            .filter(|r| std::path::Path::new(&r.path).exists())
            .take(MAX_CARDS)
            .collect();
        if files.is_empty() {
            self.start_empty(ui, area, &c);
            return;
        }
        // As many columns as fit (at most five), no more than there are files.
        let fit = ((area.width() - 2.0 * PAD + GAP) / (CARD_W + GAP))
            .floor()
            .max(1.0) as usize;
        let cols = fit.min(5).min(files.len());
        let grid_w = cols as f32 * CARD_W + (cols - 1) as f32 * GAP;
        let rows = files.len().div_ceil(cols);
        let content_h = 44.0 + rows as f32 * (CARD_H + GAP) + 30.0;
        let top = (area.center().y - content_h / 2.0).max(area.top() + PAD);
        let left = area.center().x - grid_w / 2.0;
        let mut action: Option<(PathBuf, bool)> = None;
        let mut forget: Option<String> = None;
        let mut open_dialog = false;
        let inner = Rect::from_min_max(pos2(left, top), pos2(left + grid_w, area.bottom()));
        ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
            egui::ScrollArea::vertical()
                .id_salt("pdfr-start")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(grid_w);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Recent files")
                                .size(18.0)
                                .strong()
                                .color(c.text),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            open_dialog = brand_button(ui, "Open\u{2026}", 34.0)
                                .on_hover_text("Ctrl+O")
                                .clicked();
                        });
                    });
                    ui.add_space(10.0);
                    for row in files.chunks(cols) {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = GAP;
                            for r in row {
                                if let Some(a) = self.file_card(ui, &c, r, &mut forget) {
                                    action = Some(a);
                                }
                            }
                        });
                        ui.add_space(GAP);
                    }
                    ui.label(
                        RichText::new("Drop a PDF anywhere to open it. F1 shows every shortcut.")
                            .size(12.0)
                            .color(c.weak),
                    );
                });
        });
        if open_dialog {
            self.run(super::commands::Command::Open);
        }
        if let Some(p) = forget {
            self.assist.a.store.settings.recent.retain(|r| r.path != p);
            self.assist.a.save();
        }
        match action {
            Some((p, true)) => self.open_in_new_window(p),
            Some((p, false)) => self.request_open(p),
            None => {}
        }
    }

    /// One recent file: its first page, name, and when and where.
    /// Returns the file to open (and whether in a new window).
    fn file_card(
        &mut self,
        ui: &mut egui::Ui,
        c: &Colors,
        r: &RecentFile,
        forget: &mut Option<String>,
    ) -> Option<(PathBuf, bool)> {
        let (rect, resp) = ui.allocate_exact_size(vec2(CARD_W, CARD_H), Sense::click());
        let path = std::path::Path::new(&r.path);
        let hovered = resp.hovered();
        let p = ui.painter();
        p.rect_filled(rect, 10.0, if hovered { c.hover } else { c.card });
        p.rect_stroke(
            rect,
            10.0,
            Stroke::new(1.0, if hovered { c.accent } else { c.border }),
            egui::StrokeKind::Inside,
        );
        // The page, as tall as the picture area allows, centred.
        let area = Rect::from_min_size(
            rect.min + vec2(12.0, 12.0),
            vec2(CARD_W - 24.0, THUMB_H - 12.0),
        );
        let dark = self.dark_pages();
        let pic = self.start_pictures.get(ui.ctx(), &r.path, dark);
        let aspect = match &pic {
            Some(Some(t)) => t.size_vec2().x / t.size_vec2().y.max(1.0),
            _ => 1.0 / 1.414,
        };
        let (w, h) = if area.width() / area.height() > aspect {
            (area.height() * aspect, area.height())
        } else {
            (area.width(), area.width() / aspect)
        };
        let page = Rect::from_center_size(area.center(), vec2(w, h));
        let p = ui.painter();
        p.rect_filled(
            page.translate(vec2(1.0, 2.0)),
            2.0,
            Color32::from_black_alpha(50),
        );
        p.rect_filled(page, 2.0, self.page_fill());
        match &pic {
            Some(Some(t)) => {
                p.image(
                    t.id(),
                    page,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            // Not readable here (protected, damaged): a plain page.
            Some(None) => {
                crate::icons::paint(
                    p,
                    page.shrink(page.width() * 0.3),
                    crate::icons::Icon::Open,
                    c.weak,
                );
            }
            _ => {}
        }
        // Name and when, cut to the card's width.
        let name = path
            .file_stem()
            .map_or(r.path.clone(), |n| n.to_string_lossy().into_owned());
        let text_rect = Rect::from_min_max(
            pos2(rect.left() + 12.0, rect.top() + THUMB_H + 6.0),
            pos2(rect.right() - 12.0, rect.bottom() - 6.0),
        );
        let mut tui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(text_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        tui.spacing_mut().item_spacing.y = 2.0;
        // Not selectable: a click anywhere on the card opens the file.
        tui.add(
            egui::Label::new(RichText::new(&name).strong().color(c.text))
                .truncate()
                .selectable(false),
        );
        let folder = path
            .parent()
            .map(|f| super::palette::short_folder(&f.to_string_lossy()))
            .unwrap_or_default();
        tui.add(
            egui::Label::new(
                RichText::new(format!(
                    "{} \u{b7} {folder}",
                    super::recent::when(r.opened_at)
                ))
                .size(12.0)
                .color(c.weak),
            )
            .truncate()
            .selectable(false),
        );
        let mut open = None;
        let resp = resp
            .on_hover_text(&r.path)
            .on_hover_cursor(CursorIcon::PointingHand);
        if resp.clicked() {
            open = Some((path.to_path_buf(), false));
        }
        resp.context_menu(|ui| {
            if ui.button("Open").clicked() {
                open = Some((path.to_path_buf(), false));
                ui.close();
            }
            if ui.button("Open in new window").clicked() {
                open = Some((path.to_path_buf(), true));
                ui.close();
            }
            if ui.button("Remove from this list").clicked() {
                *forget = Some(r.path.clone());
                ui.close();
            }
        });
        open
    }

    /// No recent files: the icon, one line, and one big way to start, on a
    /// drop zone that lights up while a file is dragged over the window.
    fn start_empty(&mut self, ui: &mut egui::Ui, area: Rect, c: &Colors) {
        let dragging = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
        let w = 460.0f32.min(area.width() - 2.0 * PAD);
        let zone = Rect::from_center_size(
            area.center(),
            vec2(w, 340.0f32.min(area.height() - 2.0 * PAD)),
        );
        // The drop zone: a dashed rounded outline, coral while dragging.
        let p = ui.painter();
        let (line, fill) = if dragging {
            (ROSE, ROSE.gamma_multiply(0.06))
        } else {
            (c.border, Color32::TRANSPARENT)
        };
        p.rect_filled(zone, 20.0, fill);
        dashed_round_rect(p, zone, 20.0, Stroke::new(1.5, line));

        let mut open = false;
        let inner = zone.shrink2(vec2(24.0, 0.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
            ui.vertical_centered(|ui| {
                let content_h = 76.0 + 18.0 + 30.0 + 22.0 + 26.0 + 46.0 + 18.0 + 18.0;
                ui.add_space(((inner.height() - content_h) / 2.0).max(0.0));
                let (r, _) = ui.allocate_exact_size(vec2(76.0, 76.0), Sense::hover());
                if let Some(t) = self.app_icon_texture(ui.ctx()) {
                    ui.painter().image(
                        t.id(),
                        r,
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                ui.add_space(18.0);
                let title = if dragging {
                    "Drop it to open"
                } else {
                    "Open a PDF to fill in"
                };
                ui.label(RichText::new(title).size(24.0).strong().color(c.text));
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Drag a file here, or pick one from your computer.")
                        .size(14.0)
                        .color(c.weak),
                );
                ui.add_space(26.0);
                open = brand_button(ui, "Open a PDF\u{2026}", 46.0).clicked();
                ui.add_space(18.0);
                let open_keys = super::commands::key_text(
                    ui.ctx(),
                    &egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::O),
                );
                ui.label(
                    RichText::new(format!(
                        "{open_keys} opens a file \u{b7} F1 shows every shortcut"
                    ))
                    .size(12.0)
                    .color(c.weak),
                );
            });
        });
        if open {
            self.run(super::commands::Command::Open);
        }
    }

    /// pdfr's icon as a texture, made once.
    fn app_icon_texture(&mut self, ctx: &egui::Context) -> Option<TextureHandle> {
        let id = egui::Id::unique("pdfr-app-icon");
        if let Some(t) = ctx.data(|d| d.get_temp::<TextureHandle>(id)) {
            return Some(t);
        }
        let icon = crate::system::app_icon()?;
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [icon.width as usize, icon.height as usize],
            &icon.rgba,
        );
        let t = ctx.load_texture("pdfr-app-icon", image, egui::TextureOptions::LINEAR);
        ctx.data_mut(|d| d.insert_temp(id, t.clone()));
        Some(t)
    }
}

/// A rounded rectangle outline drawn in dashes.
fn dashed_round_rect(p: &egui::Painter, r: Rect, radius: f32, stroke: Stroke) {
    let mut pts = Vec::new();
    let corners = [
        (pos2(r.right() - radius, r.top() + radius), -90.0f32),
        (pos2(r.right() - radius, r.bottom() - radius), 0.0),
        (pos2(r.left() + radius, r.bottom() - radius), 90.0),
        (pos2(r.left() + radius, r.top() + radius), 180.0),
    ];
    for (c, start) in corners {
        for i in 0..=8 {
            let a = (start + i as f32 * 90.0 / 8.0).to_radians();
            pts.push(c + vec2(a.cos(), a.sin()) * radius);
        }
    }
    pts.push(pts[0]);
    p.extend(egui::Shape::dashed_line(&pts, stroke, 7.0, 6.0));
}
