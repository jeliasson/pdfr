//! Interface theming. One small module: palette sources (Omarchy, built-in
//! light/dark) mapped onto the interface only. The PDF page and typed text
//! are never themed: the paper stays as the PDF defines it and text is black.

use eframe::egui::{self, Color32, Visuals};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    pub name: String,
    pub dark: bool,
    pub window: Color32,
    pub toolbar: Color32,
    pub text: Color32,
    pub weak_text: Color32,
    pub button: Color32,
    /// Area around the pages.
    pub canvas: Color32,
    /// Highlights: active toolbar tool, drag preview.
    pub accent: Color32,
    /// Tint of fillable spots, drawn over the paper (screen only).
    pub empty_tint: Color32,
    pub selection: Color32,
    pub caret: Color32,
}

/// Relative luminance (WCAG).
pub fn luminance(c: Color32) -> f32 {
    let f = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
}

pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// Alpha-blends `c` (with alpha) over an opaque background.
pub fn over(c: Color32, bg: Color32) -> Color32 {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    mix(bg, Color32::from_rgb(r, g, b), a as f32 / 255.0)
}

/// A tint of `accent` that stays light enough over white paper for black
/// text to read (contrast ≥ 12:1) yet visible (at least a small difference).
fn paper_tint(accent: Color32, strength: f32) -> Color32 {
    let [r, g, b, _] = accent.to_srgba_unmultiplied();
    // Grey or very dark accents give a weak tint; push towards a soft blue.
    let base = if luminance(accent) < 0.02 || (r.max(g).max(b) - r.min(g).min(b)) < 20 {
        Color32::from_rgb(70, 120, 210)
    } else {
        accent
    };
    let mut a = strength;
    loop {
        let c = Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), (a * 255.0) as u8);
        let on_paper = over(c, Color32::WHITE);
        if contrast(Color32::BLACK, on_paper) >= 12.0 || a <= 0.06 {
            return c;
        }
        a -= 0.02;
    }
}

/// Makes sure text is readable on `bg`, switching to black or white if not.
fn readable(text: Color32, bg: Color32, min: f32) -> Color32 {
    if contrast(text, bg) >= min {
        return text;
    }
    if contrast(Color32::BLACK, bg) > contrast(Color32::WHITE, bg) {
        Color32::BLACK
    } else {
        Color32::WHITE
    }
}

impl Palette {
    pub fn builtin(dark: bool) -> Palette {
        if dark {
            Self::from_parts(
                "built-in dark",
                true,
                hex("#1e1f22").unwrap(),
                hex("#2b2d31").unwrap(),
                hex("#16171a").unwrap(),
                hex("#e3e5e8").unwrap(),
                hex("#5b8def").unwrap(),
                hex("#3d4f73").unwrap(),
                hex("#3a3c42").unwrap(),
            )
        } else {
            Self::from_parts(
                "built-in light",
                false,
                hex("#f4f4f5").unwrap(),
                hex("#e7e7ea").unwrap(),
                hex("#d4d4d8").unwrap(),
                hex("#18181b").unwrap(),
                hex("#2563eb").unwrap(),
                hex("#bfd3fb").unwrap(),
                hex("#dcdce0").unwrap(),
            )
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn from_parts(
        name: &str,
        dark: bool,
        window: Color32,
        toolbar: Color32,
        canvas: Color32,
        text: Color32,
        accent: Color32,
        selection: Color32,
        button: Color32,
    ) -> Palette {
        let text_t = readable(text, toolbar, 4.5);
        // The accent outlines a field on white paper: keep it visible there.
        let accent_on_paper = if contrast(accent, Color32::WHITE) < 2.2 {
            mix(accent, Color32::from_rgb(20, 60, 160), 0.6)
        } else {
            accent
        };
        let sel = {
            // Selection is drawn behind black text on paper; keep it light.
            let [r, g, b, _] = accent_on_paper.to_srgba_unmultiplied();
            Color32::from_rgba_unmultiplied(r, g, b, 90)
        };
        let _ = selection;
        Palette {
            name: name.to_string(),
            dark,
            window,
            toolbar,
            text: text_t,
            weak_text: mix(text_t, toolbar, 0.35),
            button,
            canvas,
            accent: accent_on_paper,
            empty_tint: paper_tint(accent, 0.16),
            selection: sel,
            caret: Color32::BLACK,
        }
    }

    pub fn visuals(&self) -> Visuals {
        let mut v = if self.dark {
            Visuals::dark()
        } else {
            Visuals::light()
        };
        v.panel_fill = self.toolbar;
        v.window_fill = self.window;
        v.extreme_bg_color = self.canvas;
        v.faint_bg_color = self.window;
        v.override_text_color = Some(self.text);
        v.selection.bg_fill = self.accent.gamma_multiply(0.6);
        v.hyperlink_color = self.accent;
        v.widgets.inactive.weak_bg_fill = self.button;
        v.widgets.inactive.bg_fill = self.button;
        v.widgets.hovered.weak_bg_fill = mix(self.button, self.accent, 0.25);
        v.widgets.active.weak_bg_fill = mix(self.button, self.accent, 0.45);
        v.text_cursor.stroke.color = self.accent;
        v
    }
}

/// Parses `#rgb`, `#rrggbb` or `#rrggbbaa` (alpha ignored).
pub fn hex(s: &str) -> Option<Color32> {
    let s = s.trim().trim_start_matches('#');
    let p = |i: usize, n: usize| u8::from_str_radix(s.get(i..i + n)?, 16).ok();
    match s.len() {
        3 => Some(Color32::from_rgb(
            p(0, 1)? * 17,
            p(1, 1)? * 17,
            p(2, 1)? * 17,
        )),
        6 | 8 => Some(Color32::from_rgb(p(0, 2)?, p(2, 2)?, p(4, 2)?)),
        _ => None,
    }
}

/// Minimal `key = "value"` reader for Omarchy's colors.toml.
fn parse_kv(text: &str) -> std::collections::HashMap<String, String> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            if l.is_empty() || l.starts_with('#') || l.starts_with('[') {
                return None;
            }
            let (k, v) = l.split_once('=')?;
            let v = v.trim();
            let v = match v.strip_prefix('"') {
                Some(rest) => rest.split('"').next()?,
                None => v.split_whitespace().next()?,
            };
            Some((k.trim().to_string(), v.to_string()))
        })
        .collect()
}

