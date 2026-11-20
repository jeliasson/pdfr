//! PKCS#11: the NSS database (through NSS's own module, `libsoftokn3`) and
//! hardware tokens (through `p11-kit-proxy`). Modules are loaded at run
//! time from the system's library directories only; pdfr links no C code.
//!
//! The private key stays in NSS or on the token: pdfr sends a hash and gets
//! the signature back.

use super::{CertChoice, Source};
use crate::{Cert, SigAlg, Signer};
use cryptoki::context::{CInitializeArgs, Pkcs11};
use cryptoki::mechanism::Mechanism;
use cryptoki::object::{Attribute, AttributeType, ObjectClass};
use cryptoki::session::UserType;
use cryptoki::slot::Slot;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use zeroize::Zeroizing;

/// A loaded PKCS#11 module.
pub(crate) struct Module {
    ctx: Pkcs11,
    /// One operation at a time per module (tokens are not reentrant).
    lock: Mutex<()>,
}

/// Which key to use: module, slot, and the `CKA_ID` shared by the
/// certificate and its private key.
#[derive(Clone)]
pub(crate) struct KeyRef {
    module: Arc<Module>,
    slot: Slot,
    id: Vec<u8>,
}

// --- Finding modules -----------------------------------------------------------

const LIB_DIRS: &[&str] = &[
    "/usr/lib",
    "/usr/lib64",
    "/usr/lib/x86_64-linux-gnu",
    "/usr/lib/aarch64-linux-gnu",
    "/usr/lib/x86_64-linux-gnu/nss",
    "/usr/lib/aarch64-linux-gnu/nss",
    "/usr/lib64/nss",
    "/usr/local/lib",
    "/opt/homebrew/lib",
];

fn find_lib(names: &[&str]) -> Option<PathBuf> {
    LIB_DIRS
        .iter()
        .flat_map(|d| names.iter().map(move |n| Path::new(d).join(n)))
        .find(|p| p.is_file())
}

/// The NSS database to use: `$PDFR_NSS_DB`, `~/.pki/nssdb` (Chromium,
/// Okular, LibreOffice), or the default Firefox profile.
pub fn nss_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("PDFR_NSS_DB") {
        let d = PathBuf::from(d);
        return d.join("cert9.db").is_file().then_some(d);
    }
    let home = dirs_home()?;
    let pki = home.join(".pki/nssdb");
    if pki.join("cert9.db").is_file() {
        return Some(pki);
    }
    let ff = home.join(".mozilla/firefox");
    let mut profiles: Vec<PathBuf> = std::fs::read_dir(&ff)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.join("cert9.db").is_file())
        .collect();
    profiles.sort_by_key(|p| {
        let n = p.to_string_lossy();
        (!n.ends_with(".default-release"), !n.ends_with(".default"))
    });
    profiles.into_iter().next()
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// NSS's module opened on `dir`, read-only. NSS can be opened once per
/// process, so the first directory asked for is the one used.
fn nss(dir: &Path) -> Option<Arc<Module>> {
    static NSS: OnceLock<Option<Arc<Module>>> = OnceLock::new();
    NSS.get_or_init(|| {
        let lib = find_lib(&["libsoftokn3.so"])?;
        let dir = dir.to_str()?.replace('\'', "");
        let params = std::ffi::CString::new(format!(
            "configdir='sql:{dir}' certPrefix='' keyPrefix='' secmod='secmod.db' flags=readOnly"
        ))
        .ok()?;
        // NSS takes its configuration in C_Initialize's reserved argument,
        // which cryptoki doesn't pass: initialise through the raw function
        // list, then use cryptoki on the same (already loaded) library.
        unsafe {
            let raw = cryptoki_sys::Pkcs11::new(&lib).ok()?;
            let mut list = std::ptr::null_mut();
            if raw.C_GetFunctionList(&mut list) != cryptoki_sys::CKR_OK || list.is_null() {
                return None;
            }
            let mut args = cryptoki_sys::CK_C_INITIALIZE_ARGS {
                CreateMutex: None,
                DestroyMutex: None,
                LockMutex: None,
                UnlockMutex: None,
                flags: cryptoki_sys::CKF_OS_LOCKING_OK,
                pReserved: params.as_ptr() as *mut _,
            };
            let rv = (*list).C_Initialize?(&mut args as *mut _ as *mut _);
            if rv != cryptoki_sys::CKR_OK && rv != cryptoki_sys::CKR_CRYPTOKI_ALREADY_INITIALIZED {
                return None;
            }
            // Keep the library loaded for the life of the process.
            std::mem::forget(raw);
        }
        let ctx = Pkcs11::new(&lib).ok()?;
        Some(Arc::new(Module {
            ctx,
            lock: Mutex::new(()),
        }))
    })
    .clone()
}

