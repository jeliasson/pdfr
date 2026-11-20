//! Certificate chains and the roots they are checked against.
//!
//! Two kinds of root: the EU Trusted List (qualified trust service
//! providers, bundled as a snapshot made at release time) and the system's
//! trust store (the roots the computer trusts for websites).

use crate::{Cert, oids};
use const_oid::ObjectIdentifier as Oid;
use der::{Decode, Encode};
use pkcs8::DecodePublicKey;
use spki::SubjectPublicKeyInfoOwned;

/// Why a root is trusted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    /// On the EU Trusted List (eIDAS qualified).
    EuQualified,
    /// In the computer's trust store.
    System,
}

#[derive(Default)]
pub struct TrustStore {
    anchors: Vec<(Cert, Anchor)>,
}

/// The EU Trusted List snapshot: PEM certificates written by `tools/eutl`.
static EUTL: &str = include_str!("../eutl-roots.pem");

const SYSTEM_BUNDLES: &[&str] = &[
    "/etc/ssl/certs/ca-certificates.crt",
    "/etc/pki/tls/certs/ca-bundle.crt",
    "/etc/ssl/ca-bundle.pem",
    "/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem",
    "/etc/ssl/cert.pem",
];

impl TrustStore {
    /// The bundled EU roots and the system's.
    pub fn standard() -> TrustStore {
        let mut t = TrustStore::default();
        for c in Cert::from_pem_bundle(EUTL) {
            t.add(c, Anchor::EuQualified);
        }
        if let Some(pem) = SYSTEM_BUNDLES
            .iter()
            .find_map(|p| std::fs::read_to_string(p).ok())
        {
            for c in Cert::from_pem_bundle(&pem) {
                t.add(c, Anchor::System);
            }
        }
        t
    }

    pub fn add(&mut self, c: Cert, kind: Anchor) {
        if !self.anchors.iter().any(|(a, _)| *a == c) {
            self.anchors.push((c, kind));
        }
    }

    pub fn len(&self) -> usize {
        self.anchors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.anchors.is_empty()
    }

    /// The anchor kind for `c` itself, or for a root with its subject and key.
    fn anchor_for(&self, c: &Cert) -> Option<Anchor> {
        let best = |a: Anchor| match a {
            Anchor::EuQualified => 0,
            Anchor::System => 1,
        };
        self.anchors
            .iter()
            .filter(|(a, _)| a.subject() == c.subject() && a.spki_der() == c.spki_der())
            .map(|(_, k)| *k)
            .min_by_key(|k| best(*k))
    }

    fn issuers_of<'a>(&'a self, c: &Cert) -> impl Iterator<Item = &'a Cert> {
        let issuer = c.issuer().clone();
        self.anchors
            .iter()
            .map(|(a, _)| a)
            .filter(move |a| *a.subject() == issuer)
    }
}

/// `leaf`'s issuers from `pool`, nearest first (stops at a self-issued
/// certificate or when no issuer is found).
pub fn order_chain(leaf: &Cert, pool: &[Cert]) -> Vec<Cert> {
    let mut out: Vec<Cert> = Vec::new();
    let mut cur = leaf.clone();
    while !cur.self_issued() && out.len() < 10 {
        let Some(next) = pool
            .iter()
            .find(|c| c.subject() == cur.issuer() && !out.contains(c) && verify_cert(&cur, c))
        else {
            break;
        };
        out.push(next.clone());
        cur = next.clone();
    }
    out
}

/// What checking a certificate's chain found.
#[derive(Clone, Debug)]
pub struct ChainCheck {
    /// The chain from the leaf's issuer upward.
    pub chain: Vec<Cert>,
    /// The root it ends in, if trusted.
    pub anchor: Option<Anchor>,
    /// Something wrong along the chain (expired at signing time, bad
    /// signature), if anything.
    pub problem: Option<String>,
}

/// Builds and checks `leaf`'s chain from `pool` and the trust store, with
/// validity checked at time `at`.
pub fn check_chain(leaf: &Cert, pool: &[Cert], store: &TrustStore, at: u64) -> ChainCheck {
    let mut chain = Vec::new();
    let mut problem = None;
    if !leaf.valid_at(at) {
        problem = Some(format!(
            "the signer's certificate was not valid at the time of signing ({})",
            if at < leaf.not_before() {
                "not yet valid"
            } else {
                "expired"
            }
        ));
    }
    let mut cur = leaf.clone();
    let mut anchor = store.anchor_for(&cur);
    while anchor.is_none() && chain.len() < 10 {
        let next = pool
            .iter()
            .chain(store.issuers_of(&cur))
            .find(|c| c.subject() == cur.issuer() && verify_cert(&cur, c))
            .cloned();
        let Some(next) = next else {
            if !cur.self_issued() {
                problem.get_or_insert_with(|| {
                    format!("the issuer \"{}\" was not found", cur.issuer_name())
                });
            }
            break;
        };
        if chain.contains(&next) {
            break;
        }
        if !next.valid_at(at) && problem.is_none() {
            problem = Some(format!(
                "the issuing certificate \"{}\" was not valid at the time of signing",
                next.name()
            ));
        }
        anchor = store.anchor_for(&next);
        let self_issued = next.self_issued();
        chain.push(next.clone());
        if self_issued {
            break;
        }
        cur = next;
    }
    ChainCheck {
        chain,
        anchor,
        problem,
    }
}

