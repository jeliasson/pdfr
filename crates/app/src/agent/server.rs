//! A window's side of agent control: a listener thread takes requests off
//! the socket and hands them to the UI thread, which answers each one (see
//! `app/agent.rs`). Nothing here touches the document.

use eframe::egui;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

/// How long a connection waits for the window's answer before giving up.
/// The window answers long before this (it has its own time limits).
const ANSWER_WAIT: Duration = Duration::from_secs(180);

/// One request from an agent, waiting for the UI thread.
pub struct Request {
    pub op: String,
    pub args: Value,
    /// The UI thread sends `{"ok":…,"result":…,"state":…}` here.
    pub reply: Sender<Value>,
}

/// The listening socket of this window. Dropping it stops listening and
/// removes the socket.
pub struct Server {
    pub rx: Receiver<Request>,
    path: PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

impl Server {
    /// Starts listening. `None` where it cannot (no runtime folder, or a
    /// system without Unix sockets).
    #[cfg(unix)]
    pub fn start(ctx: egui::Context) -> Option<Server> {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        use std::os::unix::net::UnixListener;

        let dir = super::bridge::own_dir();
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir)
            .ok()?;
        // Only the owner can tighten it: someone else's folder is refused.
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).ok()?;
        super::bridge::check_dir(&dir).ok()?;
        let path = dir.join(super::bridge::socket_name(std::process::id()));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).ok()?;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));

        let (tx, rx) = channel::<Request>();
        let watched = path.clone();
        std::thread::Builder::new()
            .name("agent-listen".into())
            .spawn(move || {
                for conn in listener.incoming() {
                    // The socket was removed (the server was dropped, or
                    // agents were turned off): stop.
                    if !watched.exists() {
                        break;
                    }
                    let Ok(stream) = conn else { continue };
                    let (tx, ctx) = (tx.clone(), ctx.clone());
                    let _ = std::thread::Builder::new()
                        .name("agent-conn".into())
                        .spawn(move || serve(stream, tx, ctx));
                }
            })
            .ok()?;
        Some(Server { rx, path })
    }

    #[cfg(not(unix))]
    pub fn start(_ctx: egui::Context) -> Option<Server> {
        let _ = (ANSWER_WAIT, channel::<Request>, json!(null));
        None
    }
}

/// One agent connection: a request per line, answered in order.
#[cfg(unix)]
fn serve(stream: std::os::unix::net::UnixStream, tx: Sender<Request>, ctx: egui::Context) {
    use std::io::{BufRead, BufReader, Write};

    // Only this user's own programs may drive the window.
    if super::bridge::check_peer(&stream).is_err() {
        return;
    }
    let Ok(mut out) = stream.try_clone() else {
        return;
    };
    super::client_joined();
    ctx.request_repaint();
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let answer = match serde_json::from_str::<Value>(&line) {
            Ok(req) => {
                let id = req["id"].clone();
                let (reply, answered) = channel();
                let sent = tx.send(Request {
                    op: req["op"].as_str().unwrap_or_default().to_string(),
                    args: req["args"].clone(),
                    reply,
                });
                ctx.request_repaint();
                let mut v = match sent {
                    Ok(()) => answered.recv_timeout(ANSWER_WAIT).unwrap_or_else(|_| {
                        failure("timeout", "The pdfr window did not answer in time.")
                    }),
                    Err(_) => failure("window_gone", "The pdfr window is closing."),
                };
                v["id"] = id;
                v
            }
            Err(e) => failure("bad_request", &format!("Not a JSON request: {e}.")),
        };
        let mut text = answer.to_string();
        text.push('\n');
        if out.write_all(text.as_bytes()).is_err() {
            break;
        }
    }
    super::client_left();
    ctx.request_repaint();
}

#[cfg(unix)]
pub fn failure(code: &str, message: &str) -> Value {
    json!({"ok": false, "error": {"code": code, "message": message}})
}
