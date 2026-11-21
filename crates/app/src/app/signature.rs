//! Signing: the dialog where a signature (or initials) is drawn or typed,
//! the place tool that puts it on the page, and moving and resizing a
//! placed signature.

use super::*;
use crate::assist_ui::widgets::{
    Colors, checkbox, panel_frame, primary_button, row_divider, secondary_button, segmented,
};
use egui::{Align, Layout, RichText};
use pdfr_core::signature::Ink;

/// Size of the drawing pad, in screen points.
pub(super) const PAD_W: f32 = 460.0;
const PAD_H: f32 = 160.0;
/// Pen width on the pad, in screen points.
const PEN: f32 = 2.6;
/// Default height of a signature placed on an empty spot, in page points.
const SIGN_H: f32 = 36.0;
const INITIALS_H: f32 = 24.0;
/// The resize handle at a selected signature's bottom-right corner.
const HANDLE: f32 = 9.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SignTab {
    Draw,
    Type,
    /// A photo or scan of a signature.
    Image,
}

/// A picture chosen on the Image tab: the original (scaled down once),
/// how light counts as paper, and the cleaned-up result.
pub(super) struct Picture {
    original: pdfr_core::signature::SigImage,
    threshold: u8,
    ink: Option<Arc<Ink>>,
}

pub(super) struct SignDialog {
    pub initials: bool,
    tab: SignTab,
    /// Finished strokes on the pad (pad-local points).
    strokes: Vec<Vec<[f32; 2]>>,
    drawing: bool,
    name: String,
    person: Option<u64>,
    save: bool,
    /// Where the signature goes once made, if already known.
    target: Option<Target>,
    typed: Option<(String, Arc<Ink>)>,
    picture: Option<Picture>,
    picture_error: Option<String>,
}

/// A spot to sign: a "sign here" field of the PDF (filled as a box) or a
/// box labelled as a signature (signed on its line).
#[derive(Clone, Copy, Debug)]
pub(super) struct Target {
    pub page: usize,
    pub rect: PRect,
    pub on_line: bool,
}

/// Moving or resizing a placed signature with the mouse.
#[derive(Clone, Copy, Debug)]
pub(super) struct SigDrag {
    id: u64,
    resize: bool,
    start: (f32, f32),
    orig: PRect,
}

/// Whether a box's label asks for a signature.
pub(super) fn signature_label(label: &str) -> bool {
    let l = label.to_lowercase();
    let date = ["date", "datum", "fecha", "data", "päiväys", "dato"];
    if date.iter().any(|d| l.contains(d)) {
        return false;
    }
    [
        "signature",
        "sign here",
        "signed",
        "underskrift",
        "namnteckning",
        "signatur",
        "unterschrift",
        "firma",
        "handtekening",
        "allekirjoitus",
        "podpis",
        "assinatura",
        "подпись",
    ]
    .iter()
    .any(|k| l.contains(k))
}

/// A person's initials from their name ("Anna Maria Lindqvist" -> "AML").
fn initials_of(name: &str) -> String {
    name.split(|c: char| c.is_whitespace() || c == '-')
        .filter_map(|w| w.chars().next())
        .filter(|c| c.is_alphabetic())
        .flat_map(char::to_uppercase)
        .collect()
}

impl Target {
    /// Where `ink` goes on this spot.
    pub(super) fn rect_for(&self, ink: &Ink) -> PRect {
        let t = self.rect;
        if !self.on_line {
            return ink.fit(&t.inset(1.0));
        }
        // On a line: taller than the box if need be, standing on the line,
        // starting at its left.
        let mut h = t.height().clamp(24.0, 54.0);
        let mut w = h * ink.aspect;
        if w > t.width() - 4.0 {
            w = (t.width() - 4.0).max(10.0);
            h = w / ink.aspect;
        }
        let y1 = t.y1 + h * 0.08;
        PRect::new(t.x0 + 2.0, y1 - h, t.x0 + 2.0 + w, y1)
    }
}

