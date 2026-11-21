//! The line protocol between `pdfr mcp` and a pdfr window, and where the
//! windows' sockets live.
//!
//! One JSON object per line, each way:
//!
//! ```text
//! → {"id":7,"op":"fill","args":{"ref":"f12","value":"Anna"}}
//! ← {"id":7,"ok":true,"result":{…},"state":{…}}
//! ← {"id":7,"ok":false,"error":{"code":"paused_by_user","message":"…"},"state":{…}}
//! ```
//!
//! Every window listens on its own socket, `agent-<pid>.sock`, in a folder
//! only the user can enter.

use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Whether this process runs inside a Flatpak sandbox.
fn in_flatpak() -> bool {
    Path::new("/.flatpak-info").exists()
}

/// The runtime folder (`$XDG_RUNTIME_DIR`), or the temp folder where there
/// is none (macOS, where it is already private to the user).
fn runtime_dir() -> (PathBuf, bool) {
    match std::env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty()) {
        Some(d) => (PathBuf::from(d), true),
        None => (std::env::temp_dir(), false),
    }
}

/// Where this process's window puts its socket.
pub fn own_dir() -> PathBuf {
    let (rt, xdg) = runtime_dir();
    if xdg && in_flatpak() {
        // The one runtime folder a sandboxed app shares with the host.
        rt.join("app").join(crate::system::APP_ID)
    } else if xdg || cfg!(target_os = "macos") {
        rt.join("pdfr")
    } else {
        // A shared /tmp: one folder per user.
        let user = std::env::var("USER").unwrap_or_else(|_| "user".into());
        rt.join(format!("pdfr-{user}"))
    }
}

/// Every folder a window's socket may be in: this process's own, and the
/// one Flatpak windows use (so a host `pdfr mcp` finds them too).
pub fn all_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![own_dir()];
    let (rt, xdg) = runtime_dir();
    if xdg {
        for d in [rt.join("pdfr"), rt.join("app").join(crate::system::APP_ID)] {
            if !dirs.contains(&d) {
                dirs.push(d);
            }
        }
    }
    dirs
}

/// A window's socket file name (windows listen only on Unix for now).
#[cfg(unix)]
pub fn socket_name(pid: u32) -> String {
    format!("agent-{pid}.sock")
}

/// The sockets of the windows that may be running, oldest first.
pub fn sockets() -> Vec<PathBuf> {
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = vec![];
    for dir in all_dirs() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with("agent-") && name.ends_with(".sock") {
                let at = e
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::UNIX_EPOCH);
                found.push((at, e.path()));
            }
        }
    }
    found.sort();
    found.into_iter().map(|f| f.1).collect()
}

/// The process id in a socket's name.
pub fn pid_of(socket: &Path) -> Option<u32> {
    socket
        .file_name()?
        .to_str()?
        .strip_prefix("agent-")?
        .strip_suffix(".sock")?
        .parse()
        .ok()
}

/// A failed call: a short code for the agent to act on, and a sentence.
#[derive(Clone, Debug, PartialEq)]
pub struct Failure {
    pub code: String,
    pub message: String,
}

impl Failure {
    pub fn new(code: &str, message: impl Into<String>) -> Failure {
        Failure {
            code: code.into(),
            message: message.into(),
        }
    }
}

/// A connection to one window.
pub struct Client {
    #[cfg(unix)]
    stream: std::os::unix::net::UnixStream,
    #[cfg(unix)]
    reader: BufReader<std::os::unix::net::UnixStream>,
    next_id: u64,
}

impl Client {
    #[cfg(unix)]
    pub fn connect(socket: &Path) -> std::io::Result<Client> {
        let stream = std::os::unix::net::UnixStream::connect(socket)?;
        let reader = BufReader::new(stream.try_clone()?);
        Ok(Client {
            stream,
            reader,
            next_id: 1,
        })
    }

    #[cfg(not(unix))]
    pub fn connect(_socket: &Path) -> std::io::Result<Client> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "agent control is not available on this system yet",
        ))
    }

    /// One request and its answer: `(result, state)` or the failure.
    #[cfg(unix)]
    pub fn call(
        &mut self,
        op: &str,
        args: &Value,
        timeout: Duration,
    ) -> Result<(Value, Value), Failure> {
        let gone = |e: std::io::Error| {
            Failure::new(
                "window_gone",
                format!("The pdfr window stopped answering: {e}."),
            )
        };
        let id = self.next_id;
        self.next_id += 1;
        let mut line = serde_json::json!({"id": id, "op": op, "args": args}).to_string();
        line.push('\n');
        self.stream.write_all(line.as_bytes()).map_err(gone)?;
        self.stream.set_read_timeout(Some(timeout)).map_err(gone)?;
        loop {
            let mut answer = String::new();
            match self.reader.read_line(&mut answer) {
                Ok(0) => {
                    return Err(Failure::new("window_gone", "The pdfr window was closed."));
                }
                Ok(_) => {}
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    return Err(Failure::new(
                        "timeout",
                        "The pdfr window did not answer in time.",
                    ));
                }
                Err(e) => return Err(gone(e)),
            }
            let Ok(v) = serde_json::from_str::<Value>(&answer) else {
                continue;
            };
            // An answer to an earlier call that timed out: skip it.
            if v["id"].as_u64() != Some(id) {
                continue;
            }
            let state = v["state"].clone();
            return if v["ok"].as_bool() == Some(true) {
                Ok((v["result"].clone(), state))
            } else {
                Err(Failure::new(
                    v["error"]["code"].as_str().unwrap_or("failed"),
                    v["error"]["message"]
                        .as_str()
                        .unwrap_or("The request failed."),
                ))
            };
        }
    }

    #[cfg(not(unix))]
    pub fn call(
        &mut self,
        _op: &str,
        _args: &Value,
        _timeout: Duration,
    ) -> Result<(Value, Value), Failure> {
        let _ = (&self.next_id, BufReader::new(std::io::empty()).fill_buf());
        let _ = std::io::sink().write(&[]);
        Err(Failure::new(
            "unsupported",
            "Agent control is not available on this system yet.",
        ))
    }
}

/// Connects to a window, removing its socket if nothing listens there any
/// more (the window crashed).
pub fn connect_or_clean(socket: &Path) -> Option<Client> {
    match Client::connect(socket) {
        Ok(c) => Some(c),
        Err(e) => {
            if e.kind() == std::io::ErrorKind::ConnectionRefused {
                let _ = std::fs::remove_file(socket);
            }
            None
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn socket_names_carry_the_pid() {
        let p = PathBuf::from("/run/user/1000/pdfr").join(socket_name(4242));
        assert_eq!(pid_of(&p), Some(4242));
        assert_eq!(pid_of(Path::new("/tmp/other.sock")), None);
    }
}
