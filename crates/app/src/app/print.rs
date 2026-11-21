//! Printing through each system's own print dialog, with no external
//! program and no printing library:
//! - Linux: the XDG print portal (D-Bus) receives the filled PDF.
//! - Windows: the standard print dialog; pages are rendered at the printer's
//!   resolution by pdfr's own renderer and sent with GDI.
//! - macOS: PDFKit prints the filled PDF through the standard print panel.

#[cfg(not(target_os = "macos"))]
use super::files::DialogResult;
use super::*;

/// Whether printing works on this platform.
pub(super) const AVAILABLE: bool = cfg!(any(target_os = "linux", windows, target_os = "macos"));

impl App {
    pub(super) fn print(&mut self) {
        let Some(doc) = &self.doc else { return };
        if self.dialog_open() {
            return;
        }
        // What is printed is what would be saved, flattened: the values are
        // drawn into the pages, because many printers that read PDFs
        // themselves skip form fields. A read-only file prints as it is.
        let bytes = if doc.form.editable() {
            match pdfr_core::save::save_with(&doc.form, &doc.fields(), &doc.mark_list()) {
                Ok(b) => b,
                Err(e) => {
                    self.status = format!("Could not print: {e}");
                    return;
                }
            }
        } else {
            doc.form.bytes.to_vec()
        };
        let bytes = pdfr_core::flatten::flatten_for_print(&bytes).unwrap_or(bytes);
        let paper = doc.sizes.first().map(|&(w, h)| Paper::of(w, h));
        let title = doc.name.clone();
        let parent = self.parent;
        // macOS: the print panel is a sheet on the window, run by AppKit on
        // the main thread without blocking it.
        #[cfg(target_os = "macos")]
        {
            let _ = paper;
            self.status = match mac::print(&bytes, &title, parent) {
                Ok(()) => "Print dialog opened.".into(),
                Err(e) => format!("Could not print: {e}"),
            };
        }
        // Elsewhere the dialog is waited for on a background thread (built
        // there: attaching it to the window uses handles that cannot move
        // between threads mid-call).
        // Linux without a print portal: pdfr's own dialog, straight to CUPS.
        #[cfg(target_os = "linux")]
        if !crate::system::portal::has("Print") {
            let _ = paper;
            self.open_cups_dialog(bytes, title);
            return;
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.status = "Printing…".into();
            self.dialog_in_background(move || {
                DialogResult::Printed(platform_print(bytes, title, parent, paper))
            });
        }
    }
}

/// The print dialog pdfr shows itself when the desktop has none.
#[cfg(target_os = "linux")]
pub(super) struct CupsDialog {
    printers: Vec<super::cups::Printer>,
    sel: usize,
    copies: u32,
    bytes: Vec<u8>,
    title: String,
}

#[cfg(target_os = "linux")]
impl App {
    fn open_cups_dialog(&mut self, bytes: Vec<u8>, title: String) {
        match super::cups::printers() {
            Ok((printers, default)) if !printers.is_empty() => {
                let sel = default
                    .and_then(|d| printers.iter().position(|p| p.name == d))
                    .unwrap_or(0);
                self.cups_dialog = Some(CupsDialog {
                    printers,
                    sel,
                    copies: 1,
                    bytes,
                    title,
                });
            }
            Ok(_) => {
                self.status =
                    "No printers are set up. Add one in your system's printer settings.".into()
            }
            Err(e) => {
                self.status = format!(
                    "Could not print: {e}. Install xdg-desktop-portal-gtk (a print dialog) or CUPS."
                )
            }
        }
    }

