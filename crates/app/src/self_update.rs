//! "Update now": pdfr updates itself, the way it was installed.
//!
//! - Installed from the archive or disk image (`install.sh`, a `.tar.gz` or
//!   the `.dmg`): pdfr downloads the release file from GitHub, checks it
//!   against the release's `SHA256SUMS`, whose signature (`SHA256SUMS.sig`,
//!   made by pdfr's CI) must match the pinned release key, and swaps it in.
//!   The old copy is kept until the new one is in place.
//! - Homebrew: `brew upgrade pdfr`.
//! - apt / dnf / zypper: the system asks for the administrator password
//!   (`pkexec`), then the package manager updates pdfr.
//! - The AUR: a terminal opens with `yay -S pdfr` (or paru), because AUR
//!   helpers ask their own questions.
//!
//! Every way ends the same: pdfr waits until the installed pdfr reports the
//! new version (`pdfr --version`), then offers to restart into it.

use crate::system::InstallKind;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

const REPO: &str = "https://github.com/kjellberg/pdfr/releases/download";
/// Release files are tens of megabytes; refuse anything absurd.
const MAX_DOWNLOAD: u64 = 400 << 20;
/// How long to wait for an update in a terminal.
const TERMINAL_WAIT: Duration = Duration::from_secs(30 * 60);

/// How this install can update itself.
#[derive(Clone, Debug, PartialEq)]
pub enum Method {
    /// Download the release and swap it in. `dest` is the folder made from
    /// the Linux archive, or `pdfr.app` on macOS.
    Replace { dest: PathBuf },
    /// Run a command and wait for it (Homebrew, or a package manager behind
    /// the system's password dialog).
    Run { program: PathBuf, args: Vec<String> },
    /// Open a terminal with this command (AUR helpers).
    Terminal { command: String },
}

impl Method {
    /// What the button says.
    pub fn button(&self) -> &'static str {
        match self {
            Method::Terminal { .. } => "Update in terminal",
            _ => "Update now",
        }
    }
}

/// How the installed pdfr can be updated from inside pdfr, if it can.
pub fn method(install: InstallKind) -> Option<Method> {
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    method_for(install, &exe, &|p| which(p))
}

fn method_for(
    install: InstallKind,
    exe: &Path,
    find: &dyn Fn(&str) -> Option<PathBuf>,
) -> Option<Method> {
    match install {
        InstallKind::Direct => replace_dest(exe).map(|dest| Method::Replace { dest }),
        InstallKind::Homebrew => {
            let brew = brew_prefix(exe)
                .map(|p| p.join("bin/brew"))
                .filter(|b| b.is_file())
                .or_else(|| find("brew"))?;
            Some(Method::Run {
                program: brew,
                args: vec!["upgrade".into(), "pdfr".into()],
            })
        }
        InstallKind::Dpkg => Some(Method::Run {
            program: find("pkexec")?,
            args: vec![
                "sh".into(),
                "-c".into(),
                "apt-get update && apt-get install -y --only-upgrade pdfr".into(),
            ],
        }),
        InstallKind::Rpm => {
            let pkexec = find("pkexec")?;
            let args: Vec<String> = if let Some(dnf) = find("dnf") {
                vec![
                    path_str(&dnf),
                    "upgrade".into(),
                    "-y".into(),
                    "--refresh".into(),
                    "pdfr".into(),
                ]
            } else {
                let zypper = find("zypper")?;
                vec![
                    path_str(&zypper),
                    "--non-interactive".into(),
                    "update".into(),
                    "pdfr".into(),
                ]
            };
            Some(Method::Run {
                program: pkexec,
                args,
            })
        }
        InstallKind::Pacman => {
            let helper = ["yay", "paru"].into_iter().find(|h| find(h).is_some())?;
            Some(Method::Terminal {
                command: format!("{helper} -S {}", aur_package()),
            })
        }
        InstallKind::Package | InstallKind::Flatpak | InstallKind::Dev => None,
    }
}

fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// The folder (Linux) or app bundle (macOS) an archive install lives in,
/// when it looks like one pdfr made and can be written to. A pdfr binary
/// copied into some folder of the user's own is never touched.
fn replace_dest(exe: &Path) -> Option<PathBuf> {
    let dir = exe.parent()?;
    let dest = if cfg!(target_os = "macos") {
        // …/pdfr.app/Contents/MacOS/pdfr
        let app = dir.parent()?.parent()?;
        let ok = dir.file_name()? == "MacOS"
            && app.extension()? == "app"
            && app.join("Contents/Info.plist").is_file();
        ok.then(|| app.to_path_buf())?
    } else if cfg!(target_os = "linux") {
        // The archive's layout: pdfr, install.sh, packaging/linux.
        let ok = exe.file_name()? == "pdfr"
            && dir.join("install.sh").is_file()
            && dir.join("packaging/linux").is_dir();
        ok.then(|| dir.to_path_buf())?
    } else {
        return None;
    };
    writable(dest.parent()?).then_some(dest)
}

fn writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".pdfr-write-test-{}", std::process::id()));
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// Homebrew's prefix, from a path inside its Cellar.
fn brew_prefix(exe: &Path) -> Option<PathBuf> {
    let s = exe.to_string_lossy();
    let i = s.find("/Cellar/")?;
    Some(PathBuf::from(&s[..i]))
}

/// Which AUR package is installed: `pdfr` (built here) or `pdfr-bin`.
fn aur_package() -> &'static str {
    let bin = std::fs::read_dir("/var/lib/pacman/local").is_ok_and(|mut d| {
        d.any(|e| e.is_ok_and(|e| e.file_name().to_string_lossy().starts_with("pdfr-bin-")))
    });
    if bin { "pdfr-bin" } else { "pdfr" }
}

pub(crate) fn which(name: &str) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    // Apps started from the Dock or a launcher get a short PATH.
    for d in [
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/home/linuxbrew/.linuxbrew/bin",
        "/usr/bin",
        "/bin",
    ] {
        dirs.push(d.into());
    }
    dirs.into_iter().map(|d| d.join(name)).find(|p| p.is_file())
}

/// The pdfr to start after updating.
fn restart_exe(method: &Method) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    Some(match method {
        Method::Replace { dest } if cfg!(target_os = "macos") => dest.join("Contents/MacOS/pdfr"),
        Method::Replace { dest } => dest.join("pdfr"),
        // The Cellar path names the old version; bin/pdfr follows upgrades.
        Method::Run { .. } if brew_prefix(&exe).is_some() => brew_prefix(&exe)?.join("bin/pdfr"),
        _ => exe,
    })
}

/// Where an update stands.
#[derive(Clone, Debug, PartialEq)]
pub enum Stage {
    Idle,
    /// Working; the text says on what.
    Busy(String),
    /// Installed; the new pdfr is here.
    Ready {
        version: String,
        exe: PathBuf,
    },
    Failed(String),
}

enum Msg {
    Busy(String),
    Ready(String),
    Failed(String),
}

/// An update running on a background thread.
pub struct Updater {
    pub stage: Stage,
    rx: Option<Receiver<Msg>>,
    exe: Option<PathBuf>,
}

impl Default for Updater {
    fn default() -> Self {
        Updater {
            stage: Stage::Idle,
            rx: None,
            exe: None,
        }
    }
}

impl Updater {
    pub fn busy(&self) -> bool {
        matches!(self.stage, Stage::Busy(_))
    }

    /// Starts updating to `version`.
    pub fn start(&mut self, ctx: &eframe::egui::Context, method: Method, version: String) {
        if self.busy() {
            return;
        }
        let Some(exe) = restart_exe(&method) else {
            self.stage = Stage::Failed("pdfr could not find itself".into());
            return;
        };
        let (tx, rx) = channel();
        self.stage = Stage::Busy(match &method {
            Method::Replace { .. } => format!("Downloading pdfr {version}\u{2026}"),
            Method::Terminal { .. } => "Opening a terminal\u{2026}".into(),
            Method::Run { .. } => "Updating\u{2026}".into(),
        });
        self.exe = Some(exe.clone());
        self.rx = Some(rx);
        let ctx = ctx.clone();
        std::thread::Builder::new()
            .name("pdfr-update".into())
            .spawn(move || {
                let tell = |m: Msg| {
                    let _ = tx.send(m);
                    ctx.request_repaint();
                };
                let r = run(&method, &version, &exe, &tx, &ctx);
                tell(match r {
                    Ok(v) => Msg::Ready(v),
                    Err(e) => Msg::Failed(e),
                });
            })
            .ok();
    }

