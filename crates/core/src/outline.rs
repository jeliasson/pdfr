//! The document's outline (bookmarks): titles in a tree, each going to a
//! place in the document.

use crate::field::ObjId;
use crate::forms::deref;
use crate::geom::PageGeom;
use crate::links::{Pages, Target, resolve_dest};
use lopdf::{Dictionary, Document, Object};

#[derive(Clone, Debug, PartialEq)]
pub struct OutlineItem {
    pub title: String,
    /// 0 for top-level entries.
    pub depth: usize,
    /// Where it goes (`None` when the entry has no usable destination).
    pub target: Option<Target>,
    /// Whether the file shows it opened (its children listed).
    pub open: bool,
    /// Whether it has children.
    pub has_children: bool,
}

/// Most entries read (a damaged file could loop or be huge).
const MAX_ITEMS: usize = 5000;

/// The outline in reading order, flattened with depths.
pub(crate) fn read(d: &Document, ids: &[ObjId], geoms: &[PageGeom]) -> Vec<OutlineItem> {
    let pages = Pages { ids, geoms };
    let Some(root) = d
        .catalog()
        .ok()
        .and_then(|c| c.get(b"Outlines").ok())
        .and_then(|o| deref(d, o))
        .and_then(|o| o.as_dict().ok())
    else {
        return vec![];
    };
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    walk(d, root, 0, &pages, &mut out, &mut seen);
    out
}

fn first_child(d: &Document, node: &Dictionary) -> Option<ObjId> {
    node.get(b"First").ok()?.as_reference().ok().filter(|id| d.get_dictionary(*id).is_ok())
}

fn walk(
    d: &Document,
    parent: &Dictionary,
    depth: usize,
    pages: &Pages,
    out: &mut Vec<OutlineItem>,
    seen: &mut std::collections::HashSet<ObjId>,
) {
    if depth > 32 {
        return;
    }
    let mut cur = first_child(d, parent);
    while let Some(id) = cur {
        if out.len() >= MAX_ITEMS || !seen.insert(id) {
            return;
        }
        let Ok(node) = d.get_dictionary(id) else { return };
        let title = node
            .get(b"Title")
            .ok()
            .and_then(|o| deref(d, o))
            .and_then(|o| lopdf::decode_text_string(o).ok())
            .map(|t| t.split_whitespace().collect::<Vec<_>>().join(" "))
            .unwrap_or_default();
        let target = node
            .get(b"Dest")
            .ok()
            .and_then(|dest| resolve_dest(d, dest, pages, 0))
            .or_else(|| {
                let a = deref(d, node.get(b"A").ok()?)?.as_dict().ok()?;
                match deref(d, a.get(b"S").ok()?)? {
                    Object::Name(n) if n == b"GoTo" => resolve_dest(d, a.get(b"D").ok()?, pages, 0),
                    Object::Name(n) if n == b"URI" => {
                        let u = deref(d, a.get(b"URI").ok()?)?.as_str().ok()?;
                        Some(Target::Uri(String::from_utf8_lossy(u).into_owned()))
                    }
                    _ => None,
                }
            });
        let count = node
            .get(b"Count")
            .ok()
            .and_then(|o| deref(d, o))
            .and_then(|o| o.as_i64().ok())
            .unwrap_or(0);
        let has_children = first_child(d, node).is_some();
        out.push(OutlineItem {
            title,
            depth,
            target,
            open: count > 0,
            has_children,
        });
        if has_children {
            walk(d, node, depth + 1, pages, out, seen);
        }
        cur = node.get(b"Next").ok().and_then(|o| o.as_reference().ok());
    }
}
