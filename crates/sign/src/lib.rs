//! Digital signatures for pdfr: certificates and keys (NSS database,
//! PKCS#11 tokens, PKCS#12 files), PAdES signatures (CMS / CAdES with an
//! optional RFC 3161 timestamp), and checking signatures in PDFs.
//!
//! pdfr never sees a private key held by NSS or a token: it sends the hash
//! of what is signed and gets the signature back. A key read from a .p12
//! file is held only while signing and wiped when dropped.

pub mod cades;
pub mod cert;
pub mod ltv;
pub mod oids;
pub mod revocation;
pub mod signer;
pub mod trust;
pub mod tsa;
pub mod verify;

pub use cert::Cert;

/// How a key signs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SigAlg {
    /// RSA, PKCS#1 v1.5, SHA-256.
    RsaSha256,
    /// ECDSA on P-256 with SHA-256.
    EcdsaP256Sha256,
    /// ECDSA on P-384 with SHA-384.
    EcdsaP384Sha384,
}

/// A key that can sign, with its certificate.
pub trait Signer: Send {
    /// The signer's certificate.
    fn cert(&self) -> &Cert;
    /// Intermediate certificates up to (not necessarily including) the root.
    fn chain(&self) -> &[Cert];
    fn alg(&self) -> SigAlg;
    /// Signs `msg` (the signer hashes it as `alg` says). ECDSA signatures
    /// are returned DER-encoded.
    fn sign(&self, msg: &[u8]) -> Result<Vec<u8>, String>;
}

/// SHA-256 of `data`.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    use sha2::Digest;
    sha2::Sha256::digest(data).into()
}

/// Unix seconds now.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