    /// Takes in progress from the background thread.
    pub fn poll(&mut self) {
        let Some(rx) = &self.rx else { return };
        while let Ok(m) = rx.try_recv() {
            match m {
                Msg::Busy(s) => self.stage = Stage::Busy(s),
                Msg::Ready(version) => {
                    self.stage = Stage::Ready {
                        version,
                        exe: self.exe.clone().unwrap_or_default(),
                    };
                    self.rx = None;
                    return;
                }
                Msg::Failed(e) => {
                    self.stage = Stage::Failed(e);
                    self.rx = None;
                    return;
                }
            }
        }
    }
}

fn run(
    method: &Method,
    version: &str,
    exe: &Path,
    tx: &Sender<Msg>,
    ctx: &eframe::egui::Context,
) -> Result<String, String> {
    let say = |s: String| {
        let _ = tx.send(Msg::Busy(s));
        ctx.request_repaint();
    };
    match method {
        Method::Replace { dest } => {
            replace(dest, version, &say)?;
            installed_newer(exe).ok_or_else(|| {
                "the new pdfr was put in place but does not start; run the installer again".into()
            })
        }
        Method::Run { program, args } => {
            let out = Command::new(program)
                .args(args)
                .stdin(Stdio::null())
                .output()
                .map_err(|e| format!("could not run {}: {e}", program.display()))?;
            if !out.status.success() {
                // pkexec: 126 = the password dialog was dismissed.
                if out.status.code() == Some(126) || out.status.code() == Some(127) {
                    return Err("cancelled".into());
                }
                return Err(last_line(&out.stderr)
                    .unwrap_or_else(|| format!("{} failed", program.display())));
            }
            installed_newer(exe)
                .ok_or_else(|| "the package manager has no newer pdfr yet; try again later".into())
        }
        Method::Terminal { command } => {
            open_terminal(command)?;
            say("Finish the update in the terminal\u{2026}".into());
            let start = Instant::now();
            while start.elapsed() < TERMINAL_WAIT {
                std::thread::sleep(Duration::from_secs(2));
                if let Some(v) = installed_newer(exe) {
                    return Ok(v);
                }
            }
            Err("the update in the terminal did not finish".into())
        }
    }
}

fn last_line(b: &[u8]) -> Option<String> {
    String::from_utf8_lossy(b)
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(|l| l.chars().take(200).collect())
}

/// The version `exe --version` reports, when newer than this pdfr.
fn installed_newer(exe: &Path) -> Option<String> {
    let out = Command::new(exe)
        .arg("--version")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    let v = s.trim().strip_prefix("pdfr ")?.to_string();
    crate::updates::is_newer(&v, crate::updates::current()).then_some(v)
}

/// The release file for this computer.
fn asset(version: &str) -> Option<String> {
    if cfg!(target_os = "macos") {
        Some(format!("pdfr-v{version}-macos.zip"))
    } else if cfg!(target_os = "linux") {
        let arch = match std::env::consts::ARCH {
            "x86_64" => "x86_64",
            "aarch64" => "aarch64",
            _ => return None,
        };
        Some(format!("pdfr-v{version}-{arch}-linux.tar.gz"))
    } else {
        None
    }
}

fn download(url: &str) -> Result<Vec<u8>, String> {
    crate::updates::download_agent()
        .get(url)
        .call()
        .map_err(|e| format!("could not download {url}: {e}"))?
        .body_mut()
        .with_config()
        .limit(MAX_DOWNLOAD)
        .read_to_vec()
        .map_err(|e| format!("the download broke off: {e}"))
}

/// The checksum `SHA256SUMS` lists for `name`.
fn expected_sum(sums: &str, name: &str) -> Option<[u8; 32]> {
    sums.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        let hex = it.next()?;
        let file = it.next()?.trim_start_matches('*');
        if file != name || hex.len() != 64 {
            return None;
        }
        let mut out = [0u8; 32];
        for (i, b) in out.iter_mut().enumerate() {
            *b = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
        }
        Some(out)
    })
}

