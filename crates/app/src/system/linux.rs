//! Linux desktop integration through freedesktop files: `mimeapps.list`
//! for the default app, desktop entries and hicolor icons for the app menu.

use super::{APP_ID, DefaultApp, ICON_SVG, ICONS, InstallKind, mimeapps, portal};
use std::path::{Path, PathBuf};

const PDF: &str = "application/pdf";
const DESKTOP_ENTRY: &str =
    include_str!("../../../../packaging/linux/io.github.kjellberg.pdfr.desktop");

fn desktop_id() -> String {
    format!("{APP_ID}.desktop")
}

fn is_pdfr(id: &str) -> bool {
    id == desktop_id() || id == "pdfr.desktop" || id == "pdfr-bin.desktop"
}

fn in_flatpak() -> bool {
    Path::new("/.flatpak-info").exists()
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn env_dir(var: &str, fallback: &str) -> Option<PathBuf> {
    match std::env::var_os(var) {
        Some(v) if !v.is_empty() && Path::new(&v).is_absolute() => Some(PathBuf::from(v)),
        _ => home().map(|h| h.join(fallback)),
    }
}

fn env_dirs(var: &str, fallback: &str) -> Vec<PathBuf> {
    let v = std::env::var(var)
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| fallback.to_string());
    v.split(':')
        .filter(|s| s.starts_with('/'))
        .map(PathBuf::from)
        .collect()
}

fn config_home() -> Option<PathBuf> {
    env_dir("XDG_CONFIG_HOME", ".config")
}

fn data_home() -> Option<PathBuf> {
    env_dir("XDG_DATA_HOME", ".local/share")
}

/// Where desktop entries are looked up, most important first.
fn application_dirs() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = data_home().into_iter().collect();
    v.extend(env_dirs("XDG_DATA_DIRS", "/usr/local/share:/usr/share"));
    v.into_iter().map(|d| d.join("applications")).collect()
}

/// `mimeapps.list` files in the order the spec says to read them.
fn mimeapps_files() -> Vec<PathBuf> {
    let desktops: Vec<String> = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .collect();
    let mut dirs: Vec<PathBuf> = config_home().into_iter().collect();
    dirs.extend(env_dirs("XDG_CONFIG_DIRS", "/etc/xdg"));
    dirs.extend(application_dirs());
    let mut out = Vec::new();
    for d in dirs {
        for de in &desktops {
            out.push(d.join(format!("{de}-mimeapps.list")));
        }
        out.push(d.join("mimeapps.list"));
    }
    out
}

fn find_desktop_file(id: &str) -> Option<PathBuf> {
    application_dirs()
        .into_iter()
        .map(|d| d.join(id))
        .find(|p| p.is_file())
}

pub fn default_pdf_app() -> DefaultApp {
    for f in mimeapps_files() {
        let Ok(text) = std::fs::read_to_string(&f) else {
            continue;
        };
        // The first listed app that is installed wins.
        for id in mimeapps::defaults_for(&text, PDF) {
            if is_pdfr(&id) && (find_desktop_file(&id).is_some() || in_flatpak()) {
                return DefaultApp::Pdfr;
            }
            if let Some(path) = find_desktop_file(&id) {
                let name = std::fs::read_to_string(path)
                    .ok()
                    .and_then(|t| mimeapps::desktop_name(&t))
                    .unwrap_or(id);
                return DefaultApp::Other(name);
            }
        }
    }
    DefaultApp::Unknown
}

pub fn in_app_menu() -> bool {
    in_flatpak() || find_desktop_file(&desktop_id()).is_some()
}

pub fn can_make_default() -> bool {
    !in_flatpak()
}

