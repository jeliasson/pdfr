//! Links: the PDF's link annotations (web addresses, jumps within the
//! document, named destinations) and web or e-mail addresses printed as
//! plain text, all as rectangles in display space.
//!
//! Destinations are resolved here once, for links and for the outline.

use crate::field::ObjId;
use crate::forms::deref;
use crate::geom::{PageGeom, Rect};
use crate::text::TextRun;
use lopdf::{Dictionary, Document, Object};

/// Where a link goes.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// A web page or e-mail address (`https:`, `mailto:` ...).
    Uri(String),
    /// A page of this document; `y` is where on it (display space) to show
    /// at the top, if the link says.
    Dest { page: usize, y: Option<f32> },
    /// A viewer action: `NextPage`, `PrevPage`, `FirstPage`, `LastPage`.
    Named(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub page: usize,
    pub rect: Rect,
    pub target: Target,
}

/// Page numbers by page object id, for resolving destinations.
pub(crate) struct Pages<'a> {
    pub ids: &'a [ObjId],
    pub geoms: &'a [PageGeom],
}

impl Pages<'_> {
    fn index(&self, id: ObjId) -> Option<usize> {
        self.ids.iter().position(|&p| p == id)
    }
}

fn name_of(d: &Document, o: &Object) -> Option<Vec<u8>> {
    match deref(d, o)? {
        Object::Name(n) => Some(n.clone()),
        _ => None,
    }
}

fn string_bytes(d: &Document, o: &Object) -> Option<Vec<u8>> {
    match deref(d, o)? {
        Object::String(s, _) => Some(s.clone()),
        Object::Name(n) => Some(n.clone()),
        _ => None,
    }
}

fn num(d: &Document, o: &Object) -> Option<f32> {
    deref(d, o)?.as_float().ok()
}

/// Looks `key` up in a name tree (`/Names` and `/Kids`), depth-limited.
fn name_tree<'a>(d: &'a Document, node: &'a Dictionary, key: &[u8], depth: u8) -> Option<&'a Object> {
    if depth > 24 {
        return None;
    }
    if let Some(Object::Array(names)) = node.get(b"Names").ok().and_then(|o| deref(d, o)) {
        for pair in names.chunks(2) {
            if let [k, v] = pair
                && string_bytes(d, k).as_deref() == Some(key)
            {
                return Some(v);
            }
        }
    }
    if let Some(Object::Array(kids)) = node.get(b"Kids").ok().and_then(|o| deref(d, o)) {
        for k in kids {
            let Some(kid) = deref(d, k).and_then(|o| o.as_dict().ok()) else {
                continue;
            };
            // Skip subtrees whose /Limits exclude the key.
            if let Some(Object::Array(lim)) = kid.get(b"Limits").ok().and_then(|o| deref(d, o))
                && let [lo, hi] = lim.as_slice()
                && let (Some(lo), Some(hi)) = (string_bytes(d, lo), string_bytes(d, hi))
                && (key < lo.as_slice() || key > hi.as_slice())
            {
                continue;
            }
            if let Some(v) = name_tree(d, kid, key, depth + 1) {
                return Some(v);
            }
        }
    }
    None
}

/// A named destination's value (an array, or a dictionary with `/D`).
fn named_dest<'a>(d: &'a Document, name: &[u8]) -> Option<&'a Object> {
    let cat = d.catalog().ok()?;
    // PDF 1.2+: the /Dests name tree under /Names.
    if let Some(tree) = cat
        .get(b"Names")
        .ok()
        .and_then(|o| deref(d, o))
        .and_then(|o| o.as_dict().ok())
        .and_then(|n| n.get(b"Dests").ok())
        .and_then(|o| deref(d, o))
        .and_then(|o| o.as_dict().ok())
        && let Some(v) = name_tree(d, tree, name, 0)
    {
        return Some(v);
    }
    // PDF 1.1: a /Dests dictionary in the catalog.
    cat.get(b"Dests")
        .ok()
        .and_then(|o| deref(d, o))
        .and_then(|o| o.as_dict().ok())
        .and_then(|dests| dests.get(name).ok())
}

