//! The online licence check (docs/license-keys.md): when a registered copy
//! has internet, it tells pdfr.app the licence id and this installation's
//! random id, and learns whether the key was revoked or is in use on too many
//! computers. Nothing else is sent. Network trouble changes nothing: a copy
//! registered offline stays registered.

use std::sync::mpsc::{Receiver, channel};

const CHECK_URL: &str = "https://pdfr.app/api/v1/licenses/check";

/// What pdfr.app said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    /// "active", "unknown", "device_limit" or "revoked".
    pub status: String,
    pub message: Option<String>,
}

/// A check running on a background thread.
#[derive(Default)]
pub struct Checker {
    rx: Option<Receiver<Result<Answer, String>>>,
}

impl Checker {
    pub fn running(&self) -> bool {
        self.rx.is_some()
    }

    pub fn start(&mut self, ctx: &eframe::egui::Context, id: String, install: String) {
        if self.running() {
            return;
        }
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::Builder::new()
            .name("pdfr-license-check".into())
            .spawn(move || {
                let _ = tx.send(check(&id, &install));
                ctx.request_repaint();
            })
            .ok();
        self.rx = Some(rx);
    }

    /// The answer, once it has arrived.
    pub fn poll(&mut self) -> Option<Result<Answer, String>> {
        let r = self.rx.as_ref()?.try_recv().ok()?;
        self.rx = None;
        Some(r)
    }
}

/// The endpoint: pdfr.app, or `PDFR_LICENSE_URL` in debug builds (a local
/// pdfr.app).
fn url() -> String {
    if cfg!(debug_assertions)
        && let Ok(u) = std::env::var("PDFR_LICENSE_URL")
    {
        return u;
    }
    CHECK_URL.to_string()
}

fn os() -> &'static str {
    if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(windows) {
        "windows"
    } else {
        "other"
    }
}

fn check(id: &str, install: &str) -> Result<Answer, String> {
    let body = serde_json::json!({
        "id": id,
        "install": install,
        "version": crate::updates::current(),
        "os": os(),
    });
    let text = crate::updates::agent()
        .post(&url())
        .header("Content-Type", "application/json")
        .send(body.to_string())
        .map_err(|e| format!("could not reach pdfr.app: {e}"))?
        .body_mut()
        .with_config()
        .limit(64 * 1024)
        .read_to_string()
        .map_err(|e| e.to_string())?;
    parse(&text).ok_or_else(|| "pdfr.app gave an unexpected answer".to_string())
}

pub fn parse(json: &str) -> Option<Answer> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let status = v.get("status")?.as_str()?;
    if !matches!(status, "active" | "unknown" | "device_limit" | "revoked") {
        return None;
    }
    Some(Answer {
        status: status.to_string(),
        message: v
            .get("message")
            .and_then(|m| m.as_str())
            .map(|m| m.chars().take(300).collect()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_answers() {
        assert_eq!(
            parse(r#"{"status":"active","manage_url":"https://pdfr.app/account"}"#),
            Some(Answer {
                status: "active".into(),
                message: None
            })
        );
        let a = parse(r#"{"status":"revoked","message":"The purchase was refunded."}"#).unwrap();
        assert_eq!(a.status, "revoked");
        assert_eq!(a.message.as_deref(), Some("The purchase was refunded."));
        assert_eq!(parse(r#"{"status":"pwned"}"#), None);
        assert_eq!(parse(r#"{"error":"Too many requests"}"#), None);
        assert_eq!(parse("<html>"), None);
    }
}
