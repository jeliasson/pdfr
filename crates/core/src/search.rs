//! Finding words in the pages' text layer (Ctrl+F).
//!
//! Matching ignores case. A plain letter in the query also matches its
//! accented forms ("cafe" finds "café", "malmo" finds "Malmö"), while an
//! accented letter matches only itself ("å" does not find "a"). Spaces in the
//! query match any space, line break or gap between phrases, and a word
//! hyphenated across a line break is found whole.

use crate::geom::Rect;
use crate::text::TextRun;

/// One place a query was found.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub page: usize,
    /// The matched characters' boxes, one per phrase it spans.
    pub rects: Vec<Rect>,
    /// The match with some words around it, for a list.
    pub snippet: String,
}

/// A page's text as one sequence: each character with the phrase and the
/// index it came from (`None` for the space joining two phrases).
struct PageText {
    chars: Vec<char>,
    from: Vec<Option<(usize, usize)>>,
}

/// Phrases in reading order: by line, then left to right.
fn reading_order(runs: &[TextRun]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..runs.len()).collect();
    idx.sort_by(|&a, &b| {
        let (ra, rb) = (&runs[a], &runs[b]);
        let same_line =
            (ra.rect.y1 - rb.rect.y1).abs() < 0.3 * ra.size.max(rb.size).max(1.0);
        if same_line {
            ra.rect.x0.total_cmp(&rb.rect.x0)
        } else {
            ra.rect.y1.total_cmp(&rb.rect.y1)
        }
    });
    idx
}

fn page_text(runs: &[TextRun]) -> PageText {
    let mut t = PageText {
        chars: vec![],
        from: vec![],
    };
    for ri in reading_order(runs) {
        let r = &runs[ri];
        if !t.chars.is_empty() {
            // A word broken with a hyphen at the end of a line joins up.
            let prev_line_end = t.chars.last() == Some(&'-')
                && t.from.last().copied().flatten().is_some();
            if prev_line_end {
                t.chars.pop();
                t.from.pop();
            } else {
                t.chars.push(' ');
                t.from.push(None);
            }
        }
        for (ci, c) in r.text.chars().enumerate() {
            t.chars.push(c);
            t.from.push(Some((ri, ci)));
        }
    }
    t
}

/// A character without its accent, lowercased (`É` -> `e`, `ø` -> `o`).
pub fn fold(c: char) -> char {
    let l = c.to_lowercase().next().unwrap_or(c);
    match l {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => 'a',
        'ç' | 'ć' | 'č' | 'ĉ' | 'ċ' => 'c',
        'ď' | 'đ' => 'd',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ė' | 'ę' | 'ě' => 'e',
        'ğ' | 'ģ' => 'g',
        'ì' | 'í' | 'î' | 'ï' | 'ī' | 'į' | 'ı' => 'i',
        'ķ' => 'k',
        'ĺ' | 'ļ' | 'ľ' | 'ł' => 'l',
        'ñ' | 'ń' | 'ņ' | 'ň' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ő' => 'o',
        'ŕ' | 'ř' => 'r',
        'ś' | 'ş' | 'š' | 'ș' => 's',
        'ţ' | 'ť' | 'ț' => 't',
        'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' | 'ų' => 'u',
        'ý' | 'ÿ' => 'y',
        'ź' | 'ż' | 'ž' => 'z',
        '\u{2019}' | '\u{2018}' => '\'',
        '\u{201c}' | '\u{201d}' => '"',
        '\u{2013}' | '\u{2014}' => '-',
        other => other,
    }
}

fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Whether query character `q` matches page character `c`.
fn matches(q: char, c: char) -> bool {
    if fold(q) == lower(q) {
        fold(c) == lower(q)
    } else {
        lower(c) == lower(q)
    }
}

/// The query as characters, with runs of spaces as one space, trimmed.
fn normalise(query: &str) -> Vec<char> {
    let mut out: Vec<char> = Vec::new();
    for c in query.trim().chars() {
        let c = if c.is_whitespace() { ' ' } else { c };
        if c == ' ' && out.last() == Some(&' ') {
            continue;
        }
        out.push(c);
    }
    out
}

