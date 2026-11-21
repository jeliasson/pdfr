//! The page sidebar (Ctrl+B): thumbnails of every page, with marks where
//! boxes are filled, and the document's outline (bookmarks).

use super::*;
use crate::assist_ui::widgets::Colors;
use egui::RichText;
use pdfr_core::links::Target;

const THUMB_W: f32 = 120.0;
const LABEL_H: f32 = 18.0;
const ROW_GAP: f32 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(super) enum SideTab {
    #[default]
    Pages,
    Outline,
    /// The document's digital signatures and their checks.
    Signatures,
}

impl App {
    pub(super) fn sidebar_open(&self) -> bool {
        self.assist.a.store.settings.sidebar && self.doc.is_some()
    }

    pub(super) fn toggle_sidebar(&mut self) {
        let s = &mut self.assist.a.store.settings.sidebar;
        *s = !*s;
        self.assist.a.save();
    }

    /// The height of one thumbnail row: room for the tallest page.
    fn thumb_row_h(&self) -> f32 {
        let tallest = self.doc.as_ref().map_or(1.414, |d| {
            d.sizes
                .iter()
                .map(|(w, h)| h / w.max(1.0))
                .fold(0.0f32, f32::max)
                .clamp(0.3, 2.0)
        });
        THUMB_W * tallest + LABEL_H + ROW_GAP
    }

    pub(super) fn sidebar(&mut self, ui: &mut egui::Ui) {
        let c = Colors::new(&self.palette);
        let has_outline = self
            .doc
            .as_ref()
            .is_some_and(|d| !d.form.outline.is_empty());
        let signed = self
            .doc
            .as_ref()
            .is_some_and(|d| d.form.is_digitally_signed());
        if (!has_outline && self.side_tab == SideTab::Outline)
            || (!signed && self.side_tab == SideTab::Signatures)
        {
            self.side_tab = SideTab::Pages;
        }
        if has_outline || signed {
            let mut tabs = vec![(SideTab::Pages, "Pages")];
            if has_outline {
                tabs.push((SideTab::Outline, "Outline"));
            }
            if signed {
                tabs.push((SideTab::Signatures, "Signatures"));
            }
            if let Some(t) = crate::assist_ui::widgets::segmented(ui, &c, &tabs, self.side_tab) {
                self.side_tab = t;
            }
            ui.add_space(8.0);
        }
        match self.side_tab {
            SideTab::Pages => self.thumbnails(ui, &c),
            SideTab::Outline => self.outline(ui, &c),
            SideTab::Signatures => self.signatures_panel(ui, &c),
        }
    }

    fn thumbnails(&mut self, ui: &mut egui::Ui, c: &Colors) {
        let n = self.doc.as_ref().map_or(0, |d| d.sizes.len());
        let row_h = self.thumb_row_h();
        let current = self.current_page();
        let follow = self.side_follow != Some(current);
        let mut clicked = None;
        let mut visible: Vec<usize> = Vec::new();
        let mut area = egui::ScrollArea::vertical()
            .id_salt("pdfr-thumbs")
            .auto_shrink([false, false]);
        if follow {
            // Keep the current page in view as the document scrolls.
            let top = (current as f32 * row_h - ui.available_height() / 2.0 + row_h / 2.0).max(0.0);
            area = area.vertical_scroll_offset(top);
            self.side_follow = Some(current);
        }
        area.show_rows(ui, row_h, n, |ui, range| {
            for i in range {
                visible.push(i);
                let (row, resp) =
                    ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::click());
                let Some(doc) = &self.doc else { return };
                let (w, h) = doc.sizes[i];
                let tw = THUMB_W.min(row.width() - 16.0);
                let th = tw * h / w.max(1.0);
                let page = Rect::from_min_size(
                    pos2(row.center().x - tw / 2.0, row.top() + ROW_GAP / 2.0),
                    vec2(tw, th),
                );
                let p = ui.painter();
                if i == current {
                    p.rect_filled(page.expand(4.0), 4.0, c.accent.gamma_multiply(0.35));
                } else if resp.hovered() {
                    p.rect_filled(page.expand(4.0), 4.0, c.hover);
                }
                p.rect_filled(page, 0.0, self.page_fill());
                if let Some(t) = doc.thumbs.get(&i) {
                    p.image(
                        t.id(),
                        page,
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                // Where boxes are filled in.
                let s = tw / w.max(1.0);
                for e in doc
                    .entries
                    .iter()
                    .filter(|e| e.f.page == i && !e.f.is_empty())
                {
                    let r = e.f.rect;
                    let mark = Rect::from_min_max(
                        page.min + vec2(r.x0 * s, r.y0 * s),
                        page.min
                            + vec2(r.x1 * s, (r.y0 * s + (r.y1 - r.y0) * s).max(r.y0 * s + 1.5)),
                    );
                    p.rect_filled(mark, 0.0, c.accent.gamma_multiply(0.6));
                }
                p.text(
                    pos2(row.center().x, page.bottom() + 3.0),
                    egui::Align2::CENTER_TOP,
                    format!("{}", i + 1),
                    egui::FontId::proportional(12.0),
                    if i == current { c.text } else { c.weak },
                );
                if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                    clicked = Some(i);
                }
            }
        });
        if let Some(i) = clicked {
            self.scroll_to_page(i);
            self.side_follow = Some(i);
        }
        self.want_thumbs(&visible);
    }