/// p11-kit's proxy: every PKCS#11 module configured on the system.
fn p11kit() -> Option<Arc<Module>> {
    static P11: OnceLock<Option<Arc<Module>>> = OnceLock::new();
    P11.get_or_init(|| {
        let lib = find_lib(&["p11-kit-proxy.so"])?;
        let ctx = Pkcs11::new(&lib).ok()?;
        ctx.initialize(CInitializeArgs::OsThreads).ok()?;
        Some(Arc::new(Module {
            ctx,
            lock: Mutex::new(()),
        }))
    })
    .clone()
}

// --- Listing -------------------------------------------------------------------

pub(crate) fn list_nss(dir: &Path) -> Vec<CertChoice> {
    let Some(m) = nss(dir) else { return vec![] };
    list(&m, |_| Source::Nss(dir.to_path_buf()))
}

pub(crate) fn list_tokens() -> Vec<CertChoice> {
    let Some(m) = p11kit() else { return vec![] };
    list(&m, |label| Source::Token(label.to_string()))
}

fn list(m: &Arc<Module>, source: impl Fn(&str) -> Source) -> Vec<CertChoice> {
    let _g = m.lock.lock().unwrap_or_else(|e| e.into_inner());
    let mut out = Vec::new();
    let Ok(slots) = m.ctx.get_slots_with_token() else {
        return out;
    };
    for slot in slots {
        let Ok(info) = m.ctx.get_token_info(slot) else {
            continue;
        };
        let Ok(s) = m.ctx.open_ro_session(slot) else {
            continue;
        };
        // Certificates are public objects: no login needed to list them.
        let Ok(handles) = s.find_objects(&[Attribute::Class(ObjectClass::CERTIFICATE)]) else {
            continue;
        };
        let mut certs: Vec<(Cert, Vec<u8>)> = Vec::new();
        for h in handles {
            let Ok(attrs) = s.get_attributes(h, &[AttributeType::Value, AttributeType::Id]) else {
                continue;
            };
            let (mut der, mut id) = (None, Vec::new());
            for a in attrs {
                match a {
                    Attribute::Value(v) => der = Some(v),
                    Attribute::Id(i) => id = i,
                    _ => {}
                }
            }
            if let Some(c) = der.and_then(|d| Cert::from_der(&d).ok()) {
                certs.push((c, id));
            }
        }
        let pool: Vec<Cert> = certs.iter().map(|(c, _)| c.clone()).collect();
        for (cert, id) in &certs {
            if id.is_empty() || cert.is_ca() {
                continue;
            }
            // A private key may be hidden until login; tokens that need a
            // login show every certificate with an ID as signable.
            let has_key = s
                .find_objects(&[
                    Attribute::Class(ObjectClass::PRIVATE_KEY),
                    Attribute::Id(id.clone()),
                ])
                .is_ok_and(|k| !k.is_empty());
            let needs_pin = info.login_required();
            if !has_key && !needs_pin {
                continue;
            }
            let others: Vec<Cert> = pool.iter().filter(|c| *c != cert).cloned().collect();
            out.push(CertChoice {
                chain: crate::trust::order_chain(cert, &others),
                cert: cert.clone(),
                source: source(info.label()),
                needs_pin,
                pin_pad: info.protected_authentication_path(),
                key: KeyRef {
                    module: m.clone(),
                    slot,
                    id: id.clone(),
                },
            });
        }
    }
    out
}

// --- Signing -------------------------------------------------------------------

struct TokenSigner {
    cert: Cert,
    chain: Vec<Cert>,
    alg: SigAlg,
    key: KeyRef,
    pin: Option<Zeroizing<String>>,
    login: bool,
}

