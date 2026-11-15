//! People on a form, and remembered profiles.
//!
//! A form can be about several people: a child and two guardians, an
//! applicant and a partner, or a table with one family member per row. Each
//! person's details are remembered together as a *profile*, so choosing a
//! person's name brings their own address, ID and phone, not someone else's.
//!
//! Grouping fields into people is done locally from the field order: a new
//! name, ID or date of birth starts a new person (unless it sits on the same
//! row, like a second surname); other details belong to the person they
//! follow. This covers one person per row, per section, or per document.
//! When a form has more than one person, the AI may refine the grouping and
//! name each person's role, from field labels and section headings only.

use crate::labels::FieldInfo;
use crate::meaning::Meaning;
use pdfr_core::field::Field;
use pdfr_core::geom::Rect;
use pdfr_core::text::TextRun;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// Keys that start a new person when they repeat.
pub const ANCHORS: &[&str] = &[
    "full_name",
    "given_name",
    "family_name",
    "national_id",
    "passport_number",
    "date_of_birth",
];

/// Roles a person can have on a form (closed vocabulary for the AI).
pub const ROLES: &[&str] = &[
    "applicant",
    "child",
    "partner",
    "parent",
    "guardian",
    "representative",
    "other",
];

/// Keys that are not details of a person.
pub fn is_person_key(key: &str) -> bool {
    !matches!(key, "signature_date")
}

fn name_family(key: &str) -> bool {
    matches!(key, "full_name" | "given_name" | "family_name")
}

/// One remembered person.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Profile {
    pub id: u64,
    /// Canonical key -> value (dates as ISO, names given-name first).
    pub values: BTreeMap<String, String>,
    /// Roles this person has had on forms ("applicant", "child", ...).
    #[serde(default)]
    pub roles: Vec<String>,
    #[serde(default)]
    pub count: u32,
    #[serde(default)]
    pub last_used: u64,
}

impl Profile {
    /// A short name for lists: the person's name, else e-mail, else ID.
    pub fn title(&self) -> String {
        if let Some(n) = self.values.get("full_name") {
            return n.clone();
        }
        let g = self.values.get("given_name").cloned().unwrap_or_default();
        let f = self.values.get("family_name").cloned().unwrap_or_default();
        let n = format!("{g} {f}").trim().to_string();
        if !n.is_empty() {
            return n;
        }
        for k in ["email", "national_id", "passport_number", "phone"] {
            if let Some(v) = self.values.get(k) {
                return v.clone();
            }
        }
        format!("Person {}", self.id)
    }
}

/// A person on the current form.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Person {
    pub role: Option<String>,
}

