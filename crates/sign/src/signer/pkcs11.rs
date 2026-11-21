//! PKCS#11: the NSS database (through NSS's own module, `libsoftokn3`) and
//! hardware tokens (through `p11-kit-proxy`). Modules are loaded at run
//! time from the system's library directories only; pdfr links no C code.
//!
//! The private key stays in NSS or on the token: pdfr sends a hash and gets
//! the signature back.

use super::{CertChoice, KeyHandle, PinTries, Source};
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
    /// The library, for the calls cryptoki doesn't offer (see `sign_raw`).
    lib: PathBuf,
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
            lib,
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
            lib,
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
                pin_tries: PinTries::of(&info),
                key: KeyHandle::Pkcs11(KeyRef {
                    module: m.clone(),
                    slot,
                    id: id.clone(),
                }),
            });
        }
    }
    out
}

// --- When nothing shows up ----------------------------------------------------

/// The first thing that stops a card from showing up, in words; for when
/// no card or token certificate was found. The ID card guide on pdfr.app
/// walks through the same steps. (macOS lists cards through the Keychain.)
#[cfg(not(target_os = "macos"))]
pub(crate) fn diagnose() -> Option<String> {
    const DRIVER: &str =
        "install OpenSC or your country's ID card software, then open this window again";
    let Some(m) = p11kit() else {
        return Some(
            "Smartcard support isn't installed. Install the p11-kit package to sign with an ID card or token."
                .into(),
        );
    };
    if !Path::new("/run/pcscd/pcscd.comm").exists() {
        return Some(
            "The smartcard service isn't running. Start it with: sudo systemctl enable --now pcscd.socket"
                .into(),
        );
    }
    let _g = m.lock.lock().unwrap_or_else(|e| e.into_inner());
    let all = m.ctx.get_all_slots().unwrap_or_default();
    let with_token = m.ctx.get_slots_with_token().unwrap_or_default();
    let empty_readers = all.iter().filter(|s| !with_token.contains(s)).count();
    Some(if empty_readers > 0 {
        format!(
            "A card reader is connected, but there's no card in it that pdfr can read. If the card is in, it needs its driver: {DRIVER}."
        )
    } else {
        format!(
            "No card reader found. Plug in your reader and insert the card. If it's plugged in, {DRIVER}."
        )
    })
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
    key: &KeyRef,
    pin: Option<Zeroizing<String>>,
) -> Result<Box<dyn Signer>, String> {
    Ok(Box::new(TokenSigner {
        alg: super::alg_of(&c.cert)?,
        cert: c.cert.clone(),
        chain: c.chain.clone(),
        key: key.clone(),
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
        // What the token signs: a DigestInfo for RSA, the bare hash for ECDSA.
        let (mech, data) = match self.alg {
            SigAlg::RsaSha256 => {
                let mut di = SHA256_DIGEST_INFO.to_vec();
                di.extend(sha2::Sha256::digest(msg));
                (Mechanism::RsaPkcs, di)
            }
            SigAlg::EcdsaP256Sha256 => (Mechanism::Ecdsa, sha2::Sha256::digest(msg).to_vec()),
            SigAlg::EcdsaP384Sha384 => (Mechanism::Ecdsa, sha2::Sha384::digest(msg).to_vec()),
        };
        let raw = self
            .sign_on_token(&mech, &data)
            .map_err(|e| self.explain(e))?;
        match self.alg {
            SigAlg::RsaSha256 => Ok(raw),
            SigAlg::EcdsaP256Sha256 => p256_der(&raw),
            SigAlg::EcdsaP384Sha384 => p384_der(&raw),
        }
    }
}

/// Why signing on a token failed.
enum TokenError {
    WrongPin,
    PinLocked,
    Other(String),
}

impl TokenSigner {
    fn sign_on_token(&self, mech: &Mechanism, data: &[u8]) -> Result<Vec<u8>, TokenError> {
        let m = &self.key.module;
        let s = m
            .ctx
            .open_ro_session(self.key.slot)
            .map_err(|e| TokenError::Other(format!("could not reach the certificate: {e}")))?;
        let pin = self
            .pin
            .as_ref()
            .map(|p| cryptoki::types::AuthPin::new(p.as_str().to_string()));
        if self.login {
            match s.login(UserType::User, pin.as_ref()) {
                Ok(())
                | Err(cryptoki::error::Error::Pkcs11(
                    cryptoki::error::RvError::UserAlreadyLoggedIn,
                    _,
                )) => {}
                Err(e) => return Err(login_error(e)),
            }
        }
        let found = s
            .find_objects(&[
                Attribute::Class(ObjectClass::PRIVATE_KEY),
                Attribute::Id(self.key.id.clone()),
            ])
            .map_err(|e| TokenError::Other(e.to_string()))
            .and_then(|k| {
                k.into_iter()
                    .next()
                    .ok_or(TokenError::Other("the private key was not found".into()))
            });
        let out = found.and_then(|key| {
            // ⛔ The signing keys of most national ID cards (DNIe, Belgian
            // eID, Estonian ID-kaart…) are CKA_ALWAYS_AUTHENTICATE: the PIN
            // is asked again between C_SignInit and C_Sign. cryptoki's
            // sign() does both in one call, so those cards fail with
            // CKR_USER_NOT_LOGGED_IN; they sign through the raw path.
            let always = s
                .get_attributes(key, &[AttributeType::AlwaysAuthenticate])
                .ok()
                .into_iter()
                .flatten()
                .any(|a| matches!(a, Attribute::AlwaysAuthenticate(true)));
            if always {
                sign_raw(
                    &m.lib,
                    self.key.slot,
                    &self.key.id,
                    self.pin.as_ref().map(|p| p.as_str()),
                    mech,
                    data,
                )
            } else {
                s.sign(mech, key, data)
                    .map_err(|e| TokenError::Other(format!("signing failed: {e}")))
            }
        });
        if self.login {
            let _ = s.logout();
        }
        out
    }

    /// The message for a failed signature: with a wrong PIN, how many tries
    /// the card says are left, so nobody locks a card by surprise.
    fn explain(&self, e: TokenError) -> String {
        match e {
            TokenError::WrongPin => {
                let tries = self
                    .key
                    .module
                    .ctx
                    .get_token_info(self.key.slot)
                    .map(|i| PinTries::of(&i))
                    .unwrap_or(PinTries::Ok);
                match tries {
                    PinTries::Ok => format!("{WRONG_PIN}."),
                    PinTries::Low => format!("{WRONG_PIN}: only a few tries are left."),
                    PinTries::FinalTry => {
                        format!("{WRONG_PIN}: one try left, after which the card locks.")
                    }
                    PinTries::Locked => format!("{PIN_LOCKED}{PIN_LOCKED_HELP}"),
                }
            }
            TokenError::PinLocked => format!("{PIN_LOCKED}{PIN_LOCKED_HELP}"),
            TokenError::Other(e) => e,
        }
    }
}

/// The start of every wrong-PIN message (see `is_pin_error`).
pub const WRONG_PIN: &str = "wrong PIN or password";
/// The start of every locked-PIN message.
pub const PIN_LOCKED: &str = "the PIN is locked";
const PIN_LOCKED_HELP: &str =
    ". Unlock it with your PUK, using the tool from whoever issued the card.";

/// Whether a signing error is about the PIN, so the dialog asks again.
pub fn is_pin_error(e: &str) -> bool {
    e.contains(WRONG_PIN) || e.contains(PIN_LOCKED)
}

fn login_error(e: cryptoki::error::Error) -> TokenError {
    use cryptoki::error::{Error, RvError};
    match e {
        Error::Pkcs11(RvError::PinIncorrect, _) => TokenError::WrongPin,
        Error::Pkcs11(RvError::PinLocked, _) => TokenError::PinLocked,
        e => TokenError::Other(format!("could not log in: {e}")),
    }
}

fn rv_error(rv: cryptoki_sys::CK_RV, what: &str) -> TokenError {
    match rv {
        cryptoki_sys::CKR_PIN_INCORRECT => TokenError::WrongPin,
        cryptoki_sys::CKR_PIN_LOCKED => TokenError::PinLocked,
        rv => TokenError::Other(format!("{what} (PKCS#11 error {rv:#x})")),
    }
}

/// Signs with a CKA_ALWAYS_AUTHENTICATE key: log in, C_SignInit, log in
/// again for this one operation (CKU_CONTEXT_SPECIFIC), C_Sign. The same
/// PIN serves both logins; a PIN-pad reader asks on its own pad.
///
/// Through the raw function list of the already initialised library, the
/// way `nss()` initialises NSS: cryptoki offers no single-part C_Sign after
/// a separate C_SignInit.
fn sign_raw(
    lib: &Path,
    slot: Slot,
    id: &[u8],
    pin: Option<&str>,
    mech: &Mechanism,
    data: &[u8],
) -> Result<Vec<u8>, TokenError> {
    use cryptoki_sys::*;
    let fail = |what: &str| TokenError::Other(format!("signing failed: {what}"));
    // SAFETY: every pointer handed to the library points into a live local
    // for the duration of the call; lengths match their buffers.
    unsafe {
        let raw = cryptoki_sys::Pkcs11::new(lib).map_err(|e| fail(&e.to_string()))?;
        let mut list = std::ptr::null_mut();
        if raw.C_GetFunctionList(&mut list) != CKR_OK || list.is_null() {
            return Err(fail("the card's module has no function list"));
        }
        let f = &*list;
        let (Some(open), Some(close), Some(login), Some(logout)) =
            (f.C_OpenSession, f.C_CloseSession, f.C_Login, f.C_Logout)
        else {
            return Err(fail("the card's module is incomplete"));
        };
        let (Some(find_init), Some(find), Some(find_final), Some(sign_init), Some(sign)) = (
            f.C_FindObjectsInit,
            f.C_FindObjects,
            f.C_FindObjectsFinal,
            f.C_SignInit,
            f.C_Sign,
        ) else {
            return Err(fail("the card's module is incomplete"));
        };

        let mut h: CK_SESSION_HANDLE = 0;
        let rv = open(
            slot.into(),
            CKF_SERIAL_SESSION,
            std::ptr::null_mut(),
            None,
            &mut h,
        );
        if rv != CKR_OK {
            return Err(rv_error(rv, "could not reach the certificate"));
        }
        let mut pin_bytes: Vec<u8> = pin.map(|p| p.as_bytes().to_vec()).unwrap_or_default();
        let (pin_ptr, pin_len) = if pin.is_some() {
            (pin_bytes.as_mut_ptr(), pin_bytes.len() as CK_ULONG)
        } else {
            (std::ptr::null_mut(), 0)
        };

        let result = (|| {
            let rv = login(h, CKU_USER, pin_ptr, pin_len);
            if rv != CKR_OK && rv != CKR_USER_ALREADY_LOGGED_IN {
                return Err(rv_error(rv, "could not log in"));
            }
            let mut class: CK_OBJECT_CLASS = CKO_PRIVATE_KEY;
            let mut id = id.to_vec();
            let mut template = [
                CK_ATTRIBUTE {
                    type_: CKA_CLASS,
                    pValue: &mut class as *mut _ as *mut _,
                    ulValueLen: std::mem::size_of::<CK_OBJECT_CLASS>() as CK_ULONG,
                },
                CK_ATTRIBUTE {
                    type_: CKA_ID,
                    pValue: id.as_mut_ptr() as *mut _,
                    ulValueLen: id.len() as CK_ULONG,
                },
            ];
            let rv = find_init(h, template.as_mut_ptr(), template.len() as CK_ULONG);
            if rv != CKR_OK {
                return Err(rv_error(rv, "could not look for the key"));
            }
            let mut key: CK_OBJECT_HANDLE = 0;
            let mut count: CK_ULONG = 0;
            let rv = find(h, &mut key, 1, &mut count);
            find_final(h);
            if rv != CKR_OK || count == 0 {
                return Err(TokenError::Other("the private key was not found".into()));
            }

            let mut mechanism = CK_MECHANISM {
                mechanism: match mech {
                    Mechanism::RsaPkcs => CKM_RSA_PKCS,
                    _ => CKM_ECDSA,
                },
                pParameter: std::ptr::null_mut(),
                ulParameterLen: 0,
            };
            let rv = sign_init(h, &mut mechanism, key);
            if rv != CKR_OK {
                return Err(rv_error(rv, "the card refused to start signing"));
            }
            let rv = login(h, CKU_CONTEXT_SPECIFIC, pin_ptr, pin_len);
            if rv != CKR_OK {
                return Err(rv_error(rv, "the card refused the PIN for signing"));
            }
            let mut data = data.to_vec();
            let mut len: CK_ULONG = 0;
            let rv = sign(
                h,
                data.as_mut_ptr(),
                data.len() as CK_ULONG,
                std::ptr::null_mut(),
                &mut len,
            );
            if rv != CKR_OK {
                return Err(rv_error(rv, "signing failed"));
            }
            let mut out = vec![0u8; len as usize];
            let rv = sign(
                h,
                data.as_mut_ptr(),
                data.len() as CK_ULONG,
                out.as_mut_ptr(),
                &mut len,
            );
            if rv != CKR_OK {
                return Err(rv_error(rv, "signing failed"));
            }
            out.truncate(len as usize);
            Ok(out)
        })();

        logout(h);
        close(h);
        zeroize::Zeroize::zeroize(&mut pin_bytes);
        result
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