/// `SHA256SUMS` as text, once its signature (`SHA256SUMS.sig`) checks out
/// against the release-signing key `public`. No signature, no update: the
/// checksums alone would only prove the files match each other.
fn verified_sums(sums: &[u8], sig: Option<&[u8]>, public: &[u8; 32]) -> Result<String, String> {
    let sig = sig.ok_or("the release is not signed (no SHA256SUMS.sig); nothing was changed")?;
    if !pdfr_license::release::verify(public, sums, sig) {
        return Err(
            "the release's signature does not match (SHA256SUMS.sig); nothing was changed".into(),
        );
    }
    String::from_utf8(sums.to_vec()).map_err(|_| "the release's SHA256SUMS is not text".into())
}

/// Downloads, checks and swaps in the release.
fn replace(dest: &Path, version: &str, say: &dyn Fn(String)) -> Result<(), String> {
    if !crate::updates::valid_version(version) {
        return Err(format!("{version:?} is not a pdfr version"));
    }
    let name = asset(version).ok_or("there is no pdfr download for this computer")?;
    // Unpack next to the old copy, so the swap is a rename.
    let parent = dest.parent().ok_or("bad install folder")?;
    check_parent(parent)?;
    let base = format!("{REPO}/v{version}");
    let sums = download(&format!("{base}/SHA256SUMS"))?;
    let sig = download(&format!("{base}/SHA256SUMS.sig")).ok();
    let sums = verified_sums(&sums, sig.as_deref(), &pdfr_license::release::PUBLIC_KEY)?;
    let want = expected_sum(&sums, &name)
        .ok_or_else(|| format!("the release lists no checksum for {name}"))?;
    let data = download(&format!("{base}/{name}"))?;
    if pdfr_sign::sha256(&data) != want {
        return Err(format!(
            "{name} is damaged (checksum mismatch); nothing was changed"
        ));
    }
    say("Installing\u{2026}".into());
    let work = make_work(parent)?;
    let r = unpack_and_swap(&work, &name, &data, &want, dest);
    let _ = std::fs::remove_dir_all(&work);
    r
}

/// Refuses an install folder's parent that anyone may write to: another
/// user could swap files in under pdfr's feet.
fn check_parent(parent: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::metadata(parent)
            .map_err(|e| format!("could not read {}: {e}", parent.display()))?;
        if meta.permissions().mode() & 0o002 != 0 {
            return Err(format!(
                "{} can be changed by anyone; reinstall pdfr somewhere private to update it",
                parent.display()
            ));
        }
    }
    let _ = parent;
    Ok(())
}

/// A new, private (0700), randomly named work folder in `parent`.
fn make_work(parent: &Path) -> Result<PathBuf, String> {
    let mut r = [0u8; 8];
    getrandom::fill(&mut r).map_err(|e| format!("no randomness: {e}"))?;
    let hex: String = r.iter().map(|b| format!("{b:02x}")).collect();
    let work = parent.join(format!(".pdfr-update-{hex}"));
    create_private_dir(&work)?;
    Ok(work)
}

/// Creates `dir`, which must not exist yet (not even as a link), as a
/// folder only this user can open.
fn create_private_dir(dir: &Path) -> Result<(), String> {
    let fail = |e: &dyn std::fmt::Display| format!("could not create {}: {e}", dir.display());
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt};
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(dir)
            .map_err(|e| fail(&e))?;
        let meta = std::fs::symlink_metadata(dir).map_err(|e| fail(&e))?;
        // SAFETY: geteuid has no preconditions and cannot fail.
        let me = unsafe { libc::geteuid() };
        if !meta.is_dir() || meta.uid() != me || meta.mode() & 0o077 != 0 {
            return Err(fail(&"it is not a private folder of this user"));
        }
    }
    #[cfg(not(unix))]
    std::fs::create_dir(dir).map_err(|e| fail(&e))?;
    Ok(())
}