impl App {
    /// Opens the signing dialog (or the initials one).
    pub(super) fn open_sign(&mut self, initials: bool, target: Option<Target>) {
        let people = &self.assist.a.store.profiles;
        // The person most recently filled in, if any.
        let person = people.iter().max_by_key(|p| p.last_used).map(|p| p.id);
        let name = person
            .and_then(|id| people.iter().find(|p| p.id == id))
            .map(|p| p.title())
            .unwrap_or_default();
        let name = if initials { initials_of(&name) } else { name };
        let save = self.assist.a.store.settings.remember != pdfr_assist::memory::Remember::Never;
        self.tool = Tool::None;
        self.placing = None;
        self.sign = Some(SignDialog {
            initials,
            tab: SignTab::Draw,
            strokes: vec![],
            drawing: false,
            name,
            person,
            save,
            target,
            typed: None,
            picture: None,
            picture_error: None,
        });
    }

    /// The signing target for the focused box, if it asks for a signature.
    pub(super) fn focused_sign_target(&self) -> Option<Target> {
        let e = self.focused_entry()?;
        (e.f.kind.is_text() && e.f.label.as_deref().is_some_and(signature_label)).then_some(
            Target {
                page: e.f.page,
                rect: e.f.rect,
                on_line: true,
            },
        )
    }

    /// The PDF's own "sign here" spots that are not signed yet.
    pub(super) fn open_sign_spots(&self) -> Vec<(usize, PRect)> {
        let Some(doc) = &self.doc else { return vec![] };
        doc.form
            .sign_here
            .iter()
            .filter(|(p, r)| {
                !doc.entries.iter().any(|e| {
                    e.f.kind.is_signature() && e.f.page == *p && e.f.rect.overlap_ratio(r) > 0.3
                })
            })
            .copied()
            .collect()
    }

    /// The spot to sign under a screen point, if any.
    pub(super) fn sign_target_at(&self, rects: &[Rect], p: Pos2) -> Option<Target> {
        let page = rects.iter().position(|r| r.contains(p))?;
        let xf = PageXf {
            origin: rects[page].min,
            zoom: self.zoom,
        };
        let (x, y) = xf.to_page(p);
        if let Some((_, r)) = self
            .open_sign_spots()
            .into_iter()
            .find(|(pg, r)| *pg == page && r.contains(x, y))
        {
            return Some(Target {
                page,
                rect: r,
                on_line: false,
            });
        }
        let doc = self.doc.as_ref()?;
        doc.entries
            .iter()
            .filter(|e| e.f.page == page && e.f.rect.contains(x, y) && e.f.kind.is_text())
            .find(|e| e.f.label.as_deref().is_some_and(signature_label))
            .map(|e| Target {
                page,
                rect: e.f.rect,
                on_line: true,
            })
    }

    /// Where the signature being placed would go for the pointer at `p`.
    pub(super) fn placement_at(&self, rects: &[Rect], p: Pos2) -> Option<(usize, PRect)> {
        let ink = self.placing.as_ref()?;
        if let Some(t) = self.sign_target_at(rects, p) {
            return Some((t.page, t.rect_for(ink)));
        }
        let page = rects.iter().position(|r| r.contains(p))?;
        let xf = PageXf {
            origin: rects[page].min,
            zoom: self.zoom,
        };
        let (x, y) = xf.to_page(p);
        let mut h = if self.placing_initials {
            INITIALS_H
        } else {
            SIGN_H
        };
        let mut w = h * ink.aspect;
        if w > 220.0 {
            w = 220.0;
            h = w / ink.aspect;
        }
        Some((
            page,
            PRect::new(x - w / 2.0, y - h / 2.0, x + w / 2.0, y + h / 2.0),
        ))
    }