/// Normalises a value for comparing (case, spaces and punctuation ignored).
pub fn same_value(a: &str, b: &str) -> bool {
    let n = |s: &str| {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    let (a, b) = (n(a), n(b));
    !a.is_empty() && a == b
}

/// Local grouping: `person[i]` for every field (None: not about a person).
pub fn group_people(
    fields: &[Field],
    meanings: &[Option<Meaning>],
    info: &[FieldInfo],
) -> Vec<Option<usize>> {
    let mut out = vec![None; fields.len()];
    let mut current: Option<usize> = None;
    let mut count = 0usize;
    // Anchor keys seen in the current person, with the field that set them.
    let mut seen: Vec<(String, usize)> = Vec::new();
    for (i, f) in fields.iter().enumerate() {
        if f.kind.is_toggle() {
            continue;
        }
        let Some(m) = meanings.get(i).cloned().flatten() else {
            continue;
        };
        if !is_person_key(&m.key) {
            continue;
        }
        // Later members of a group of boxes (date parts, split numbers)
        // follow their first member.
        let follower = info
            .get(i)
            .and_then(|x| x.group.as_ref())
            .is_some_and(|g| g.index > 0);
        if ANCHORS.contains(&m.key.as_str()) && !follower {
            let clash = seen.iter().find(|(k, _)| {
                *k == m.key
                    || (name_family(k)
                        && name_family(&m.key)
                        && (k == "full_name") != (m.key == "full_name"))
            });
            if let Some((_, j)) = clash {
                let a = &fields[*j];
                let same_row = a.page == f.page
                    && (a.rect.center().1 - f.rect.center().1).abs()
                        < 0.5 * a.rect.height().min(f.rect.height());
                if !same_row {
                    current = None;
                }
            }
        }
        let p = *current.get_or_insert_with(|| {
            count += 1;
            seen.clear();
            count - 1
        });
        if ANCHORS.contains(&m.key.as_str()) && !follower {
            seen.push((m.key.clone(), i));
        }
        out[i] = Some(p);
    }
    out
}

/// Row number of every field (fields whose vertical centres are close share
/// a row), numbered through the document.
pub fn rows(fields: &[Field]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..fields.len()).collect();
    idx.sort_by(|&a, &b| {
        let (fa, fb) = (&fields[a], &fields[b]);
        fa.page
            .cmp(&fb.page)
            .then(fa.rect.center().1.total_cmp(&fb.rect.center().1))
    });
    let mut out = vec![0; fields.len()];
    let mut row = 0;
    let mut anchor: Option<usize> = None;
    for &i in &idx {
        if let Some(a) = anchor {
            let (fa, fi) = (&fields[a], &fields[i]);
            if fa.page != fi.page
                || (fi.rect.center().1 - fa.rect.center().1).abs()
                    > 0.5 * fa.rect.height().min(fi.rect.height()).max(4.0)
            {
                row += 1;
                anchor = Some(i);
            }
        } else {
            anchor = Some(i);
        }
        out[i] = row;
    }
    out
}

/// Could this page text be a section heading? Headings are short, mostly
/// capitals or numbered ("2. DATOS DE LA PERSONA SOLICITANTE"); anything else
/// (sentences, values printed on the page) is never sent.
pub fn heading_like(s: &str) -> bool {
    let letters: Vec<char> = s.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.len() < 4 || s.split_whitespace().count() > 12 || s.chars().count() > 90 {
        return false;
    }
    let lower = s.to_lowercase();
    if lower.contains("http") || lower.contains("www.") || s.contains('@') {
        return false;
    }
    // Long digit runs are numbers (IDs, phone numbers), never headings.
    let mut run = 0;
    for c in s.chars() {
        run = if c.is_ascii_digit() { run + 1 } else { 0 };
        if run >= 4 {
            return false;
        }
    }
    let upper = letters.iter().filter(|c| c.is_uppercase()).count();
    if upper * 10 < letters.len() * 7 {
        return false;
    }
    // Capitals alone could be a name or address printed on the page: a
    // heading must also be numbered or use a heading word.
    let t = s.trim_start();
    let numbered = t.chars().next().is_some_and(|c| c.is_ascii_digit())
        || t.split_whitespace()
            .next()
            .is_some_and(|w| w.len() <= 4 && w.ends_with(['.', ')']));
    let n = crate::meaning::normalise(s);
    numbered || n.split_whitespace().any(|w| HEADING_WORDS.contains(&w))
}

/// Words that mark a section heading (normalised: lower case, no accents).
const HEADING_WORDS: &[&str] = &[
    "section",
    "part",
    "details",
    "information",
    "data",
    "applicant",
    "child",
    "children",
    "parent",
    "parents",
    "guardian",
    "partner",
    "spouse",
    "employer",
    "contact",
    "address",
    "declaration",
    "personal",
    "seccion",
    "apartado",
    "datos",
    "dato",
    "solicitante",
    "persona",
    "personas",
    "alumno",
    "alumna",
    "menor",
    "tutor",
    "tutora",
    "guarda",
    "domicilio",
    "contacto",
    "notificacion",
    "representante",
    "conyuge",
    "familiar",
    "uppgifter",
    "sokande",
    "barn",
    "foralder",
    "vardnadshavare",
    "make",
    "maka",
    "sambo",
    "adress",
    "angaben",
    "antragsteller",
    "kind",
    "informations",
    "demandeur",
    "enfant",
];