/// Builds a palette from Omarchy's `colors.toml`. `light_marker` is true if a
/// `light.mode` file sits next to it (older Omarchy versions).
pub fn from_omarchy(text: &str, light_marker: bool, name: &str) -> Option<Palette> {
    let kv = parse_kv(text);
    let c = |k: &str| kv.get(k).and_then(|v| hex(v));
    let bg = c("background")?;
    let fg = c("foreground")?;
    let dark = match kv.get("mode").map(|s| s.as_str()) {
        Some("light") => false,
        Some("dark") => true,
        _ => !light_marker && luminance(bg) < 0.4,
    };
    let toward = if dark {
        Color32::BLACK
    } else {
        Color32::from_gray(90)
    };
    let toolbar = c("dark_background").unwrap_or_else(|| mix(bg, toward, 0.08));
    let canvas = c("darker_background").unwrap_or_else(|| mix(bg, toward, 0.18));
    let button = c("lighter_background").unwrap_or_else(|| mix(bg, fg, 0.12));
    let accent = c("accent")
        .or_else(|| c("blue"))
        .or_else(|| c("color4"))
        .unwrap_or(Color32::from_rgb(80, 130, 220));
    let selection = c("selection")
        .or_else(|| c("selection_background"))
        .unwrap_or_else(|| mix(bg, accent, 0.4));
    Some(Palette::from_parts(
        name, dark, bg, toolbar, canvas, fg, accent, selection, button,
    ))
}

/// Candidate Omarchy theme folders, newest location first.
pub fn omarchy_dirs() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return vec![];
    };
    vec![
        home.join(".local/state/omarchy/current/theme"),
        home.join(".config/omarchy/current/theme"),
    ]
}

/// Reads the active Omarchy theme, if there is one. Never fails loudly.
pub fn load_omarchy() -> Option<Palette> {
    load_from(&omarchy_dirs())
}

pub fn load_from(dirs: &[PathBuf]) -> Option<Palette> {
    for dir in dirs {
        let Ok(text) = std::fs::read_to_string(dir.join("colors.toml")) else {
            continue;
        };
        let light = dir.join("light.mode").exists();
        let name = std::fs::read_to_string(dir.parent().unwrap_or(dir).join("theme.name"))
            .map(|s| format!("Omarchy: {}", s.trim()))
            .unwrap_or_else(|_| "Omarchy".into());
        if let Some(p) = from_omarchy(&text, light, &name) {
            return Some(p);
        }
    }
    None
}

/// A fingerprint of the theme files: changes when the folder is replaced,
/// re-symlinked or edited.
fn theme_stamp(dirs: &[PathBuf]) -> Vec<u8> {
    let mut out = Vec::new();
    for dir in dirs {
        if let Ok(b) = std::fs::read(dir.join("colors.toml")) {
            out.extend_from_slice(&b);
        }
        out.push(dir.join("light.mode").exists() as u8);
    }
    out
}

/// Polls the theme files every 400 ms and sends a new palette when they
/// change. Polling (rather than inotify) handles the folder being deleted and
/// replaced, or re-symlinked, without special cases.
pub fn watch(tx: Sender<Option<Palette>>, ctx: egui::Context) {
    watch_dirs(omarchy_dirs(), tx, move || ctx.request_repaint());
}

