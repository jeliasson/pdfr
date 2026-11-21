//! Checks before saving: answers that look wrong for what the box asks
//! (an email without @, an IBAN whose check digits don't add up, a birth
//! date in the future, a Spanish DNI/NIE or Swedish personnummer with the
//! wrong check character) and boxes still empty. All local: nothing is
//! sent anywhere.

use crate::dates;
use crate::{Analysis, Meaning};
use pdfr_core::field::{Field, FieldKind, Origin};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// The answer looks wrong.
    Problem,
    /// Nothing filled in.
    Empty,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    /// Index into the fields.
    pub field: usize,
    pub severity: Severity,
    pub message: String,
}

/// Every finding, problems first, then empty boxes, each in page order.
pub fn check(an: &Analysis, fields: &[Field]) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut seen_names = std::collections::HashSet::new();
    for (i, f) in fields.iter().enumerate() {
        if f.read_only
            || !matches!(
                f.kind,
                FieldKind::Text {
                    password: false,
                    ..
                }
            )
        {
            continue;
        }
        // Widgets of one real field share a value: check it once.
        if let Origin::AcroForm { name, .. } = &f.origin
            && !seen_names.insert(name.clone())
        {
            continue;
        }
        let v = f.value.trim();
        if v.is_empty() {
            if f.backdrop.as_ref().is_none_or(|b| b.printed.is_empty()) {
                out.push(Finding {
                    field: i,
                    severity: Severity::Empty,
                    message: "Empty".into(),
                });
            }
            continue;
        }
        let meaning = an.meanings.get(i).cloned().flatten();
        if let Some(m) = meaning
            && let Some(msg) = problem(&m, v, f)
        {
            out.push(Finding {
                field: i,
                severity: Severity::Problem,
                message: msg,
            });
        }
    }
    out.sort_by_key(|x| {
        let f = &fields[x.field];
        (
            x.severity,
            f.page,
            (f.rect.y0 * 10.0) as i64,
            (f.rect.x0 * 10.0) as i64,
        )
    });
    out
}

/// What looks wrong with `v` as an answer for `m`, if anything.
fn problem(m: &Meaning, v: &str, f: &Field) -> Option<String> {
    // One character per box (a value split over boxes): too little to judge.
    if f.kind.is_comb() || m.part.is_some() || v.chars().count() < 3 {
        return None;
    }
    match m.key.as_str() {
        "email" if !looks_like_email(v) => Some("This doesn't look like an email address.".into()),
        "iban" => iban_problem(v),
        "bic" if !looks_like_bic(v) => {
            Some("A BIC/SWIFT code has 8 or 11 letters and digits.".into())
        }
        "phone" => {
            let digits = v.chars().filter(char::is_ascii_digit).count();
            let junk = v
                .chars()
                .any(|c| !(c.is_ascii_digit() || " +-()./".contains(c)));
            (junk || !(6..=15).contains(&digits))
                .then(|| "This doesn't look like a phone number.".into())
        }
        "national_id" | "tax_id" => id_problem(v),
        k if m.is_date() => date_problem(k, v),
        _ => None,
    }
}

fn looks_like_email(v: &str) -> bool {
    let Some((local, domain)) = v.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.contains('@')
        && !v.contains(char::is_whitespace)
        && domain
            .rsplit_once('.')
            .is_some_and(|(host, tld)| !host.is_empty() && tld.len() >= 2)
}

fn looks_like_bic(v: &str) -> bool {
    let s: String = v.chars().filter(|c| !c.is_whitespace()).collect();
    (s.len() == 8 || s.len() == 11)
        && s.chars().all(|c| c.is_ascii_alphanumeric())
        && s[..6].chars().all(|c| c.is_ascii_alphabetic())
}

/// The IBAN check: country letters, two check digits, mod 97 == 1.
fn iban_problem(v: &str) -> Option<String> {
    let s: String = v
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| c.to_ascii_uppercase())
        .collect();
    let shaped = s.len() >= 15
        && s.len() <= 34
        && s.chars().all(|c| c.is_ascii_alphanumeric())
        && s[..2].chars().all(|c| c.is_ascii_alphabetic())
        && s[2..4].chars().all(|c| c.is_ascii_digit());
    if !shaped {
        return Some("An IBAN starts with two letters and two digits, then up to 30 more.".into());
    }
    let moved = format!("{}{}", &s[4..], &s[..4]);
    let mut rem: u32 = 0;
    for c in moved.chars() {
        let n = c.to_digit(36)?;
        for d in n.to_string().chars() {
            rem = (rem * 10 + d.to_digit(10)?) % 97;
        }
    }
    (rem != 1).then(|| "The IBAN's check digits don't match: a digit may be wrong.".into())
}