pub fn make_default() -> Result<String, String> {
    if in_flatpak() {
        return Err(
            "Choose pdfr for PDF files in your desktop's settings, under Default Applications."
                .into(),
        );
    }
    if find_desktop_file(&desktop_id()).is_none() {
        return Err(
            "pdfr isn't in your app menu yet. Add it in Settings \u{2192} General first.".into(),
        );
    }
    let dir = config_home().ok_or("no home folder")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let main = dir.join("mimeapps.list");
    let mut targets = vec![main];
    // A desktop-specific file in the same folder would override ours.
    for f in mimeapps_files() {
        if f.parent() == Some(dir.as_path())
            && !targets.contains(&f)
            && std::fs::read_to_string(&f)
                .is_ok_and(|t| !mimeapps::defaults_for(&t, PDF).is_empty())
        {
            targets.push(f);
        }
    }
    for f in targets {
        let old = std::fs::read_to_string(&f).unwrap_or_default();
        let new = mimeapps::set_default(&old, PDF, &desktop_id());
        write_atomic(&f, new.as_bytes())
            .map_err(|e| format!("could not update {}: {e}", f.display()))?;
    }
    Ok("PDF files now open with pdfr.".into())
}

/// Takes pdfr out of the user's PDF default, so the desktop goes back to the
/// app it would pick otherwise. `Ok` says which one that is.
pub fn unset_default() -> Result<String, String> {
    if in_flatpak() {
        return Err(
            "Choose another app for PDF files in your desktop's settings, under Default Applications."
                .into(),
        );
    }
    let dir = config_home().ok_or("no home folder")?;
    for f in mimeapps_files() {
        if f.parent() != Some(dir.as_path()) {
            continue;
        }
        let Ok(old) = std::fs::read_to_string(&f) else {
            continue;
        };
        if let Some(new) = mimeapps::remove_default(&old, PDF, is_pdfr) {
            write_atomic(&f, new.as_bytes())
                .map_err(|e| format!("could not update {}: {e}", f.display()))?;
        }
    }
    match default_pdf_app() {
        DefaultApp::Other(name) => Ok(format!("PDF files now open with {name}.")),
        DefaultApp::Unknown => Ok("pdfr is no longer the default PDF app.".into()),
        DefaultApp::Pdfr => Err(
            "Your system still picks pdfr. Choose another app for PDF files in your desktop's settings."
                .into(),
        ),
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("pdfr-tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

pub fn open_url(url: &str) -> Result<(), String> {
    portal::open_uri(url)
}

pub fn install_kind() -> InstallKind {
    if in_flatpak() {
        return InstallKind::Flatpak;
    }
    let exe = std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .unwrap_or_default();
    let s = exe.to_string_lossy();
    if s.contains("/Cellar/") || s.contains("/linuxbrew/") || s.contains("/homebrew/") {
        return InstallKind::Homebrew;
    }
    if !(s.starts_with("/usr/") || s.starts_with("/opt/")) {
        return InstallKind::Direct;
    }
    let pacman = std::fs::read_dir("/var/lib/pacman/local").is_ok_and(|mut d| {
        d.any(|e| {
            e.is_ok_and(|e| {
                // "pdfr-0.1.0-1" or "pdfr-bin-0.1.0-1".
                e.file_name()
                    .to_string_lossy()
                    .strip_prefix("pdfr-")
                    .is_some_and(|r| {
                        r.starts_with(|c: char| c.is_ascii_digit()) || r.starts_with("bin-")
                    })
            })
        })
    });
    if pacman {
        InstallKind::Pacman
    } else if Path::new("/var/lib/dpkg/info/pdfr.list").exists() {
        InstallKind::Dpkg
    } else if Path::new("/var/lib/rpm").is_dir() || Path::new("/usr/lib/sysimage/rpm").is_dir() {
        InstallKind::Rpm
    } else {
        InstallKind::Package
    }
}

/// A path quoted for a desktop entry's `Exec` key.
fn exec_quote(p: &Path) -> String {
    let s = p.to_string_lossy();
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-+".contains(c))
    {
        return s.into_owned();
    }
    let mut q = String::from("\"");
    for c in s.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            q.push('\\');
        }
        q.push(c);
    }
    q.push('"');
    // The desktop file format itself escapes backslashes once more.
    q.replace('\\', "\\\\")
}

/// The packaged desktop entry, pointed at this executable.
fn desktop_entry_for(exe: &Path) -> String {
    let exec = exec_quote(exe);
    DESKTOP_ENTRY
        .lines()
        .map(|l| {
            if let Some(rest) = l.strip_prefix("Exec=pdfr") {
                format!("Exec={exec}{rest}")
            } else if l.starts_with("TryExec=") {
                format!("TryExec={}", exe.display())
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn local_bin() -> Option<PathBuf> {
    home().map(|h| h.join(".local/bin"))
}

fn icon_paths(data: &Path) -> Vec<(PathBuf, &'static [u8])> {
    let base = data.join("icons/hicolor");
    let mut v: Vec<(PathBuf, &'static [u8])> = ICONS
        .iter()
        .map(|(s, png)| (base.join(format!("{s}x{s}/apps/{APP_ID}.png")), *png))
        .collect();
    v.push((base.join(format!("scalable/apps/{APP_ID}.svg")), ICON_SVG));
    v
}

pub fn register() -> Result<String, String> {
    if in_flatpak() {
        return Err("The Flatpak version is already in your app menu.".into());
    }
    let exe = std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .map_err(|e| format!("cannot find pdfr itself: {e}"))?;
    let data = data_home().ok_or("no home folder")?;
    let apps = data.join("applications");
    std::fs::create_dir_all(&apps).map_err(|e| e.to_string())?;
    for (path, bytes) in icon_paths(&data) {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    write_atomic(&apps.join(desktop_id()), desktop_entry_for(&exe).as_bytes())
        .map_err(|e| e.to_string())?;
    let mut done = vec!["added pdfr to the app menu and \u{201c}Open with\u{201d}".to_string()];
    if let Some(bin) = local_bin() {
        let link = bin.join("pdfr");
        if link != exe {
            std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
            if link.symlink_metadata().is_ok() {
                // Only replace our own earlier link.
                if std::fs::read_link(&link).is_ok() {
                    let _ = std::fs::remove_file(&link);
                }
            }
            match std::os::unix::fs::symlink(&exe, &link) {
                Ok(()) => {
                    let on_path = std::env::var_os("PATH")
                        .is_some_and(|p| std::env::split_paths(&p).any(|d| d == bin));
                    done.push(if on_path {
                        "the pdfr command works in a terminal".into()
                    } else {
                        format!(
                            "linked the pdfr command in {} (add it to your PATH)",
                            bin.display()
                        )
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(format!("could not link the pdfr command: {e}")),
            }
        }
    }
    Ok(capitalise_first(&done.join("; ")) + ".")
}

pub fn unregister() -> Result<String, String> {
    let data = data_home().ok_or("no home folder")?;
    let _ = std::fs::remove_file(data.join("applications").join(desktop_id()));
    for (path, _) in icon_paths(&data) {
        let _ = std::fs::remove_file(path);
    }
    if let Some(link) = local_bin().map(|b| b.join("pdfr"))
        && std::fs::read_link(&link).is_ok()
    {
        let _ = std::fs::remove_file(link);
    }
    Ok("Removed pdfr from the app menu and the terminal.".into())
}

fn capitalise_first(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_paths_are_quoted() {
        assert_eq!(exec_quote(Path::new("/opt/pdfr/pdfr")), "/opt/pdfr/pdfr");
        assert_eq!(
            exec_quote(Path::new("/home/a b/pdfr")),
            "\"/home/a b/pdfr\""
        );
        assert_eq!(exec_quote(Path::new("/x/$y/pdfr")), "\"/x/\\\\$y/pdfr\"");
    }

    #[test]
    fn desktop_entry_points_at_the_binary() {
        let e = desktop_entry_for(Path::new("/home/u/apps/pdfr"));
        assert!(e.contains("\nExec=/home/u/apps/pdfr %f\n"));
        assert!(e.contains("\nTryExec=/home/u/apps/pdfr\n"));
        // The "New Window" action too.
        assert!(e.contains("\nExec=/home/u/apps/pdfr\n"));
        assert!(e.contains(&format!("Icon={APP_ID}")));
    }
}
