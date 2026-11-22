//! Comments and markup on the page: notes (Ctrl+Shift+A), highlighting,
//! underlining and striking through selected text (Ctrl+Shift+H), text
//! boxes (Ctrl+Shift+T) and shapes. Saved as standard PDF annotations
//! (`pdfr_core::markup`); drawn live here so they can be selected, moved,
//! resized, recoloured and removed, each change one undo step.

use super::*;
use crate::assist_ui::widgets::{self, Colors};
use commands::Command;
use pdfr_core::markup::{self as pm, MarkKind, Markup};

/// One mark plus a stable id.
#[derive(Clone, Debug)]
pub struct MarkEntry {
    pub id: u64,
    pub m: Markup,
}

/// The colours offered for marks.
pub(super) const COLOURS: [(&str, [u8; 3]); 6] = [
    ("Yellow", [255, 212, 0]),
    ("Green", [52, 199, 89]),
    ("Blue", [0, 122, 255]),
    ("Red", [255, 59, 48]),
    ("Purple", [175, 82, 222]),
    ("Black", [0, 0, 0]),
];

fn default_colour(kind: MarkKind) -> [u8; 3] {
    match kind {
        MarkKind::Highlight | MarkKind::Note => COLOURS[0].1,
        MarkKind::Underline => COLOURS[2].1,
        MarkKind::FreeText => COLOURS[5].1,
        _ => COLOURS[3].1,
    }
}

fn c32(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// What on a selected mark the pointer grabbed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Grip {
    Move,
    Corner,
    Start,
    End,
}

#[derive(Clone, Debug)]
struct MarkDrag {
    id: u64,
    grip: Grip,
    start: (f32, f32),
    orig: Markup,
    moved: bool,
}

/// A note's comment or a text box's text being typed.
#[derive(Clone, Debug)]
pub(crate) struct MarkEdit {
    pub id: u64,
    pub text: String,
    orig: String,
    /// The mark was just made: its undo step is the one that made it.
    created: bool,
    /// Give the text box the keyboard on the next frame.
    focus: bool,
}

#[derive(Default)]
pub(crate) struct MarksUi {
    /// The selected mark.
    pub sel: Option<u64>,
    /// The toolbar's markup menu is open.
    pub menu: bool,
    /// Where the toolbar's button is, for the menu.
    pub anchor: Option<Rect>,
    /// The colour picked in the menu, for the marks made next.
    pub colour: Option<[u8; 3]>,
    drag: Option<MarkDrag>,
    pub edit: Option<MarkEdit>,
    /// The mark whose right-click menu is open.
    ctx: Option<u64>,
}

const HANDLE: f32 = 9.0;
const EDIT_ID: &str = "pdfr-mark-edit";

impl Doc {
    pub fn mark_list(&self) -> Vec<Markup> {
        self.marks.iter().map(|e| e.m.clone()).collect()
    }
    pub(super) fn add_mark(&mut self, m: Markup) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.marks.push(MarkEntry { id, m });
        id
    }
    pub(super) fn mark(&self, id: u64) -> Option<&Markup> {
        self.marks.iter().find(|e| e.id == id).map(|e| &e.m)
    }
    fn mark_mut(&mut self, id: u64) -> Option<&mut Markup> {
        self.marks.iter_mut().find(|e| e.id == id).map(|e| &mut e.m)
    }
}

fn seg_dist(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (p.0 - a.0 - t * dx).hypot(p.1 - a.1 - t * dy)
}

impl App {
    fn geom(&self, page: usize) -> Option<pdfr_core::geom::PageGeom> {
        self.doc.as_ref()?.form.pages.get(page).copied()
    }

    fn xf(&self, rects: &[Rect], page: usize) -> Option<PageXf> {
        Some(PageXf {
            origin: rects.get(page)?.min,
            zoom: self.zoom,
        })
    }

    pub(super) fn toggle_tool(&mut self, t: Tool) {
        self.marks_ui.menu = false;
        self.tool = if self.tool == t { Tool::None } else { t };
        if self.tool == Tool::None {
            return;
        }
        self.status = match t {
            Tool::Note => "Click where the note goes. Esc cancels.",
            Tool::TextBox => "Click or drag where the text box goes. Esc cancels.",
            _ => "Drag on the page to draw. Esc cancels.",
        }
        .into();
    }

    fn colour_for(&self, kind: MarkKind) -> [u8; 3] {
        self.marks_ui.colour.unwrap_or_else(|| default_colour(kind))
    }

    /// Adds a mark as one undo step and selects it.
    fn add_mark(&mut self, m: Markup) -> Option<u64> {
        self.finish_mark_edit();
        self.snapshot();
        let doc = self.doc.as_mut()?;
        let id = doc.add_mark(m);
        doc.dirty = true;
        self.tool = Tool::None;
        self.drag_start = None;
        self.select_mark(Some(id));
        Some(id)
    }

    pub(super) fn select_mark(&mut self, id: Option<u64>) {
        self.marks_ui.sel = id;
        if id.is_some() {
            self.focus = None;
            self.text_sel = None;
            self.choice_open = None;
            self.assist.close_list();
        }
    }

    /// The selected text's spans, one per run: page and display rects.
    fn selection_rects(&self) -> Option<(usize, Vec<PRect>)> {
        let sel = self.text_sel.as_ref()?;
        let runs = self.doc.as_ref()?.text.get(&sel.page)?;
        let rects: Vec<PRect> = pdfr_core::text::selection(runs, sel.anchor, sel.head)
            .into_iter()
            .map(|(ri, from, to)| {
                let r = &runs[ri];
                let (a, b) = (r.char_rect(from), r.char_rect(to - 1));
                PRect::new(a.x0, r.rect.y0, b.x1, r.rect.y1)
            })
            .filter(|r| r.width() > 0.0)
            .collect();
        (!rects.is_empty()).then_some((sel.page, rects))
    }