    /// Asks the workers for the visible thumbnails not made yet.
    fn want_thumbs(&mut self, visible: &[usize]) {
        let Some(doc) = &self.doc else { return };
        let missing: Vec<usize> = visible
            .iter()
            .copied()
            .filter(|p| !doc.thumbs.contains_key(p))
            .collect();
        if missing != self.thumbs_asked {
            self.worker.request_thumbs(&missing);
            self.thumbs_asked = missing;
        }
    }

    fn outline(&mut self, ui: &mut egui::Ui, c: &Colors) {
        let Some(doc) = &self.doc else { return };
        let items = doc.form.outline.clone();
        let current = self.current_page();
        // The section being read: the last entry starting on or before
        // the current page.
        let here = items
            .iter()
            .enumerate()
            .rfind(|(_, o)| matches!(o.target, Some(Target::Dest { page, .. }) if page <= current))
            .map(|(i, _)| i);
        let mut go = None;
        let mut flip = None;
        egui::ScrollArea::vertical()
            .id_salt("pdfr-outline")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                // Hidden below a closed entry until the depth comes back up.
                let mut hide_below: Option<usize> = None;
                for (i, o) in items.iter().enumerate() {
                    if let Some(d) = hide_below {
                        if o.depth > d {
                            continue;
                        }
                        hide_below = None;
                    }
                    let open = self.outline_open.get(&i).copied().unwrap_or(o.open);
                    if o.has_children && !open {
                        hide_below = Some(o.depth);
                    }
                    ui.horizontal(|ui| {
                        ui.add_space(o.depth as f32 * 14.0);
                        if o.has_children {
                            // A small triangle: pointing down when open.
                            let (r, resp) =
                                ui.allocate_exact_size(vec2(12.0, 12.0), Sense::click());
                            let m = r.center();
                            let pts = if open {
                                vec![
                                    m + vec2(-4.0, -2.0),
                                    m + vec2(4.0, -2.0),
                                    m + vec2(0.0, 3.0),
                                ]
                            } else {
                                vec![
                                    m + vec2(-2.0, -4.0),
                                    m + vec2(3.0, 0.0),
                                    m + vec2(-2.0, 4.0),
                                ]
                            };
                            ui.painter().add(egui::Shape::convex_polygon(
                                pts,
                                c.weak,
                                Stroke::NONE,
                            ));
                            if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                                flip = Some((i, !open));
                            }
                        } else {
                            ui.add_space(12.0);
                        }
                        let text = RichText::new(&o.title).color(if o.target.is_some() {
                            c.text
                        } else {
                            c.weak
                        });
                        let r = ui.selectable_label(here == Some(i), text);
                        if r.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                            go = Some(i);
                        }
                    });
                }
            });
        if let Some((i, v)) = flip {
            self.outline_open.insert(i, v);
        }
        if let Some(t) = go.and_then(|i| items[i].target.clone()) {
            let page = match &t {
                Target::Dest { page, .. } => *page,
                _ => current,
            };
            let link = pdfr_core::links::Link {
                page,
                rect: PRect::default(),
                target: t,
            };
            self.follow_link(&link);
        }
    }
}