/// Resolves a destination (explicit array, name or string) to a page and
/// the y to show at the top.
pub(crate) fn resolve_dest(d: &Document, o: &Object, pages: &Pages, depth: u8) -> Option<Target> {
    if depth > 4 {
        return None;
    }
    match deref(d, o)? {
        Object::Name(n) | Object::String(n, _) => {
            let v = named_dest(d, n)?;
            resolve_dest(d, v, pages, depth + 1)
        }
        Object::Dictionary(dict) => resolve_dest(d, dict.get(b"D").ok()?, pages, depth + 1),
        Object::Array(a) => {
            let page = match a.first()? {
                Object::Reference(id) => pages.index(*id)?,
                // Some files give the page number instead (remote-style).
                Object::Integer(n) => usize::try_from(*n).ok().filter(|n| *n < pages.ids.len())?,
                _ => return None,
            };
            let geom = pages.geoms[page];
            let kind = a.get(1).and_then(|k| name_of(d, k)).unwrap_or_default();
            // The user-space top edge the view should start at, if given.
            let top = match kind.as_slice() {
                b"XYZ" => a.get(3).and_then(|v| num(d, v)),
                b"FitH" | b"FitBH" => a.get(2).and_then(|v| num(d, v)),
                b"FitR" => a.get(5).and_then(|v| num(d, v)),
                _ => None,
            };
            let left = match kind.as_slice() {
                b"XYZ" | b"FitV" | b"FitBV" | b"FitR" => a.get(2).and_then(|v| num(d, v)),
                _ => None,
            };
            let y = top.map(|t| {
                let (_, y) = geom.to_display(left.unwrap_or(geom.crop[0]), t);
                y.clamp(0.0, geom.display_size().1)
            });
            Some(Target::Dest { page, y })
        }
        _ => None,
    }
}

/// What a link annotation's `/A` or `/Dest` points at.
fn link_target(d: &Document, annot: &Dictionary, pages: &Pages) -> Option<Target> {
    if let Ok(dest) = annot.get(b"Dest") {
        return resolve_dest(d, dest, pages, 0);
    }
    let a = deref(d, annot.get(b"A").ok()?)?.as_dict().ok()?;
    match name_of(d, a.get(b"S").ok()?)?.as_slice() {
        b"URI" => {
            let uri = string_bytes(d, a.get(b"URI").ok()?)?;
            let uri = String::from_utf8_lossy(&uri).trim().to_string();
            (!uri.is_empty()).then_some(Target::Uri(uri))
        }
        b"GoTo" => resolve_dest(d, a.get(b"D").ok()?, pages, 0),
        b"Named" => {
            let n = name_of(d, a.get(b"N").ok()?)?;
            Some(Target::Named(String::from_utf8_lossy(&n).into_owned()))
        }
        _ => None,
    }
}

/// Every link annotation of the document.
pub(crate) fn read(d: &Document, ids: &[ObjId], geoms: &[PageGeom]) -> Vec<Link> {
    let pages = Pages { ids, geoms };
    let mut out = Vec::new();
    for (pi, &pid) in ids.iter().enumerate() {
        let Some(Object::Array(annots)) = d
            .get_dictionary(pid)
            .ok()
            .and_then(|p| p.get(b"Annots").ok())
            .and_then(|o| deref(d, o))
        else {
            continue;
        };
        for a in annots {
            let Some(dict) = deref(d, a).and_then(|o| o.as_dict().ok()) else {
                continue;
            };
            if name_of(d, dict.get(b"Subtype").ok().unwrap_or(&Object::Null)).as_deref()
                != Some(b"Link")
            {
                continue;
            }
            let Some(r) = dict
                .get(b"Rect")
                .ok()
                .and_then(|o| deref(d, o))
                .and_then(|o| o.as_array().ok())
                .and_then(|a| {
                    let v: Vec<f32> = a.iter().filter_map(|o| num(d, o)).collect();
                    (v.len() == 4).then(|| [v[0], v[1], v[2], v[3]])
                })
            else {
                continue;
            };
            let rect = geoms[pi].rect_to_display(r);
            if rect.width() < 1.0 || rect.height() < 1.0 {
                continue;
            }
            if let Some(target) = link_target(d, dict, &pages) {
                out.push(Link {
                    page: pi,
                    rect,
                    target,
                });
            }
        }
    }
    out
}

