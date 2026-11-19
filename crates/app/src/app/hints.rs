//! Ctrl+L: jump labels (like vimium, leap or flash.nvim). Every box in view
//! gets a short home-row label; typing it focuses that box. Esc, a click or
//! a label that matches nothing puts them away.

use super::*;

/// Home row first, so most jumps never leave it.
const KEYS: &[char] = &['a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l'];

pub struct Hints {
    /// (label, box id), in reading order.
    pub labels: Vec<(String, u64)>,
    pub typed: String,
}

/// `n` labels, all the same length, so none is a prefix of another.
pub(super) fn labels(n: usize) -> Vec<String> {
    let k = KEYS.len();
    if n <= k {
        return KEYS[..n].iter().map(|c| c.to_string()).collect();
    }
    let mut v = Vec::with_capacity(n.min(k * k));
    'outer: for a in KEYS {
        for b in KEYS {
            if v.len() == n {
                break 'outer;
            }
            v.push(format!("{a}{b}"));
        }
    }
    v
}

impl App {
    pub(super) fn start_hints(&mut self) {
        let Some(doc) = &self.doc else { return };
        let rects = self.page_rects();
        let vp = self.viewport;
        let visible: Vec<u64> = self
            .editable_ids()
            .into_iter()
            .filter(|id| {
                let Some(i) = doc.index_of(*id) else {
                    return false;
                };
                let f = &doc.entries[i].f;
                rects.get(f.page).is_some_and(|pr| {
                    PageXf {
                        origin: pr.min,
                        zoom: self.zoom,
                    }
                    .rect(&f.rect)
                    .intersects(vp)
                })
            })
            .collect();
        if visible.is_empty() {
            self.status = "No boxes in view.".into();
            return;
        }
        self.choice_open = None;
        self.assist.close_list();
        let names = labels(visible.len());
        self.hints = Some(Hints {
            labels: names.into_iter().zip(visible).collect(),
            typed: String::new(),
        });
    }

    /// While labels are shown, typed letters go to them and nowhere else.
    pub(super) fn hints_keys(&mut self, ctx: &egui::Context) {
        let Some(h) = &mut self.hints else { return };
        let mut cancel = ctx.input(|i| i.pointer.any_pressed());
        ctx.input_mut(|i| {
            i.events.retain(|ev| match ev {
                Event::Text(t) => {
                    h.typed.extend(
                        t.chars()
                            .filter(|c| !c.is_whitespace())
                            .flat_map(char::to_lowercase),
                    );
                    false
                }
                Event::Key {
                    key, pressed: true, ..
                } => {
                    match key {
                        Key::Escape => cancel = true,
                        Key::Backspace => {
                            h.typed.pop();
                        }
                        _ => {}
                    }
                    false
                }
                Event::Key { .. } => false,
                _ => true,
            });
        });
        let hit = h
            .labels
            .iter()
            .find(|(l, _)| *l == h.typed)
            .map(|(_, id)| *id);
        let alive = h.labels.iter().any(|(l, _)| l.starts_with(&h.typed));
        if let Some(id) = hit {
            self.hints = None;
            self.focus_field(id, true, ctx);
        } else if cancel || !alive {
            self.hints = None;
        }
    }

    /// Draws the labels over their boxes (screen only).
    pub(super) fn draw_hints(&self, painter: &egui::Painter, rects: &[Rect]) {
        let (Some(h), Some(doc)) = (&self.hints, &self.doc) else {
            return;
        };
        let bg = super::canvas::strong(self.palette.accent);
        let font = egui::FontId::monospace(13.0);
        for (label, id) in &h.labels {
            if !label.starts_with(&h.typed) {
                continue;
            }
            let Some(i) = doc.index_of(*id) else { continue };
            let f = &doc.entries[i].f;
            let Some(pr) = rects.get(f.page) else {
                continue;
            };
            let r = PageXf {
                origin: pr.min,
                zoom: self.zoom,
            }
            .rect(&f.rect);
            let rest = &label[h.typed.len()..];
            let galley = painter.layout_no_wrap(rest.to_uppercase(), font.clone(), Color32::WHITE);
            let size = galley.size() + vec2(8.0, 4.0);
            let chip = Rect::from_min_size(r.left_top() + vec2(-2.0, -size.y * 0.5), size);
            painter.rect_filled(
                chip.translate(vec2(0.0, 1.0)),
                4.0,
                Color32::from_black_alpha(60),
            );
            painter.rect_filled(chip, 4.0, bg);
            painter.galley(chip.min + vec2(4.0, 2.0), galley, Color32::WHITE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::labels;

    #[test]
    fn labels_are_prefix_free() {
        assert_eq!(labels(3), ["a", "s", "d"]);
        let l = labels(30);
        assert_eq!(l.len(), 30);
        assert!(l.iter().all(|x| x.len() == 2));
        for a in &l {
            assert_eq!(l.iter().filter(|b| b.starts_with(a.as_str())).count(), 1);
        }
        assert_eq!(labels(500).len(), 81);
    }
}