    /// Puts a signature on the page, as one undo step, and selects it.
    pub(super) fn place_signature(&mut self, page: usize, rect: PRect, ink: Arc<Ink>) {
        if self.place_digital(page, rect) {
            return;
        }
        let lock = self.assist.a.store.settings.lock_signatures;
        self.snapshot();
        let Some(doc) = self.doc.as_mut() else { return };
        let id = doc.add(Field::new_signature(page, rect, ink, lock));
        doc.reorder();
        doc.dirty = true;
        self.assist.invalidate();
        self.tool = Tool::None;
        self.placing = None;
        self.focus = Some(Focus {
            id,
            ed: Editor::default(),
            ghost: None,
            ghost_dismissed: false,
        });
        self.status = "Signed. Drag to move it, or its corner to resize. Delete removes it.".into();
    }

    /// Switches a signature between an annotation and part of the page.
    pub(super) fn toggle_lock(&mut self, id: u64) {
        let Some(i) = self.doc.as_ref().and_then(|d| d.index_of(id)) else {
            return;
        };
        self.snapshot();
        let doc = self.doc.as_mut().unwrap();
        if let FieldKind::Signature { lock, .. } = &mut doc.entries[i].f.kind {
            *lock = !*lock;
        }
        doc.dirty = true;
    }

    /// The handle for resizing a selected signature, in screen space.
    pub(super) fn sig_handle(&self, rects: &[Rect], f: &Field) -> Option<Rect> {
        let pr = rects.get(f.page)?;
        let r = PageXf {
            origin: pr.min,
            zoom: self.zoom,
        }
        .rect(&f.rect);
        Some(Rect::from_center_size(
            r.right_bottom(),
            vec2(HANDLE, HANDLE),
        ))
    }

    /// Mouse on a signature with no tool: starts, continues or ends moving
    /// or resizing it. Returns whether it handled the pointer.
    pub(super) fn sig_mouse(&mut self, resp: &egui::Response, rects: &[Rect], p: Pos2) -> bool {
        if let Some(d) = self.sig_drag {
            if resp.drag_stopped() {
                self.sig_drag = None;
                if let Some(doc) = self.doc.as_mut() {
                    doc.reorder();
                }
                return true;
            }
            if resp.dragged() {
                let Some(doc) = self.doc.as_mut() else {
                    return true;
                };
                let Some(i) = doc.index_of(d.id) else {
                    return true;
                };
                let page = doc.entries[i].f.page;
                let Some(pr) = rects.get(page) else {
                    return true;
                };
                let (x, y) = PageXf {
                    origin: pr.min,
                    zoom: self.zoom,
                }
                .to_page(p);
                let (dx, dy) = (x - d.start.0, y - d.start.1);
                let o = d.orig;
                doc.entries[i].f.rect = if d.resize {
                    // Keep the shape: the larger of the two pulls wins.
                    let aspect = o.width() / o.height().max(1e-3);
                    let w = (o.width() + dx).max(o.width() + dy * aspect).max(12.0);
                    PRect::new(o.x0, o.y0, o.x0 + w, o.y0 + w / aspect)
                } else {
                    PRect::new(o.x0 + dx, o.y0 + dy, o.x1 + dx, o.y1 + dy)
                };
                doc.dirty = true;
                return true;
            }
        }
        if !resp.drag_started() {
            return false;
        }
        let editable = self.doc.as_ref().is_some_and(|d| d.form.editable());
        if !editable {
            return false;
        }
        // The handle of the selected signature first, then any signature.
        let focused = self
            .focused_entry()
            .filter(|e| e.f.kind.is_signature())
            .cloned();
        let press = resp.ctx.input(|i| i.pointer.press_origin()).unwrap_or(p);
        let on_handle = focused
            .as_ref()
            .and_then(|e| self.sig_handle(rects, &e.f))
            .is_some_and(|h| h.expand(3.0).contains(press));
        let hit = if on_handle {
            focused.map(|e| e.id)
        } else {
            self.field_at(rects, press).map(|h| h.0).filter(|id| {
                self.doc
                    .as_ref()
                    .and_then(|d| d.index_of(*id))
                    .is_some_and(|i| self.doc.as_ref().unwrap().entries[i].f.kind.is_signature())
            })
        };
        let Some(id) = hit else { return false };
        let doc = self.doc.as_ref().unwrap();
        let f = &doc.entries[doc.index_of(id).unwrap()].f;
        let Some(pr) = rects.get(f.page) else {
            return false;
        };
        let start = PageXf {
            origin: pr.min,
            zoom: self.zoom,
        }
        .to_page(press);
        let orig = f.rect;
        self.snapshot();
        self.sig_drag = Some(SigDrag {
            id,
            resize: on_handle,
            start,
            orig,
        });
        self.focus = Some(Focus {
            id,
            ed: Editor::default(),
            ghost: None,
            ghost_dismissed: false,
        });
        true
    }

