//! Where signing keys come from.
//!
//! - The NSS database (`~/.pki/nssdb`, shared by Chromium, Okular and
//!   LibreOffice, or a Firefox profile): certificates installed in the
//!   browser, such as the Spanish FNMT certificate.
//! - PKCS#11 tokens (smartcards, national ID cards, USB tokens) through
//!   p11-kit, which offers every module the system has configured.
//! - A PKCS#12 file (.p12 / .pfx).

#[cfg(target_os = "macos")]
mod keychain;
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
    /// The macOS Keychain.
    Keychain,
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
            Source::Keychain => "Keychain".into(),
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
    /// What the token says about wrong PINs so far.
    pub pin_tries: PinTries,
    pub(crate) key: KeyHandle,
}

/// How close a card is to locking its PIN, as the card reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinTries {
    Ok,
    /// A wrong PIN was entered; a few tries are left.
    Low,
    /// One more wrong PIN locks the card.
    FinalTry,
    /// The PIN is locked (the PUK unlocks it).
    Locked,
}

impl PinTries {
    pub(crate) fn of(info: &cryptoki::slot::TokenInfo) -> PinTries {
        Self::from_flags(
            info.user_pin_locked(),
            info.user_pin_final_try(),
            info.user_pin_count_low(),
        )
    }

    fn from_flags(locked: bool, final_try: bool, low: bool) -> PinTries {
        if locked {
            PinTries::Locked
        } else if final_try {
            PinTries::FinalTry
        } else if low {
            PinTries::Low
        } else {
            PinTries::Ok
        }
    }
}

/// How to reach a certificate's private key.
pub(crate) enum KeyHandle {
    Pkcs11(pkcs11::KeyRef),
    #[cfg(target_os = "macos")]
    Keychain(security_framework::identity::SecIdentity),
}

/// The signature algorithm for a certificate's key.
pub(crate) fn alg_of(cert: &Cert) -> Result<crate::SigAlg, String> {
    let spki = &cert.x.tbs_certificate.subject_public_key_info;
    match spki.algorithm.oid {
        crate::oids::RSA => Ok(crate::SigAlg::RsaSha256),
        crate::oids::EC_PUBLIC_KEY => match crate::oids::curve(spki) {
            Some(crate::oids::P256) => Ok(crate::SigAlg::EcdsaP256Sha256),
            Some(crate::oids::P384) => Ok(crate::SigAlg::EcdsaP384Sha384),
            _ => Err("this kind of elliptic curve key is not supported".into()),
        },
        _ => Err("this kind of key is not supported".into()),
    }
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
        match &self.key {
            KeyHandle::Pkcs11(k) => pkcs11::signer(self, k, pin),
            #[cfg(target_os = "macos")]
            KeyHandle::Keychain(id) => {
                let _ = pin; // macOS asks for permission itself
                keychain::signer(self, id)
            }
        }
    }
}

/// Certificates with private keys: the macOS Keychain, the NSS database
/// and PKCS#11 tokens. May take a moment (cards are asked); call off the
/// UI thread.
pub fn discover() -> Vec<CertChoice> {
    let mut out = Vec::new();
    #[cfg(target_os = "macos")]
    out.extend(keychain::list());
    if let Some(dir) = pkcs11::nss_dir() {
        out.extend(pkcs11::list_nss(&dir));
    }
    out.extend(pkcs11::list_tokens());
    // The same certificate through two routes: keep the first.
    let mut seen = std::collections::HashSet::new();
    out.retain(|c| seen.insert(c.cert.der.clone()));
    sort_for_signing(&mut out);
    out
}

/// Signature certificates first. An ID card holds two: one to log in to
/// websites and one to sign (non-repudiation). Both may sign, but the
/// signing one is what authorities expect, so it is offered first.
fn sort_for_signing(out: &mut [CertChoice]) {
    out.sort_by_key(|c| !c.cert.is_signature_cert());
}

impl CertChoice {
    /// For a card certificate that is only for logging in, while the same
    /// card also holds a signature certificate: a note for the list.
    pub fn login_only(&self, all: &[&CertChoice]) -> bool {
        matches!(self.source, Source::Token(_))
            && !self.cert.is_signature_cert()
            && all
                .iter()
                .any(|o| o.source == self.source && o.cert.is_signature_cert())
    }
}

/// Why no card or token shows up, when that can be told: the first thing
/// to fix, in words. None when tokens are visible or nothing is wrong.
#[cfg(not(target_os = "macos"))]
pub fn diagnose() -> Option<String> {
    pkcs11::diagnose()
}

#[cfg(test)]
mod tests {
    use super::PinTries;

    #[test]
    fn pin_tries_reads_the_worst_flag() {
        assert_eq!(PinTries::from_flags(false, false, false), PinTries::Ok);
        assert_eq!(PinTries::from_flags(false, false, true), PinTries::Low);
        assert_eq!(PinTries::from_flags(false, true, true), PinTries::FinalTry);
        assert_eq!(PinTries::from_flags(true, true, true), PinTries::Locked);
    }
}
