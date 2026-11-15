//! Printing: the filled document goes to the desktop's print dialog through
//! the XDG print portal (D-Bus), so pdfr runs no external program and needs
//! no printing library. The portal shows the system print dialog (printer,
//! copies, pages) and sends the PDF to the printer.

use super::files::DialogResult;
use super::*;

/// Whether printing works on this platform.
pub(super) const AVAILABLE: bool = cfg!(target_os = "linux");

impl App {
    pub(super) fn print(&mut self) {
        let Some(doc) = &self.doc else { return };
        if self.dialog_open() {
            return;
        }
        // What is printed is exactly what would be saved: the filled form
        // with its appearances. A read-only file prints as it is.
        let bytes = if doc.form.editable() {
            match pdfr_core::save::save(&doc.form, &doc.fields()) {
                Ok(b) => b,
                Err(e) => {
                    self.status = format!("Could not print: {e}");
                    return;
                }
            }
        } else {
            doc.form.bytes.to_vec()
        };
        let title = doc.name.clone();
        let parent = self.parent;
        self.status = "Printing…".into();
        // The portal call is built on the background thread: attaching it to
        // the window uses handles that cannot move between threads mid-call.
        self.dialog_in_background(move || {
            DialogResult::Printed(pollster::block_on(portal::print(bytes, title, parent)))
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
    ) -> Result<bool, String> {
        let file =
            private_copy(&bytes).map_err(|e| format!("could not prepare the document: {e}"))?;
        // Kept alive until the dialog is done (a Wayland export ends when dropped).
        let window = super::window_id::of(parent);
        let conn = Connection::session()
            .await
            .map_err(|e| format!("no desktop session bus: {e}"))?;
        let no_dict: HashMap<&str, Value> = HashMap::new();

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
                &no_dict,
                &no_dict,
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

#[cfg(not(target_os = "linux"))]
mod portal {
    use super::super::files::ParentWindow;
    pub async fn print(_: Vec<u8>, _: String, _: Option<ParentWindow>) -> Result<bool, String> {
        Err("printing is not available on this system yet".into())
    }
}
