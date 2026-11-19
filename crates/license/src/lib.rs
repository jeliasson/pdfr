//! pdfr licence keys.
//!
//! A key is `PDFR1.<payload>.<signature>`: the payload is JSON, the signature
//! is Ed25519 over the payload's bytes, both base64url without padding. Keys
//! are made by pdfr.app, which alone holds the private key; pdfr checks them
//! here, offline, with the public keys in [`TRUSTED_KEYS`]. The format is
//! specified, with a test vector, in `docs/license-keys.md`.
//!
//! Revocation and the per-key device limit are checked online by the app
//! (`crates/app/src/license_ui/online.rs`); nothing here touches the network.

use std::fmt;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};

pub const PREFIX: &str = "PDFR1.";
/// Signature length in base64url characters (64 bytes, no padding).
const SIG_CHARS: usize = 86;

/// The public keys release builds trust, by key id. Their private halves
/// live only on pdfr.app (`LICENSE_SIGNING_KEY`).
pub const TRUSTED_KEYS: &[(u8, [u8; 32])] = &[
    // kid 1, i-7pkweUYHOpO3Kjl2WTVdP8Fpur7kBRSleFGAHOjTg (2026-11-17)
    (
        1,
        [
            139, 238, 233, 147, 7, 148, 96, 115, 169, 59, 114, 163, 151, 101, 147, 85, 211, 252,
            22, 155, 171, 238, 64, 81, 74, 87, 133, 24, 1, 206, 141, 56,
        ],
    ),
];

/// Where people get and manage keys.
pub const GET_URL: &str = "https://pdfr.app/get";
pub const ACCOUNT_URL: &str = "https://pdfr.app/account";
pub const PRICING_URL: &str = "https://pdfr.app/#pricing";

/// What a key's JSON payload holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Payload {
    /// Format version, 1.
    pub v: u32,
    /// Which public key signed it.
    pub kid: u8,
    /// The licence's id on pdfr.app (a UUID).
    pub id: String,
    /// "personal", "business", "founder", or a later plan.
    pub plan: String,
    /// Who it is registered to.
    pub name: String,
    /// Issue date, `YYYY-MM-DD`.
    pub issued: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Plan {
    Personal,
    Business,
    Founder,
    /// A plan this version of pdfr doesn't know yet.
    Other(String),
}

impl Plan {
    fn parse(s: &str) -> Plan {
        match s {
            "personal" => Plan::Personal,
            "business" => Plan::Business,
            "founder" => Plan::Founder,
            other => Plan::Other(other.to_string()),
        }
    }

    pub fn label(&self) -> String {
        match self {
            Plan::Personal => "Personal".into(),
            Plan::Business => "Business".into(),
            Plan::Founder => "Founder".into(),
            Plan::Other(s) => {
                let mut c = s.chars();
                c.next()
                    .map(|f| f.to_uppercase().chain(c).collect())
                    .unwrap_or_default()
            }
        }
    }

    /// Whether the licence covers commercial use. Unknown future plans are
    /// assumed paid.
    pub fn commercial(&self) -> bool {
        !matches!(self, Plan::Personal)
    }
}

/// A verified licence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct License {
    pub id: String,
    pub plan: Plan,
    pub name: String,
    pub issued: String,
    pub kid: u8,
    /// The key itself, normalised (no whitespace or surrounding text).
    pub key: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// No `PDFR1.` key in the text.
    NotFound,
    /// Something that starts like a key but isn't one (cut off, mistyped).
    Malformed,
    /// Signed with a key this version of pdfr doesn't know.
    UnknownSigner(u8),
    /// The signature doesn't match: the key was changed or made up.
    BadSignature,
    /// A newer key format; update pdfr.
    Version(u32),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotFound => write!(f, "No licence key found. Keys start with \"PDFR1.\"."),
            Error::Malformed => write!(
                f,
                "This key is incomplete. Copy the whole key and try again."
            ),
            Error::UnknownSigner(_) => {
                write!(f, "This key isn't from pdfr.app, or needs a newer pdfr.")
            }
            Error::BadSignature => write!(f, "This key isn't valid."),
            Error::Version(_) => write!(f, "This key needs a newer version of pdfr."),
        }
    }
}

impl std::error::Error for Error {}

