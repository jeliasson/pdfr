//! Reading and editing `mimeapps.list` (freedesktop "Association between
//! MIME types and applications" spec), without touching anything else in
//! the file.

const DEFAULTS: &str = "Default Applications";

/// The desktop file ids set as default for `mime` in one file's text.
pub fn defaults_for(text: &str, mime: &str) -> Vec<String> {
    let mut in_section = false;
    for line in text.lines() {
        let t = line.trim();
        if let Some(name) = t.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            in_section = name == DEFAULTS;
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some((k, v)) = t.split_once('=')
            && k.trim() == mime
        {
            return v
                .split(';')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect();
        }
    }
    Vec::new()
}

/// `text` with `mime` set to `id` in `[Default Applications]`, which is
/// created if missing. Every other line is kept as it was.
pub fn set_default(text: &str, mime: &str, id: &str) -> String {
    let entry = format!("{mime}={id};");
    let mut out: Vec<String> = Vec::new();
    let mut in_section = false;
    let mut section_seen = false;
    let mut done = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') {
            if in_section && !done {
                // End of our section without the key: add it before the
                // blank lines that separate sections.
                let at = out
                    .iter()
                    .rposition(|l| !l.trim().is_empty())
                    .map_or(out.len(), |i| i + 1);
                out.insert(at, entry.clone());
                done = true;
            }
            in_section = &t[1..t.len() - 1] == DEFAULTS;
            section_seen |= in_section;
            out.push(line.to_string());
            continue;
        }
        if in_section
            && let Some((k, _)) = t.split_once('=')
            && k.trim() == mime
        {
            if !done {
                out.push(entry.clone());
                done = true;
            }
            continue;
        }
        out.push(line.to_string());
    }
    if !done {
        if !section_seen {
            if out.last().is_some_and(|l| !l.trim().is_empty()) {
                out.push(String::new());
            }
            out.push(format!("[{DEFAULTS}]"));
        }
        out.push(entry);
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

/// `text` without the ids `drop` picks from `mime`'s default entry (the
/// line goes if nothing is left), or None if there was nothing to remove.
pub fn remove_default(text: &str, mime: &str, drop: impl Fn(&str) -> bool) -> Option<String> {
    let mut out: Vec<String> = Vec::new();
    let mut in_section = false;
    let mut changed = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') {
            in_section = &t[1..t.len() - 1] == DEFAULTS;
        } else if in_section
            && let Some((k, v)) = t.split_once('=')
            && k.trim() == mime
        {
            let ids: Vec<&str> = v.split(';').map(str::trim).filter(|s| !s.is_empty()).collect();
            let keep: Vec<&str> = ids.iter().copied().filter(|id| !drop(id)).collect();
            if keep.len() != ids.len() {
                changed = true;
                if !keep.is_empty() {
                    out.push(format!("{mime}={};", keep.join(";")));
                }
                continue;
            }
        }
        out.push(line.to_string());
    }
    changed.then(|| {
        let mut s = out.join("\n");
        s.push('\n');
        s
    })
}

/// The `Name=` of a desktop file (the untranslated one).
pub fn desktop_name(text: &str) -> Option<String> {
    let mut in_entry = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_entry = t == "[Desktop Entry]";
            continue;
        }
        if in_entry && let Some(v) = t.strip_prefix("Name=") {
            return Some(v.trim().to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const PDF: &str = "application/pdf";

    #[test]
    fn reads_defaults() {
        let t = "[Added Associations]\napplication/pdf=a.desktop;\n\n[Default Applications]\ntext/html=firefox.desktop\napplication/pdf=org.gnome.Evince.desktop;b.desktop;\n";
        assert_eq!(
            defaults_for(t, PDF),
            vec!["org.gnome.Evince.desktop", "b.desktop"]
        );
        assert!(defaults_for("[Added Associations]\napplication/pdf=a.desktop;\n", PDF).is_empty());
    }

    #[test]
    fn replaces_existing_entry_and_keeps_the_rest() {
        let t = "# mine\n[Default Applications]\ntext/html=firefox.desktop\napplication/pdf=evince.desktop;\nimage/png=imv.desktop\n\n[Added Associations]\nx/y=z.desktop;\n";
        let s = set_default(t, PDF, "io.github.kjellberg.pdfr.desktop");
        assert_eq!(
            s,
            "# mine\n[Default Applications]\ntext/html=firefox.desktop\napplication/pdf=io.github.kjellberg.pdfr.desktop;\nimage/png=imv.desktop\n\n[Added Associations]\nx/y=z.desktop;\n"
        );
    }

    #[test]
    fn adds_to_section_before_the_next_one() {
        let t = "[Default Applications]\ntext/html=firefox.desktop\n\n[Added Associations]\nx/y=z.desktop;\n";
        let s = set_default(t, PDF, "p.desktop");
        assert_eq!(
            s,
            "[Default Applications]\ntext/html=firefox.desktop\napplication/pdf=p.desktop;\n\n[Added Associations]\nx/y=z.desktop;\n"
        );
        assert_eq!(defaults_for(&s, PDF), vec!["p.desktop"]);
    }

    #[test]
    fn creates_the_section_and_the_file() {
        assert_eq!(
            set_default("", PDF, "p.desktop"),
            "[Default Applications]\napplication/pdf=p.desktop;\n"
        );
        assert_eq!(
            set_default("[Added Associations]\nx/y=z.desktop;", PDF, "p.desktop"),
            "[Added Associations]\nx/y=z.desktop;\n\n[Default Applications]\napplication/pdf=p.desktop;\n"
        );
    }

    #[test]
    fn duplicate_keys_collapse_to_one() {
        let t = "[Default Applications]\napplication/pdf=a.desktop\napplication/pdf=b.desktop\n";
        assert_eq!(
            set_default(t, PDF, "p.desktop"),
            "[Default Applications]\napplication/pdf=p.desktop;\n"
        );
    }

    #[test]
    fn removes_only_pdfr_from_the_default() {
        let t = "[Default Applications]\napplication/pdf=pdfr.desktop;org.gnome.Evince.desktop;\ntext/plain=gedit.desktop;\n";
        let r = remove_default(t, PDF, |id| id == "pdfr.desktop").unwrap();
        assert_eq!(r, "[Default Applications]\napplication/pdf=org.gnome.Evince.desktop;\ntext/plain=gedit.desktop;\n");
        let only = "[Default Applications]\napplication/pdf=pdfr.desktop;\n[Added Associations]\napplication/pdf=pdfr.desktop;\n";
        let r = remove_default(only, PDF, |id| id == "pdfr.desktop").unwrap();
        assert_eq!(r, "[Default Applications]\n[Added Associations]\napplication/pdf=pdfr.desktop;\n");
        assert_eq!(remove_default(&r, PDF, |id| id == "pdfr.desktop"), None);
    }

    #[test]
    fn desktop_names() {
        let t = "[Desktop Entry]\nName[sv]=Dokument\nName=Document Viewer\n[Desktop Action x]\nName=Other\n";
        assert_eq!(desktop_name(t).as_deref(), Some("Document Viewer"));
    }
}