/// Every match of `query` on one page, in reading order.
pub fn search_page(page: usize, runs: &[TextRun], query: &str) -> Vec<Hit> {
    let q = normalise(query);
    if q.is_empty() {
        return vec![];
    }
    let t = page_text(runs);
    let n = t.chars.len();
    let mut hits = Vec::new();
    let mut i = 0;
    while i < n {
        // Match q at i; a space in the query takes one or more spaces.
        let mut k = i;
        let mut ok = true;
        for &qc in &q {
            if qc == ' ' {
                if k >= n || t.chars[k] != ' ' {
                    ok = false;
                    break;
                }
                while k < n && t.chars[k] == ' ' {
                    k += 1;
                }
            } else {
                if k >= n || !matches(qc, t.chars[k]) {
                    ok = false;
                    break;
                }
                k += 1;
            }
        }
        if !ok {
            i += 1;
            continue;
        }
        hits.push(Hit {
            page,
            rects: rects_of(runs, &t.from[i..k]),
            snippet: snippet(&t.chars, i, k),
        });
        i = k.max(i + 1);
    }
    hits
}

/// Searches every page that has text, in page order.
pub fn search(pages: &[(usize, &[TextRun])], query: &str) -> Vec<Hit> {
    let mut v: Vec<&(usize, &[TextRun])> = pages.iter().collect();
    v.sort_by_key(|(p, _)| *p);
    v.into_iter()
        .flat_map(|(p, runs)| search_page(*p, runs, query))
        .collect()
}

/// One box per phrase covering the matched characters.
fn rects_of(runs: &[TextRun], from: &[Option<(usize, usize)>]) -> Vec<Rect> {
    let mut out: Vec<(usize, Rect)> = Vec::new();
    for &(ri, ci) in from.iter().flatten() {
        let r = runs[ri].char_rect(ci);
        match out.last_mut() {
            Some((last, acc)) if *last == ri => {
                acc.x0 = acc.x0.min(r.x0);
                acc.x1 = acc.x1.max(r.x1);
                acc.y0 = acc.y0.min(r.y0);
                acc.y1 = acc.y1.max(r.y1);
            }
            _ => out.push((ri, r)),
        }
    }
    out.into_iter().map(|(_, r)| r).collect()
}

/// The match with a few words of context, cut at word boundaries.
fn snippet(chars: &[char], a: usize, b: usize) -> String {
    const AROUND: usize = 28;
    let mut s = a.saturating_sub(AROUND);
    while s > 0 && s < a && chars[s - 1] != ' ' {
        s += 1;
    }
    let mut e = (b + AROUND).min(chars.len());
    while e < chars.len() && e > b && chars[e] != ' ' {
        e -= 1;
    }
    let mut out = String::new();
    if s > 0 {
        out.push('\u{2026}');
    }
    out.extend(&chars[s..e]);
    if e < chars.len() {
        out.push('\u{2026}');
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, x: f32, y: f32) -> TextRun {
        let xs: Vec<f32> = (0..text.chars().count()).map(|i| x + i as f32 * 5.0).collect();
        let ws = vec![5.0; xs.len()];
        TextRun {
            rect: Rect::new(x, y - 8.0, x + xs.len() as f32 * 5.0, y + 2.0),
            text: text.into(),
            size: 10.0,
            xs,
            ws,
        }
    }

    #[test]
    fn finds_words_ignoring_case_and_plain_accents() {
        let runs = vec![run("Résumé of Åsa Öberg, Malmö", 10.0, 100.0)];
        assert_eq!(search_page(0, &runs, "resume").len(), 1);
        assert_eq!(search_page(0, &runs, "MALMO").len(), 1);
        assert_eq!(search_page(0, &runs, "malmö").len(), 1);
        // An accented letter in the query matches only itself.
        assert_eq!(search_page(0, &runs, "Äsa").len(), 0);
        let h = &search_page(0, &runs, "of")[0];
        assert_eq!(h.rects.len(), 1);
        let r = h.rects[0];
        assert!((r.x0 - 45.0).abs() < 1e-3 && (r.x1 - 55.0).abs() < 1e-3, "{r:?}");
    }

    #[test]
    fn matches_across_phrases_lines_and_hyphens() {
        let runs = vec![
            run("Reference", 10.0, 100.0),
            run("number: 4711", 100.0, 100.0),
            run("Send the applica-", 10.0, 120.0),
            run("tion to the office", 10.0, 140.0),
        ];
        let h = search_page(3, &runs, "reference  number");
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].rects.len(), 2, "one box per phrase");
        assert_eq!(h[0].page, 3);
        assert_eq!(search_page(0, &runs, "application").len(), 1);
        assert_eq!(search_page(0, &runs, "the").len(), 2);
        assert!(search_page(0, &runs, "  ").is_empty());
    }

    #[test]
    fn snippets_show_context() {
        let runs = vec![run(
            "Please return the signed form to the address shown at the bottom of this page",
            10.0,
            100.0,
        )];
        let h = &search_page(0, &runs, "address")[0];
        assert!(h.snippet.contains("address"));
        assert!(h.snippet.starts_with('\u{2026}') && h.snippet.ends_with('\u{2026}'));
    }
}