fn is_b64(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

/// Finds a key in pasted text: an email, with line breaks inside the key,
/// quote markers, text before and after. Returns it without whitespace.
pub fn extract(text: &str) -> Result<String, Error> {
    let start = text.find(PREFIX).ok_or(Error::NotFound)?;
    let mut chars = text[start + PREFIX.len()..].chars().peekable();
    let mut payload = String::new();
    // The payload runs to the next dot; line breaks (and an email client's
    // "> " quoting after them) may sit inside it.
    loop {
        match chars.next() {
            Some('.') if !payload.is_empty() => break,
            Some(c) if is_b64(c) => payload.push(c),
            Some(c) if c.is_whitespace() || c == '>' => {}
            _ => return Err(Error::Malformed),
        }
    }
    // The signature has a fixed length, so text after it is never swallowed.
    let mut sig = String::new();
    while sig.len() < SIG_CHARS {
        match chars.next() {
            Some(c) if is_b64(c) => sig.push(c),
            Some(c) if c.is_whitespace() || c == '>' => {}
            _ => return Err(Error::Malformed),
        }
    }
    if chars.peek().is_some_and(|&c| is_b64(c)) {
        return Err(Error::Malformed);
    }
    Ok(format!("{PREFIX}{payload}.{sig}"))
}

/// Verifies a key (or text containing one) against `keys`.
pub fn verify(text: &str, keys: &[(u8, [u8; 32])]) -> Result<License, Error> {
    let key = extract(text)?;
    let body = &key[PREFIX.len()..];
    let (p, s) = body.split_once('.').ok_or(Error::Malformed)?;
    let payload_bytes = B64.decode(p).map_err(|_| Error::Malformed)?;
    let sig: [u8; 64] = B64
        .decode(s)
        .ok()
        .and_then(|b| b.try_into().ok())
        .ok_or(Error::Malformed)?;
    // The key id is needed to pick the public key; nothing else in the
    // payload is trusted before the signature checks out.
    #[derive(Deserialize)]
    struct Kid {
        kid: u8,
    }
    let kid = serde_json::from_slice::<Kid>(&payload_bytes)
        .map_err(|_| Error::Malformed)?
        .kid;
    let public = keys
        .iter()
        .find(|(k, _)| *k == kid)
        .ok_or(Error::UnknownSigner(kid))?;
    let vk = VerifyingKey::from_bytes(&public.1).map_err(|_| Error::UnknownSigner(kid))?;
    vk.verify_strict(&payload_bytes, &Signature::from_bytes(&sig))
        .map_err(|_| Error::BadSignature)?;
    let payload: Payload = serde_json::from_slice(&payload_bytes).map_err(|_| Error::Malformed)?;
    if payload.v != 1 {
        return Err(Error::Version(payload.v));
    }
    Ok(License {
        id: payload.id,
        plan: Plan::parse(&payload.plan),
        name: payload.name,
        issued: payload.issued,
        kid,
        key,
    })
}

/// Makes a key. pdfr.app does the same in TypeScript; this is for tests and
/// `pdfr-license issue`.
pub fn issue(payload: &Payload, seed: &[u8; 32]) -> String {
    let bytes = serde_json::to_vec(payload).expect("payload serialises");
    let sig = SigningKey::from_bytes(seed).sign(&bytes);
    format!(
        "{PREFIX}{}.{}",
        B64.encode(&bytes),
        B64.encode(sig.to_bytes())
    )
}

/// The public key for a private seed.
pub fn public_key(seed: &[u8; 32]) -> [u8; 32] {
    SigningKey::from_bytes(seed).verifying_key().to_bytes()
}

/// The keys this build trusts: [`TRUSTED_KEYS`], plus `PDFR_LICENSE_TEST_KEY`
/// (`<kid>:<base64url public key>`) in debug builds, for testing against a
/// staging server.
pub fn trusted_keys() -> Vec<(u8, [u8; 32])> {
    let mut keys = TRUSTED_KEYS.to_vec();
    if cfg!(debug_assertions)
        && let Ok(v) = std::env::var("PDFR_LICENSE_TEST_KEY")
        && let Some(k) = parse_public(&v)
    {
        keys.push(k);
    }
    keys
}

/// Parses `<kid>:<base64url public key>`.
pub fn parse_public(s: &str) -> Option<(u8, [u8; 32])> {
    let (kid, key) = s.trim().split_once(':')?;
    let bytes: [u8; 32] = B64.decode(key).ok()?.try_into().ok()?;
    Some((kid.parse().ok()?, bytes))
}

pub fn encode(bytes: &[u8]) -> String {
    B64.encode(bytes)
}

pub fn decode32(s: &str) -> Option<[u8; 32]> {
    B64.decode(s.trim()).ok()?.try_into().ok()
}

/* ------------------------------------------------------------------------- */
/* Files                                                                      */

/// pdfr's data folder: `--data-dir` when given, otherwise `~/.local/share/pdfr`
/// (or the platform's equivalent).
pub fn data_dir(over: Option<&Path>) -> Option<PathBuf> {
    match over {
        Some(d) => Some(d.to_path_buf()),
        None => dirs::data_dir().map(|d| d.join("pdfr")),
    }
}

const KEY_FILE: &str = "license.key";
const INSTALL_FILE: &str = "install_id";
const STATE_FILE: &str = "license_state.json";

/// The saved key, if any (unverified).
pub fn load_key(dir: &Path) -> Option<String> {
    std::fs::read_to_string(dir.join(KEY_FILE)).ok()
}

pub fn save_key(dir: &Path, key: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    write_atomic(&dir.join(KEY_FILE), format!("{key}\n").as_bytes())
}

pub fn remove_key(dir: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(dir.join(KEY_FILE)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// This installation's random id, made on first use. It identifies the
/// computer to the device limit and nothing else.
pub fn install_id(dir: &Path) -> String {
    let path = dir.join(INSTALL_FILE);
    if let Ok(s) = std::fs::read_to_string(&path) {
        let s = s.trim();
        if s.len() == 32 && s.chars().all(|c| c.is_ascii_hexdigit()) {
            return s.to_string();
        }
    }
    let mut b = [0u8; 16];
    if getrandom::fill(&mut b).is_err() {
        // No randomness: fall back to the time, still unique enough.
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        b = t.to_le_bytes();
    }
    let id: String = b.iter().map(|x| format!("{x:02x}")).collect();
    let _ = std::fs::create_dir_all(dir);
    let _ = write_atomic(&path, id.as_bytes());
    id
}

/// What the app remembers between runs: when the reminder last showed and
/// what the last online check said.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    /// First start (Unix seconds); the grace period counts from here.
    #[serde(default)]
    pub first_run: u64,
    /// When the reminder last showed.
    #[serde(default)]
    pub last_nag: u64,
    /// Saves since the reminder last showed.
    #[serde(default)]
    pub saves: u32,
    /// When the last online check finished.
    #[serde(default)]
    pub last_check: u64,
    /// Its answer: "active", "revoked", "device_limit", "unknown".
    #[serde(default)]
    pub last_status: Option<String>,
    /// The server's message with that answer, if any.
    #[serde(default)]
    pub last_message: Option<String>,
}

impl State {
    pub fn load(dir: &Path) -> State {
        std::fs::read(dir.join(STATE_FILE))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let json = serde_json::to_vec_pretty(self).expect("state serialises");
        write_atomic(&dir.join(STATE_FILE), &json)
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/* ------------------------------------------------------------------------- */
/* The test vector (docs/license-keys.md)                                     */

/// The test vector's private seed: bytes 0..32. Never trusted by releases.
pub const TEST_SEED: [u8; 32] = {
    let mut s = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        s[i] = i as u8;
        i += 1;
    }
    s
};
pub const TEST_KID: u8 = 255;

pub fn test_payload() -> Payload {
    Payload {
        v: 1,
        kid: TEST_KID,
        id: "00000000-0000-4000-8000-000000000001".into(),
        plan: "business".into(),
        name: "Test Licensee".into(),
        issued: "2026-11-17".into(),
    }
}

/// The key `issue(&test_payload(), &TEST_SEED)` must produce, in every
/// implementation.
pub const TEST_KEY: &str = "PDFR1.eyJ2IjoxLCJraWQiOjI1NSwiaWQiOiIwMDAwMDAwMC0wMDAwLTQwMDAtODAwMC0wMDAwMDAwMDAwMDEiLCJwbGFuIjoiYnVzaW5lc3MiLCJuYW1lIjoiVGVzdCBMaWNlbnNlZSIsImlzc3VlZCI6IjIwMjYtMTEtMTcifQ.1RTd1fKstl83t5f5eTYD6C_OjAz85DUgOnr5T-pWLF1RESIIRLMHeQXWRQN-XErTy2VFvDOA-cpGYDfmYsCLBg";

/// The test public key, for injecting into tests.
pub fn test_keys() -> Vec<(u8, [u8; 32])> {
    vec![(TEST_KID, public_key(&TEST_SEED))]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(plan: &str, name: &str) -> String {
        let mut p = test_payload();
        p.plan = plan.into();
        p.name = name.into();
        issue(&p, &TEST_SEED)
    }

    #[test]
    fn test_vector() {
        let k = issue(&test_payload(), &TEST_SEED);
        println!("{k}\npublic {}", encode(&public_key(&TEST_SEED)));
        assert_eq!(k, TEST_KEY);
        let l = verify(TEST_KEY, &test_keys()).unwrap();
        assert_eq!(l.plan, Plan::Business);
        assert_eq!(l.name, "Test Licensee");
        assert_eq!(l.issued, "2026-11-17");
        assert_eq!(l.kid, TEST_KID);
    }

    #[test]
    fn plans() {
        let l = verify(&key("personal", "Jane"), &test_keys()).unwrap();
        assert_eq!(l.plan, Plan::Personal);
        assert!(!l.plan.commercial());
        let l = verify(&key("founder", "Acme"), &test_keys()).unwrap();
        assert!(l.plan.commercial());
        let l = verify(&key("team", "Acme"), &test_keys()).unwrap();
        assert_eq!(l.plan, Plan::Other("team".into()));
        assert_eq!(l.plan.label(), "Team");
    }

    #[test]
    fn pasted_from_an_email() {
        let k = key("personal", "Jane Doe");
        let wrapped: String = k
            .chars()
            .collect::<Vec<_>>()
            .chunks(40)
            .map(|c| c.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n> ");
        let mail = format!("Hi Jane,\n\nYour key:\n\n> {wrapped}\n\nThanks for registering!");
        let l = verify(&mail, &test_keys()).unwrap();
        assert_eq!(l.name, "Jane Doe");
        assert_eq!(l.key, k);
        // Text straight after the key is not taken as part of it.
        assert!(verify(&format!("{k}Thanks"), &test_keys()).is_err());
        assert!(verify(&format!("  {k}.  "), &test_keys()).is_ok());
    }

    #[test]
    fn rejects() {
        let k = key("personal", "Jane");
        assert_eq!(verify("hello", &test_keys()), Err(Error::NotFound));
        assert_eq!(
            verify(&k[..k.len() - 5], &test_keys()),
            Err(Error::Malformed)
        );
        assert_eq!(verify(&k, &[]), Err(Error::UnknownSigner(TEST_KID)));
        // Another payload under the same signature.
        let other = key("founder", "Jane");
        let forged = format!(
            "{}.{}",
            other.rsplit_once('.').unwrap().0,
            k.rsplit_once('.').unwrap().1
        );
        assert_eq!(verify(&forged, &test_keys()), Err(Error::BadSignature));
        // A flipped signature character.
        let mut b = k.clone().into_bytes();
        let last = b.len() - 3;
        b[last] = if b[last] == b'A' { b'B' } else { b'A' };
        assert_eq!(
            verify(&String::from_utf8(b).unwrap(), &test_keys()),
            Err(Error::BadSignature)
        );
        // Signed by another key that claims the trusted id.
        let mut seed = TEST_SEED;
        seed[0] = 99;
        assert_eq!(
            verify(&issue(&test_payload(), &seed), &test_keys()),
            Err(Error::BadSignature)
        );
        // Newer format.
        let mut p = test_payload();
        p.v = 2;
        assert_eq!(
            verify(&issue(&p, &TEST_SEED), &test_keys()),
            Err(Error::Version(2))
        );
    }

    #[test]
    fn files() {
        let dir = std::env::temp_dir().join(format!("pdfr-license-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(load_key(&dir).is_none());
        save_key(&dir, TEST_KEY).unwrap();
        assert_eq!(load_key(&dir).unwrap().trim(), TEST_KEY);
        remove_key(&dir).unwrap();
        remove_key(&dir).unwrap();
        assert!(load_key(&dir).is_none());
        let id = install_id(&dir);
        assert_eq!(id.len(), 32);
        assert_eq!(install_id(&dir), id);
        let s = State {
            first_run: 5,
            saves: 2,
            ..Default::default()
        };
        s.save(&dir).unwrap();
        assert_eq!(State::load(&dir), s);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