/// Whether `issuer`'s key signed `child`.
pub fn verify_cert(child: &Cert, issuer: &Cert) -> bool {
    let Ok(tbs) = child.x.tbs_certificate.to_der() else {
        return false;
    };
    let Some(sig) = child.x.signature.as_bytes() else {
        return false;
    };
    let alg = &child.x.signature_algorithm;
    verify(
        &issuer.x.tbs_certificate.subject_public_key_info,
        alg.oid,
        alg.parameters
            .as_ref()
            .and_then(|p| p.to_der().ok())
            .as_deref(),
        None,
        &tbs,
        sig,
    )
    .is_ok()
}

/// Hash of `msg` with the algorithm `oid`.
pub(crate) fn hash(oid: Oid, msg: &[u8]) -> Option<Vec<u8>> {
    use sha2::Digest;
    Some(match oid {
        oids::SHA1 => sha1::Sha1::digest(msg).to_vec(),
        oids::SHA256 => sha2::Sha256::digest(msg).to_vec(),
        oids::SHA384 => sha2::Sha384::digest(msg).to_vec(),
        oids::SHA512 => sha2::Sha512::digest(msg).to_vec(),
        _ => return None,
    })
}

/// Name of a digest algorithm, for messages.
pub fn hash_name(oid: Oid) -> &'static str {
    match oid {
        oids::SHA1 => "SHA-1",
        oids::SHA256 => "SHA-256",
        oids::SHA384 => "SHA-384",
        oids::SHA512 => "SHA-512",
        _ => "unknown",
    }
}

/// Checks `sig` over `msg` with the key `spki`. `alg` is the signature
/// algorithm; `digest` the digest algorithm when `alg` doesn't say (plain
/// `rsaEncryption`). Returns the digest algorithm used.
pub(crate) fn verify(
    spki: &SubjectPublicKeyInfoOwned,
    alg: Oid,
    alg_params: Option<&[u8]>,
    digest: Option<Oid>,
    msg: &[u8],
    sig: &[u8],
) -> Result<Oid, String> {
    let spki_der = spki.to_der().map_err(|e| e.to_string())?;
    let digest = match alg {
        oids::RSA_SHA1 => oids::SHA1,
        oids::RSA_SHA256 | oids::ECDSA_SHA256 => oids::SHA256,
        oids::RSA_SHA384 | oids::ECDSA_SHA384 => oids::SHA384,
        oids::RSA_SHA512 | oids::ECDSA_SHA512 => oids::SHA512,
        oids::RSA_PSS => {
            let p = alg_params
                .and_then(|p| rsa::pkcs1::RsaPssParams::from_der(p).ok())
                .ok_or("RSA-PSS without parameters")?;
            p.hash.oid
        }
        _ => digest.ok_or("unknown digest algorithm")?,
    };
    let h = hash(digest, msg).ok_or("unsupported digest algorithm")?;
    match spki.algorithm.oid {
        oids::RSA => {
            let pk =
                rsa::RsaPublicKey::from_public_key_der(&spki_der).map_err(|e| e.to_string())?;
            let ok = if alg == oids::RSA_PSS {
                match digest {
                    oids::SHA256 => pk.verify(rsa::Pss::new::<sha2::Sha256>(), &h, sig),
                    oids::SHA384 => pk.verify(rsa::Pss::new::<sha2::Sha384>(), &h, sig),
                    oids::SHA512 => pk.verify(rsa::Pss::new::<sha2::Sha512>(), &h, sig),
                    _ => return Err("unsupported RSA-PSS digest".into()),
                }
            } else {
                let scheme = match digest {
                    oids::SHA1 => rsa::Pkcs1v15Sign::new::<sha1::Sha1>(),
                    oids::SHA256 => rsa::Pkcs1v15Sign::new::<sha2::Sha256>(),
                    oids::SHA384 => rsa::Pkcs1v15Sign::new::<sha2::Sha384>(),
                    oids::SHA512 => rsa::Pkcs1v15Sign::new::<sha2::Sha512>(),
                    _ => return Err("unsupported digest".into()),
                };
                pk.verify(scheme, &h, sig)
            };
            ok.map_err(|_| "the signature does not match".to_string())?;
        }
        oids::EC_PUBLIC_KEY => {
            use signature::hazmat::PrehashVerifier;
            let curve = oids::curve(spki).ok_or("unknown elliptic curve")?;
            let ok = match curve {
                oids::P256 => {
                    let k = p256::ecdsa::VerifyingKey::from_public_key_der(&spki_der)
                        .map_err(|e| e.to_string())?;
                    let s = p256::ecdsa::Signature::from_der(sig).map_err(|e| e.to_string())?;
                    k.verify_prehash(&h, &s).is_ok()
                }
                oids::P384 => {
                    let k = p384::ecdsa::VerifyingKey::from_public_key_der(&spki_der)
                        .map_err(|e| e.to_string())?;
                    let s = p384::ecdsa::Signature::from_der(sig).map_err(|e| e.to_string())?;
                    k.verify_prehash(&h, &s).is_ok()
                }
                _ => return Err("unsupported elliptic curve".into()),
            };
            if !ok {
                return Err("the signature does not match".into());
            }
        }
        _ => return Err("unsupported key type".into()),
    }
    Ok(digest)
}