pub(crate) fn signer(
    c: &CertChoice,
    pin: Option<Zeroizing<String>>,
) -> Result<Box<dyn Signer>, String> {
    let spki = &c.cert.x.tbs_certificate.subject_public_key_info;
    let alg = match spki.algorithm.oid {
        crate::oids::RSA => SigAlg::RsaSha256,
        crate::oids::EC_PUBLIC_KEY => match crate::oids::curve(spki) {
            Some(crate::oids::P256) => SigAlg::EcdsaP256Sha256,
            Some(crate::oids::P384) => SigAlg::EcdsaP384Sha384,
            _ => return Err("this kind of elliptic curve key is not supported".into()),
        },
        _ => return Err("this kind of key is not supported".into()),
    };
    Ok(Box::new(TokenSigner {
        cert: c.cert.clone(),
        chain: c.chain.clone(),
        alg,
        key: c.key.clone(),
        pin,
        login: c.needs_pin,
    }))
}

/// DER prefix of a SHA-256 DigestInfo (RSA PKCS#1 v1.5).
const SHA256_DIGEST_INFO: [u8; 19] = [
    0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05,
    0x00, 0x04, 0x20,
];

impl Signer for TokenSigner {
    fn cert(&self) -> &Cert {
        &self.cert
    }
    fn chain(&self) -> &[Cert] {
        &self.chain
    }
    fn alg(&self) -> SigAlg {
        self.alg
    }
    fn sign(&self, msg: &[u8]) -> Result<Vec<u8>, String> {
        use sha2::Digest;
        let m = &self.key.module;
        let _g = m.lock.lock().unwrap_or_else(|e| e.into_inner());
        let s = m
            .ctx
            .open_ro_session(self.key.slot)
            .map_err(|e| format!("could not reach the certificate: {e}"))?;
        if self.login {
            let pin = self
                .pin
                .as_ref()
                .map(|p| cryptoki::types::AuthPin::new(p.as_str().to_string()));
            match s.login(UserType::User, pin.as_ref()) {
                Ok(()) => {}
                Err(cryptoki::error::Error::Pkcs11(
                    cryptoki::error::RvError::UserAlreadyLoggedIn,
                    _,
                )) => {}
                Err(cryptoki::error::Error::Pkcs11(cryptoki::error::RvError::PinIncorrect, _)) => {
                    return Err("wrong PIN or password".into());
                }
                Err(cryptoki::error::Error::Pkcs11(cryptoki::error::RvError::PinLocked, _)) => {
                    return Err("the PIN is locked".into());
                }
                Err(e) => return Err(format!("could not log in: {e}")),
            }
        }
        let key = s
            .find_objects(&[
                Attribute::Class(ObjectClass::PRIVATE_KEY),
                Attribute::Id(self.key.id.clone()),
            ])
            .map_err(|e| e.to_string())?
            .into_iter()
            .next()
            .ok_or("the private key was not found")?;
        let out = match self.alg {
            SigAlg::RsaSha256 => {
                let mut di = SHA256_DIGEST_INFO.to_vec();
                di.extend(sha2::Sha256::digest(msg));
                s.sign(&Mechanism::RsaPkcs, key, &di)
            }
            SigAlg::EcdsaP256Sha256 => s.sign(&Mechanism::Ecdsa, key, &sha2::Sha256::digest(msg)),
            SigAlg::EcdsaP384Sha384 => s.sign(&Mechanism::Ecdsa, key, &sha2::Sha384::digest(msg)),
        };
        if self.login {
            let _ = s.logout();
        }
        let raw = out.map_err(|e| format!("signing failed: {e}"))?;
        match self.alg {
            SigAlg::RsaSha256 => Ok(raw),
            SigAlg::EcdsaP256Sha256 => p256_der(&raw),
            SigAlg::EcdsaP384Sha384 => p384_der(&raw),
        }
    }
}

/// Raw `r || s` (PKCS#11) to DER, P-256.
fn p256_der(raw: &[u8]) -> Result<Vec<u8>, String> {
    p256::ecdsa::Signature::from_slice(raw)
        .map(|s| s.to_der().as_bytes().to_vec())
        .map_err(|e| e.to_string())
}

/// Raw `r || s` (PKCS#11) to DER, P-384.
fn p384_der(raw: &[u8]) -> Result<Vec<u8>, String> {
    p384::ecdsa::Signature::from_slice(raw)
        .map(|s| s.to_der().as_bytes().to_vec())
        .map_err(|e| e.to_string())
}
