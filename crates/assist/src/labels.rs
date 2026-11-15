//! Layer 1: field labels — the words a person would read to know what goes
//! in a field. No AI.

use pdfr_core::field::{Field, FieldKind, Origin, TextMode};
use pdfr_core::geom::Rect;
use pdfr_core::text::TextRun;
use std::collections::HashMap;

/// A field's position in a group of fields sharing one label (date parts,
/// a split IBAN).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupPos {
    pub group: usize,
    pub index: usize,
    pub count: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FieldInfo {
    pub label: Option<String>,
    pub group: Option<GroupPos>,
}

/// Removes underscores, dot leaders and trailing colons from a label.
pub fn clean(s: &str) -> String {
    let mut out = String::new();
    let mut prev: Option<char> = None;
    for c in s.chars() {
        let leader = c == '_' || (c == '.' && prev == Some('.')) || c == '…';
        if leader {
            if out.ends_with('.') && c == '.' {
                out.pop();
            }
        } else {
            out.push(c);
        }
        prev = Some(c);
    }
    let t = out.split_whitespace().collect::<Vec<_>>().join(" ");
    t.trim_matches(|c: char| c == ':' || c == '*' || c.is_whitespace() || c == '-')
        // Sentence pieces: ", con número de ..." -> "con número de ...".
        .trim_start_matches(|c: char| c == ',' || c == ';' || c == '.' || c.is_whitespace())
        .trim_end_matches(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .to_string()
}

/// Is a field name (`/T`) something a person would read?
pub fn readable_name(name: &str) -> Option<String> {
    let last = name.rsplit('.').next().unwrap_or(name);
    // Split camelCase and separators.
    let mut s = String::new();
    let mut prev_lower = false;
    for c in last.chars() {
        if c == '_' || c == '-' || c == '[' || c == ']' {
            s.push(' ');
            prev_lower = false;
            continue;
        }
        if c.is_uppercase() && prev_lower {
            s.push(' ');
        }
        prev_lower = c.is_lowercase();
        s.push(c);
    }
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let letters = s.chars().filter(|c| c.is_alphabetic()).count();
    let digits = s.chars().filter(|c| c.is_ascii_digit()).count();
    if letters < 2 || digits > letters {
        return None;
    }
    let lower = s.to_lowercase();
    let generic = [
        "text",
        "field",
        "fill",
        "textfield",
        "text field",
        "check box",
        "checkbox",
        "untitled",
        "undefined",
        "topmostsubform",
        "page",
        "form",
        "txt",
        "tf",
        "box",
        "button",
        "combo box",
        "dropdown",
        "list box",
    ];
    let stem: String = lower
        .trim_end_matches(|c: char| c.is_ascii_digit() || c == ' ')
        .to_string();
    if generic.contains(&stem.as_str()) || stem.len() < 2 {
        return None;
    }
    Some(s)
}

fn v_overlap(a: &Rect, b: &Rect) -> f32 {
    a.y1.min(b.y1) - a.y0.max(b.y0)
}
fn h_overlap(a: &Rect, b: &Rect) -> f32 {
    a.x1.min(b.x1) - a.x0.max(b.x0)
}

/// Could this text be a field label? Sentences, URLs, e-mail addresses and
/// long passages are page text, not labels, and are never used or sent.
pub fn plausible_label(s: &str) -> bool {
    let words = s.split_whitespace().count();
    let lower = s.to_lowercase();
    let letters = s.chars().filter(|c| c.is_alphabetic()).count();
    letters >= 2
        && words <= 8
        && s.chars().count() <= 60
        && !lower.contains("http")
        && !lower.contains("www.")
        && !lower.contains(".es/")
        && !lower.contains(".com")
        && !s.contains('@')
        && !(words > 4 && s.trim_end().ends_with('.'))
}

/// Finds the best label for a field from page text: left of it on the same
/// line, above it, or inside the same box. Prefers the closest; never looks
/// past another field, and only uses the part of a phrase that lies over the
/// field's own column (labels of neighbouring boxes often touch).
fn from_page_text(f: &Field, runs: &[TextRun], others: &[&Field]) -> Option<String> {
    let r = f.rect;
    let mut best: Option<(f32, String)> = None;
    // `sentence`: the piece directly precedes the field and starts at a
    // natural boundary (start of the line, after a dot leader or a comma), as
    // in forms written as one sentence: "D./Dª ____ con D.N.I. nº ____". It
    // may then start lowercase and end in "en" / "de".
    let mut consider = |dist: f32, text: &str, whole: &str, sentence: bool| {
        let t = clean(text);
        if !plausible_label(&t) {
            return;
        }
        // A piece cut out of a longer phrase must look like a label of its
        // own, not the middle of a sentence ("de notificaciones", "Datos de la").
        if t != clean(whole) && !sentence {
            let lower_start = t.chars().next().is_some_and(|c| c.is_lowercase());
            let last = t.split_whitespace().last().unwrap_or("");
            let dangling = last.chars().count() <= 2 && last.chars().all(|c| c.is_lowercase());
            if lower_start || dangling {
                return;
            }
        }
        if best.as_ref().is_none_or(|b| dist < b.0) {
            best = Some((dist, t));
        }
    };
    let is_toggle = f.kind.is_toggle();
    let on_line = |o: &Field| v_overlap(&o.rect, &r) > 0.3 * r.height().min(o.rect.height());
    // Where the neighbouring fields on this line end and begin.
    let left_limit = others
        .iter()
        .filter(|o| on_line(o) && o.rect.x1 <= r.x0 + 1.0)
        .map(|o| o.rect.x1)
        .fold(f32::MIN, f32::max);
    let right_limit = others
        .iter()
        .filter(|o| on_line(o) && o.rect.x0 >= r.x1 - 1.0)
        .map(|o| o.rect.x0)
        .fold(f32::MAX, f32::min);
    for run in runs {
        let t = run.rect;
        let line = Rect::new(r.x0, r.y0 - 2.0, r.x1, r.y1 + 2.0);
        // Left, on the same line (the run may run into the field, as with
        // "Email ______" where the underscores are the field).
        if v_overlap(&t, &line) > 0.4 * t.height() && t.x0 < r.x0 - 1.0 {
            let dist = (r.x0 - t.x1).max(0.0);
            if dist < 260.0 {
                let blocked = others
                    .iter()
                    .any(|o| on_line(o) && o.rect.x0 >= t.x1 - 1.0 && o.rect.x1 <= r.x0 + 1.0);
                if !blocked {
                    let piece = run.clip(left_limit - 1.0, r.x0 + 1.0);
                    let sentence = dist < 12.0
                        && leaders_over(run, &r)
                        && starts_at_boundary(&run.text, &piece);
                    consider(dist, &piece, &run.text, sentence);
                }
            }
        }
        // Right of a checkbox: its caption.
        if is_toggle && v_overlap(&t, &line) > 0.3 * t.height() && t.x0 >= r.x1 - 1.0 {
            let dist = t.x0 - r.x1;
            if dist < 40.0 {
                consider(
                    dist * 0.5,
                    &run.clip(r.x1 - 1.0, right_limit),
                    &run.text,
                    false,
                );
            }
        }
        // Above (or inside the top of the same box).
        if h_overlap(&t, &r) > 0.0 && t.y1 <= r.y0 + 2.0 {
            let dist = r.y0 - t.y1;
            // Where the part of the phrase over this field begins.
            let start = run.clip_start(r.x0 - 8.0, r.x1);
            // Cutting the phrase on the left is only right when the cut-off
            // part is the label of the neighbouring field on the same row.
            let cut_ok = start.is_some_and(|x| x <= t.x0 + 1.0)
                || others
                    .iter()
                    .any(|o| on_line(o) && o.rect.x1 <= r.x0 + 2.0 && o.rect.x0 <= t.x0 + 8.0);
            if dist < 16.0 && cut_ok && start.is_some_and(|x| x < r.x1 && x >= r.x0 - 8.0) {
                let blocked = others.iter().any(|o| {
                    h_overlap(&o.rect, &r) > 0.0
                        && o.rect.y0 >= t.y1 - 1.0
                        && o.rect.y1 <= r.y0 + 1.0
                });
                if !blocked {
                    consider(
                        dist * 1.3 + 2.0,
                        &run.clip(r.x0 - 8.0, r.x1),
                        &run.text,
                        false,
                    );
                }
            }
        }
        // Inside the field's own box (top-left corner caption).
        if r.contains(t.x0 + 0.5, t.y0 + 0.5)
            && r.contains(t.x1 - 0.5, t.y1 - 0.5)
            && t.y1 < r.y0 + r.height() * 0.5
        {
            consider(1.0, &run.text, &run.text, false);
        }
    }
    if best.is_none()
        && let Some(tail) = previous_line_tail(f, runs, others)
    {
        let t = clean(&tail);
        if plausible_label(&t) {
            best = Some((30.0, t));
        }
    }
    best.map(|b| b.1)
}

/// Whether the blank is drawn with dot leaders inside this run of text
/// ("con D.N.I. nº ………………, con"): the mark of a form written as a sentence.
fn leaders_over(run: &TextRun, r: &Rect) -> bool {
    run.rect.x1 > r.x0 + 4.0 && (run.text.contains('…') || run.text.contains("...."))
}

/// Whether `piece` (cut from `run`) begins where a new phrase begins: at the
/// start of the run, or after a dot leader, comma or other punctuation.
fn starts_at_boundary(run: &str, piece: &str) -> bool {
    let piece = piece.trim();
    if piece.is_empty() {
        return false;
    }
    let Some(pos) = run.find(piece) else {
        return false;
    };
    match run[..pos].trim_end().chars().last() {
        None => true,
        Some(c) => matches!(c, '…' | '.' | '_' | ',' | ';' | ':' | ')'),
    }
}

/// For a field that starts a line in a form written as running text, the
/// label is the end of the line before ("... en el día" / "____, con ..."):
/// the words after the last punctuation, if the line above runs to the
/// right margin and starts at the field's left edge.
fn previous_line_tail(f: &Field, runs: &[TextRun], others: &[&Field]) -> Option<String> {
    let r = f.rect;
    // Only for blanks drawn as dot leaders at the start of a line of text.
    let leader_line = runs.iter().any(|t| {
        v_overlap(&t.rect, &r) > 0.3 * t.rect.height()
            && (t.rect.x0 - r.x0).abs() < 8.0
            && t.text.trim_start().starts_with(['…', '.', '_'])
    });
    if !leader_line {
        return None;
    }
    let right_margin = runs.iter().map(|t| t.rect.x1).fold(f32::MIN, f32::max);
    let above = runs
        .iter()
        .filter(|t| t.rect.y1 <= r.y0 + 2.0 && r.y0 - t.rect.y1 < 2.5 * t.rect.height())
        .filter(|t| (t.rect.x0 - r.x0).abs() < 12.0 && t.rect.x1 > right_margin - 6.0)
        .max_by(|a, b| a.rect.y1.total_cmp(&b.rect.y1))?;
    // Nothing between the line and the field, and no field ends that line.
    let blocked = others.iter().any(|o| {
        o.rect.y1 > above.rect.y0
            && o.rect.y0 < r.y0 - 1.0
            && o.rect.x1 > above.rect.x1 - 40.0
            && o.rect.y0 > above.rect.y0 - 2.0
    });
    if blocked {
        return None;
    }
    let text = above
        .text
        .trim_end_matches(|c: char| c == '…' || c == '.' || c == '_' || c.is_whitespace());
    let cut = text.rfind([')', ',', ';', ':', '…']).map_or(0, |i| {
        i + text[i..].chars().next().map_or(1, char::len_utf8)
    });
    let tail = text[cut..].trim();
    (tail.split_whitespace().count() <= 5 && !tail.is_empty()).then(|| tail.to_string())
}

/// Labels every field and finds groups of boxes that share a label.
pub fn label_fields(fields: &[Field], text: &HashMap<usize, Vec<TextRun>>) -> Vec<FieldInfo> {
    let mut info = vec![FieldInfo::default(); fields.len()];
    let none: Vec<TextRun> = Vec::new();
    for (i, f) in fields.iter().enumerate() {
        let runs = text.get(&f.page).unwrap_or(&none);
        let others: Vec<&Field> = fields
            .iter()
            .enumerate()
            .filter(|(j, o)| *j != i && o.page == f.page)
            .map(|(_, o)| o)
            .collect();
        let mut label = None;
        if let Origin::AcroForm { name, .. } = &f.origin {
            label = f.tooltip.as_deref().map(clean).filter(|s| !s.is_empty());
            // Our own saved fields carry the label in /TU; their names are generic.
            if label.is_none() && !name.starts_with("pdfr_") {
                label = readable_name(name);
            }
        }
        if label.is_none() {
            label = from_page_text(f, runs, &others);
        }
        info[i].label = label;
    }
    // Groups: rows of boxes on one line with nothing written between them.
    let mut gid = 0;
    let mut visited = vec![false; fields.len()];
    let mut combs: Vec<usize> = (0..fields.len())
        .filter(|&i| fields[i].kind.is_comb())
        .collect();
    combs.sort_by(|&a, &b| {
        (
            fields[a].page,
            fields[a].rect.y0 as i32,
            fields[a].rect.x0 as i32,
        )
            .cmp(&(
                fields[b].page,
                fields[b].rect.y0 as i32,
                fields[b].rect.x0 as i32,
            ))
    });
    for &start in &combs {
        if visited[start] {
            continue;
        }
        let mut row = vec![start];
        visited[start] = true;
        loop {
            let last = &fields[*row.last().unwrap()];
            let cell = cell_width(last).max(4.0);
            let next = combs.iter().copied().filter(|&k| !visited[k]).find(|&k| {
                let o = &fields[k];
                o.page == last.page
                    && (o.rect.center().1 - last.rect.center().1).abs() < last.rect.height() * 0.4
                    && o.rect.x0 >= last.rect.x1 - 1.0
                    && o.rect.x0 - last.rect.x1 < cell * 2.5
                    && !text.get(&o.page).unwrap_or(&none).iter().any(|t| {
                        let gap = Rect::new(last.rect.x1, last.rect.y0, o.rect.x0, last.rect.y1);
                        h_overlap(&t.rect, &gap) > 1.0
                            && v_overlap(&t.rect, &gap) > 1.0
                            && t.text.chars().any(|c| c.is_alphanumeric())
                    })
            });
            match next {
                Some(k) => {
                    visited[k] = true;
                    row.push(k);
                }
                None => break,
            }
        }
        if row.len() >= 2 {
            let label = row.iter().find_map(|&k| info[k].label.clone());
            for (idx, &k) in row.iter().enumerate() {
                info[k].group = Some(GroupPos {
                    group: gid,
                    index: idx,
                    count: row.len(),
                });
                info[k].label = label.clone();
            }
            gid += 1;
        }
    }
    info
}

fn cell_width(f: &Field) -> f32 {
    match &f.kind {
        FieldKind::Text {
            mode: TextMode::Comb(c),
            ..
        } => c.iter().map(|c| c.1 - c.0).sum::<f32>() / c.len().max(1) as f32,
        _ => f.rect.height(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cleaning() {
        assert_eq!(clean("Email ________"), "Email");
        assert_eq!(clean("Name.........:"), "Name");
        assert_eq!(clean("  Full name: "), "Full name");
        assert_eq!(
            clean("Date of birth (DD MM YYYY)"),
            "Date of birth (DD MM YYYY)"
        );
    }
    #[test]
    fn plausible() {
        assert!(plausible_label(
            "Relación familiar (padre, madre o tutor/a legal)"
        ));
        assert!(plausible_label("Cód. postal"));
        assert!(!plausible_label(
            "https juntadeandalucia es protecciondedatos detalle 188656 html"
        ));
        assert!(!plausible_label(
            "https://www.juntadeandalucia.es/notificaciones"
        ));
        assert!(!plausible_label(
            "La persona solicitante que presente esta solicitud deberá cumplimentar los datos"
        ));
        assert!(!plausible_label(
            "Marque solo una opción en cada uno de los casos."
        ));
    }
    /// A form written as running text, blanks drawn with dot leaders:
    /// "D./Dª ………… con D.N.I. nº …………, con" / "domicilio en …………," /
    /// "… en el día" + "…………, con número de entrada …………".
    #[test]
    fn sentence_form_labels() {
        let line = |text: &str, y: f32| {
            let xs: Vec<f32> = (0..text.chars().count())
                .map(|i| 50.0 + i as f32 * 5.0)
                .collect();
            let x1 = 50.0 + text.chars().count() as f32 * 5.0;
            TextRun {
                rect: Rect::new(50.0, y, x1, y + 10.0),
                text: text.to_string(),
                size: 9.0,
                xs,
            }
        };
        // The blank over chars `a..b` of a line.
        let blank = |a: usize, b: usize, y: f32| {
            Field::new_text(
                0,
                Rect::new(
                    50.0 + a as f32 * 5.0,
                    y - 4.0,
                    50.0 + b as f32 * 5.0,
                    y + 10.0,
                ),
                TextMode::OnLine,
                Origin::Detected,
            )
        };
        let l1 = "D./Dª ………………………… con D.N.I. nº …………………, con";
        let l2 = "domicilio a efectos de notificación en ……………………………………,";
        let l3 = "como solicitante de la licencia de obra menor (procedimiento) en el día";
        let l4 = "…………………, con número de entrada en este Ayuntamiento ……………………, presenta";
        let pos = |l: &str, from: usize| {
            let c: Vec<char> = l.chars().collect();
            let a = (from..c.len()).find(|&i| c[i] == '…').unwrap();
            let b = (a..c.len()).find(|&i| c[i] != '…').unwrap_or(c.len());
            (a, b)
        };
        let (a1, b1) = pos(l1, 0);
        let (a2, b2) = pos(l1, b1);
        let (a3, b3) = pos(l2, 0);
        let (a4, b4) = pos(l4, 0);
        let (a5, b5) = pos(l4, b4);
        let fields = vec![
            blank(a1, b1, 0.0),
            blank(a2, b2, 0.0),
            blank(a3, b3, 20.0),
            blank(a4, b4, 60.0),
            blank(a5, b5, 60.0),
        ];
        let text = HashMap::from([(
            0,
            vec![
                line(l1, 0.0),
                line(l2, 20.0),
                line(l3, 40.0),
                line(l4, 60.0),
            ],
        )]);
        let labels: Vec<Option<String>> = label_fields(&fields, &text)
            .into_iter()
            .map(|i| i.label)
            .collect();
        assert_eq!(
            labels,
            vec![
                Some("D./Dª".to_string()),
                Some("con D.N.I. nº".to_string()),
                Some("domicilio a efectos de notificación en".to_string()),
                Some("en el día".to_string()),
                Some("con número de entrada en este Ayuntamiento".to_string()),
            ]
        );
    }

    #[test]
    fn clipping() {
        // "TELÉFONO:CORREO" set without a space: each box keeps its own part.
        let text = "TELEFONO:CORREO".to_string();
        let xs: Vec<f32> = (0..text.len()).map(|i| 10.0 + i as f32 * 5.0).collect();
        let run = TextRun {
            rect: Rect::new(10.0, 0.0, 85.0, 8.0),
            text,
            size: 8.0,
            xs,
        };
        assert_eq!(run.clip(8.0, 55.0), "TELEFONO:");
        assert_eq!(run.clip(55.0, 200.0), "CORREO");
        // A word cut by the edge is dropped, not kept in part.
        assert_eq!(run.clip(8.0, 40.0), "");
        assert_eq!(run.clip(70.0, 200.0), "");
    }
    #[test]
    fn field_names() {
        assert_eq!(readable_name("contact.email").as_deref(), Some("email"));
        assert_eq!(readable_name("firstName").as_deref(), Some("first Name"));
        assert_eq!(readable_name("Text12"), None);
        assert_eq!(readable_name("topmostSubform[0].Page1[0].f1_01[0]"), None);
        assert_eq!(readable_name("undefined_3"), None);
        assert_eq!(readable_name("postal_code").as_deref(), Some("postal code"));
    }
}
