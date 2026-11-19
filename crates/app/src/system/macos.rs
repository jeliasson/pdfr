//! macOS integration. pdfr.app's Info.plist declares PDF support, which
//! puts it in Finder's "Open With"; making it the default asks the system,
//! which shows its own confirmation. Untested on a real Mac yet.

use super::{DefaultApp, InstallKind};
use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSString, NSURL};

fn pdf_type() -> Option<objc2::rc::Retained<objc2_uniform_type_identifiers::UTType>> {
    objc2_uniform_type_identifiers::UTType::typeWithIdentifier(&NSString::from_str("com.adobe.pdf"))
}

pub fn default_pdf_app() -> DefaultApp {
    let Some(pdf) = pdf_type() else {
        return DefaultApp::Unknown;
    };
    let Some(url) = NSWorkspace::sharedWorkspace().URLForApplicationToOpenContentType(&pdf) else {
        return DefaultApp::Unknown;
    };
    let path = url.path().map(|p| p.to_string()).unwrap_or_default();
    let mine = bundle_url()
        .and_then(|u| u.path())
        .is_some_and(|p| p.to_string().trim_end_matches('/') == path.trim_end_matches('/'));
    if mine {
        DefaultApp::Pdfr
    } else {
        let name = std::path::Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or(path);
        DefaultApp::Other(name)
    }
}

pub fn can_make_default() -> bool {
    bundle_url().is_some()
}

/// The running app's bundle, when started from pdfr.app.
fn bundle_url() -> Option<objc2::rc::Retained<NSURL>> {
    let exe = std::env::current_exe().ok()?;
    let app = exe
        .ancestors()
        .find(|p| p.extension().is_some_and(|e| e == "app"))?;
    let s = NSString::from_str(&app.to_string_lossy());
    Some(NSURL::fileURLWithPath(&s))
}

pub fn make_default() -> Result<String, String> {
    let app = bundle_url().ok_or("start pdfr from pdfr.app to make it the default")?;
    let ws = NSWorkspace::sharedWorkspace();
    let pdf = pdf_type().ok_or("macOS does not know the PDF type")?;
    // macOS 12+: the system asks the user to confirm.
    ws.setDefaultApplicationAtURL_toOpenContentType_completionHandler(&app, &pdf, None);
    Ok("Confirm in the dialog macOS shows.".into())
}

pub fn unset_default() -> Result<String, String> {
    Err("To use another app, select a PDF in Finder, choose Get Info, and change \u{201c}Open with\u{201d}.".into())
}

pub fn open_url(url: &str) -> Result<(), String> {
    let u = NSURL::URLWithString(&NSString::from_str(url)).ok_or("not a link")?;
    if NSWorkspace::sharedWorkspace().openURL(&u) {
        Ok(())
    } else {
        Err("macOS could not open the link".into())
    }
}

pub fn install_kind() -> InstallKind {
    let exe = std::env::current_exe().unwrap_or_default();
    let s = exe.to_string_lossy();
    if s.contains("/Caskroom/") || s.contains("/Cellar/") || s.contains("/homebrew/") {
        InstallKind::Homebrew
    } else {
        InstallKind::Direct
    }
}

pub fn register() -> Result<String, String> {
    Err("Drag pdfr.app to Applications to add it to Launchpad and Spotlight.".into())
}

pub fn unregister() -> Result<String, String> {
    Err("Move pdfr.app to the Trash to remove it.".into())
}

/// Files Finder asked pdfr to open (double-click, Open With, dropping on the
/// Dock icon). They arrive as an Apple Event, not as arguments; winit does
/// not pass it on, so pdfr answers `application:openURLs:` itself.
static OPENED: std::sync::Mutex<Vec<std::path::PathBuf>> = std::sync::Mutex::new(Vec::new());
static WAKE: std::sync::OnceLock<eframe::egui::Context> = std::sync::OnceLock::new();

pub fn set_wake(ctx: &eframe::egui::Context) {
    let _ = WAKE.set(ctx.clone());
}

pub fn take_opened() -> Vec<std::path::PathBuf> {
    std::mem::take(&mut *OPENED.lock().unwrap())
}

/// Must run before the event loop starts (eframe's event loop hook), so
/// files given at launch are not missed. The method is added to NSObject:
/// winit's app delegate class is made later, and AppKit only asks the
/// delegate.
pub fn install_open_handler() {
    use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
    use objc2_foundation::NSArray;

    unsafe extern "C-unwind" fn open_urls(
        _this: *mut AnyObject,
        _cmd: Sel,
        _app: *mut AnyObject,
        urls: *mut NSArray<NSURL>,
    ) {
        // SAFETY: AppKit passes a valid NSArray of NSURL for this selector.
        let Some(urls) = (unsafe { urls.as_ref() }) else {
            return;
        };
        let mut list = OPENED.lock().unwrap();
        for i in 0..urls.count() {
            let url = urls.objectAtIndex(i);
            if let Some(p) = url.path() {
                list.push(std::path::PathBuf::from(p.to_string()));
            }
        }
        if let Some(ctx) = WAKE.get() {
            ctx.request_repaint();
        }
    }

    let Some(cls) = AnyClass::get(c"NSObject") else {
        return;
    };
    // SAFETY: the function matches the "v@:@@" signature of
    // -application:openURLs:; adding a method to NSObject is allowed.
    unsafe {
        let imp: Imp = std::mem::transmute::<
            unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, *mut NSArray<NSURL>),
            Imp,
        >(open_urls);
        objc2::ffi::class_addMethod(
            cls as *const AnyClass as *mut AnyClass,
            objc2::sel!(application:openURLs:),
            imp,
            c"v@:@@".as_ptr(),
        );
    }
}
