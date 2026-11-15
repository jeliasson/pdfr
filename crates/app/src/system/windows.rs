//! Windows integration. The installer (packaging/windows) registers pdfr
//! for "Open with" and Default apps; Windows does not let apps make
//! themselves the default, so the button opens the Settings page for pdfr.
//! Untested on real Windows yet.

use super::{DefaultApp, InstallKind};

fn shell_open(target: &str) -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (verb, file) = (wide("open"), wide(target));
    // SAFETY: both strings are NUL-terminated and live across the call.
    let r = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // Values above 32 mean success.
    if r as usize > 32 {
        Ok(())
    } else {
        Err("Windows could not open it".into())
    }
}

pub fn default_pdf_app() -> DefaultApp {
    DefaultApp::Unknown
}

pub fn can_make_default() -> bool {
    true
}

pub fn make_default() -> Result<String, String> {
    shell_open("ms-settings:defaultapps?registeredAppUser=pdfr")
        .or_else(|_| shell_open("ms-settings:defaultapps"))?;
    Ok("Choose pdfr for .pdf in the Settings window that opened.".into())
}

pub fn open_url(url: &str) -> Result<(), String> {
    shell_open(url)
}

pub fn install_kind() -> InstallKind {
    let exe = std::env::current_exe().unwrap_or_default();
    let s = exe.to_string_lossy().to_lowercase();
    if s.contains("\\winget\\") || s.contains("\\microsoft\\winget") {
        InstallKind::Package
    } else {
        InstallKind::Direct
    }
}

pub fn register() -> Result<String, String> {
    Err("Use the pdfr installer to add pdfr to the Start menu.".into())
}

pub fn unregister() -> Result<String, String> {
    Err("Uninstall pdfr from Settings \u{2192} Apps.".into())
}