/// Checks the check character of IDs whose format is recognised: Spanish
/// DNI and NIE, Swedish personnummer. Other formats are left alone.
fn id_problem(v: &str) -> Option<String> {
    let s: String = v
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    const LETTERS: &[u8] = b"TRWAGMYFPDXBNJZSQVHLCKE";
    let spanish = |digits: &str, letter: char| {
        let n: u32 = digits.parse().ok()?;
        let want = LETTERS[(n % 23) as usize] as char;
        (want != letter).then(|| {
            format!(
                "The letter of this DNI/NIE should be {want}: a digit or the letter may be wrong."
            )
        })
    };
    let b = s.as_bytes();
    if s.len() == 9 && b[..8].iter().all(u8::is_ascii_digit) && b[8].is_ascii_alphabetic() {
        return spanish(&s[..8], b[8] as char);
    }
    if s.len() == 9
        && matches!(b[0], b'X' | b'Y' | b'Z')
        && b[1..8].iter().all(u8::is_ascii_digit)
        && b[8].is_ascii_alphabetic()
    {
        let lead = (b[0] - b'X').to_string();
        return spanish(&format!("{lead}{}", &s[1..8]), b[8] as char);
    }
    // Swedish personnummer: (YY)YYMMDD-NNNN with a Luhn check digit.
    let looks_swedish = v.contains('-') || v.contains('+') || s.len() == 12;
    if looks_swedish && (s.len() == 10 || s.len() == 12) && s.chars().all(|c| c.is_ascii_digit()) {
        let ten = &s[s.len() - 10..];
        let (mm, dd): (u32, u32) = (ten[2..4].parse().ok()?, ten[4..6].parse().ok()?);
        // Day + 60 is a coordination number.
        if (1..=12).contains(&mm) && (1..=31).contains(&(dd % 60)) && !luhn(ten) {
            return Some(
                "The last digit of this personnummer doesn't match: a digit may be wrong.".into(),
            );
        }
    }
    None
}