    /// Who writes comments: the person pdfr has filled in most often (on
    /// your own computer, that's you), by name. Without a remembered name,
    /// the computer account's name.
    pub(super) fn author(&self) -> String {
        let profiles = &self.assist.a.store.profiles;
        profiles
            .iter()
            .filter_map(|p| Some((p.count, p.last_used, p.name()?)))
            .max_by_key(|(count, last, _)| (*count, *last))
            .map(|(_, _, n)| n)
            .unwrap_or_else(|| {
                std::env::var("USER")
                    .or_else(|_| std::env::var("USERNAME"))
                    .unwrap_or_default()
            })
    }

    /// Highlights, underlines or strikes through the selected text; with
    /// `comment`, opens the comment for typing.
    pub(super) fn mark_selection(&mut self, kind: MarkKind, comment: bool) {
        let Some((page, rects)) = self.selection_rects() else {
            self.status = "Select some text on the page first (drag over it).".into();
            return;
        };
        let Some(g) = self.geom(page) else { return };
        let mut m = Markup::over_text(kind, page, &g, &rects, self.colour_for(kind));
        m.author = self.author();
        let Some(id) = self.add_mark(m) else { return };
        if comment {
            self.open_mark_edit(id, true);
        }
        self.status = format!("{}. Ctrl+Z takes it off.", kind.label());
    }

    fn place_note(&mut self, page: usize, x: f32, y: f32) {
        let Some(g) = self.geom(page) else { return };
        let h = pm::NOTE_SIZE / 2.0;
        let mut m = Markup::note(page, &g, x - h, y - h, self.colour_for(MarkKind::Note));
        m.author = self.author();
        if let Some(id) = self.add_mark(m) {
            self.open_mark_edit(id, true);
        }
    }

    fn place_text_box(&mut self, page: usize, r: Option<PRect>, at: (f32, f32)) {
        let Some(g) = self.geom(page) else { return };
        let size = pm::TEXT_SIZE;
        let one_line =
            size * pdfr_core::layout::LINE_SPACING + 2.0 * pdfr_core::layout::PAD + size * 0.3;
        let r = r.unwrap_or_else(|| {
            let (x, y) = (at.0 - pdfr_core::layout::PAD, at.1 - one_line / 2.0);
            PRect::new(x, y, x + 200.0, y + one_line)
        });
        let mut m = Markup::text_box(page, &g, r, self.colour_for(MarkKind::FreeText));
        m.author = self.author();
        if let Some(id) = self.add_mark(m) {
            self.open_mark_edit(id, true);
        }
    }

    fn place_shape(&mut self, kind: MarkKind, page: usize, a: (f32, f32), b: (f32, f32)) {
        let Some(g) = self.geom(page) else { return };
        let mut m = Markup::shape(kind, page, &g, a, b, self.colour_for(kind));
        m.author = self.author();
        self.add_mark(m);
        self.status = format!(
            "{} drawn. Drag to move it; Delete removes it.",
            kind.label()
        );
    }

    /// Opens a note's comment or a text box's text for typing.
    pub(super) fn open_mark_edit(&mut self, id: u64, created: bool) {
        if self.marks_ui.edit.as_ref().is_some_and(|e| e.id == id) {
            return;
        }
        self.finish_mark_edit();
        let Some(text) = self
            .doc
            .as_ref()
            .and_then(|d| d.mark(id))
            .map(|m| m.text.clone())
        else {
            return;
        };
        self.select_mark(Some(id));
        self.marks_ui.edit = Some(MarkEdit {
            id,
            orig: text.clone(),
            text,
            created,
            focus: true,
        });
    }

    /// Ends typing: the text goes into the mark as one undo step (a new
    /// mark's step is the one that made it). A new note or text box left
    /// empty is taken away again.
    pub(super) fn finish_mark_edit(&mut self) {
        let Some(mut e) = self.marks_ui.edit.take() else {
            return;
        };
        e.text = e.text.trim_end().to_string();
        let Some(m) = self.doc.as_ref().and_then(|d| d.mark(e.id)).cloned() else {
            return;
        };
        let empty_new = e.created
            && e.text.trim().is_empty()
            && matches!(m.kind, MarkKind::Note | MarkKind::FreeText);
        if empty_new {
            let doc = self.doc.as_mut().unwrap();
            doc.marks.retain(|x| x.id != e.id);
            self.undo.pop();
            self.marks_ui.sel = None;
            return;
        }
        if e.text == e.orig {
            return;
        }
        if !e.created {
            self.snapshot();
        }
        let g = self.geom(m.page);
        let doc = self.doc.as_mut().unwrap();
        doc.dirty = true;
        if let Some(m) = doc.mark_mut(e.id) {
            m.text = e.text;
            // A text box grows to hold its text.
            if m.kind == MarkKind::FreeText
                && let Some(g) = g
            {
                let mut r = m.display_rect(&g);
                let need = m.text_height(&g);
                if r.height() < need {
                    r.y1 = r.y0 + need;
                    m.set_display_rect(&g, r);
                }
            }
        }
    }

    pub(super) fn delete_mark(&mut self, id: u64) {
        if self.doc.as_ref().and_then(|d| d.mark(id)).is_none() {
            return;
        }
        if self.marks_ui.edit.as_ref().is_some_and(|e| e.id == id) {
            self.marks_ui.edit = None;
        }
        self.snapshot();
        let doc = self.doc.as_mut().unwrap();
        doc.marks.retain(|e| e.id != id);
        doc.dirty = true;
        if self.marks_ui.sel == Some(id) {
            self.marks_ui.sel = None;
        }
        self.status = "Removed. Ctrl+Z brings it back.".into();
    }

