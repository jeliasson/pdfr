//! macOS: certificates with private keys from the Keychain (including ID
//! cards that macOS offers through CryptoTokenKit). Signing happens inside
//! the Keychain with Security.framework: the key never leaves it, and
//! macOS itself asks the user to allow pdfr to use it.

use super::{CertChoice, KeyHandle, Source};
use crate::{Cert, SigAlg, Signer};
use security_framework::identity::SecIdentity;
use security_framework::item::{ItemClass, ItemSearchOptions, Limit, Reference, SearchResult};
use security_framework::key::Algorithm;

fn search(class: ItemClass) -> Vec<Reference> {
    ItemSearchOptions::new()
        .class(class)
        .load_refs(true)
        .limit(Limit::All)
        .search()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|r| match r {
            SearchResult::Ref(r) => Some(r),
            _ => None,
        })
        .collect()
}

/// Identities (certificate + private key) in the user's keychains.
pub(crate) fn list() -> Vec<CertChoice> {
    // Every certificate, as the pool to find issuers in.
    let pool: Vec<Cert> = search(ItemClass::certificate())
        .into_iter()
        .filter_map(|r| match r {
            Reference::Certificate(c) => Cert::from_der(&c.to_der()).ok(),
            _ => None,
        })
        .collect();
    let mut out = Vec::new();
    for r in search(ItemClass::identity()) {
        let Reference::Identity(id) = r else { continue };
        let Some(cert) = id
            .certificate()
            .ok()
            .and_then(|c| Cert::from_der(&c.to_der()).ok())
        else {
            continue;
        };
        // Identities for TLS servers, code signing and the like are not
        // for documents.
        if cert.is_ca() || !cert.can_sign_documents() {
            continue;
        }
        let others: Vec<Cert> = pool.iter().filter(|c| **c != cert).cloned().collect();
        out.push(CertChoice {
            chain: crate::trust::order_chain(&cert, &others),
            cert,
            source: Source::Keychain,
            needs_pin: false,
            pin_pad: false,
            pin_tries: super::PinTries::Ok,
            key: KeyHandle::Keychain(id),
        });
    }
    out
}

struct KeychainSigner {
    cert: Cert,
    chain: Vec<Cert>,
    alg: SigAlg,
    id: SecIdentity,
}

pub(crate) fn signer(c: &CertChoice, id: &SecIdentity) -> Result<Box<dyn Signer>, String> {
    Ok(Box::new(KeychainSigner {
        alg: super::alg_of(&c.cert)?,
        cert: c.cert.clone(),
        chain: c.chain.clone(),
        id: id.clone(),
    }))
}

impl Signer for KeychainSigner {
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
        let key = self
            .id
            .private_key()
            .map_err(|e| format!("the key could not be reached in the Keychain ({e})"))?;
        // The "message" algorithms hash inside the Keychain; ECDSA comes
        // back DER-encoded (X9.62), as CMS wants it.
        let alg = match self.alg {
            SigAlg::RsaSha256 => Algorithm::RSASignatureMessagePKCS1v15SHA256,
            SigAlg::EcdsaP256Sha256 => Algorithm::ECDSASignatureMessageX962SHA256,
            SigAlg::EcdsaP384Sha384 => Algorithm::ECDSASignatureMessageX962SHA384,
        };
        key.create_signature(alg, msg).map_err(|e| {
            format!(
                "macOS did not sign ({}). If you were asked, allow pdfr to use the key",
                e.description()
            )
        })
    }
}