    pub(super) fn cups_dialog_ui(&mut self, ctx: &egui::Context) {
        use crate::assist_ui::widgets::*;
        let Some(d) = &mut self.cups_dialog else {
            return;
        };
        let c = Colors::new(&self.palette);
        let mut answer: Option<bool> = None;
        egui::Modal::new(Id::unique("pdfr-print"))
            .frame(panel_frame(ctx, &self.palette, &c, 24))
            .show(ctx, |ui| {
                ui.set_width(420.0);
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                ui.label(
                    egui::RichText::new("Print")
                        .size(20.0)
                        .strong()
                        .color(c.text),
                );
                card(ui, &c, None, |ui| {
                    setting_row(ui, &c, "Printer", "", |ui| {
                        egui::ComboBox::from_id_salt("pdfr-printer")
                            .selected_text(d.printers[d.sel].label())
                            .width(220.0)
                            .show_ui(ui, |ui| {
                                for (i, p) in d.printers.iter().enumerate() {
                                    ui.selectable_value(&mut d.sel, i, p.label());
                                }
                            });
                    });
                    row_divider(ui, &c);
                    setting_row(ui, &c, "Copies", "", |ui| {
                        ui.add(egui::DragValue::new(&mut d.copies).range(1..=99));
                    });
                });
                note(
                    ui,
                    &c,
                    "Paper size, colour and other options are the printer's defaults.",
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if primary_button(ui, &c, "Print").clicked() {
                        answer = Some(true);
                    }
                    if secondary_button(ui, &c, "Cancel").clicked() {
                        answer = Some(false);
                    }
                });
            });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            answer = Some(false);
        }
        let Some(go) = answer else { return };
        let d = self.cups_dialog.take().unwrap();
        if !go {
            self.status = "Printing cancelled.".into();
            return;
        }
        let printer = d.printers[d.sel].name.clone();
        self.status = "Printing…".into();
        self.dialog_in_background(move || {
            DialogResult::Printed(
                super::cups::print(&printer, &d.bytes, &d.title, d.copies).map(|_| true),
            )
        });
    }
}

#[cfg(target_os = "linux")]
mod portal {
    //! The two calls of `org.freedesktop.portal.Print`: PreparePrint shows
    //! the print dialog and answers with a token; Print sends the file with
    //! that token. Only the token is read from the answer, so differences
    //! between portal versions in the other fields do not matter.

    use super::super::files::ParentWindow;
    use futures_lite::StreamExt;
    use std::collections::HashMap;
    use std::io::Write;
    use std::os::fd::AsFd;
    use zbus::zvariant::{Fd, OwnedValue, Value};
    use zbus::{Connection, MatchRule, MessageStream, message::Type as MsgType};

    const DEST: &str = "org.freedesktop.portal.Desktop";
    const PATH: &str = "/org/freedesktop/portal/desktop";
    const IFACE: &str = "org.freedesktop.portal.Print";

    /// `Ok(true)` printed, `Ok(false)` cancelled by the user.
    pub async fn print(
        bytes: Vec<u8>,
        title: String,
        parent: Option<ParentWindow>,
        paper: Option<super::Paper>,
    ) -> Result<bool, String> {
        let file =
            private_copy(&bytes).map_err(|e| format!("could not prepare the document: {e}"))?;
        // Kept alive until the dialog is done (a Wayland export ends when dropped).
        let window = super::window_id::of(parent);
        let conn = Connection::session()
            .await
            .map_err(|e| format!("no desktop session bus: {e}"))?;
        // The document's paper and orientation as the dialog's starting
        // point (GtkPrintSettings / GtkPageSetup keys); everything else
        // (printer, colour, quality, duplex, copies) is the dialog's.
        let mut settings: HashMap<&str, Value> = HashMap::new();
        let mut page_setup: HashMap<&str, Value> = HashMap::new();
        if let Some(p) = paper {
            let orientation = if p.landscape { "landscape" } else { "portrait" };
            settings.insert("orientation", Value::from(orientation));
            settings.insert("paper-format", Value::from(p.name));
            settings.insert("paper-width", Value::from(format!("{}", p.width_mm)));
            settings.insert("paper-height", Value::from(format!("{}", p.height_mm)));
            page_setup.insert("PPDName", Value::from(p.name));
            page_setup.insert("Name", Value::from(p.name));
            page_setup.insert("DisplayName", Value::from(p.display));
            page_setup.insert("Width", Value::from(p.width_mm));
            page_setup.insert("Height", Value::from(p.height_mm));
            page_setup.insert("Orientation", Value::from(orientation));
            for m in ["MarginTop", "MarginBottom", "MarginLeft", "MarginRight"] {
                page_setup.insert(m, Value::from(0.0f64));
            }
        }

        let opts = |token: &str| {
            HashMap::from([
                ("handle_token", Value::from(token.to_string())),
                ("modal", Value::from(true)),
            ])
        };
        let t1 = handle_token();
        let (code, results) = request(
            &conn,
            "PreparePrint",
            &t1,
            &(
                window.id.as_str(),
                title.as_str(),
                &settings,
                &page_setup,
                opts(&t1),
            ),
        )
        .await?;
        match code {
            0 => {}
            1 => return Ok(false),
            _ => return Err("the print dialog failed".into()),
        }
        let token: u32 = results
            .get("token")
            .and_then(|v| v.downcast_ref::<u32>().ok())
            .ok_or("the print dialog gave no print job")?;

        let t2 = handle_token();
        let mut o = opts(&t2);
        o.insert("token", Value::from(token));
        let (code, _) = request(
            &conn,
            "Print",
            &t2,
            &(
                window.id.as_str(),
                title.as_str(),
                Fd::from(file.as_fd()),
                o,
            ),
        )
        .await?;
        match code {
            0 => Ok(true),
            1 => Ok(false),
            _ => Err("the printer did not accept the document".into()),
        }
    }