    fn recolour_mark(&mut self, id: u64, c: [u8; 3]) {
        if self
            .doc
            .as_ref()
            .and_then(|d| d.mark(id))
            .is_none_or(|m| m.color == c)
        {
            return;
        }
        self.snapshot();
        let doc = self.doc.as_mut().unwrap();
        if let Some(m) = doc.mark_mut(id) {
            m.color = c;
        }
        doc.dirty = true;
    }

    /// After undo or redo: forget marks that are gone.
    pub(super) fn marks_after_undo(&mut self) {
        self.marks_ui.edit = None;
        self.marks_ui.drag = None;
        self.marks_ui.ctx = None;
        let exists = |id: u64| self.doc.as_ref().and_then(|d| d.mark(id)).is_some();
        if self.marks_ui.sel.is_some_and(|id| !exists(id)) {
            self.marks_ui.sel = None;
        }
    }

    // --- Pointer -------------------------------------------------------------

    /// The mark under a screen point (the top one).
    pub(super) fn mark_at(&self, rects: &[Rect], p: Pos2) -> Option<u64> {
        let doc = self.doc.as_ref()?;
        let page = rects.iter().position(|r| r.contains(p))?;
        let g = self.geom(page)?;
        let (x, y) = self.xf(rects, page)?.to_page(p);
        let tol = 5.0 / self.zoom;
        doc.marks
            .iter()
            .rev()
            .filter(|e| e.m.page == page)
            .find(|e| {
                let m = &e.m;
                let r = m.display_rect(&g);
                match m.kind {
                    MarkKind::Square | MarkKind::Circle => {
                        r.inset(-tol).contains(x, y)
                            && !r.inset(tol + m.size.max(1.0)).contains(x, y)
                    }
                    MarkKind::Line | MarkKind::Arrow => {
                        let (a, b) = m.display_line(&g);
                        seg_dist((x, y), a, b) <= tol + m.size / 2.0
                    }
                    k if k.is_text_markup() => m.display_quads(&g).iter().any(|q| {
                        let xs = q.map(|p| p.0);
                        let ys = q.map(|p| p.1);
                        let min = |v: [f32; 4]| v.into_iter().fold(f32::MAX, f32::min);
                        let max = |v: [f32; 4]| v.into_iter().fold(f32::MIN, f32::max);
                        PRect::new(min(xs), min(ys), max(xs), max(ys)).contains(x, y)
                    }),
                    _ => r.contains(x, y),
                }
            })
            .map(|e| e.id)
    }

    /// The grips of a selected mark, on screen.
    fn grips(&self, rects: &[Rect], id: u64) -> Vec<(Grip, Rect)> {
        let Some(m) = self.doc.as_ref().and_then(|d| d.mark(id)) else {
            return vec![];
        };
        let (Some(g), Some(xf)) = (self.geom(m.page), self.xf(rects, m.page)) else {
            return vec![];
        };
        let at = |x: f32, y: f32| Rect::from_center_size(xf.pt(x, y), vec2(HANDLE, HANDLE));
        if m.kind.is_line() {
            let (a, b) = m.display_line(&g);
            vec![(Grip::Start, at(a.0, a.1)), (Grip::End, at(b.0, b.1))]
        } else if m.kind.resizable() {
            let r = m.display_rect(&g);
            vec![(Grip::Corner, at(r.x1, r.y1))]
        } else {
            vec![]
        }
    }

    /// The grip of the selected mark under a screen point.
    fn grip_at(&self, rects: &[Rect], p: Pos2) -> Option<(u64, Grip)> {
        let id = self.marks_ui.sel?;
        self.grips(rects, id)
            .into_iter()
            .find(|(_, r)| r.expand(3.0).contains(p))
            .map(|(g, _)| (id, g))
    }

    /// The placing tools (note, text box, shapes). Returns whether it was one.
    pub(super) fn mark_tool_mouse(
        &mut self,
        resp: &egui::Response,
        rects: &[Rect],
        p: Pos2,
    ) -> bool {
        if !matches!(self.tool, Tool::Note | Tool::TextBox | Tool::Shape(_)) {
            return false;
        }
        let real_click = resp.ctx.input(|i| i.pointer.any_released());
        let at = self.page_point(rects, p);
        if resp.drag_started() && self.tool != Tool::Note {
            let press = resp.ctx.input(|i| i.pointer.press_origin()).unwrap_or(p);
            self.drag_start = self.page_point(rects, press);
        }
        if resp.drag_stopped()
            && let Some((page, a)) = self.drag_start.take()
            && let Some(xf) = self.xf(rects, page)
        {
            let b = xf.to_page(p);
            let big = (b.0 - a.0).abs() >= 6.0 || (b.1 - a.1).abs() >= 6.0;
            match self.tool {
                Tool::TextBox => {
                    let r = PRect::from_points(a, b);
                    let r = (r.width() >= 20.0 && r.height() >= 8.0).then_some(r);
                    self.place_text_box(page, r, a);
                }
                Tool::Shape(k) if big => self.place_shape(k, page, a, b),
                _ => {}
            }
            return true;
        }
        if resp.clicked()
            && real_click
            && let Some((page, (x, y))) = at
        {
            match self.tool {
                Tool::Note => self.place_note(page, x, y),
                Tool::TextBox => self.place_text_box(page, None, (x, y)),
                Tool::Shape(k) => {
                    let (w, h) = if k.is_line() {
                        (80.0, -50.0)
                    } else {
                        (90.0, 60.0)
                    };
                    let (a, b) = if k.is_line() {
                        ((x, y), (x + w, y + h))
                    } else {
                        ((x - w / 2.0, y - h / 2.0), (x + w / 2.0, y + h / 2.0))
                    };
                    self.place_shape(k, page, a, b);
                }
                _ => {}
            }
        }
        true
    }