/// The Apple Developer Team ID pdfr.app is signed by. `None` while
/// releases are signed ad hoc: then only the signature's integrity is
/// checked. TODO: set this once the Developer ID certificate is in CI.
const MAC_TEAM_ID: Option<&str> = None;

/// The Team ID in `codesign -dv` output.
fn team_id(codesign_dv: &str) -> Option<&str> {
    codesign_dv
        .lines()
        .find_map(|l| l.trim().strip_prefix("TeamIdentifier="))
        .map(str::trim)
        .filter(|t| *t != "not set")
}

/// Checks pdfr.app's code signature (and its Team ID, when known).
fn check_mac_signature(app: &Path) -> Result<(), String> {
    let out = Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(app)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("could not check the download's signature: {e}"))?;
    if !out.status.success() {
        return Err("the downloaded pdfr.app's signature is not valid; nothing was changed".into());
    }
    if let Some(want) = MAC_TEAM_ID {
        let out = Command::new("/usr/bin/codesign")
            .args(["-dv", "--verbose=2"])
            .arg(app)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("could not check the download's signature: {e}"))?;
        // codesign -d writes to stderr.
        let text = String::from_utf8_lossy(&out.stderr);
        if team_id(&text) != Some(want) {
            return Err(
                "the downloaded pdfr.app is not signed by pdfr; nothing was changed".into(),
            );
        }
    }
    Ok(())
}

fn unpack_and_swap(
    work: &Path,
    name: &str,
    data: &[u8],
    want: &[u8; 32],
    dest: &Path,
) -> Result<(), String> {
    let archive = work.join(name);
    std::fs::write(&archive, data)
        .map_err(|e| format!("could not write {}: {e}", archive.display()))?;
    // Unpack exactly the file that was checked: read it back from disk.
    let written = std::fs::read(&archive)
        .map_err(|e| format!("could not read {}: {e}", archive.display()))?;
    if pdfr_sign::sha256(&written) != *want {
        return Err(format!(
            "{name} changed on disk (checksum mismatch); nothing was changed"
        ));
    }
    let (new, check) = if cfg!(target_os = "macos") {
        let out = work.join("app");
        run_ok(
            Command::new("/usr/bin/ditto")
                .arg("-x")
                .arg("-k")
                .arg(&archive)
                .arg(&out),
        )?;
        let app = out.join("pdfr.app");
        let exe = app.join("Contents/MacOS/pdfr");
        (app, exe)
    } else {
        run_ok(
            Command::new("tar")
                .arg("-xzf")
                .arg(&archive)
                .arg("-C")
                .arg(work),
        )?;
        let dir = work.join(name.trim_end_matches(".tar.gz"));
        let exe = dir.join("pdfr");
        (dir, exe)
    };
    if !check.is_file() {
        return Err("the download has no pdfr in it".into());
    }
    if cfg!(target_os = "macos") {
        check_mac_signature(&new)?;
    }
    swap(&new, dest)?;
    if cfg!(target_os = "macos") {
        // Tell Launch Services, so "Open With" knows the new copy.
        let ls = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";
        let _ = Command::new(ls).arg("-f").arg(dest).output();
    } else {
        // Refresh the app menu entry and icons.
        let _ = Command::new(dest.join("pdfr"))
            .arg("--register")
            .stdin(Stdio::null())
            .output();
    }
    Ok(())
}

fn run_ok(cmd: &mut Command) -> Result<(), String> {
    let out = cmd
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("could not unpack the download: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(last_line(&out.stderr).unwrap_or_else(|| "could not unpack the download".into()))
    }
}