    /// Draws the PDF's unsigned "sign here" spots.
    pub(super) fn draw_sign_spots(&self, painter: &egui::Painter, rects: &[Rect]) {
        let accent = self.palette.accent;
        for (page, r) in self.open_sign_spots() {
            let Some(pr) = rects.get(page) else { continue };
            let sr = PageXf {
                origin: pr.min,
                zoom: self.zoom,
            }
            .rect(&r);
            painter.rect_filled(sr, 2.0, accent.gamma_multiply(0.12));
            painter.rect_stroke(
                sr,
                2.0,
                Stroke::new(1.0, accent.gamma_multiply(0.7)),
                egui::StrokeKind::Inside,
            );
            painter.text(
                sr.left_center() + vec2(6.0, 0.0),
                egui::Align2::LEFT_CENTER,
                "Sign here",
                egui::FontId::proportional((sr.height() * 0.4).clamp(9.0, 14.0)),
                crate::app::canvas::strong(accent),
            );
        }
    }

    /// Draws a placed signature, with its frame and handle when selected.
    pub(super) fn draw_signature(
        &self,
        painter: &egui::Painter,
        rects: &[Rect],
        f: &Field,
        r: Rect,
        selected: bool,
        hovered: bool,
    ) {
        let FieldKind::Signature { ink, lock } = &f.kind else {
            return;
        };
        crate::ink::paint(painter, ink, r, Color32::WHITE, self.dark_pages());
        if selected || hovered {
            let c = if selected {
                self.palette.accent
            } else {
                self.palette.accent.gamma_multiply(0.5)
            };
            painter.rect_stroke(
                r.expand(2.0),
                2.0,
                Stroke::new(1.0, c),
                egui::StrokeKind::Middle,
            );
        }
        if selected && let Some(h) = self.sig_handle(rects, f) {
            painter.rect_filled(h, 2.0, Color32::WHITE);
            painter.rect_stroke(
                h,
                2.0,
                Stroke::new(1.5, self.palette.accent),
                egui::StrokeKind::Inside,
            );
            if *lock {
                painter.text(
                    r.left_top() + vec2(0.0, -4.0),
                    egui::Align2::LEFT_BOTTOM,
                    "Locked into the page when saved",
                    egui::FontId::proportional(11.0),
                    self.palette.accent,
                );
            }
        }
    }