    /// Selecting, moving and resizing marks with no tool. Returns whether
    /// it handled the pointer.
    pub(super) fn mark_mouse(&mut self, resp: &egui::Response, rects: &[Rect], p: Pos2) -> bool {
        if let Some(mut d) = self.marks_ui.drag.take() {
            if resp.dragged() && !resp.drag_stopped() {
                self.drag_mark(&mut d, rects, p);
                self.marks_ui.drag = Some(d);
            }
            return true;
        }
        let editable = self.doc.as_ref().is_some_and(|d| d.form.editable());
        let ctx = resp.ctx.clone();
        let press = ctx.input(|i| i.pointer.press_origin()).unwrap_or(p);
        let real_click = ctx.input(|i| i.pointer.any_released());
        if resp.drag_started() && editable {
            let grabbed = self.grip_at(rects, press).or_else(|| {
                let id = self.mark_at(rects, press)?;
                let m = self.doc.as_ref()?.mark(id)?;
                (!m.kind.is_text_markup()).then_some((id, Grip::Move))
            });
            let Some((id, grip)) = grabbed else {
                return false;
            };
            let Some(m) = self.doc.as_ref().and_then(|d| d.mark(id)).cloned() else {
                return false;
            };
            let Some(xf) = self.xf(rects, m.page) else {
                return false;
            };
            self.finish_mark_edit();
            self.select_mark(Some(id));
            self.marks_ui.drag = Some(MarkDrag {
                id,
                grip,
                start: xf.to_page(press),
                orig: m,
                moved: false,
            });
            return true;
        }
        if resp.secondary_clicked()
            && let Some(id) = self.mark_at(rects, p)
        {
            self.select_mark(Some(id));
            self.marks_ui.ctx = Some(id);
            return true;
        }
        let clicked = resp.clicked() && real_click;
        let double = resp.double_clicked() && real_click;
        if !(clicked || double) {
            return false;
        }
        let Some(id) = self.mark_at(rects, p) else {
            self.finish_mark_edit();
            self.marks_ui.sel = None;
            return false;
        };
        let kind = self.doc.as_ref().and_then(|d| d.mark(id)).map(|m| m.kind);
        // Clicks on a box inside highlighted text still reach the box.
        if kind.is_some_and(|k| k.is_text_markup()) && self.field_at(rects, p).is_some() {
            self.marks_ui.sel = None;
            return false;
        }
        self.select_mark(Some(id));
        match kind {
            Some(MarkKind::Note) if editable => self.open_mark_edit(id, false),
            Some(MarkKind::FreeText) if editable && double => self.open_mark_edit(id, false),
            _ => {}
        }
        true
    }

    fn drag_mark(&mut self, d: &mut MarkDrag, rects: &[Rect], p: Pos2) {
        let (Some(g), Some(xf)) = (self.geom(d.orig.page), self.xf(rects, d.orig.page)) else {
            return;
        };
        let (x, y) = xf.to_page(p);
        let (dx, dy) = (x - d.start.0, y - d.start.1);
        if !d.moved {
            if dx.abs() < 0.5 && dy.abs() < 0.5 {
                return;
            }
            self.snapshot();
            d.moved = true;
        }
        let o = &d.orig;
        let m = match d.grip {
            Grip::Move => o.moved(&g, dx, dy),
            Grip::Corner => {
                let r = o.display_rect(&g);
                let mut m = o.clone();
                let nr = PRect::new(
                    r.x0,
                    r.y0,
                    (r.x1 + dx).max(r.x0 + 10.0),
                    (r.y1 + dy).max(r.y0 + 8.0),
                );
                m.set_display_rect(&g, nr);
                m
            }
            Grip::Start | Grip::End => {
                let (a, b) = o.display_line(&g);
                let end = d.grip == Grip::End;
                let q = if end { b } else { a };
                let mut m = o.clone();
                m.set_line_end(&g, end, (q.0 + dx, q.1 + dy));
                m
            }
        };
        let doc = self.doc.as_mut().unwrap();
        if let Some(t) = doc.mark_mut(d.id) {
            *t = m;
        }
        doc.dirty = true;
    }

    /// The pointer over marks: grabbing and resizing cursors. Returns
    /// whether it set one.
    pub(super) fn mark_cursor(&self, rects: &[Rect], p: Pos2, ctx: &egui::Context) -> bool {
        if self.marks_ui.drag.is_some() {
            ctx.set_cursor_icon(CursorIcon::Grabbing);
            return true;
        }
        if let Some((_, g)) = self.grip_at(rects, p) {
            ctx.set_cursor_icon(if g == Grip::Corner {
                CursorIcon::ResizeNwSe
            } else {
                CursorIcon::Crosshair
            });
            return true;
        }
        let Some(id) = self.mark_at(rects, p) else {
            return false;
        };
        let kind = self.doc.as_ref().and_then(|d| d.mark(id)).map(|m| m.kind);
        if kind.is_some_and(|k| k.is_text_markup()) {
            return false;
        }
        ctx.set_cursor_icon(CursorIcon::Grab);
        true
    }