    /// Calls a portal method and waits for its Response signal.
    async fn request<B>(
        conn: &Connection,
        method: &str,
        token: &str,
        body: &B,
    ) -> Result<(u32, HashMap<String, OwnedValue>), String>
    where
        B: serde::Serialize + zbus::zvariant::DynamicType,
    {
        let err = |e: zbus::Error| match &e {
            zbus::Error::MethodError(name, ..)
                if name.as_str().contains("ServiceUnknown")
                    || name.as_str().contains("UnknownInterface")
                    || name.as_str().contains("UnknownMethod") =>
            {
                "no print dialog is available (install xdg-desktop-portal-gtk)".to_string()
            }
            _ => e.to_string(),
        };
        // Subscribe before calling, so a fast answer is not missed.
        let sender = conn
            .unique_name()
            .map(|n| n.trim_start_matches(':').replace('.', "_"))
            .unwrap_or_default();
        let path = format!("{PATH}/request/{sender}/{token}");
        let rule = MatchRule::builder()
            .msg_type(MsgType::Signal)
            .interface("org.freedesktop.portal.Request")
            .and_then(|b| b.member("Response"))
            .and_then(|b| b.path(path))
            .map_err(err)?
            .build();
        let mut stream = MessageStream::for_match_rule(rule, conn, None)
            .await
            .map_err(err)?;
        conn.call_method(Some(DEST), PATH, Some(IFACE), method, body)
            .await
            .map_err(err)?;
        let msg = stream
            .next()
            .await
            .ok_or("the print dialog closed unexpectedly")?
            .map_err(err)?;
        msg.body()
            .deserialize::<(u32, HashMap<String, OwnedValue>)>()
            .map_err(|e| e.to_string())
    }

    fn handle_token() -> String {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        format!("pdfr{:x}", n & 0xffff_ffff_ffff)
    }