    /// The signing dialog, while open.
    pub(super) fn sign_window(&mut self, ctx: &egui::Context) {
        let Some(dlg) = self.sign.as_mut() else {
            return;
        };
        let pal = self.palette.clone();
        let c = Colors::new(&pal);
        let store = &self.assist.a.store;
        let people: Vec<(u64, String)> = store.profiles.iter().map(|p| (p.id, p.title())).collect();
        let mut saved: Vec<&pdfr_assist::memory::SavedSignature> = store
            .signatures
            .iter()
            .filter(|s| s.initials == dlg.initials)
            .collect();
        saved.sort_by_key(|s| std::cmp::Reverse(s.last_used));
        let saved: Vec<(u64, Option<u64>, Arc<Ink>)> = saved
            .iter()
            .filter_map(|s| Some((s.id, s.person, Arc::new(Ink::decode(&s.ink)?))))
            .collect();
        let mut use_saved: Option<(u64, Arc<Ink>)> = None;
        let mut done: Option<Arc<Ink>> = None;
        let mut cancel = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
        let mut pick_image = false;
        let mut to_certificate = false;
        let title = if dlg.initials { "Initials" } else { "Sign" };
        egui::Modal::new(Id::unique("pdfr-sign"))
            .frame(panel_frame(ctx, &pal, &c, 20))
            .show(ctx, |ui| {
                ui.set_width(PAD_W);
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                ui.label(RichText::new(title).size(20.0).strong().color(c.text));
                if !saved.is_empty() {
                    ui.label(RichText::new("Saved").size(12.0).color(c.weak));
                    ui.horizontal_wrapped(|ui| {
                        for (id, person, ink) in &saved {
                            let (r, resp) =
                                ui.allocate_exact_size(vec2(140.0, 52.0), Sense::click());
                            let p = ui.painter();
                            let fill = if resp.hovered() { c.hover } else { c.card };
                            p.rect_filled(r, 6.0, fill);
                            p.rect_filled(r.shrink(4.0), 4.0, Color32::WHITE);
                            crate::ink::paint(p, ink, r.shrink(8.0), Color32::WHITE, false);
                            let who = person
                                .and_then(|pid| people.iter().find(|(i, _)| *i == pid))
                                .map(|(_, n)| n.clone());
                            let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
                            let resp = match who {
                                Some(n) => resp.on_hover_text(format!("{n}: use this one")),
                                None => resp.on_hover_text("Use this one"),
                            };
                            if resp.clicked() {
                                use_saved = Some((*id, ink.clone()));
                            }
                        }
                    });
                    row_divider(ui, &c);
                }
                // `None` is the Certificate tab (a digital signature),
                // which is its own dialog.
                let mut tabs = vec![
                    (Some(SignTab::Draw), "Draw"),
                    (Some(SignTab::Type), "Type"),
                    (Some(SignTab::Image), "Image"),
                ];
                if !dlg.initials {
                    tabs.push((None, "Certificate"));
                }
                match segmented(ui, &c, &tabs, Some(dlg.tab)) {
                    Some(Some(t)) => dlg.tab = t,
                    Some(None) => to_certificate = true,
                    None => {}
                }
                match dlg.tab {
                    SignTab::Draw => draw_pad(ui, dlg, &c),
                    SignTab::Type => type_pad(ui, dlg, &c),
                    SignTab::Image => pick_image |= image_pad(ui, dlg, &c),
                }
                if !people.is_empty() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Whose").color(c.weak));
                        let current = dlg
                            .person
                            .and_then(|id| people.iter().find(|(i, _)| *i == id))
                            .map_or("Nobody in particular".to_string(), |(_, n)| n.clone());
                        egui::ComboBox::from_id_salt("pdfr-sign-person")
                            .selected_text(current)
                            .width(220.0)
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_label(dlg.person.is_none(), "Nobody in particular")
                                    .clicked()
                                {
                                    dlg.person = None;
                                }
                                for (id, n) in &people {
                                    if ui.selectable_label(dlg.person == Some(*id), n).clicked() {
                                        dlg.person = Some(*id);
                                        dlg.name = if dlg.initials {
                                            initials_of(n)
                                        } else {
                                            n.clone()
                                        };
                                        dlg.typed = None;
                                    }
                                }
                            });
                    });
                }
                let ink = current_ink(dlg);
                ui.horizontal(|ui| {
                    let cb = checkbox(ui, &c, dlg.save);
                    let t = ui.add(
                        egui::Label::new(RichText::new("Save for next time").color(c.weak))
                            .sense(Sense::click()),
                    );
                    if cb.clicked() || t.clicked() {
                        dlg.save = !dlg.save;
                    }
                    t.on_hover_text(
                        "Kept on this computer only. Delete it any time in Settings → People.",
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let label = if dlg.target.is_some() {
                            "Sign"
                        } else {
                            "Place on the page"
                        };
                        let ok = ink.as_ref().is_some_and(|i| !i.is_empty());
                        let enter = ok
                            && ui.input(|i| i.key_pressed(Key::Enter))
                            && dlg.tab == SignTab::Draw;
                        if ui
                            .add_enabled_ui(ok, |ui| primary_button(ui, &c, label))
                            .inner
                            .clicked()
                            || enter
                        {
                            done = ink.clone();
                        }
                        if secondary_button(ui, &c, "Cancel")
                            .on_hover_text("Esc")
                            .clicked()
                        {
                            cancel = true;
                        }
                    });
                });
            });
        if cancel {
            self.sign = None;
            return;
        }
        if to_certificate {
            let target = self.sign.take().and_then(|d| d.target);
            self.open_digital(target);
            return;
        }
        if pick_image {
            self.pick_signature_image();
            return;
        }
        let dlg = self.sign.as_ref().unwrap();
        let (initials, target, person) = (dlg.initials, dlg.target, dlg.person);
        let ink = if let Some((id, ink)) = use_saved {
            self.assist.a.store.touch_signature(id);
            self.assist.a.save();
            ink
        } else if let Some(ink) = done {
            if dlg.save {
                self.assist
                    .a
                    .store
                    .add_signature(person, initials, ink.encode());
                self.assist.a.save();
            }
            ink
        } else {
            return;
        };
        self.sign = None;
        self.placing_initials = initials;
        match target {
            Some(t) => {
                let r = t.rect_for(&ink);
                self.place_signature(t.page, r, ink);
            }
            None => {
                self.placing = Some(ink);
                self.tool = Tool::PlaceSignature;
                self.status = "Click where the signature goes. Esc cancels.".into();
            }
        }
    }
}

