//! Desktop integration: the default PDF app, how pdfr was installed (for
//! update hints), the `pdfr` command, registering a tarball install with the
//! desktop, opening links, and the desktop's light/dark preference.
//!
//! No external programs: on Linux everything goes through files defined by
//! the freedesktop specifications and the XDG desktop portals (D-Bus).

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "linux")]
pub mod mimeapps;
#[cfg(target_os = "linux")]
pub mod portal;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
use linux as os;
#[cfg(target_os = "macos")]
use macos as os;
#[cfg(windows)]
use windows as os;

/// The reverse-DNS id: Wayland app id, desktop file, icon and AppStream id.
pub const APP_ID: &str = "io.github.kjellberg.pdfr";
pub const RELEASES_URL: &str = "https://github.com/kjellberg/pdfr/releases";

/// Installs pdfr, or updates it in place, on Linux and macOS
/// (`packaging/install.sh`, served at pdfr.app).
pub const INSTALL_COMMAND: &str = "curl -fsSL https://pdfr.app/install.sh | sh";

/// The icon, pre-rendered at the sizes desktops ask for.
pub const ICONS: &[(u32, &[u8])] = &[
    (
        16,
        include_bytes!("../../../../assets/icon/png/pdfr-16.png"),
    ),
    (
        22,
        include_bytes!("../../../../assets/icon/png/pdfr-22.png"),
    ),
    (
        24,
        include_bytes!("../../../../assets/icon/png/pdfr-24.png"),
    ),
    (
        32,
        include_bytes!("../../../../assets/icon/png/pdfr-32.png"),
    ),
    (
        48,
        include_bytes!("../../../../assets/icon/png/pdfr-48.png"),
    ),
    (
        64,
        include_bytes!("../../../../assets/icon/png/pdfr-64.png"),
    ),
    (
        128,
        include_bytes!("../../../../assets/icon/png/pdfr-128.png"),
    ),
    (
        256,
        include_bytes!("../../../../assets/icon/png/pdfr-256.png"),
    ),
    (
        512,
        include_bytes!("../../../../assets/icon/png/pdfr-512.png"),
    ),
];
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub const ICON_SVG: &[u8] = include_bytes!("../../../../assets/icon/pdfr.svg");

/// The window icon (X11, Windows, an unbundled macOS app; Wayland desktops
/// take the installed icon named by the app id instead).
pub fn window_icon() -> Option<eframe::egui::IconData> {
    let png = ICONS.iter().find(|(s, _)| *s == 256)?.1;
    let mut dec = png::Decoder::new(std::io::Cursor::new(png));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.color_type != png::ColorType::Rgba {
        return None;
    }
    buf.truncate(info.buffer_size());
    Some(eframe::egui::IconData {
        rgba: buf,
        width: info.width,
        height: info.height,
    })
}

/// Which app opens PDFs.
#[cfg_attr(windows, allow(dead_code))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DefaultApp {
    Pdfr,
    /// Another app, by its display name.
    Other(String),
    /// Not set, or this system does not say.
    Unknown,
}

pub fn default_pdf_app() -> DefaultApp {
    os::default_pdf_app()
}

/// Makes pdfr the default PDF app, or opens the system page where the user
/// does it. `Ok` carries what to tell the user.
pub fn make_default() -> Result<String, String> {
    os::make_default()
}

/// Stops pdfr being the default PDF app, or explains where to change it.
pub fn unset_default() -> Result<String, String> {
    os::unset_default()
}

/// Opens a web link in the user's browser.
pub fn open_url(url: &str) -> Result<(), String> {
    os::open_url(url)
}

/// How pdfr was installed, which decides how it is updated.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallKind {
    Pacman,
    Dpkg,
    Rpm,
    /// Some other system package manager.
    Package,
    Homebrew,
    Flatpak,
    /// Installed with `install.sh`, a downloaded archive or disk image, or a
    /// build from source that was copied somewhere. On Linux and macOS the
    /// install command updates it.
    Direct,
    /// Run from a Cargo `target` folder (`cargo run`). Never offers to
    /// register itself, become the default app or check for updates: it
    /// would point the desktop at a binary that changes with every build.
    Dev,
}

impl InstallKind {
    /// Updates come through a package manager.
    pub fn managed(self) -> bool {
        !matches!(self, InstallKind::Direct | InstallKind::Dev)
    }