pub fn watch_dirs(
    dirs: Vec<PathBuf>,
    tx: Sender<Option<Palette>>,
    wake: impl Fn() + Send + 'static,
) {
    std::thread::Builder::new()
        .name("theme-watch".into())
        .spawn(move || {
            let mut last = theme_stamp(&dirs);
            loop {
                std::thread::sleep(Duration::from_millis(400));
                let now = theme_stamp(&dirs);
                if now != last {
                    last = now;
                    if tx.send(load_from(&dirs)).is_err() {
                        return;
                    }
                    wake();
                }
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme(name: &str) -> Option<String> {
        std::fs::read_to_string(format!("/usr/share/omarchy/themes/{name}/colors.toml")).ok()
    }

    fn check(p: &Palette) {
        assert!(
            contrast(p.text, p.toolbar) >= 4.5,
            "{}: toolbar text {:.2}",
            p.name,
            contrast(p.text, p.toolbar)
        );
        let tint = over(p.empty_tint, Color32::WHITE);
        assert!(
            contrast(Color32::BLACK, tint) >= 12.0,
            "{}: tint too dark",
            p.name
        );
        assert!(tint != Color32::WHITE, "{}: tint invisible", p.name);
        assert!(
            contrast(p.accent, Color32::WHITE) >= 2.2,
            "{}: accent invisible on paper",
            p.name
        );
    }

    #[test]
    fn builtin_palettes_are_readable() {
        check(&Palette::builtin(true));
        check(&Palette::builtin(false));
    }

    #[test]
    fn parses_current_omarchy_format() {
        let p = from_omarchy("mode = \"dark\"\naccent = \"#81a1c1\"\nbackground = \"#2e3440\"\nforeground = \"#d8dee9\"\n", false, "t").unwrap();
        assert!(p.dark);
        assert_eq!(p.window, hex("#2e3440").unwrap());
        check(&p);
        // Older format: no mode key, light.mode marker file.
        let p = from_omarchy(
            "background = \"#fafafa\"\nforeground = \"#222222\"\n",
            true,
            "t",
        )
        .unwrap();
        assert!(!p.dark);
    }

    #[test]
    fn malformed_files_fall_back() {
        assert!(from_omarchy("", false, "t").is_none());
        assert!(from_omarchy("background = 12\nforeground = \"#zzzzzz\"", false, "t").is_none());
        assert!(from_omarchy("\u{0}\u{1}garbage", false, "t").is_none());
    }

    #[test]
    fn live_switch_when_the_folder_is_replaced() {
        let root = std::env::temp_dir().join(format!("pdfr-theme-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let cur = root.join("current/theme");
        std::fs::create_dir_all(&cur).unwrap();
        std::fs::write(cur.join("colors.toml"), "mode = \"dark\"\nbackground = \"#000000\"\nforeground = \"#ffffff\"\naccent = \"#8d8d8d\"\n").unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        watch_dirs(vec![cur.clone()], tx, || {});
        std::thread::sleep(Duration::from_millis(100));
        // Like omarchy-theme-set: build next-theme, delete current, move next into place.
        let next = root.join("current/next-theme");
        std::fs::create_dir_all(&next).unwrap();
        std::fs::write(next.join("colors.toml"), "mode = \"light\"\nbackground = \"#ffffff\"\nforeground = \"#000000\"\naccent = \"#6e6e6e\"\n").unwrap();
        let t = std::time::Instant::now();
        std::fs::remove_dir_all(&cur).unwrap();
        std::fs::rename(&next, &cur).unwrap();
        let mut got = None;
        while t.elapsed() < Duration::from_secs(2) {
            if let Ok(p) = rx.try_recv() {
                got = p;
                if got.is_some() {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let p = got.expect("theme change seen");
        assert!(!p.dark);
        assert!(
            t.elapsed() < Duration::from_secs(1),
            "restyle took {:?}",
            t.elapsed()
        );
        // A malformed file falls back (None = built-in palette), never panics.
        std::fs::write(cur.join("colors.toml"), "garbage = [[[").unwrap();
        let t = std::time::Instant::now();
        let mut fell_back = false;
        while t.elapsed() < Duration::from_secs(2) {
            if let Ok(None) = rx.try_recv() {
                fell_back = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(fell_back);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn darkest_and_lightest_omarchy_themes_are_readable() {
        // Skips silently on machines without Omarchy.
        for name in [
            "vantablack",
            "white",
            "catppuccin-latte",
            "nord",
            "flexoki-light",
            "matte-black",
        ] {
            if let Some(t) = theme(name) {
                let p = from_omarchy(&t, false, name).unwrap();
                check(&p);
                assert_eq!(
                    p.dark,
                    !["white", "catppuccin-latte", "flexoki-light"].contains(&name),
                    "{name}"
                );
            }
        }
    }
}