impl SignDialog {
    /// Switches to the Draw, Type or Image tab.
    pub(super) fn show_tab(&mut self, t: SignTab) {
        self.tab = t;
    }

    /// Types a name on the Type tab (tests, screenshots).
    pub(super) fn type_name(&mut self, name: &str) {
        self.tab = SignTab::Type;
        self.name = name.into();
        self.typed = None;
    }
}

/// The ink the dialog would place now.
fn current_ink(dlg: &mut SignDialog) -> Option<Arc<Ink>> {
    match dlg.tab {
        SignTab::Draw => {
            let ink = Ink::from_drawing(&dlg.strokes, PEN);
            (!ink.is_empty()).then(|| Arc::new(ink))
        }
        SignTab::Type => typed_ink(dlg),
        SignTab::Image => dlg.picture.as_ref().and_then(|p| p.ink.clone()),
    }
}

impl App {
    /// A picture was chosen for the Image tab.
    pub(super) fn signature_image_picked(&mut self, path: PathBuf) {
        let Some(dlg) = self.sign.as_mut() else {
            return;
        };
        dlg.tab = SignTab::Image;
        let loaded = std::fs::metadata(&path)
            .map_err(|e| e.to_string())
            .and_then(|m| {
                if m.len() > 40 * 1024 * 1024 {
                    Err("the picture is too large".into())
                } else {
                    std::fs::read(&path).map_err(|e| e.to_string())
                }
            })
            .and_then(|b| pdfr_core::signature::decode_any(&b));
        match loaded {
            Ok(img) => {
                let original =
                    pdfr_core::signature::shrink(&img, pdfr_core::signature::IMAGE_MAX * 2);
                let mut p = Picture {
                    original,
                    threshold: 190,
                    ink: None,
                };
                clean_picture(&mut p);
                dlg.picture_error = p.ink.is_none().then(|| {
                    "No signature found in the picture: try a darker photo on white paper.".into()
                });
                dlg.picture = Some(p);
            }
            Err(e) => {
                dlg.picture = None;
                dlg.picture_error = Some(format!("Could not use the picture: {e}."));
            }
        }
    }
}