/// Section headings of the document and, per field, the index of the heading
/// it falls under. Headings are text that is not any field's label.
pub fn sections(
    fields: &[Field],
    info: &[FieldInfo],
    text: &HashMap<usize, Vec<TextRun>>,
) -> (Vec<String>, Vec<Option<usize>>) {
    let labels: Vec<String> = info
        .iter()
        .filter_map(|i| i.label.as_ref())
        .map(|l| crate::meaning::normalise(l))
        .collect();
    let inside_field = |page: usize, r: &Rect| {
        fields
            .iter()
            .any(|f| f.page == page && f.rect.contains(r.center().0, r.center().1))
    };
    // (page, y, text) of every heading, in reading order.
    let mut heads: Vec<(usize, f32, String)> = Vec::new();
    let mut pages: Vec<&usize> = text.keys().collect();
    pages.sort();
    for &p in pages {
        for run in &text[&p] {
            let t = crate::labels::clean(&run.text);
            if !heading_like(&t) || inside_field(p, &run.rect) {
                continue;
            }
            let n = crate::meaning::normalise(&t);
            if labels
                .iter()
                .any(|l| *l == n || (n.contains(l.as_str()) && l.len() * 2 > n.len()))
            {
                continue;
            }
            heads.push((p, run.rect.y1, t));
        }
    }
    heads.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let mut names: Vec<String> = Vec::new();
    let mut per_field = vec![None; fields.len()];
    for (i, f) in fields.iter().enumerate() {
        let h = heads
            .iter()
            .rev()
            .find(|(p, y, _)| *p < f.page || (*p == f.page && *y <= f.rect.y0 + 1.0));
        if let Some((_, _, t)) = h {
            let k = match names.iter().position(|n| n == t) {
                Some(k) => k,
                None => {
                    names.push(t.clone());
                    names.len() - 1
                }
            };
            per_field[i] = Some(k);
        }
    }
    (names, per_field)
}

// ----------------------------------------------------------------- AI step

/// Instruction for grouping fields into people. Kept short.
pub const PEOPLE_INSTRUCTION: &str = "The fields of one form are listed as number|section|row|label, with the form's SECTIONS. \
Group the fields by the person they are about. Reply only with one line per field: number:group:role, e.g. `1:1:applicant`; no other text. \
group is a small number; the same person keeps the same group in every section. Use group 0 for fields not about one specific person. \
role is one of: applicant (the person filling in or signing), child, partner, parent, guardian, representative, other.";

/// The request: section headings and fields (label, section, row). No values.
pub fn build_people_prompt(
    sections: &[String],
    lines: &[(Option<usize>, usize, String)],
) -> String {
    let mut s = String::from("SECTIONS:\n");
    for (i, h) in sections.iter().enumerate() {
        s.push_str(&format!("{}:{}\n", i + 1, crate::ai::prepare_label(h)));
    }
    s.push_str("FIELDS:\n");
    for (i, (sec, row, label)) in lines.iter().enumerate() {
        let sec = sec
            .map(|k| (k + 1).to_string())
            .unwrap_or_else(|| "-".into());
        s.push_str(&format!(
            "{}|{}|{}|{}\n",
            i + 1,
            sec,
            row + 1,
            crate::ai::prepare_label(label)
        ));
    }
    s
}