    /// Delete removes the selected mark; Esc lets it go.
    pub(super) fn mark_keys(&mut self, ctx: &egui::Context) -> bool {
        let Some(id) = self.marks_ui.sel else {
            return false;
        };
        let (del, esc) = ctx.input(|i| {
            (
                i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace),
                i.key_pressed(Key::Escape),
            )
        });
        let editable = self.doc.as_ref().is_some_and(|d| d.form.editable());
        if del && editable {
            self.delete_mark(id);
            return true;
        }
        if esc {
            self.marks_ui.sel = None;
            return true;
        }
        false
    }

    // --- Drawing -------------------------------------------------------------

    /// Draws one mark (`faint`: a preview while placing).
    fn draw_mark(
        &self,
        painter: &egui::Painter,
        xf: &PageXf,
        m: &Markup,
        faint: bool,
        editing: bool,
    ) {
        let Some(g) = self.geom(m.page) else { return };
        let mut col = c32(m.color);
        if faint {
            col = col.gamma_multiply(0.6);
        }
        let z = xf.zoom;
        let pt = |p: (f32, f32)| xf.pt(p.0, p.1);
        let r = xf.rect(&m.display_rect(&g));
        match m.kind {
            MarkKind::Highlight => {
                let fill = Color32::from_rgba_unmultiplied(m.color[0], m.color[1], m.color[2], 105);
                for q in m.display_quads(&g) {
                    let pts = vec![pt(q[0]), pt(q[1]), pt(q[3]), pt(q[2])];
                    painter.add(egui::Shape::convex_polygon(pts, fill, Stroke::NONE));
                }
            }
            MarkKind::Underline | MarkKind::StrikeOut => {
                let t = if m.kind == MarkKind::Underline {
                    pm::UNDERLINE_AT
                } else {
                    pm::STRIKE_AT
                };
                for q in m.display_quads(&g) {
                    let (a, b, w) = pm::text_line(q, t);
                    painter.line_segment([pt(a), pt(b)], Stroke::new(w * z, col));
                }
            }
            MarkKind::Note => {
                let s = r.width() / pm::NOTE_SIZE;
                let ip = |x: f32, y: f32| r.min + vec2(x * s, y * s);
                let dark = c32(m.color.map(|v| (v as f32 * 0.55) as u8));
                let b = pm::NOTE_BODY;
                let body = Rect::from_min_max(ip(b[0], b[1]), ip(b[2], b[3]));
                painter.rect_filled(body, 0.0, col);
                let tail: Vec<Pos2> = pm::NOTE_TAIL.iter().map(|t| ip(t.0, t.1)).collect();
                painter.add(egui::Shape::convex_polygon(tail, col, Stroke::NONE));
                let outline = vec![
                    ip(b[0], b[1]),
                    ip(b[2], b[1]),
                    ip(b[2], b[3]),
                    ip(pm::NOTE_TAIL[2].0, pm::NOTE_TAIL[2].1),
                    ip(pm::NOTE_TAIL[1].0, pm::NOTE_TAIL[1].1),
                    ip(pm::NOTE_TAIL[0].0, pm::NOTE_TAIL[0].1),
                    ip(b[0], b[3]),
                ];
                painter.add(egui::Shape::closed_line(outline, Stroke::new(s, dark)));
                for y in pm::NOTE_LINES {
                    painter.line_segment(
                        [ip(pm::NOTE_LINE_X.0, y), ip(pm::NOTE_LINE_X.1, y)],
                        Stroke::new(1.2 * s, dark),
                    );
                }
            }
            MarkKind::FreeText => {
                if editing {
                    return;
                }
                if m.text.is_empty() || faint {
                    painter.rect_stroke(
                        r,
                        0.0,
                        Stroke::new(1.0, col.gamma_multiply(0.6)),
                        egui::StrokeKind::Inside,
                    );
                }
                let f = m.text_field(&g);
                let clip = painter.with_clip_rect(r.expand(2.0));
                draw::text(&clip, xf, &layout(&f, &m.text), col);
            }
            MarkKind::Square => {
                let w = m.size * z;
                painter.rect_stroke(
                    r.shrink(w / 2.0),
                    0.0,
                    Stroke::new(w, col),
                    egui::StrokeKind::Middle,
                );
            }
            MarkKind::Circle => {
                let w = m.size * z;
                let rr = r.shrink(w / 2.0);
                painter.add(egui::Shape::ellipse_stroke(
                    rr.center(),
                    rr.size() / 2.0,
                    Stroke::new(w, col),
                ));
            }
            MarkKind::Line | MarkKind::Arrow => {
                let (a, b) = m.display_line(&g);
                let st = Stroke::new(m.size * z, col);
                painter.line_segment([pt(a), pt(b)], st);
                if m.kind == MarkKind::Arrow {
                    let barbs = pm::arrow_barbs(a.0, a.1, b.0, b.1, m.arrow_len());
                    painter.add(egui::Shape::line(
                        vec![pt(barbs[0]), pt(b), pt(barbs[1])],
                        st,
                    ));
                }
            }
        }
    }

    /// Draws every mark, the selected one's frame and grips, and the
    /// preview of the one being placed.
    pub(super) fn draw_marks(
        &self,
        painter: &egui::Painter,
        rects: &[Rect],
        pointer: Option<Pos2>,
    ) {
        let Some(doc) = &self.doc else { return };
        for e in &doc.marks {
            if let Some(xf) = self.xf(rects, e.m.page)
                && rects[e.m.page].intersects(painter.clip_rect())
            {
                let editing = self.marks_ui.edit.as_ref().is_some_and(|ed| ed.id == e.id);
                self.draw_mark(painter, &xf, &e.m, false, editing);
            }
        }
        let accent = self.palette.accent;
        if let Some(id) = self.marks_ui.sel
            && let Some(m) = doc.mark(id)
            && let (Some(g), Some(xf)) = (self.geom(m.page), self.xf(rects, m.page))
        {
            if !m.kind.is_line() {
                painter.rect_stroke(
                    xf.rect(&m.display_rect(&g)).expand(2.0),
                    2.0,
                    Stroke::new(1.0, accent),
                    egui::StrokeKind::Middle,
                );
            }
            for (_, h) in self.grips(rects, id) {
                painter.rect_filled(h, 2.0, Color32::WHITE);
                painter.rect_stroke(h, 2.0, Stroke::new(1.5, accent), egui::StrokeKind::Inside);
            }
        }
        // Hovering a note shows its comment.
        if self.tool == Tool::None
            && self.marks_ui.edit.is_none()
            && let Some(p) = pointer
            && let Some(id) = self.mark_at(rects, p)
            && let Some(m) = doc.mark(id)
            && !m.text.is_empty()
            && m.kind != MarkKind::FreeText
        {
            let tip = if m.author.is_empty() {
                m.text.clone()
            } else {
                format!("{}\n\u{2014} {}", m.text, m.author)
            };
            egui::Tooltip::always_open(
                painter.ctx().clone(),
                painter.layer_id(),
                Id::unique("pdfr-mark-tip"),
                egui::PopupAnchor::Position(p + vec2(14.0, 18.0)),
            )
            .show(|ui| {
                ui.set_max_width(280.0);
                ui.label(tip);
            });
        }
        self.draw_placing(painter, rects, pointer);
    }

    /// The mark a placing tool would make, following the pointer.
    fn draw_placing(&self, painter: &egui::Painter, rects: &[Rect], pointer: Option<Pos2>) {
        let Some(p) = pointer else { return };
        let colour = |k| self.colour_for(k);
        match (self.tool, self.drag_start) {
            (Tool::Note, _) => {
                if let Some((page, (x, y))) = self.page_point(rects, p)
                    && let (Some(g), Some(xf)) = (self.geom(page), self.xf(rects, page))
                {
                    let h = pm::NOTE_SIZE / 2.0;
                    let m = Markup::note(page, &g, x - h, y - h, colour(MarkKind::Note));
                    self.draw_mark(painter, &xf, &m, true, false);
                }
            }
            (Tool::Shape(_), Some((page, a))) | (Tool::TextBox, Some((page, a))) => {
                let (Some(g), Some(xf)) = (self.geom(page), self.xf(rects, page)) else {
                    return;
                };
                let b = xf.to_page(p);
                let m = match self.tool {
                    Tool::Shape(k) => Markup::shape(k, page, &g, a, b, colour(k)),
                    _ => Markup::text_box(
                        page,
                        &g,
                        PRect::from_points(a, b),
                        colour(MarkKind::FreeText),
                    ),
                };
                self.draw_mark(painter, &xf, &m, true, false);
            }
            _ => {}
        }
    }

    // --- Windows -------------------------------------------------------------

    /// The comment card or text box editor, the bar over selected text,
    /// a mark's right-click menu.
    pub(super) fn mark_windows(
        &mut self,
        ctx: &egui::Context,
        resp: &egui::Response,
        rects: &[Rect],
    ) {
        self.mark_editor(ctx, rects);
        self.selection_bar(ctx, rects);
        self.mark_menu(resp);
    }

    fn mark_editor(&mut self, ctx: &egui::Context, rects: &[Rect]) {
        let Some(e) = self.marks_ui.edit.clone() else {
            return;
        };
        let Some(m) = self.doc.as_ref().and_then(|d| d.mark(e.id)).cloned() else {
            self.marks_ui.edit = None;
            return;
        };
        let (Some(g), Some(xf)) = (self.geom(m.page), self.xf(rects, m.page)) else {
            return;
        };
        let r = xf.rect(&m.display_rect(&g));
        let mut text = e.text.clone();
        let (mut done, mut delete) = (false, false);
        let area = if m.kind == MarkKind::FreeText {
            // Typed straight onto the page.
            let font = egui::FontId::new(
                m.size * self.zoom,
                egui::FontFamily::Name(draw::FONT_FAMILY.into()),
            );
            egui::Area::new(Id::unique("pdfr-text-box-edit"))
                .fixed_pos(r.min)
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    let te = egui::TextEdit::multiline(&mut text)
                        .id(Id::unique(EDIT_ID))
                        .font(font)
                        .text_color(c32(m.color))
                        .desired_width(r.width().max(40.0))
                        .desired_rows(1)
                        .margin(egui::Margin::same(
                            (pdfr_core::layout::PAD * self.zoom) as i8,
                        ))
                        .background_color(Color32::from_white_alpha(235))
                        .hint_text("Type here");
                    let out = ui.add(te);
                    if e.focus {
                        out.request_focus();
                    }
                    if out.lost_focus() {
                        done = true;
                    }
                })
        } else {
            let pal = self.palette.clone();
            let c = Colors::new(&pal);
            let at = if r.right() + 290.0 < self.viewport.right() {
                r.right_top() + vec2(8.0, 0.0)
            } else {
                r.left_top() - vec2(288.0, 0.0)
            };
            egui::Area::new(Id::unique("pdfr-comment-card"))
                .fixed_pos(at)
                .constrain(true)
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    widgets::panel_frame(ctx, &pal, &c, 12).show(ui, |ui| {
                        ui.set_width(256.0);
                        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                        ui.horizontal(|ui| {
                            let title = if m.kind == MarkKind::Note {
                                "Note"
                            } else {
                                "Comment"
                            };
                            ui.label(egui::RichText::new(title).strong().color(c.text));
                            if !m.author.is_empty() {
                                ui.label(egui::RichText::new(&m.author).color(c.weak));
                            }
                        });
                        let out = ui.add(
                            egui::TextEdit::multiline(&mut text)
                                .id(Id::unique(EDIT_ID))
                                .desired_rows(4)
                                .desired_width(f32::INFINITY)
                                .hint_text("Write a comment…"),
                        );
                        if e.focus {
                            out.request_focus();
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            done |= widgets::primary_button(ui, &c, "Done")
                                .on_hover_text("Ctrl+Enter")
                                .clicked();
                            delete = widgets::danger_button(ui, "Delete", false).clicked();
                        });
                    });
                })
        };
        let keys = ctx.input(|i| {
            i.key_pressed(Key::Escape) || (i.modifiers.command && i.key_pressed(Key::Enter))
        });
        // A press anywhere else puts the text in.
        let outside = ctx.input(|i| {
            i.pointer.any_pressed()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|p| !area.response.rect.contains(p))
        });
        if let Some(ed) = self.marks_ui.edit.as_mut() {
            ed.text = text;
            ed.focus = false;
        }
        if delete {
            let created = e.created;
            self.marks_ui.edit = None;
            if created {
                // Never kept: its making is undone.
                if let Some(doc) = self.doc.as_mut() {
                    doc.marks.retain(|x| x.id != e.id);
                }
                self.undo.pop();
                self.marks_ui.sel = None;
            } else {
                self.delete_mark(e.id);
            }
        } else if done || keys || (outside && !e.focus) {
            self.finish_mark_edit();
        }
    }

    /// A small bar over selected text: highlight, underline, strike
    /// through, comment.
    fn selection_bar(&mut self, ctx: &egui::Context, rects: &[Rect]) {
        let editable = self.doc.as_ref().is_some_and(|d| d.form.editable());
        if !editable
            || self.tool != Tool::None
            || self.marks_ui.edit.is_some()
            || self.text_sel.as_ref().is_none_or(|s| s.dragging)
            || self.selection_text().is_none()
        {
            return;
        }
        let Some((page, spans)) = self.selection_rects() else {
            return;
        };
        let Some(xf) = self.xf(rects, page) else {
            return;
        };
        let first = xf.rect(&spans[0]);
        if !self.viewport.contains(first.left_top()) {
            return;
        }
        let pal = self.palette.clone();
        let c = Colors::new(&pal);
        let mut run = None;
        egui::Area::new(Id::unique("pdfr-selection-bar"))
            .fixed_pos(first.left_top() - vec2(0.0, 42.0))
            .constrain(true)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                widgets::panel_frame(ctx, &pal, &c, 4).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        for cmd in [
                            Command::HighlightText,
                            Command::UnderlineText,
                            Command::StrikeText,
                            Command::CommentText,
                        ] {
                            let s = cmd.spec();
                            let tip = commands::tooltip(ctx, cmd);
                            if widgets::icon_button(ui, &c, s.icon, &tip).clicked() {
                                run = Some(cmd);
                            }
                        }
                    });
                });
            });
        if let Some(cmd) = run {
            self.run(cmd);
        }
    }

    /// A mark's right-click menu: edit, colour, delete.
    fn mark_menu(&mut self, resp: &egui::Response) {
        let Some(id) = self.marks_ui.ctx else { return };
        let Some(m) = self.doc.as_ref().and_then(|d| d.mark(id)).cloned() else {
            self.marks_ui.ctx = None;
            return;
        };
        let editable = self.doc.as_ref().is_some_and(|d| d.form.editable());
        let (mut edit, mut delete, mut colour) = (false, false, None);
        let shown = resp.context_menu(|ui| {
            ui.label(egui::RichText::new(m.kind.label()).weak());
            if !editable {
                return;
            }
            let what = if m.kind == MarkKind::FreeText {
                "Edit text"
            } else if m.text.is_empty() {
                "Add a comment…"
            } else {
                "Edit comment…"
            };
            if ui.button(what).clicked() {
                edit = true;
                ui.close();
            }
            if let Some(c) = colour_row(ui, Some(m.color)) {
                colour = Some(c);
                ui.close();
            }
            ui.separator();
            if ui.button("Delete").clicked() {
                delete = true;
                ui.close();
            }
        });
        if edit {
            self.open_mark_edit(id, false);
        } else if delete {
            self.delete_mark(id);
        } else if let Some(c) = colour {
            self.recolour_mark(id, c);
        }
        if shown.is_none() || edit || delete || colour.is_some() {
            self.marks_ui.ctx = None;
        }
    }

    /// The toolbar's markup menu, under its button.
    pub(super) fn markup_menu(&mut self, ctx: &egui::Context) {
        if !self.marks_ui.menu {
            return;
        }
        let Some(anchor) = self.marks_ui.anchor else {
            return;
        };
        let pal = self.palette.clone();
        let c = Colors::new(&pal);
        let mut run = None;
        let mut colour = None;
        let current = self
            .marks_ui
            .sel
            .and_then(|id| self.doc.as_ref()?.mark(id))
            .map(|m| m.color)
            .or(self.marks_ui.colour);
        let area = egui::Area::new(Id::unique("pdfr-markup-menu"))
            .fixed_pos(anchor.left_bottom() + vec2(0.0, 6.0))
            .constrain(true)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                widgets::panel_frame(ctx, &pal, &c, 6).show(ui, |ui| {
                    ui.set_width(250.0);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (i, cmd) in [
                        Command::HighlightText,
                        Command::UnderlineText,
                        Command::StrikeText,
                        Command::CommentText,
                        Command::AddNote,
                        Command::AddTextBox,
                        Command::AddRectangle,
                        Command::AddEllipse,
                        Command::AddArrow,
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        if i == 4 || i == 6 {
                            ui.add_space(3.0);
                            widgets::row_divider(ui, &c);
                            ui.add_space(3.0);
                        }
                        if self.menu_row(ui, &c, cmd) {
                            run = Some(cmd);
                        }
                    }
                    ui.add_space(6.0);
                    widgets::row_divider(ui, &c);
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.add_space(6.0);
                        colour = colour_row(ui, current);
                    });
                });
            });
        if let Some(cmd) = run {
            self.marks_ui.menu = false;
            self.run(cmd);
        }
        if let Some(col) = colour {
            self.marks_ui.colour = Some(col);
            if let Some(id) = self.marks_ui.sel {
                self.recolour_mark(id, col);
            }
        }
        let outside = ctx.input(|i| {
            i.pointer.any_pressed()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|p| !area.response.rect.contains(p) && !anchor.contains(p))
        });
        if outside {
            self.marks_ui.menu = false;
        }
    }

    /// One row of the markup menu: icon, label and shortcut.
    fn menu_row(&self, ui: &mut egui::Ui, c: &Colors, cmd: Command) -> bool {
        let s = cmd.spec();
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
        let on = self.is_on(cmd);
        if on || resp.hovered() {
            ui.painter().rect_filled(
                r,
                6.0,
                if on {
                    c.accent.gamma_multiply(0.2)
                } else {
                    c.hover
                },
            );
        }
        crate::icons::paint(
            ui.painter(),
            Rect::from_center_size(r.left_center() + vec2(16.0, 0.0), vec2(16.0, 16.0)),
            s.icon,
            c.text,
        );
        ui.painter().text(
            r.left_center() + vec2(34.0, 0.0),
            egui::Align2::LEFT_CENTER,
            s.label,
            egui::FontId::proportional(13.5),
            c.text,
        );
        if let Some(k) = s.shortcuts.first() {
            ui.painter().text(
                r.right_center() - vec2(8.0, 0.0),
                egui::Align2::RIGHT_CENTER,
                commands::key_text(ui.ctx(), k),
                egui::FontId::proportional(12.0),
                c.weak,
            );
        }
        resp.on_hover_cursor(CursorIcon::PointingHand).clicked()
    }

    // --- Sidebar -------------------------------------------------------------

    /// The Comments tab: every note and mark with its text and page.
    pub(super) fn comments_panel(&mut self, ui: &mut egui::Ui, c: &Colors) {
        let Some(doc) = &self.doc else { return };
        let mut items: Vec<(u64, Markup, String)> = doc
            .marks
            .iter()
            .map(|e| (e.id, e.m.clone(), self.marked_text(&e.m)))
            .collect();
        items.sort_by(|a, b| {
            let (ra, rb) = (
                a.1.display_rect(&doc.form.pages[a.1.page]),
                b.1.display_rect(&doc.form.pages[b.1.page]),
            );
            a.1.page
                .cmp(&b.1.page)
                .then(ra.y0.total_cmp(&rb.y0))
                .then(ra.x0.total_cmp(&rb.x0))
        });
        let mut go = None;
        egui::ScrollArea::vertical()
            .id_salt("pdfr-comments")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                for (id, m, quoted) in &items {
                    let on = self.marks_ui.sel == Some(*id);
                    let resp = egui::Frame::new()
                        .fill(if on {
                            c.accent.gamma_multiply(0.2)
                        } else {
                            c.card
                        })
                        .stroke(Stroke::new(1.0, c.border))
                        .corner_radius(6)
                        .inner_margin(egui::Margin::symmetric(8, 6))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                let (r, _) =
                                    ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                                ui.painter().circle_filled(r.center(), 4.5, c32(m.color));
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} \u{b7} page {}",
                                        m.kind.label(),
                                        m.page + 1
                                    ))
                                    .size(11.5)
                                    .color(c.weak),
                                );
                            });
                            if !quoted.is_empty() {
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(format!("\u{201c}{quoted}\u{201d}"))
                                            .italics()
                                            .size(12.0)
                                            .color(c.weak),
                                    )
                                    .truncate(),
                                );
                            }
                            if !m.text.is_empty() {
                                ui.add(
                                    egui::Label::new(egui::RichText::new(&m.text).color(c.text))
                                        .wrap(),
                                );
                            }
                        })
                        .response
                        .interact(Sense::click());
                    if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                        go = Some(*id);
                    }
                }
            });
        if let Some(id) = go {
            self.go_to_mark(id);
        }
    }

    /// The page text under a text mark (for the list).
    fn marked_text(&self, m: &Markup) -> String {
        if !m.kind.is_text_markup() {
            return String::new();
        }
        let (Some(doc), Some(g)) = (&self.doc, self.geom(m.page)) else {
            return String::new();
        };
        let Some(runs) = doc.text.get(&m.page) else {
            return String::new();
        };
        let r = m.display_rect(&g);
        let words: Vec<String> = runs
            .iter()
            .filter(|t| t.rect.overlap_ratio(&r) > 0.5)
            .map(|t| {
                let c = r.inset(-1.0);
                t.clip(c.x0, c.x1)
            })
            .filter(|s| !s.trim().is_empty())
            .collect();
        words
            .join(" ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Scrolls to a mark and selects it.
    pub(super) fn go_to_mark(&mut self, id: u64) {
        let Some(m) = self.doc.as_ref().and_then(|d| d.mark(id)).cloned() else {
            return;
        };
        let Some(g) = self.geom(m.page) else { return };
        self.scroll_to_rect(m.page, m.display_rect(&g));
        self.select_mark(Some(id));
    }
}

/// A row of colour swatches; returns the one clicked.
fn colour_row(ui: &mut egui::Ui, current: Option<[u8; 3]>) -> Option<[u8; 3]> {
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (name, col) in COLOURS {
            let (r, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
            let p = ui.painter();
            if current == Some(col) {
                p.circle_stroke(r.center(), 9.5, Stroke::new(1.5, ui.visuals().text_color()));
            }
            p.circle_filled(r.center(), 7.0, c32(col));
            if resp
                .on_hover_text(name)
                .on_hover_cursor(CursorIcon::PointingHand)
                .clicked()
            {
                picked = Some(col);
            }
        }
    });
    picked
}
