//! A small text editor on a field's value. Caret and selection are char
//! indices. Movement that depends on geometry (up/down, click) uses the shared
//! [`layout`], so the caret lands exactly where the text is drawn.

use crate::field::Field;
use crate::layout::layout;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Editor {
    pub caret: usize,
    /// Selection anchor; equal to `caret` when nothing is selected.
    pub anchor: usize,
}

/// What happened on an insertion.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InsertOutcome {
    pub changed: bool,
    /// A comb row is full and the caret sits after its last box: the app may
    /// move focus to the next row of boxes.
    pub row_full: bool,
    /// Text that did not fit (for spilling into the next row of boxes).
    pub rest: String,
}

/// Normalises text for a field: drops control characters, and in single-line
/// fields turns newlines and tabs into spaces.
pub fn sanitize(field: &Field, text: &str) -> String {
    let multi = field.kind.is_multiline();
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() != Some(&'\n') {
                    out.push(if multi { '\n' } else { ' ' });
                }
            }
            '\n' => out.push(if multi { '\n' } else { ' ' }),
            '\t' => out.push(' '),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    if field.kind.is_comb() {
        // Boxes take one character each; spaces in pasted IDs are separators.
        out.retain(|c| c != ' ');
    }
    out
}

fn chars(s: &str) -> Vec<char> {
    s.chars().collect()
}

impl Editor {
    pub fn at(idx: usize) -> Self {
        Editor {
            caret: idx,
            anchor: idx,
        }
    }

    pub fn has_selection(&self) -> bool {
        self.caret != self.anchor
    }

    pub fn selection(&self) -> (usize, usize) {
        (self.caret.min(self.anchor), self.caret.max(self.anchor))
    }

    pub fn selected_text(&self, value: &str) -> String {
        let (a, b) = self.selection();
        value.chars().skip(a).take(b - a).collect()
    }

    fn clamp(&mut self, len: usize) {
        self.caret = self.caret.min(len);
        self.anchor = self.anchor.min(len);
    }

    fn set(&mut self, idx: usize, extend: bool) {
        self.caret = idx;
        if !extend {
            self.anchor = idx;
        }
    }

    /// Deletes the selection, if any. Returns true if something was removed.
    pub fn delete_selection(&mut self, value: &mut String) -> bool {
        if !self.has_selection() {
            return false;
        }
        let (a, b) = self.selection();
        let mut cs = chars(value);
        let b = b.min(cs.len());
        cs.drain(a.min(b)..b);
        *value = cs.into_iter().collect();
        *self = Editor::at(a);
        true
    }

    /// Inserts typed, pasted or IME-committed text.
    pub fn insert(&mut self, field: &Field, value: &mut String, text: &str) -> InsertOutcome {
        let text = sanitize(field, text);
        let mut out = InsertOutcome::default();
        self.clamp(value.chars().count());
        if self.delete_selection(value) {
            out.changed = true;
        }
        let cap = field.capacity();
        let comb = field.kind.is_comb();
        let mut cs = chars(value);
        let mut it = text.chars();
        while let Some(c) = it.next() {
            match cap {
                Some(n) if cs.len() >= n => {
                    if comb && self.caret < cs.len() {
                        // Full row: overwrite the box under the caret.
                        cs[self.caret] = c;
                        self.caret += 1;
                        out.changed = true;
                    } else {
                        out.rest.push(c);
                        out.rest.extend(it.by_ref());
                        break;
                    }
                }
                _ => {
                    cs.insert(self.caret, c);
                    self.caret += 1;
                    out.changed = true;
                }
            }
        }
        self.anchor = self.caret;
        if comb && let Some(n) = cap {
            out.row_full = cs.len() >= n && self.caret >= n;
        }
        *value = cs.into_iter().collect();
        out
    }

    /// Backspace. Returns true if the value changed; false at the start of
    /// the field (the app may then move back into a previous row of boxes).
    pub fn backspace(&mut self, value: &mut String) -> bool {
        self.clamp(value.chars().count());
        if self.delete_selection(value) {
            return true;
        }
        if self.caret == 0 {
            return false;
        }
        let mut cs = chars(value);
        cs.remove(self.caret - 1);
        *value = cs.into_iter().collect();
        *self = Editor::at(self.caret - 1);
        true
    }

    pub fn delete(&mut self, value: &mut String) -> bool {
        let len = value.chars().count();
        self.clamp(len);
        if self.delete_selection(value) {
            return true;
        }
        if self.caret >= len {
            return false;
        }
        let mut cs = chars(value);
        cs.remove(self.caret);
        *value = cs.into_iter().collect();
        self.anchor = self.caret;
        true
    }

    pub fn left(&mut self, extend: bool) {
        if self.has_selection() && !extend {
            let a = self.selection().0;
            return self.set(a, false);
        }
        self.set(self.caret.saturating_sub(1), extend);
    }