    /// The document in a file only this user can read, already deleted from
    /// the disk: the open file is handed to the portal and disappears when
    /// printing is done. It holds what the user typed, so it never lingers.
    fn private_copy(bytes: &[u8]) -> std::io::Result<std::fs::File> {
        use std::io::Seek;
        use std::os::unix::fs::OpenOptionsExt;
        let dir = std::env::temp_dir();
        for n in 0..100u32 {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.subsec_nanos());
            let path = dir.join(format!(
                "pdfr-print-{}-{nanos:x}-{n}.pdf",
                std::process::id()
            ));
            match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
            {
                Ok(mut f) => {
                    let _ = std::fs::remove_file(&path);
                    f.write_all(bytes)?;
                    f.flush()?;
                    f.rewind()?;
                    return Ok(f);
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(std::io::Error::other("no free temporary file name"))
    }
}

/// The portal's name for the app window ("x11:<id>" or "wayland:<handle>"),
/// so the dialog opens attached to it. Empty if unknown (the dialog still
/// works, as a separate window).
#[cfg(target_os = "linux")]
mod window_id {
    use super::super::files::ParentWindow;
    use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

    pub struct WindowId {
        pub id: String,
        _wayland: Option<wayland::Exported>,
    }

    pub fn of(parent: Option<ParentWindow>) -> WindowId {
        let plain = |id: String| WindowId { id, _wayland: None };
        let Some(p) = parent else {
            return plain(String::new());
        };
        match (p.window, p.display) {
            (RawWindowHandle::Xlib(h), _) => plain(format!("x11:{:x}", h.window)),
            (RawWindowHandle::Xcb(h), _) => plain(format!("x11:{:x}", h.window.get())),
            (RawWindowHandle::Wayland(w), RawDisplayHandle::Wayland(d)) => {
                // SAFETY: the pointers are the app's live surface and display.
                match unsafe { wayland::export(w.surface.as_ptr(), d.display.as_ptr()) } {
                    Some(e) => WindowId {
                        id: format!("wayland:{}", e.handle),
                        _wayland: Some(e),
                    },
                    None => plain(String::new()),
                }
            }
            _ => plain(String::new()),
        }
    }

    /// Exports the window with the xdg-foreign protocol (as rfd does for
    /// its file dialogs), on a private event queue.
    mod wayland {
        use wayland_client::protocol::{wl_registry, wl_surface::WlSurface};
        use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
        use wayland_protocols::xdg::foreign::zv2::client::{
            zxdg_exported_v2, zxdg_exported_v2::ZxdgExportedV2, zxdg_exporter_v2::ZxdgExporterV2,
        };

        pub struct Exported {
            pub handle: String,
            exported: ZxdgExportedV2,
            conn: Connection,
        }

        impl Drop for Exported {
            fn drop(&mut self) {
                self.exported.destroy();
                let _ = self.conn.flush();
            }
        }

        #[derive(Default)]
        struct State {
            handle: String,
            exporter: Option<ZxdgExporterV2>,
        }

        impl Dispatch<wl_registry::WlRegistry, ()> for State {
            fn event(
                state: &mut Self,
                registry: &wl_registry::WlRegistry,
                event: wl_registry::Event,
                _: &(),
                _: &Connection,
                qh: &QueueHandle<Self>,
            ) {
                if let wl_registry::Event::Global {
                    name,
                    interface,
                    version,
                } = event
                    && interface == "zxdg_exporter_v2"
                {
                    state.exporter = Some(registry.bind::<ZxdgExporterV2, (), State>(
                        name,
                        version.min(1),
                        qh,
                        (),
                    ));
                }
            }
        }

        impl Dispatch<ZxdgExporterV2, ()> for State {
            fn event(
                _: &mut Self,
                _: &ZxdgExporterV2,
                _: <ZxdgExporterV2 as Proxy>::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
            }
        }

        impl Dispatch<ZxdgExportedV2, ()> for State {
            fn event(
                state: &mut Self,
                _: &ZxdgExportedV2,
                event: zxdg_exported_v2::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
                if let zxdg_exported_v2::Event::Handle { handle } = event {
                    state.handle = handle;
                }
            }
        }

        /// # Safety
        /// `surface` and `display` must be the live `wl_surface` and
        /// `wl_display` of this process.
        pub unsafe fn export(
            surface: *mut std::ffi::c_void,
            display: *mut std::ffi::c_void,
        ) -> Option<Exported> {
            if surface.is_null() || display.is_null() {
                return None;
            }
            // SAFETY: per the function contract.
            let backend = unsafe {
                wayland_backend::sys::client::Backend::from_foreign_display(display.cast())
            };
            let conn = Connection::from_backend(backend);
            // SAFETY: per the function contract.
            let id = unsafe {
                wayland_backend::sys::client::ObjectId::from_ptr(
                    WlSurface::interface(),
                    surface.cast(),
                )
            }
            .ok()?;
            let surface = WlSurface::from_id(&conn, id).ok()?;
            let mut queue = conn.new_event_queue();
            let qh = queue.handle();
            let mut state = State::default();
            conn.display().get_registry(&qh, ());
            queue.roundtrip(&mut state).ok()?;
            let exporter = state.exporter.take()?;
            let exported = exporter.export_toplevel(&surface, &qh, ());
            queue.roundtrip(&mut state).ok()?;
            exporter.destroy();
            if state.handle.is_empty() {
                exported.destroy();
                return None;
            }
            Some(Exported {
                handle: state.handle,
                exported,
                conn,
            })
        }
    }
}

/// The document's paper, offered to the print dialog as its starting point,
/// so an A4 form is not printed on Letter (cut off, no top margin).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Paper {
    /// GTK / PWG paper name ("iso_a4", "na_letter"; "custom" if none fits).
    pub name: &'static str,
    pub display: &'static str,
    /// Portrait size in millimetres.
    pub width_mm: f64,
    pub height_mm: f64,
    pub landscape: bool,
}

impl Paper {
    /// From the first page's displayed size in points.
    pub fn of(w: f32, h: f32) -> Paper {
        let landscape = w > h;
        let (short, long) = if landscape { (h, w) } else { (w, h) };
        const KNOWN: [(&str, &str, f32, f32, f64, f64); 6] = [
            ("iso_a4", "A4", 595.0, 842.0, 210.0, 297.0),
            ("na_letter", "US Letter", 612.0, 792.0, 215.9, 279.4),
            ("na_legal", "US Legal", 612.0, 1008.0, 215.9, 355.6),
            ("iso_a3", "A3", 842.0, 1191.0, 297.0, 420.0),
            ("iso_a5", "A5", 420.0, 595.0, 148.0, 210.0),
            ("iso_b5", "B5", 499.0, 709.0, 176.0, 250.0),
        ];
        for (name, display, a, b, wm, hm) in KNOWN {
            if (short - a).abs() <= 4.0 && (long - b).abs() <= 4.0 {
                return Paper {
                    name,
                    display,
                    width_mm: wm,
                    height_mm: hm,
                    landscape,
                };
            }
        }
        let mm = |pt: f32| (pt as f64 * 25.4 / 72.0 * 10.0).round() / 10.0;
        Paper {
            name: "custom",
            display: "Custom",
            width_mm: mm(short),
            height_mm: mm(long),
            landscape,
        }
    }
}

/// `Ok(true)` printed, `Ok(false)` cancelled.
#[cfg(target_os = "linux")]
fn platform_print(
    bytes: Vec<u8>,
    title: String,
    parent: Option<super::files::ParentWindow>,
    paper: Option<Paper>,
) -> Result<bool, String> {
    pollster::block_on(portal::print(bytes, title, parent, paper))
}

#[cfg(windows)]
fn platform_print(
    bytes: Vec<u8>,
    title: String,
    parent: Option<super::files::ParentWindow>,
    _paper: Option<Paper>,
) -> Result<bool, String> {
    // The Windows dialog starts from the printer's own paper; each page is
    // placed at actual size and centred.
    win::print(bytes, &title, parent)
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
fn platform_print(
    _: Vec<u8>,
    _: String,
    _: Option<super::files::ParentWindow>,
    _: Option<Paper>,
) -> Result<bool, String> {
    Err("printing is not available on this system yet".into())
}

#[cfg(windows)]
mod win {
    use super::super::files::ParentWindow;
    use pdfr_core::render::{RenderCache, page_geom, render_page};
    use raw_window_handle::RawWindowHandle;
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::Foundation::{GlobalFree, HWND};
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::Storage::Xps::{
        AbortDoc, DOCINFOW, EndDoc, EndPage, StartDocW, StartPage,
    };
    use windows_sys::Win32::UI::Controls::Dialogs::*;

    /// Highest resolution pages are rendered at; the printer scales the rest.
    const MAX_DPI: f32 = 300.0;

    pub fn print(
        bytes: Vec<u8>,
        title: &str,
        parent: Option<ParentWindow>,
    ) -> Result<bool, String> {
        let pdf = pdfr_core::render::open(bytes).ok_or("could not read the document")?;
        let n = pdf.pages().len() as u32;
        let hwnd: HWND = match parent.map(|p| p.window) {
            Some(RawWindowHandle::Win32(h)) => h.hwnd.get() as HWND,
            _ => return Err("no window to attach the print dialog to".into()),
        };
        let mut ranges = [PRINTPAGERANGE {
            nFromPage: 1,
            nToPage: n.max(1),
        }; 16];
        // SAFETY: a plain C struct; all-zero is its documented empty state.
        let mut pd: PRINTDLGEXW = unsafe { zeroed() };
        pd.lStructSize = size_of::<PRINTDLGEXW>() as u32;
        pd.hwndOwner = hwnd;
        pd.Flags = PD_RETURNDC | PD_NOSELECTION | PD_NOCURRENTPAGE | PD_USEDEVMODECOPIESANDCOLLATE;
        pd.nMinPage = 1;
        pd.nMaxPage = n.max(1);
        pd.nPageRanges = 1;
        pd.nMaxPageRanges = ranges.len() as u32;
        pd.lpPageRanges = ranges.as_mut_ptr();
        pd.nCopies = 1;
        pd.nStartPage = START_PAGE_GENERAL;
        // SAFETY: `pd` is initialised as the API requires and outlives the call.
        let hr = unsafe { PrintDlgExW(&mut pd) };
        let free = |pd: &PRINTDLGEXW| unsafe {
            if !pd.hDC.is_null() {
                DeleteDC(pd.hDC);
            }
            if !pd.hDevMode.is_null() {
                GlobalFree(pd.hDevMode);
            }
            if !pd.hDevNames.is_null() {
                GlobalFree(pd.hDevNames);
            }
        };
        if hr < 0 {
            free(&pd);
            return Err(format!("the print dialog failed (0x{hr:08x})"));
        }
        if pd.dwResultAction != PD_RESULT_PRINT || pd.hDC.is_null() {
            free(&pd);
            return Ok(false);
        }
        let pages: Vec<usize> = if pd.Flags & PD_PAGENUMS != 0 {
            ranges[..pd.nPageRanges as usize]
                .iter()
                .flat_map(|r| r.nFromPage..=r.nToPage)
                .filter(|&p| p >= 1 && p <= n)
                .map(|p| p as usize - 1)
                .collect()
        } else {
            (0..n as usize).collect()
        };
        let res = print_pages(pd.hDC, &pdf, &pages, title);
        free(&pd);
        res.map(|()| true)
    }

    /// Each page at actual size (shrunk only if larger than the paper),
    /// centred on the sheet.
    fn print_pages(
        hdc: HDC,
        pdf: &pdfr_core::render::HayroPdf,
        pages: &[usize],
        title: &str,
    ) -> Result<(), String> {
        let name: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
        // SAFETY: plain C struct, then the fields the call reads.
        let mut di: DOCINFOW = unsafe { zeroed() };
        di.cbSize = size_of::<DOCINFOW>() as i32;
        di.lpszDocName = name.as_ptr();
        // SAFETY: `hdc` is the printer context the dialog returned.
        unsafe {
            if StartDocW(hdc, &di) <= 0 {
                return Err("the printer did not start the job".into());
            }
            let cap = |i: u32| GetDeviceCaps(hdc, i as i32);
            let (dpi_x, dpi_y) = (cap(LOGPIXELSX) as f32, cap(LOGPIXELSY) as f32);
            let (paper_w, paper_h) = (cap(PHYSICALWIDTH), cap(PHYSICALHEIGHT));
            let (off_x, off_y) = (cap(PHYSICALOFFSETX), cap(PHYSICALOFFSETY));
            let cache = RenderCache::new();
            let all = pdf.pages();
            for &i in pages {
                let Some(page) = all.get(i) else { continue };
                let (w_pt, h_pt) = page_geom(page).display_size();
                let (w_dev, h_dev) = (w_pt / 72.0 * dpi_x, h_pt / 72.0 * dpi_y);
                let fit = (paper_w as f32 / w_dev)
                    .min(paper_h as f32 / h_dev)
                    .min(1.0);
                let (dw, dh) = ((w_dev * fit) as i32, (h_dev * fit) as i32);
                let (dx, dy) = ((paper_w - dw) / 2 - off_x, (paper_h - dh) / 2 - off_y);
                let img = render_page(page, &cache, dpi_x.min(MAX_DPI) / 72.0 * fit);
                let mut bgra = img.rgba;
                for px in bgra.as_chunks_mut::<4>().0 {
                    px.swap(0, 2);
                }
                let mut bmi: BITMAPINFO = zeroed();
                bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
                bmi.bmiHeader.biWidth = img.width as i32;
                bmi.bmiHeader.biHeight = -(img.height as i32); // top-down rows
                bmi.bmiHeader.biPlanes = 1;
                bmi.bmiHeader.biBitCount = 32;
                bmi.bmiHeader.biCompression = BI_RGB;
                if StartPage(hdc) <= 0 {
                    AbortDoc(hdc);
                    return Err("the printer stopped the job".into());
                }
                SetStretchBltMode(hdc, HALFTONE);
                StretchDIBits(
                    hdc,
                    dx,
                    dy,
                    dw,
                    dh,
                    0,
                    0,
                    img.width as i32,
                    img.height as i32,
                    bgra.as_ptr().cast(),
                    &bmi,
                    DIB_RGB_COLORS,
                    SRCCOPY,
                );
                if EndPage(hdc) <= 0 {
                    AbortDoc(hdc);
                    return Err("the printer stopped the job".into());
                }
            }
            if EndDoc(hdc) <= 0 {
                return Err("the printer did not finish the job".into());
            }
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use super::super::files::ParentWindow;
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSPrintInfo, NSPrintPanelOptions, NSView};
    use objc2_foundation::{NSData, NSString};
    use objc2_pdf_kit::{PDFDocument, PDFPrintScalingMode};
    use raw_window_handle::RawWindowHandle;

    /// Shows the print panel as a sheet on the window (or as a dialog).
    pub fn print(bytes: &[u8], title: &str, parent: Option<ParentWindow>) -> Result<(), String> {
        let mtm = MainThreadMarker::new().ok_or("printing must start on the main thread")?;
        let data = NSData::with_bytes(bytes);
        // SAFETY: PDFKit reads the bytes; nil (None) for unreadable data.
        let doc = unsafe { PDFDocument::initWithData(PDFDocument::alloc(), &data) }
            .ok_or("could not read the document")?;
        let info = NSPrintInfo::sharedPrintInfo();
        // Actual size; shrunk only if the page is larger than the paper.
        // SAFETY: called on the main thread with a valid document.
        let op = unsafe {
            doc.printOperationForPrintInfo_scalingMode_autoRotate(
                Some(&info),
                PDFPrintScalingMode::PageScaleDownToFit,
                true,
                mtm,
            )
        }
        .ok_or("could not prepare printing")?;
        op.setJobTitle(Some(&NSString::from_str(title)));
        op.setShowsPrintPanel(true);
        // Open with every setting shown: paper, orientation, scaling,
        // copies, pages and a preview (printer features are in the panel's
        // own menus).
        let panel = op.printPanel();
        panel.setOptions(
            panel.options()
                | NSPrintPanelOptions::ShowsCopies
                | NSPrintPanelOptions::ShowsPageRange
                | NSPrintPanelOptions::ShowsPaperSize
                | NSPrintPanelOptions::ShowsOrientation
                | NSPrintPanelOptions::ShowsScaling
                | NSPrintPanelOptions::ShowsPreview,
        );
        let window = parent.and_then(|p| match p.window {
            // SAFETY: the handle is the app's own live NSView.
            RawWindowHandle::AppKit(h) => unsafe { h.ns_view.cast::<NSView>().as_ref() }.window(),
            _ => None,
        });
        match window {
            // SAFETY: no delegate; the sheet runs modally on the window.
            Some(w) => unsafe {
                op.runOperationModalForWindow_delegate_didRunSelector_contextInfo(
                    &w,
                    None,
                    None,
                    std::ptr::null_mut(),
                )
            },
            None => {
                op.runOperation();
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Paper;

    #[test]
    fn paper_sizes_are_recognised() {
        assert_eq!(Paper::of(595.0, 842.0).name, "iso_a4");
        assert_eq!(Paper::of(612.0, 792.0).name, "na_letter");
        let l = Paper::of(842.0, 595.0);
        assert!(l.landscape && l.name == "iso_a4" && l.width_mm == 210.0);
        let c = Paper::of(500.0, 500.0);
        assert_eq!((c.name, c.width_mm), ("custom", 176.4));
    }
}