fn clean_picture(p: &mut Picture) {
    p.ink = pdfr_core::signature::clean_image(&p.original, p.threshold)
        .map(|img| Arc::new(Ink::from_image(img)));
}

/// The Image tab: choose a photo or scan, see it with the paper removed,
/// adjust how light counts as paper. Returns whether to open the picker.
fn image_pad(ui: &mut egui::Ui, dlg: &mut SignDialog, c: &Colors) -> bool {
    let (r, resp) = ui.allocate_exact_size(vec2(PAD_W, 130.0), Sense::click());
    let p = ui.painter_at(r);
    p.rect_filled(r, 6.0, Color32::WHITE);
    p.rect_stroke(r, 6.0, Stroke::new(1.0, c.border), egui::StrokeKind::Inside);
    let mut pick = resp.on_hover_cursor(CursorIcon::PointingHand).clicked();
    match dlg.picture.as_ref().and_then(|p| p.ink.as_ref()) {
        Some(ink) => crate::ink::paint(&p, ink, r.shrink(12.0), Color32::WHITE, false),
        None => {
            p.text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                dlg.picture_error
                    .as_deref()
                    .unwrap_or("Click to choose a photo or scan of your signature (PNG or JPEG)"),
                egui::FontId::proportional(13.0),
                Color32::from_gray(150),
            );
        }
    }
    ui.horizontal(|ui| {
        if secondary_button(ui, c, "Choose picture…").clicked() {
            pick = true;
        }
        if let Some(pic) = dlg.picture.as_mut() {
            ui.label(RichText::new("Paper").color(c.weak));
            let mut t = pic.threshold as f32;
            let s = ui
                .add(egui::Slider::new(&mut t, 80.0..=250.0).show_value(false))
                .on_hover_text("Move left if the paper still shows, right if the ink fades");
            if s.changed() {
                pic.threshold = t as u8;
                clean_picture(pic);
            }
        }
    });
    pick
}

fn typed_ink(dlg: &mut SignDialog) -> Option<Arc<Ink>> {
    let name = dlg.name.trim().to_string();
    if name.is_empty() {
        return None;
    }
    if dlg.typed.as_ref().is_none_or(|(n, _)| *n != name) {
        dlg.typed = Some((name.clone(), Arc::new(Ink::typed(&name))));
    }
    dlg.typed.as_ref().map(|(_, i)| i.clone())
}

