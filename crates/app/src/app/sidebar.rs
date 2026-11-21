//! The page sidebar (Ctrl+B): thumbnails of every page, with marks where
//! boxes are filled, and the document's outline (bookmarks). Thumbnails
//! are dragged to move pages, Ctrl/Shift+click selects several, and the
//! right-click menu organises them (`pages.rs`).

use super::*;
use crate::assist_ui::widgets::Colors;
use egui::RichText;
use pdfr_core::links::Target;

const THUMB_W: f32 = 120.0;
const LABEL_H: f32 = 18.0;
const ROW_GAP: f32 = 10.0;

/// What a thumbnail's right-click menu asked for.
#[derive(Clone, Copy, Debug)]
enum ThumbAction {
    Turn(i32),
    Blank { after: bool },
    InsertFile,
    Extract,
    Delete,
}

/// A thumbnail's right-click menu for `pages` (of `n`).
fn thumb_menu(ui: &mut egui::Ui, pages: &[usize], n: usize) -> Option<ThumbAction> {
    let many = pages.len() > 1;
    let mut a = None;
    if ui.button("Turn left").clicked() {
        a = Some(ThumbAction::Turn(-1));
    }
    if ui.button("Turn right").clicked() {
        a = Some(ThumbAction::Turn(1));
    }
    ui.separator();
    if ui.button("Insert blank page before").clicked() {
        a = Some(ThumbAction::Blank { after: false });
    }
    if ui.button("Insert blank page after").clicked() {
        a = Some(ThumbAction::Blank { after: true });
    }
    if ui.button("Insert pages from a file…").clicked() {
        a = Some(ThumbAction::InsertFile);
    }
    ui.separator();
    let extract = if many {
        format!("Extract {} pages…", pages.len())
    } else {
        "Extract page…".into()
    };
    if ui.button(extract).clicked() {
        a = Some(ThumbAction::Extract);
    }
    let delete = if many {
        format!("Delete {} pages", pages.len())
    } else {
        "Delete page".into()
    };
    let all = pages.len() >= n;
    let r = ui.add_enabled(!all, egui::Button::new(delete));
    if r.on_disabled_hover_text("A PDF needs at least one page")
        .clicked()
    {
        a = Some(ThumbAction::Delete);
    }
    a
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub(super) enum SideTab {
    #[default]
    Pages,
    Outline,
    /// The document's digital signatures and their checks.
    Signatures,
    /// Notes and markup.
    Comments,
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

    /// The sidebar's tabs (none when there are only pages). Goes back to
    /// Pages if the current tab is gone.
    fn side_tabs(&mut self) -> Vec<(SideTab, &'static str)> {
        let has_outline = self
            .doc
            .as_ref()
            .is_some_and(|d| !d.form.outline.is_empty());
        let signed = self
            .doc
            .as_ref()
            .is_some_and(|d| d.form.is_digitally_signed());
        let has_marks = self.doc.as_ref().is_some_and(|d| !d.marks.is_empty());
        if (!has_outline && self.side_tab == SideTab::Outline)
            || (!signed && self.side_tab == SideTab::Signatures)
            || (!has_marks && self.side_tab == SideTab::Comments)
        {
            self.side_tab = SideTab::Pages;
        }
        if !has_outline && !signed && !has_marks {
            return vec![];
        }
        let mut tabs = vec![(SideTab::Pages, "Pages")];
        if has_outline {
            tabs.push((SideTab::Outline, "Outline"));
        }
        if signed {
            tabs.push((SideTab::Signatures, "Signatures"));
        }
        if has_marks {
            tabs.push((SideTab::Comments, "Comments"));
        }
        tabs
    }

    /// The sidebar as a left panel, as wide as the current tab needs: the
    /// thumbnails are narrow, the signatures' cards wide. Each tab keeps its
    /// own width if dragged.
    pub(super) fn sidebar_panel(&mut self, ui: &mut egui::Ui) {
        let tabs = self.side_tabs();
        let labels: Vec<&str> = tabs.iter().map(|(_, l)| *l).collect();
        // The frame's margins on both sides.
        let tab_bar = if tabs.is_empty() {
            0.0
        } else {
            crate::assist_ui::widgets::segmented_min_width(ui, &labels) + 12.0
        };
        let content = match self.side_tab {
            SideTab::Pages => THUMB_W + 16.0 + 12.0,
            SideTab::Outline => 230.0,
            SideTab::Signatures => 280.0,
            SideTab::Comments => 260.0,
        };
        let width = content.max(tab_bar);
        egui::Panel::left(("sidebar", self.side_tab))
            .resizable(true)
            .default_size(width)
            .size_range(tab_bar.max(130.0)..=480.0)
            .frame(
                egui::Frame::new()
                    .fill(self.palette.toolbar)
                    .inner_margin(egui::Margin::symmetric(6, 6)),
            )
            .show(ui, |ui| self.sidebar(ui, &tabs));
    }

    fn sidebar(&mut self, ui: &mut egui::Ui, tabs: &[(SideTab, &str)]) {
        let c = Colors::new(&self.palette);
        if !tabs.is_empty() {
            if let Some(t) = crate::assist_ui::widgets::segmented(ui, &c, tabs, self.side_tab) {
                self.side_tab = t;
            }
            ui.add_space(8.0);
        }
        match self.side_tab {
            SideTab::Pages => self.thumbnails(ui, &c),
            SideTab::Outline => self.outline(ui, &c),
            SideTab::Signatures => self.signatures_panel(ui, &c),
            SideTab::Comments => self.comments_panel(ui, &c),
        }
    }

    fn thumbnails(&mut self, ui: &mut egui::Ui, c: &Colors) {
        let n = self.doc.as_ref().map_or(0, |d| d.sizes.len());
        let editable = self.doc.as_ref().is_some_and(|d| d.form.editable());
        let row_h = self.thumb_row_h();
        let current = self.current_page();
        let follow = self.side_follow != Some(current) && self.side_drag.is_none();
        let mut clicked = None;
        let mut action: Option<(Vec<usize>, ThumbAction)> = None;
        let mut drag_from = None;
        let mut slot: Option<(usize, f32)> = None;
        let pointer = ui.input(|i| i.pointer.latest_pos());
        let mut visible: Vec<usize> = Vec::new();
        self.side_rows.clear();
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
            let clip = ui.clip_rect();
            for i in range {
                visible.push(i);
                let sense = if editable {
                    Sense::click_and_drag()
                } else {
                    Sense::click()
                };
                let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), sense);
                self.thumb_paint(ui, c, i, row, current, resp.hovered());
                self.side_rows.push((i, row));
                // While dragging: the gap above or below this page.
                if self.side_drag.is_some()
                    && let Some(pt) = pointer
                    && clip.contains(pt)
                    && (row.top()..=row.bottom()).contains(&pt.y)
                {
                    slot = Some(if pt.y < row.center().y {
                        (i, row.top())
                    } else {
                        (i + 1, row.bottom())
                    });
                }
                if resp.drag_started() {
                    drag_from = Some(i);
                }
                if resp.secondary_clicked() && !self.side_sel.contains(&i) {
                    self.side_sel = vec![i];
                    self.side_anchor = Some(i);
                }
                if editable {
                    let targets = if self.side_sel.contains(&i) {
                        self.side_sel.clone()
                    } else {
                        vec![i]
                    };
                    resp.context_menu(|ui| {
                        if let Some(a) = thumb_menu(ui, &targets, n) {
                            action = Some((targets.clone(), a));
                            ui.close();
                        }
                    });
                }
                if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                    clicked = Some(i);
                }
            }
            if let Some((_, y)) = slot {
                let x = clip.x_range().shrink(6.0);
                ui.painter().hline(x, y, Stroke::new(2.5, c.accent));
                ui.painter().circle_filled(pos2(x.min, y), 3.5, c.accent);
            }
            // Dragging near the top or bottom scrolls.
            if self.side_drag.is_some()
                && let Some(pt) = pointer
            {
                if pt.y < clip.top() + 30.0 && pt.y > clip.top() - 60.0 {
                    ui.scroll_with_delta(vec2(0.0, 8.0));
                } else if pt.y > clip.bottom() - 30.0 && pt.y < clip.bottom() + 60.0 {
                    ui.scroll_with_delta(vec2(0.0, -8.0));
                }
                ui.ctx().request_repaint();
            }
        });
        if let Some(i) = drag_from {
            if !self.side_sel.contains(&i) {
                self.side_sel = vec![i];
                self.side_anchor = Some(i);
            }
            self.side_drag = Some(self.side_sel.clone());
        }
        if self.side_drag.is_some() {
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
            if ui.input(|i| !i.pointer.any_down()) {
                let pages = self.side_drag.take().unwrap_or_default();
                if let Some((to, _)) = slot {
                    self.move_pages(&pages, to);
                }
            }
        }
        if let Some(i) = clicked {
            self.thumb_clicked(i, ui.input(|inp| inp.modifiers));
        }
        if let Some((pages, a)) = action {
            self.thumb_action(&pages, a);
        }
        self.want_thumbs(&visible);
    }

    /// Draws thumbnail `i` in `row`: the page picture, marks where boxes are
    /// filled, its number, and whether it is current, selected or dragged.
    fn thumb_paint(
        &self,
        ui: &egui::Ui,
        c: &Colors,
        i: usize,
        row: Rect,
        current: usize,
        hovered: bool,
    ) {
        let Some(doc) = &self.doc else { return };
        let (w, h) = doc.sizes[i];
        let tw = THUMB_W.min(row.width() - 16.0);
        let th = tw * h / w.max(1.0);
        let page = Rect::from_min_size(
            pos2(row.center().x - tw / 2.0, row.top() + ROW_GAP / 2.0),
            vec2(tw, th),
        );
        let selected = self.side_sel.contains(&i);
        let p = ui.painter();
        if i == current {
            p.rect_filled(page.expand(4.0), 4.0, c.accent.gamma_multiply(0.35));
        } else if selected {
            p.rect_filled(page.expand(4.0), 4.0, c.accent.gamma_multiply(0.2));
        } else if hovered && self.side_drag.is_none() {
            p.rect_filled(page.expand(4.0), 4.0, c.hover);
        }
        if selected {
            p.rect_stroke(
                page.expand(4.0),
                4.0,
                Stroke::new(1.5, c.accent),
                egui::StrokeKind::Inside,
            );
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
                page.min + vec2(r.x1 * s, (r.y0 * s + (r.y1 - r.y0) * s).max(r.y0 * s + 1.5)),
            );
            p.rect_filled(mark, 0.0, c.accent.gamma_multiply(0.6));
        }
        if self.side_drag.as_ref().is_some_and(|d| d.contains(&i)) {
            p.rect_filled(page, 0.0, self.palette.toolbar.gamma_multiply(0.6));
        }
        p.text(
            pos2(row.center().x, page.bottom() + 3.0),
            egui::Align2::CENTER_TOP,
            format!("{}", i + 1),
            egui::FontId::proportional(12.0),
            if i == current { c.text } else { c.weak },
        );
    }

    /// A click on thumbnail `i`: go there; with Ctrl add it to (or take it
    /// from) the selection, with Shift select the pages up to it.
    fn thumb_clicked(&mut self, i: usize, m: Modifiers) {
        if m.command {
            if let Some(k) = self.side_sel.iter().position(|&p| p == i) {
                self.side_sel.remove(k);
            } else {
                self.side_sel.push(i);
                self.side_sel.sort_unstable();
            }
            self.side_anchor = Some(i);
            return;
        }
        if m.shift {
            let a = self.side_anchor.unwrap_or(self.current_page());
            self.side_sel = (a.min(i)..=a.max(i)).collect();
        } else {
            self.side_sel = vec![i];
            self.side_anchor = Some(i);
        }
        self.scroll_to_page(i);
        self.side_follow = Some(i);
    }

    fn thumb_action(&mut self, pages: &[usize], a: ThumbAction) {
        use super::pages::PageOp;
        let (first, last) = (pages[0], pages[pages.len() - 1]);
        match a {
            ThumbAction::Turn(t) => self.rotate_pages(pages.to_vec(), t),
            ThumbAction::Blank { after } => {
                let (at, like) = if after {
                    (last + 1, last)
                } else {
                    (first, first)
                };
                self.page_op(PageOp::Blank { at, like });
            }
            ThumbAction::InsertFile => self.pick_insert_file(last + 1),
            ThumbAction::Extract => self.pick_extract(pages.to_vec()),
            ThumbAction::Delete => self.delete_pages(pages),
        }
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
