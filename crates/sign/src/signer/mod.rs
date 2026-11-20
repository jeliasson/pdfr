//! Where signing keys come from.
//!
//! - The NSS database (`~/.pki/nssdb`, shared by Chromium, Okular and
//!   LibreOffice, or a Firefox profile): certificates installed in the
//!   browser, such as the Spanish FNMT certificate.
//! - PKCS#11 tokens (smartcards, national ID cards, USB tokens) through
//!   p11-kit, which offers every module the system has configured.
//! - A PKCS#12 file (.p12 / .pfx).

pub mod p12;
pub mod pkcs11;

use crate::{Cert, Signer};
use std::path::PathBuf;
use zeroize::Zeroizing;

/// Where a certificate's key lives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// The NSS database at this directory.
    Nss(PathBuf),
    /// A PKCS#11 token, by its label.
    Token(String),
    /// A PKCS#12 file.
    File(PathBuf),
}

impl Source {
    pub fn describe(&self) -> String {
        match self {
            Source::Nss(p) => {
                if p.components().any(|c| c.as_os_str() == "firefox") {
                    "Firefox certificates".into()
                } else {
                    "Browser certificates (NSS)".into()
                }
            }
            Source::Token(t) => format!("Card or token: {t}"),
            Source::File(p) => p.file_name().map_or("Certificate file".into(), |n| {
                n.to_string_lossy().into_owned()
            }),
        }
    }
}

/// A certificate with a private key that pdfr could sign with.
pub struct CertChoice {
    pub cert: Cert,
    /// Issuing certificates found next to it.
    pub chain: Vec<Cert>,
    pub source: Source,
    /// A PIN or password is needed to sign.
    pub needs_pin: bool,
    /// The token has its own PIN pad.
    pub pin_pad: bool,
    pub(crate) key: pkcs11::KeyRef,
}

impl CertChoice {
    /// Why this certificate can't sign now, if it can't.
    pub fn problem(&self) -> Option<String> {
        self.cert.sign_problem(crate::now())
    }

    /// A signer for this certificate, logging in with `pin` if needed.
    pub fn signer(&self, pin: Option<Zeroizing<String>>) -> Result<Box<dyn Signer>, String> {
        if let Some(p) = self.problem() {
            return Err(format!("this certificate can't sign: {p}"));
        }
        pkcs11::signer(self, pin)
    }
}

/// Certificates with private keys from the NSS database and PKCS#11
/// tokens. May take a moment (cards are asked); call off the UI thread.
pub fn discover() -> Vec<CertChoice> {
    let mut out = Vec::new();
    if let Some(dir) = pkcs11::nss_dir() {
        out.extend(pkcs11::list_nss(&dir));
    }
    out.extend(pkcs11::list_tokens());
    // The same certificate through two routes: keep the first.
    let mut seen = std::collections::HashSet::new();
    out.retain(|c| seen.insert(c.cert.der.clone()));
    out
}