fn luhn(digits: &str) -> bool {
    let sum: u32 = digits
        .chars()
        .rev()
        .filter_map(|c| c.to_digit(10))
        .enumerate()
        .map(|(i, d)| {
            if i % 2 == 1 {
                let x = d * 2;
                if x > 9 { x - 9 } else { x }
            } else {
                d
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

fn date_problem(key: &str, v: &str) -> Option<String> {
    let Some((y, m, d)) = dates::parse(v, None) else {
        // Only judge things that are meant to be a whole date.
        let digits = v.chars().filter(char::is_ascii_digit).count();
        return (digits >= 6).then(|| "This doesn't look like a date.".into());
    };
    let today = dates::today();
    let n = |(y, m, d): (u32, u32, u32)| y * 10_000 + m * 100 + d;
    if key == "date_of_birth" {
        if n((y, m, d)) > n(today) {
            return Some("This birth date is in the future.".into());
        }
        if today.0.saturating_sub(y) > 120 {
            return Some("This birth date is more than 120 years ago.".into());
        }
    }
    None
}

/// What the AI is asked when explaining a form.
pub const EXPLAIN_INSTRUCTION: &str = "You help someone fill in a form. You get only the form's \
section headings and box labels, never their answers. In plain, friendly words, say in one \
sentence what the form is for, then for each section one short line on what it asks and \
anything easy to get wrong (formats, who should fill it in). At most 180 words. Plain text, \
lines starting with \"- \", no markdown headings, no questions back.";

/// The request for explaining a form: section headings and box labels, in
/// page order. Anything containing one of the form's answers is left out.
/// `None` when there is nothing to explain from.
pub fn explain_prompt(an: &Analysis, fields: &[Field]) -> Option<String> {
    let answers: Vec<String> = fields
        .iter()
        .map(|f| f.value.trim().to_lowercase())
        .filter(|v| v.chars().count() >= 3)
        .collect();
    let clean = |s: &str| {
        let low = s.to_lowercase();
        (!answers.iter().any(|a| low.contains(a.as_str()))).then(|| crate::ai::prepare_label(s))
    };
    let mut lines: Vec<String> = Vec::new();
    let mut last_section = None;
    for (i, inf) in an.info.iter().enumerate() {
        let sec = an.section_of.get(i).copied().flatten();
        if sec != last_section {
            last_section = sec;
            if let Some(h) = sec
                .and_then(|s| an.sections.get(s))
                .filter(|h| crate::profiles::heading_like(h))
                .and_then(|h| clean(h))
            {
                lines.push(format!("## {h}"));
            }
        }
        if let Some(l) = inf
            .label
            .as_deref()
            .filter(|l| crate::labels::plausible_label(l))
            .and_then(clean)
            && lines.last() != Some(&l)
        {
            lines.push(l);
        }
    }
    let labels = lines.iter().filter(|l| !l.starts_with("## ")).count();
    (labels >= 2).then(|| lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdfr_core::field::TextMode;
    use pdfr_core::geom::Rect;

    fn run(key: &str, v: &str) -> Option<String> {
        let f = Field::new_text(
            0,
            Rect::new(0.0, 0.0, 200.0, 20.0),
            TextMode::Single,
            Origin::Detected,
        );
        problem(&Meaning::new(key), v, &f)
    }

    #[test]
    fn emails() {
        assert!(run("email", "anna@example.com").is_none());
        assert!(run("email", "anna.example.com").is_some());
        assert!(run("email", "anna@example").is_some());
        assert!(run("email", "an na@example.com").is_some());
    }

    #[test]
    fn ibans() {
        assert!(run("iban", "ES91 2100 0418 4502 0005 1332").is_none());
        assert!(run("iban", "SE45 5000 0000 0583 9825 7466").is_none());
        assert!(run("iban", "ES91 2100 0418 4502 0005 1333").is_some());
        assert!(run("iban", "12345").is_some());
    }

    #[test]
    fn ids() {
        assert!(run("national_id", "12345678Z").is_none());
        assert!(run("national_id", "12345678A").is_some());
        assert!(run("national_id", "X1234567L").is_none());
        assert!(run("national_id", "X1234567T").is_some());
        assert!(run("national_id", "811218-9876").is_none());
        assert!(run("national_id", "811218-9877").is_some());
        // Unknown formats are left alone.
        assert!(run("national_id", "AB123456").is_none());
        assert!(run("tax_id", "123-45-6789").is_none());
    }

    #[test]
    fn dates_and_phones() {
        assert!(run("date_of_birth", "1981-12-18").is_none());
        assert!(run("date_of_birth", "2999-01-01").is_some());
        assert!(run("date_of_birth", "1801-01-01").is_some());
        assert!(run("phone", "+34 600 123 456").is_none());
        assert!(run("phone", "12").is_none(), "too short to judge");
        assert!(run("phone", "call me").is_some());
        assert!(run("bic", "CAIXESBBXXX").is_none());
        assert!(run("bic", "CAIX12").is_some());
    }

    #[test]
    fn explaining_sends_labels_never_answers() {
        let mk = |y: f32, v: &str| {
            let mut f = Field::new_text(
                0,
                Rect::new(0.0, y, 200.0, y + 20.0),
                TextMode::Single,
                Origin::Detected,
            );
            f.value = v.into();
            f
        };
        let fields = [mk(0.0, "Anna Lindqvist"), mk(40.0, ""), mk(80.0, "")];
        let info = |l: &str| crate::labels::FieldInfo {
            label: Some(l.into()),
            group: None,
        };
        let an = Analysis {
            info: vec![
                info("Full name"),
                info("Email address"),
                info("Anna Lindqvist"),
            ],
            sections: vec!["1. PERSONAL DETAILS".into()],
            section_of: vec![Some(0); 3],
            ..Default::default()
        };
        let p = explain_prompt(&an, &fields).unwrap();
        assert_eq!(p, "## 1. PERSONAL DETAILS\nFull name\nEmail address");
    }

    #[test]
    fn empty_and_split_boxes() {
        let mut a = Field::new_text(
            0,
            Rect::new(0.0, 0.0, 200.0, 20.0),
            TextMode::Single,
            Origin::Detected,
        );
        a.value = "not an email".into();
        let b = Field::new_text(
            0,
            Rect::new(0.0, 40.0, 200.0, 60.0),
            TextMode::Single,
            Origin::Detected,
        );
        let mut comb = Field::new_text(
            0,
            Rect::new(0.0, 80.0, 200.0, 100.0),
            TextMode::Comb(vec![(0.0, 20.0); 9]),
            Origin::Detected,
        );
        comb.value = "1234".into();
        let an = Analysis {
            meanings: vec![
                Some(Meaning::new("email")),
                Some(Meaning::new("email")),
                Some(Meaning::new("iban")),
            ],
            ..Default::default()
        };
        let got = check(&an, &[a, b, comb]);
        assert_eq!(got.len(), 2);
        assert_eq!((got[0].field, got[0].severity), (0, Severity::Problem));
        assert_eq!((got[1].field, got[1].severity), (1, Severity::Empty));
    }
}