    pub fn right(&mut self, value: &str, extend: bool) {
        let len = value.chars().count();
        if self.has_selection() && !extend {
            let b = self.selection().1;
            return self.set(b.min(len), false);
        }
        self.set((self.caret + 1).min(len), extend);
    }

    /// Word-wise movement (Ctrl+Left / Ctrl+Right).
    pub fn word_left(&mut self, value: &str, extend: bool) {
        let cs = chars(value);
        let mut i = self.caret.min(cs.len());
        while i > 0 && cs[i - 1].is_whitespace() {
            i -= 1;
        }
        while i > 0 && !cs[i - 1].is_whitespace() {
            i -= 1;
        }
        self.set(i, extend);
    }

    pub fn word_right(&mut self, value: &str, extend: bool) {
        let cs = chars(value);
        let mut i = self.caret.min(cs.len());
        while i < cs.len() && !cs[i].is_whitespace() {
            i += 1;
        }
        while i < cs.len() && cs[i].is_whitespace() {
            i += 1;
        }
        self.set(i, extend);
    }

    /// Home: start of the current visual line.
    pub fn home(&mut self, field: &Field, value: &str, extend: bool) {
        let l = layout(field, value);
        let li = l.line_of(self.caret);
        let start = l.lines.get(li).map_or(0, |l| l.start);
        self.set(start, extend);
    }

    /// End: end of the current visual line.
    pub fn end(&mut self, field: &Field, value: &str, extend: bool) {
        let l = layout(field, value);
        let li = l.line_of(self.caret);
        let idx = l.lines.get(li).map_or(0, |line| {
            if line.hard_end || line.end == line.start {
                line.end
            } else {
                line.end - 1
            }
        });
        self.set(idx, extend);
    }

    /// Up/down in multi-line fields. Returns false if there is no line in
    /// that direction (the app may then leave the field).
    pub fn vertical(&mut self, field: &Field, value: &str, down: bool, extend: bool) -> bool {
        let l = layout(field, value);
        let (x, _, li) = l.caret_pos(self.caret.min(l.len));
        let target = if down {
            li + 1
        } else if li == 0 {
            return false;
        } else {
            li - 1
        };
        if target >= l.lines.len() {
            return false;
        }
        let idx = l.hit_in_line(target, x);
        self.set(idx, extend);
        true
    }

    pub fn select_all(&mut self, value: &str) {
        self.anchor = 0;
        self.caret = value.chars().count();
    }

    /// Places the caret at a click position (display space).
    pub fn click(&mut self, field: &Field, value: &str, x: f32, y: f32, extend: bool) {
        let idx = layout(field, value).hit(x, y);
        self.set(idx, extend);
    }