/// Parses `n:group:role` lines. `None` if anything does not parse (the reply
/// is then discarded).
pub fn parse_people_reply(reply: &str, n: usize) -> Option<Vec<Option<(usize, String)>>> {
    let reply = reply
        .trim()
        .trim_start_matches("```")
        .trim_start_matches("text")
        .trim_end_matches("```")
        .trim();
    let mut out = vec![None; n];
    for line in reply.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let mut it = line.split(':').map(|s| s.trim().trim_matches('`'));
        let idx: usize = it.next()?.trim_start_matches('#').parse().ok()?;
        let group: usize = it.next()?.parse().ok()?;
        let role = it.next().unwrap_or("other").to_lowercase();
        if idx == 0
            || idx > n
            || out[idx - 1].is_some()
            || group > 99
            || !ROLES.contains(&role.as_str())
            || it.next().is_some()
        {
            return None;
        }
        out[idx - 1] = Some((group, role));
    }
    // Every field answered exactly once, or the reply is discarded.
    out.iter().all(Option::is_some).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdfr_core::field::{Origin, TextMode};

    fn tf(x: f32, y: f32) -> Field {
        Field::new_text(
            0,
            Rect::new(x, y, x + 100.0, y + 14.0),
            TextMode::Single,
            Origin::Detected,
        )
    }

    #[test]
    fn people_by_section_and_row() {
        // Child: family name, second family name (same row), given name, address.
        // Guardian: family name, given name, ID, phone, phone.
        let fields = vec![
            tf(50.0, 100.0),
            tf(160.0, 100.0),
            tf(270.0, 100.0),
            tf(50.0, 130.0),
            tf(50.0, 200.0),
            tf(270.0, 200.0),
            tf(380.0, 200.0),
            tf(50.0, 230.0),
            tf(160.0, 230.0),
        ];
        let keys = [
            "family_name",
            "family_name",
            "given_name",
            "street_address",
            "family_name",
            "given_name",
            "national_id",
            "phone",
            "phone",
        ];
        let meanings: Vec<Option<Meaning>> = keys.iter().map(|k| Some(Meaning::new(k))).collect();
        let info = vec![FieldInfo::default(); fields.len()];
        let p = group_people(&fields, &meanings, &info);
        assert_eq!(
            p,
            vec![
                Some(0),
                Some(0),
                Some(0),
                Some(0),
                Some(1),
                Some(1),
                Some(1),
                Some(1),
                Some(1)
            ]
        );
    }

    #[test]
    fn one_person_per_table_row() {
        let fields = vec![
            tf(50.0, 100.0),
            tf(160.0, 100.0),
            tf(50.0, 120.0),
            tf(160.0, 120.0),
        ];
        let keys = ["full_name", "date_of_birth", "full_name", "date_of_birth"];
        let meanings: Vec<Option<Meaning>> = keys.iter().map(|k| Some(Meaning::new(k))).collect();
        let info = vec![FieldInfo::default(); fields.len()];
        assert_eq!(
            group_people(&fields, &meanings, &info),
            vec![Some(0), Some(0), Some(1), Some(1)]
        );
    }

    #[test]
    fn headings() {
        assert!(heading_like("2. DATOS DE LA PERSONA SOLICITANTE"));
        assert!(heading_like("SECTION 1 PERSONAL DETAILS"));
        assert!(!heading_like("Marque solo una opción."));
        assert!(!heading_like("ANNA 19850312-1234"));
        assert!(
            !heading_like("ANNA SVENSSON"),
            "a name in capitals is not a heading"
        );
        assert!(!heading_like("STORGATAN 12 A"));
        assert!(heading_like("DATOS DE LA PERSONA GUARDADORA"));
        assert!(!heading_like("https://WWW.JUNTA.ES"));
    }

    #[test]
    fn people_reply() {
        let r = parse_people_reply("1:1:child\n2:1:child\n3:2:guardian\n", 3).unwrap();
        assert_eq!(r[2], Some((2, "guardian".to_string())));
        assert!(parse_people_reply("1:1:boss", 1).is_none());
        assert!(parse_people_reply("hello", 1).is_none());
        assert!(parse_people_reply("4:1:child", 3).is_none());
        assert!(
            parse_people_reply("1:1:child\n2:1:child", 3).is_none(),
            "incomplete"
        );
        assert!(
            parse_people_reply("1:1:child\n1:2:child\n2:1:child", 2).is_none(),
            "duplicate"
        );
    }
}