/// The drawing pad: white paper with a line to sign on.
fn draw_pad(ui: &mut egui::Ui, dlg: &mut SignDialog, c: &Colors) {
    let (r, resp) = ui.allocate_exact_size(vec2(PAD_W, PAD_H), Sense::click_and_drag());
    let p = ui.painter_at(r);
    p.rect_filled(r, 6.0, Color32::WHITE);
    p.rect_stroke(r, 6.0, Stroke::new(1.0, c.border), egui::StrokeKind::Inside);
    let base = r.top() + PAD_H * 0.72;
    p.line_segment(
        [pos2(r.left() + 24.0, base), pos2(r.right() - 24.0, base)],
        Stroke::new(1.0, Color32::from_gray(200)),
    );
    p.text(
        pos2(r.left() + 24.0, base + 6.0),
        egui::Align2::LEFT_TOP,
        "\u{d7}",
        egui::FontId::proportional(14.0),
        Color32::from_gray(170),
    );
    // Pressing starts a stroke (a press without moving leaves a dot).
    if resp.is_pointer_button_down_on()
        && let Some(pos) = ui.input(|i| i.pointer.interact_pos())
    {
        let local = [
            (pos.x - r.left()).clamp(0.0, PAD_W),
            (pos.y - r.top()).clamp(0.0, PAD_H),
        ];
        match dlg.strokes.last_mut() {
            Some(s) if dlg.drawing => {
                let last = *s.last().unwrap();
                if (last[0] - local[0]).hypot(last[1] - local[1]) >= 1.5 {
                    s.push(local);
                }
            }
            _ => {
                dlg.strokes.push(vec![local]);
                dlg.drawing = true;
            }
        }
    } else {
        dlg.drawing = false;
    }
    let ink_c = {
        let [r, g, b] = pdfr_core::signature::INK_RGB.map(|v| (v * 255.0) as u8);
        Color32::from_rgb(r, g, b)
    };
    for s in &dlg.strokes {
        let pts: Vec<Pos2> = s
            .iter()
            .map(|q| pos2(r.left() + q[0], r.top() + q[1]))
            .collect();
        if pts.len() == 1 {
            p.circle_filled(pts[0], PEN / 2.0, ink_c);
        } else {
            p.add(egui::Shape::line(pts.clone(), Stroke::new(PEN, ink_c)));
            for q in [pts[0], *pts.last().unwrap()] {
                p.circle_filled(q, PEN / 2.0, ink_c);
            }
        }
    }
    if dlg.strokes.is_empty() {
        p.text(
            r.center() - vec2(0.0, 18.0),
            egui::Align2::CENTER_CENTER,
            if dlg.initials {
                "Draw your initials with the mouse or trackpad"
            } else {
                "Sign with the mouse or trackpad"
            },
            egui::FontId::proportional(14.0),
            Color32::from_gray(160),
        );
    }
    resp.on_hover_cursor(CursorIcon::Crosshair);
    ui.horizontal(|ui| {
        if !dlg.strokes.is_empty() && secondary_button(ui, c, "Clear").clicked() {
            dlg.strokes.clear();
        }
        if !dlg.strokes.is_empty() && secondary_button(ui, c, "Undo stroke").clicked() {
            dlg.strokes.pop();
        }
    });
}

/// Typing the name, shown in handwriting.
fn type_pad(ui: &mut egui::Ui, dlg: &mut SignDialog, c: &Colors) {
    let te = ui.add(
        egui::TextEdit::singleline(&mut dlg.name)
            .hint_text(if dlg.initials {
                "Your initials"
            } else {
                "Your name"
            })
            .desired_width(PAD_W),
    );
    if te.changed() {
        dlg.typed = None;
    }
    let (r, _) = ui.allocate_exact_size(vec2(PAD_W, 110.0), Sense::hover());
    let p = ui.painter_at(r);
    p.rect_filled(r, 6.0, Color32::WHITE);
    p.rect_stroke(r, 6.0, Stroke::new(1.0, c.border), egui::StrokeKind::Inside);
    if let Some(ink) = typed_ink(dlg) {
        crate::ink::paint(&p, &ink, r.shrink(14.0), Color32::WHITE, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_that_ask_for_a_signature() {
        assert!(signature_label("Signature of applicant"));
        assert!(signature_label("Underskrift"));
        assert!(signature_label("Namnteckning"));
        assert!(!signature_label("Date of signature"));
        assert!(!signature_label("Name"));
        assert_eq!(initials_of("Anna-Maria Lindqvist"), "AML");
    }

    #[test]
    fn on_line_target_stands_on_the_line() {
        let t = Target {
            page: 0,
            rect: PRect::new(100.0, 500.0, 400.0, 516.0),
            on_line: true,
        };
        let ink = Ink {
            aspect: 4.0,
            ..Ink::default()
        };
        let r = t.rect_for(&ink);
        assert!(r.height() >= 24.0);
        assert!((r.x0 - 102.0).abs() < 1e-3);
        assert!(r.y1 > 516.0 && r.y1 < 520.0);
        assert!((r.width() / r.height() - 4.0).abs() < 1e-3);
    }
}
