//! The opt-in "a new version is available" check. Once a day at most, while
//! the user has it switched on, pdfr asks GitHub for the latest release's
//! version number. Nothing about the user or their files is sent (the only
//! header is `User-Agent: pdfr/<version>`). Nothing is downloaded or
//! installed until the user presses "Update now" (`self_update`).
//!
//! TLS is rustls with the pure-Rust RustCrypto provider, so no C code is
//! added to pdfr.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

const LATEST: &str = "https://api.github.com/repos/kjellberg/pdfr/releases/latest";
pub const DAY: u64 = 24 * 60 * 60;

/// A check running on a background thread.
#[derive(Default)]
pub struct Checker {
    rx: Option<Receiver<Result<String, String>>>,
}

impl Checker {
    pub fn running(&self) -> bool {
        self.rx.is_some()
    }

    pub fn start(&mut self, ctx: &eframe::egui::Context) {
        if self.running() {
            return;
        }
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::Builder::new()
            .name("pdfr-update-check".into())
            .spawn(move || {
                let _ = tx.send(latest_version());
                ctx.request_repaint();
            })
            .ok();
        self.rx = Some(rx);
    }

    /// The answer, once it has arrived.
    pub fn poll(&mut self) -> Option<Result<String, String>> {
        let r = self.rx.as_ref()?.try_recv().ok()?;
        self.rx = None;
        Some(r)
    }
}

pub(crate) fn agent() -> ureq::Agent {
    agent_with_timeout(15)
}

/// For downloading a release: tens of megabytes on a slow line.
pub(crate) fn download_agent() -> ureq::Agent {
    agent_with_timeout(15 * 60)
}

fn agent_with_timeout(secs: u64) -> ureq::Agent {
    let provider = Arc::new(rustls_rustcrypto::provider());
    let tls = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::Rustls)
        .root_certs(ureq::tls::RootCerts::WebPki)
        .unversioned_rustls_crypto_provider(provider)
        .build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .timeout_global(Some(std::time::Duration::from_secs(secs)))
        .user_agent(concat!("pdfr/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

/// The latest release's version ("1.2.0").
pub fn latest_version() -> Result<String, String> {
    let body = agent()
        .get(LATEST)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("could not reach GitHub: {e}"))?
        .body_mut()
        .with_config()
        .limit(1 << 20)
        .read_to_string()
        .map_err(|e| e.to_string())?;
    parse_latest(&body).ok_or_else(|| "GitHub gave an unexpected answer".to_string())
}

fn parse_latest(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    if v.get("prerelease").and_then(|p| p.as_bool()) == Some(true) {
        return None;
    }
    let tag = v.get("tag_name")?.as_str()?;
    let ver = tag.trim_start_matches('v');
    version_parts(ver).map(|_| ver.to_string())
}

fn version_parts(v: &str) -> Option<Vec<u64>> {
    let core = v.split(['-', '+']).next()?;
    let parts: Option<Vec<u64>> = core.split('.').map(|p| p.parse().ok()).collect();
    parts.filter(|p| !p.is_empty() && p.len() <= 4)
}

/// Whether `candidate` is newer than `current`.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (version_parts(candidate), version_parts(current)) {
        (Some(mut a), Some(mut b)) => {
            let n = a.len().max(b.len());
            a.resize(n, 0);
            b.resize(n, 0);
            a > b
        }
        _ => false,
    }
}

pub fn current() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(is_newer("0.2.0", "0.1.0"));
        assert!(is_newer("0.10.0", "0.9.3"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.2.0"));
        assert!(!is_newer("garbage", "0.1.0"));
    }

    #[test]
    fn reads_the_release() {
        assert_eq!(
            parse_latest(r#"{"tag_name":"v0.3.1","prerelease":false,"name":"x"}"#).as_deref(),
            Some("0.3.1")
        );
        assert_eq!(
            parse_latest(r#"{"tag_name":"v0.4.0","prerelease":true}"#),
            None
        );
        assert_eq!(parse_latest(r#"{"message":"Not Found"}"#), None);
        assert_eq!(parse_latest("<html>"), None);
    }
}

#[cfg(test)]
mod live {
    /// Talks to GitHub: `cargo test -p pdfr live_check -- --ignored`.
    #[test]
    #[ignore]
    fn live_check() {
        let r = super::latest_version();
        println!("{r:?}");
        // Before the first release GitHub answers 404; any HTTP answer
        // proves the TLS connection works.
        assert!(
            r.is_ok() || r.as_ref().unwrap_err().contains("404"),
            "{r:?}"
        );
    }
}
