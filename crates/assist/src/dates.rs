//! Dates: parsing what the user typed, storing ISO dates, and formatting a
//! stored date for a field (a part, a row of boxes, or a text field).

use crate::meaning::{Part, date_order};
use pdfr_core::field::Field;

pub type Ymd = (u32, u32, u32);

pub fn valid(y: u32, m: u32, d: u32) -> bool {
    let dim = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400) => 29,
        2 => 28,
        _ => return false,
    };
    (1000..=2999).contains(&y) && d >= 1 && d <= dim
}

pub fn iso((y, m, d): Ymd) -> String {
    format!("{y:04}-{m:02}-{d:02}")
}

pub fn parse_iso(s: &str) -> Option<Ymd> {
    let p: Vec<u32> = s
        .split('-')
        .map(|x| x.parse().ok())
        .collect::<Option<_>>()?;
    (p.len() == 3 && valid(p[0], p[1], p[2])).then(|| (p[0], p[1], p[2]))
}

pub fn expand_year(y: u32) -> u32 {
    let this = today().0 % 100;
    if y <= this { 2000 + y } else { 1900 + y }
}

/// Parses a typed date. `order` (from the label) decides ambiguous orders;
/// without it, a 4-digit first part means Y-M-D, otherwise D-M-Y.
pub fn parse(s: &str, order: Option<&[Part]>) -> Option<Ymd> {
    let s = s.trim();
    let parts: Vec<&str> = s
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .collect();
    let nums: Vec<String> = if parts.len() == 1 && (parts[0].len() == 8 || parts[0].len() == 6) {
        let d = parts[0];
        let o = order.unwrap_or(&[Part::Day, Part::Month, Part::Year]);
        let mut out = Vec::new();
        let mut i = 0;
        for p in o {
            let n = if *p == Part::Year { d.len() - 4 } else { 2 };
            out.push(d.get(i..i + n)?.to_string());
            i += n;
        }
        out
    } else if parts.len() == 3 {
        parts.iter().map(|s| s.to_string()).collect()
    } else {
        return None;
    };
    let n: Vec<u32> = nums.iter().map(|x| x.parse().ok()).collect::<Option<_>>()?;
    let order: Vec<Part> = match order {
        Some(o) if o.len() == 3 => o.to_vec(),
        _ if nums[0].len() == 4 => vec![Part::Year, Part::Month, Part::Day],
        _ => vec![Part::Day, Part::Month, Part::Year],
    };
    let (mut y, mut m, mut d) = (0, 0, 0);
    for (p, v) in order.iter().zip(&n) {
        match p {
            Part::Year => y = *v,
            Part::Month => m = *v,
            Part::Day => d = *v,
        }
    }
    if y < 100 {
        y = expand_year(y);
    }
    valid(y, m, d).then_some((y, m, d))
}

/// Today's date (UTC is close enough for a form date).
pub fn today() -> Ymd {
    let days = (crate::memory::now() / 86_400) as i64;
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    ((if m <= 2 { y + 1 } else { y }) as u32, m as u32, d as u32)
}

/// Formats a date for `field`: one part, a row of boxes, or text following
/// the order and separator the label shows (default DD/MM/YYYY; Swedish
/// labels without a hint get YYYY-MM-DD).
pub fn format_for((y, m, d): Ymd, part: Option<Part>, field: &Field, label: &str) -> String {
    let cap = field.capacity();
    match part {
        Some(Part::Day) => return format!("{d:02}"),
        Some(Part::Month) => return format!("{m:02}"),
        Some(Part::Year) => {
            return if cap == Some(2) {
                format!("{:02}", y % 100)
            } else {
                format!("{y:04}")
            };
        }
        None => {}
    }
    let lower = crate::meaning::normalise(label);
    let swedish = ["fodelsedatum", "datum"]
        .iter()
        .any(|w| lower.split(' ').any(|x| x == *w));
    let order = date_order(label).unwrap_or_else(|| {
        if swedish {
            vec![Part::Year, Part::Month, Part::Day]
        } else {
            vec![Part::Day, Part::Month, Part::Year]
        }
    });
    let comb = field.kind.is_comb();
    let sep = if comb {
        ""
    } else if label.contains('/') {
        "/"
    } else if label.contains('.') && !label.ends_with('.') {
        "."
    } else if swedish || label.contains('-') {
        "-"
    } else {
        "/"
    };
    let yy = comb && cap == Some(6);
    order
        .iter()
        .map(|p| match p {
            Part::Day => format!("{d:02}"),
            Part::Month => format!("{m:02}"),
            Part::Year => {
                if yy {
                    format!("{:02}", y % 100)
                } else {
                    format!("{y:04}")
                }
            }
        })
        .collect::<Vec<_>>()
        .join(sep)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdfr_core::field::{Origin, TextMode};
    use pdfr_core::geom::Rect;

    #[test]
    fn parsing() {
        assert_eq!(parse("12/03/1985", None), Some((1985, 3, 12)));
        assert_eq!(parse("1985-03-12", None), Some((1985, 3, 12)));
        assert_eq!(
            parse("03/12/1985", Some(&[Part::Month, Part::Day, Part::Year])),
            Some((1985, 3, 12))
        );
        assert_eq!(parse("12031985", None), Some((1985, 3, 12)));
        assert_eq!(parse("31/02/1985", None), None);
        assert_eq!(parse("hello", None), None);
    }

    #[test]
    fn formatting() {
        let text = Field::new_text(
            0,
            Rect::new(0.0, 0.0, 100.0, 14.0),
            TextMode::Single,
            Origin::Detected,
        );
        let r = Rect::new(0.0, 0.0, 112.0, 14.0);
        let comb8 = Field::new_text(
            0,
            r,
            TextMode::Comb(Field::equal_cells(&r, 8)),
            Origin::Detected,
        );
        let ymd = (1985, 3, 12);
        assert_eq!(format_for(ymd, None, &text, "Date of birth"), "12/03/1985");
        assert_eq!(format_for(ymd, None, &text, "Födelsedatum"), "1985-03-12");
        assert_eq!(
            format_for(ymd, None, &text, "DOB (mm/dd/yyyy)"),
            "03/12/1985"
        );
        assert_eq!(
            format_for(ymd, None, &comb8, "Date of birth (DD MM YYYY)"),
            "12031985"
        );
        assert_eq!(format_for(ymd, Some(Part::Year), &text, ""), "1985");
        assert_eq!(format_for(ymd, Some(Part::Day), &text, ""), "12");
    }

    #[test]
    fn today_is_plausible() {
        let (y, m, d) = today();
        assert!(y >= 2025 && (1..=12).contains(&m) && (1..=31).contains(&d));
    }
}