    /// pdfr may look for new versions itself (after asking). Not for
    /// development builds, and not in Flatpak: the sandbox has no network,
    /// and software centres announce Flatpak updates already.
    pub fn checks_for_updates(self) -> bool {
        !matches!(self, InstallKind::Dev | InstallKind::Flatpak)
    }

    /// How to update, for the "new version" notice.
    pub fn update_hint(self) -> Option<&'static str> {
        Some(match self {
            InstallKind::Pacman => "yay -Syu",
            InstallKind::Dpkg => "sudo apt update && sudo apt upgrade",
            InstallKind::Rpm => "sudo dnf upgrade",
            InstallKind::Homebrew => "brew upgrade pdfr",
            InstallKind::Flatpak => "flatpak update",
            InstallKind::Direct if cfg!(unix) => INSTALL_COMMAND,
            InstallKind::Package | InstallKind::Direct | InstallKind::Dev => return None,
        })
    }
}

pub fn install_kind() -> InstallKind {
    let exe = std::env::current_exe().and_then(|p| p.canonicalize());
    if exe.is_ok_and(|p| in_cargo_target(&p)) {
        return InstallKind::Dev;
    }
    os::install_kind()
}

/// Whether `exe` lies in a Cargo build folder: a `target` folder next to
/// a `Cargo.toml`.
fn in_cargo_target(exe: &std::path::Path) -> bool {
    exe.ancestors().any(|a| {
        a.file_name().is_some_and(|n| n == "target")
            && a.parent().is_some_and(|p| p.join("Cargo.toml").is_file())
    })
}

/// Whether `pdfr` can be typed in a terminal.
pub fn command_on_path() -> bool {
    let exe = if cfg!(windows) { "pdfr.exe" } else { "pdfr" };
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(exe).is_file()))
}

/// Adds pdfr to the app menu, "Open with" and the terminal for this user
/// (for installs from an archive). `Ok` carries what was done.
pub fn register() -> Result<String, String> {
    os::register()
}

/// Undoes [`register`].
pub fn unregister() -> Result<String, String> {
    os::unregister()
}

/// Whether pdfr has an app menu entry (always true where an installer or
/// app bundle provides it).
pub fn in_app_menu() -> bool {
    #[cfg(target_os = "linux")]
    {
        linux::in_app_menu()
    }
    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}

/// Whether the system can show the default-app button at all.
pub fn can_make_default() -> bool {
    os::can_make_default()
}

/// Files the system asked pdfr to open while running (macOS Finder).
pub fn take_opened() -> Vec<std::path::PathBuf> {
    #[cfg(target_os = "macos")]
    {
        macos::take_opened()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}

/// Lets system events (files to open) wake the app.
pub fn set_wake(ctx: &eframe::egui::Context) {
    #[cfg(target_os = "macos")]
    macos::set_wake(ctx);
    #[cfg(not(target_os = "macos"))]
    let _ = ctx;
}

/// Hooks to install before the event loop starts.
pub fn before_event_loop() {
    #[cfg(target_os = "macos")]
    macos::install_open_handler();
}

/// The desktop's light/dark preference, if it states one.
pub fn prefers_dark() -> Option<bool> {
    #[cfg(target_os = "linux")]
    {
        portal::prefers_dark()
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// Starts following the desktop's light/dark preference (Linux portal);
/// repaints when it changes.
pub fn watch_color_scheme(ctx: &eframe::egui::Context) {
    #[cfg(target_os = "linux")]
    portal::watch_color_scheme(ctx.clone());
    #[cfg(not(target_os = "linux"))]
    let _ = ctx;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_icon_decodes() {
        let i = window_icon().expect("icon");
        assert_eq!((i.width, i.height), (256, 256));
        assert_eq!(i.rgba.len(), 256 * 256 * 4);
    }

    #[test]
    fn downloaded_installs_update_with_the_install_line() {
        if cfg!(unix) {
            assert_eq!(InstallKind::Direct.update_hint(), Some(INSTALL_COMMAND));
        }
        assert_eq!(InstallKind::Dev.update_hint(), None);
        assert!(InstallKind::Direct.checks_for_updates());
    }

    #[test]
    fn cargo_builds_are_development_builds() {
        // This test binary itself lives in the workspace's target folder.
        let exe = std::env::current_exe().unwrap().canonicalize().unwrap();
        assert!(in_cargo_target(&exe));
        assert_eq!(install_kind(), InstallKind::Dev);
        assert!(!in_cargo_target(std::path::Path::new("/usr/bin/pdfr")));
        assert!(!in_cargo_target(std::path::Path::new("/tmp/target/pdfr")));
    }
}