    /// Selects the word around a double-click position.
    pub fn select_word_at(&mut self, field: &Field, value: &str, x: f32, y: f32) {
        let idx = layout(field, value).hit(x, y);
        let cs = chars(value);
        let mut a = idx.min(cs.len());
        let mut b = a;
        while a > 0 && !cs[a - 1].is_whitespace() {
            a -= 1;
        }
        while b < cs.len() && !cs[b].is_whitespace() {
            b += 1;
        }
        self.anchor = a;
        self.caret = b;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::{Origin, TextMode};
    use crate::geom::Rect;

    fn comb(n: usize) -> Field {
        let r = Rect::new(0.0, 0.0, 12.0 * n as f32, 14.0);
        Field::new_text(
            0,
            r,
            TextMode::Comb(Field::equal_cells(&r, n)),
            Origin::Detected,
        )
    }
    fn single() -> Field {
        Field::new_text(
            0,
            Rect::new(0.0, 0.0, 200.0, 16.0),
            TextMode::Single,
            Origin::Detected,
        )
    }
    fn multi() -> Field {
        Field::new_text(
            0,
            Rect::new(0.0, 0.0, 120.0, 200.0),
            TextMode::Multi,
            Origin::Detected,
        )
    }

    #[test]
    fn insert_and_backspace() {
        let f = single();
        let mut v = String::new();
        let mut e = Editor::default();
        e.insert(&f, &mut v, "hello");
        assert_eq!((v.as_str(), e.caret), ("hello", 5));
        e.left(false);
        e.left(false);
        e.insert(&f, &mut v, "X");
        assert_eq!(v, "helXlo");
        assert!(e.backspace(&mut v));
        assert_eq!(v, "hello");
        e.home(&f, &v, false);
        assert!(!e.backspace(&mut v));
        assert!(e.delete(&mut v));
        assert_eq!(v, "ello");
        e.end(&f, &v, false);
        assert_eq!(e.caret, 4);
        assert!(!e.delete(&mut v));
    }

    #[test]
    fn unicode_is_char_indexed() {
        let f = single();
        let mut v = String::new();
        let mut e = Editor::default();
        e.insert(&f, &mut v, "Åsa Öberg");
        assert_eq!(e.caret, 9);
        e.home(&f, &v, false);
        e.right(&v, false);
        e.insert(&f, &mut v, "-");
        assert_eq!(v, "Å-sa Öberg");
    }

    #[test]
    fn selection_replace_copy() {
        let f = single();
        let mut v = "hello world".to_string();
        let mut e = Editor::at(0);
        e.word_right(&v, true);
        assert_eq!(e.selected_text(&v), "hello ");
        e.insert(&f, &mut v, "big ");
        assert_eq!(v, "big world");
        e.select_all(&v);
        assert_eq!(e.selected_text(&v), "big world");
        assert!(e.backspace(&mut v));
        assert_eq!((v.as_str(), e.caret), ("", 0));
    }

    #[test]
    fn comb_capacity_enforced() {
        let f = comb(4);
        let mut v = String::new();
        let mut e = Editor::default();
        let o = e.insert(&f, &mut v, "123456");
        assert_eq!(v, "1234");
        assert_eq!(o.rest, "56");
        assert!(o.row_full);
        // Typing at the end of a full row changes nothing.
        let o = e.insert(&f, &mut v, "9");
        assert!(!o.changed);
        assert_eq!(v, "1234");
        assert_eq!(o.rest, "9");
    }

    #[test]
    fn comb_full_row_overwrites_under_caret() {
        let f = comb(4);
        let mut v = "1234".to_string();
        let mut e = Editor::at(1);
        let o = e.insert(&f, &mut v, "X");
        assert_eq!(v, "1X34");
        assert_eq!(e.caret, 2);
        assert!(!o.row_full);
        e.insert(&f, &mut v, "YZ");
        assert_eq!(v, "1XYZ");
        assert_eq!(e.caret, 4);
        // A partly filled row inserts normally.
        let mut v = "12".to_string();
        let mut e = Editor::at(0);
        e.insert(&f, &mut v, "0");
        assert_eq!(v, "012");
    }

    #[test]
    fn comb_drops_spaces_from_paste() {
        let f = comb(8);
        let mut v = String::new();
        let mut e = Editor::default();
        e.insert(&f, &mut v, "SE45 5000");
        assert_eq!(v, "SE455000");
    }

    #[test]
    fn max_len_non_comb() {
        let mut f = single();
        if let crate::field::FieldKind::Text { max_len, .. } = &mut f.kind {
            *max_len = Some(3);
        }
        let mut v = String::new();
        let mut e = Editor::default();
        let o = e.insert(&f, &mut v, "abcdef");
        assert_eq!(v, "abc");
        assert_eq!(o.rest, "def");
        assert!(!o.row_full);
    }

    #[test]
    fn single_line_flattens_newlines() {
        let f = single();
        let mut v = String::new();
        Editor::default().insert(&f, &mut v, "a\r\nb\nc\td");
        assert_eq!(v, "a b c d");
        let m = multi();
        let mut v = String::new();
        Editor::default().insert(&m, &mut v, "a\r\nb");
        assert_eq!(v, "a\nb");
    }

    #[test]
    fn caret_up_down_multiline() {
        let f = multi();
        let v = "first line\nfirst again\nfirst".to_string();
        let mut e = Editor::at(3); // "fir|st"
        assert!(!e.vertical(&f, &v, false, false));
        assert!(e.vertical(&f, &v, true, false));
        assert_eq!(e.caret, 11 + 3); // second line, same x
        assert!(e.vertical(&f, &v, true, false));
        assert_eq!(e.caret, 23 + 3); // third line, same x
        assert!(!e.vertical(&f, &v, true, false));
        // Moving up from the end of a long line lands at the end of a shorter one.
        let v2 = "ab\nlonger line".to_string();
        let mut e = Editor::at(v2.chars().count());
        assert!(e.vertical(&f, &v2, false, false));
        assert_eq!(e.caret, 2);
        // Home/End act on the visual line.
        let mut e = Editor::at(15);
        e.home(&f, &v, false);
        assert_eq!(e.caret, 11);
        e.end(&f, &v, false);
        assert_eq!(e.caret, 22);
    }

    #[test]
    fn caret_movement_and_selection_collapse() {
        let f = single();
        let v = "abc def".to_string();
        let mut e = Editor::at(0);
        e.right(&v, true);
        e.right(&v, true);
        assert_eq!(e.selection(), (0, 2));
        e.left(false); // collapses to the selection start
        assert_eq!((e.caret, e.anchor), (0, 0));
        e.word_right(&v, false);
        assert_eq!(e.caret, 4);
        e.word_left(&v, false);
        assert_eq!(e.caret, 0);
        e.end(&f, &v, true);
        assert_eq!(e.selection(), (0, 7));
        e.right(&v, false);
        assert_eq!(e.caret, 7);
    }

    #[test]
    fn click_places_caret() {
        let f = single();
        let v = "hello".to_string();
        let l = layout(&f, &v);
        let g = &l.lines[0].glyphs[2];
        let mut e = Editor::default();
        e.click(&f, &v, g.x + 0.1, l.lines[0].baseline - 2.0, false);
        assert_eq!(e.caret, 2);
        e.select_word_at(&f, &v, g.x, 8.0);
        assert_eq!(e.selection(), (0, 5));
    }
}