/// Moves `new` to `dest`, keeping the old copy until the new one is in.
fn swap(new: &Path, dest: &Path) -> Result<(), String> {
    let mut old = dest.as_os_str().to_owned();
    old.push(".old");
    let old = PathBuf::from(old);
    let _ = std::fs::remove_dir_all(&old);
    if dest.exists() {
        std::fs::rename(dest, &old)
            .map_err(|e| format!("could not move the old pdfr aside: {e}"))?;
    }
    if let Err(e) = std::fs::rename(new, dest) {
        let _ = std::fs::rename(&old, dest);
        return Err(format!("could not put the new pdfr in place: {e}"));
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(())
}

/// Opens the user's terminal running `command`, left open at the end.
fn open_terminal(command: &str) -> Result<(), String> {
    let script = format!("{command}; printf '\\nDone. Press Enter to close this window.'; read _");
    let sh = ["sh", "-c", script.as_str()];
    let mut tries: Vec<(String, Vec<&str>)> = vec![("xdg-terminal-exec".into(), vec![])];
    if let Ok(t) = std::env::var("TERMINAL")
        && !t.trim().is_empty()
    {
        tries.push((t, vec!["-e"]));
    }
    for (t, pre) in [
        ("x-terminal-emulator", vec!["-e"]),
        ("kgx", vec!["--"]),
        ("gnome-terminal", vec!["--"]),
        ("konsole", vec!["-e"]),
        ("ghostty", vec!["-e"]),
        ("kitty", vec![]),
        ("alacritty", vec!["-e"]),
        ("foot", vec![]),
        ("wezterm", vec!["start", "--"]),
        ("xfce4-terminal", vec!["-x"]),
        ("xterm", vec!["-e"]),
    ] {
        tries.push((t.into(), pre));
    }
    for (t, pre) in tries {
        let Some(prog) = which(&t) else { continue };
        let spawned = Command::new(prog)
            .args(&pre)
            .args(sh)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        if spawned.is_ok() {
            return Ok(());
        }
    }
    Err(format!(
        "no terminal found; update in a terminal with:  {command}"
    ))
}

/// Starts the updated pdfr, with `file` open. Called as pdfr quits.
pub fn relaunch(exe: &Path, file: Option<&Path>) {
    let mut cmd = if cfg!(target_os = "macos")
        && let Some(app) = exe
            .ancestors()
            .nth(3)
            .filter(|a| a.extension().is_some_and(|e| e == "app"))
    {
        // A new instance even though this one hasn't quite gone yet.
        let mut c = Command::new("/usr/bin/open");
        c.arg("-n").arg(app);
        if let Some(f) = file {
            c.arg("--args").arg(f);
        }
        c
    } else {
        let mut c = Command::new(exe);
        if let Some(f) = file {
            c.arg(f);
        }
        c
    };
    let _ = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_checksums() {
        let sums = format!(
            "{}  pdfr-v1.0.0-macos.zip\n{} *pdfr-v1.0.0-x86_64-linux.tar.gz\n",
            "00".repeat(32),
            "ab".repeat(32)
        );
        assert_eq!(
            expected_sum(&sums, "pdfr-v1.0.0-x86_64-linux.tar.gz"),
            Some([0xab; 32])
        );
        assert_eq!(expected_sum(&sums, "pdfr-v1.0.0-macos.zip"), Some([0; 32]));
        assert_eq!(
            expected_sum(&sums, "pdfr-v1.0.0-aarch64-linux.tar.gz"),
            None
        );
        assert_eq!(expected_sum("zz  x\n", "x"), None);
    }

    #[test]
    fn picks_how_to_update() {
        let none = |_: &str| None;
        let all = |p: &str| Some(PathBuf::from(format!("/usr/bin/{p}")));
        let exe = Path::new("/usr/bin/pdfr");
        assert_eq!(method_for(InstallKind::Dpkg, exe, &none), None);
        assert!(matches!(
            method_for(InstallKind::Dpkg, exe, &all),
            Some(Method::Run { program, .. }) if program.ends_with("pkexec")
        ));
        assert!(matches!(
            method_for(InstallKind::Rpm, exe, &all),
            Some(Method::Run { args, .. }) if args[0].ends_with("dnf")
        ));
        assert!(matches!(
            method_for(InstallKind::Pacman, exe, &all),
            Some(Method::Terminal { command }) if command.starts_with("yay -S pdfr")
        ));
        assert_eq!(method_for(InstallKind::Flatpak, exe, &all), None);
        assert_eq!(method_for(InstallKind::Package, exe, &all), None);
        // A binary in a folder of the user's own is left alone.
        assert_eq!(
            method_for(InstallKind::Direct, Path::new("/tmp/pdfr"), &all),
            None
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn finds_an_archive_install() {
        let dir = std::env::temp_dir().join(format!("pdfr-selfupd-{}", std::process::id()));
        let app = dir.join("app");
        std::fs::create_dir_all(app.join("packaging/linux")).unwrap();
        std::fs::write(app.join("pdfr"), b"").unwrap();
        std::fs::write(app.join("install.sh"), b"").unwrap();
        assert_eq!(replace_dest(&app.join("pdfr")), Some(app.clone()));
        // Swapping keeps exactly one copy.
        let new = dir.join("new");
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(new.join("marker"), b"new").unwrap();
        swap(&new, &app).unwrap();
        assert!(app.join("marker").is_file() && !app.join("pdfr").exists());
        assert!(!dir.join("app.old").exists() && !new.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn checks_the_signature() {
        use pdfr_license::release;
        let seed = [42u8; 32];
        let public = pdfr_license::public_key(&seed);
        let sums = format!("{}  pdfr-v1.0.0-macos.zip\n", "ab".repeat(32));
        let sig = release::sign(&seed, sums.as_bytes());
        // Good.
        assert_eq!(
            verified_sums(sums.as_bytes(), Some(sig.as_bytes()), &public).as_deref(),
            Ok(sums.as_str())
        );
        // Missing.
        assert!(
            verified_sums(sums.as_bytes(), None, &public)
                .unwrap_err()
                .contains("not signed")
        );
        // Tampered checksums, a tampered signature, another key.
        let evil = sums.replace("ab", "cd");
        assert!(verified_sums(evil.as_bytes(), Some(sig.as_bytes()), &public).is_err());
        let mut bad = sig.clone().into_bytes();
        bad[3] = if bad[3] == b'A' { b'B' } else { b'A' };
        assert!(verified_sums(sums.as_bytes(), Some(&bad), &public).is_err());
        assert!(verified_sums(sums.as_bytes(), Some(b"<html>404</html>"), &public).is_err());
        assert!(
            verified_sums(sums.as_bytes(), Some(sig.as_bytes()), &release::PUBLIC_KEY).is_err()
        );
    }

    /// install.sh checks the same key as pdfr.
    #[test]
    fn installer_pins_the_same_key() {
        let script = include_str!("../../../packaging/install.sh");
        let pem = pdfr_license::release::public_pem(&pdfr_license::release::PUBLIC_KEY);
        let line = pem.lines().nth(1).unwrap();
        assert!(script.contains(line), "install.sh lacks {line}");
    }

    #[test]
    fn refuses_bad_versions() {
        for v in ["0.6.2-$(id)", "0.6.2-../../x", "0.6.2-", "0.6.2 ", "1.0+x"] {
            let r = replace(Path::new("/nonexistent/pdfr"), v, &|_| {});
            assert!(r.unwrap_err().contains("not a pdfr version"), "{v}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn work_folder_is_new_and_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("pdfr-selfupd-work-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let work = make_work(&dir).unwrap();
        let mode = std::fs::metadata(&work).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
        assert_ne!(make_work(&dir).unwrap(), work);
        // An existing folder, or a link planted in its place, is refused.
        assert!(create_private_dir(&work).is_err());
        let link = dir.join(".pdfr-update-planted");
        std::os::unix::fs::symlink(&work, &link).unwrap();
        assert!(create_private_dir(&link).is_err());
        // A parent anyone can write to is refused.
        assert!(check_parent(&dir).is_ok());
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(check_parent(&dir).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reads_the_team_id() {
        let out = "Executable=/x/pdfr.app/Contents/MacOS/pdfr\nAuthority=Developer ID Application: X (ABCDE12345)\nTeamIdentifier=ABCDE12345\n";
        assert_eq!(team_id(out), Some("ABCDE12345"));
        assert_eq!(team_id("Signature=adhoc\nTeamIdentifier=not set\n"), None);
        assert_eq!(team_id(""), None);
    }

    #[test]
    fn homebrew_prefix() {
        assert_eq!(
            brew_prefix(Path::new("/opt/homebrew/Cellar/pdfr/0.4.0/bin/pdfr")),
            Some(PathBuf::from("/opt/homebrew"))
        );
        assert_eq!(brew_prefix(Path::new("/usr/bin/pdfr")), None);
    }
}