/// Web and e-mail addresses printed in a page's text, as links.
pub fn text_links(page: usize, runs: &[TextRun]) -> Vec<Link> {
    let mut out = Vec::new();
    for r in runs {
        let chars: Vec<char> = r.text.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            // A token: up to the next space.
            let start = i;
            while i < chars.len() && chars[i] != ' ' {
                i += 1;
            }
            let (mut a, mut b) = (start, i);
            i += 1;
            // Opening and closing punctuation around the address.
            while a < b && matches!(chars[a], '(' | '[' | '<' | '"' | '\'' | '\u{201c}') {
                a += 1;
            }
            while b > a
                && matches!(
                    chars[b - 1],
                    '.' | ',' | ';' | ':' | ')' | ']' | '>' | '"' | '\'' | '!' | '?' | '\u{201d}'
                )
            {
                b -= 1;
            }
            if b <= a {
                continue;
            }
            let tok: String = chars[a..b].iter().collect();
            let Some(uri) = as_uri(&tok) else { continue };
            let x0 = r.char_rect(a).x0;
            let x1 = r.char_rect(b - 1).x1;
            out.push(Link {
                page,
                rect: Rect::new(x0, r.rect.y0, x1, r.rect.y1),
                target: Target::Uri(uri),
            });
        }
    }
    out
}

/// The address a printed token stands for, if it is one.
fn as_uri(tok: &str) -> Option<String> {
    let lower = tok.to_lowercase();
    if (lower.starts_with("https://") || lower.starts_with("http://")) && tok.len() > 10 {
        return Some(tok.to_string());
    }
    if lower.starts_with("www.") && tok[4..].contains('.') && tok.len() > 7 {
        return Some(format!("https://{tok}"));
    }
    // name@domain.tld
    let (user, domain) = tok.split_once('@')?;
    let ok_user = !user.is_empty()
        && user
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '-' | '+'));
    let ok_domain = domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && domain
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '.' | '-'));
    (ok_user && ok_domain).then(|| format!("mailto:{tok}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str) -> TextRun {
        let xs: Vec<f32> = (0..text.chars().count()).map(|i| i as f32 * 5.0).collect();
        let ws = vec![5.0; xs.len()];
        TextRun {
            rect: Rect::new(0.0, 0.0, xs.len() as f32 * 5.0, 10.0),
            text: text.into(),
            size: 10.0,
            xs,
            ws,
        }
    }

    #[test]
    fn addresses_in_text() {
        let l = text_links(2, &[run("See (https://skatteverket.se/form). Mail info@example.se, or www.pdfr.app")]);
        let uris: Vec<&Target> = l.iter().map(|l| &l.target).collect();
        assert_eq!(
            uris,
            vec![
                &Target::Uri("https://skatteverket.se/form".into()),
                &Target::Uri("mailto:info@example.se".into()),
                &Target::Uri("https://www.pdfr.app".into()),
            ]
        );
        // The box covers only the address.
        assert!((l[0].rect.x0 - 25.0).abs() < 1e-3, "{:?}", l[0].rect);
        assert_eq!(l[0].page, 2);
        assert!(text_links(0, &[run("No links: e.g. 3.5 kg @ 10 kr, http://")]).is_empty());
    }
}
