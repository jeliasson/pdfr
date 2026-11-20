//! Signing with a key from a PKCS#12 file (.p12 / .pfx).
use crate::{Cert, SigAlg, Signer};
use pkcs8::{DecodePrivateKey, EncodePublicKey};
use signature::{SignatureEncoding, Signer as _};

enum Key {
    Rsa(Box<rsa::pkcs1v15::SigningKey<sha2::Sha256>>),
    P256(p256::ecdsa::SigningKey),
    P384(p384::ecdsa::SigningKey),
}

/// A key and its certificate read from a .p12 file. The key is wiped from
/// memory when this is dropped.
pub struct P12Signer {
    cert: Cert,
    chain: Vec<Cert>,
    key: Key,
}

/// Reads the first key and its certificates from a PKCS#12 file.
pub fn open(data: &[u8], password: &str) -> Result<P12Signer, String> {
    let ks = p12_keystore::KeyStore::from_pkcs12(data, password).map_err(|e| {
        let s = e.to_string();
        if s.to_lowercase().contains("mac") || s.to_lowercase().contains("decrypt") {
            "wrong password, or not a certificate file".to_string()
        } else {
            format!("could not read the certificate file: {s}")
        }
    })?;
    let (_, kc) = ks
        .private_key_chain()
        .ok_or("the file has no private key")?;
    let pk = pkcs8::PrivateKeyInfo::try_from(kc.key()).map_err(|e| e.to_string())?;
    let (key, spki) = match pk.algorithm.oid {
        crate::oids::RSA => {
            let k = rsa::RsaPrivateKey::from_pkcs8_der(kc.key()).map_err(|e| e.to_string())?;
            let spki = k
                .to_public_key()
                .to_public_key_der()
                .map_err(|e| e.to_string())?;
            (
                Key::Rsa(Box::new(rsa::pkcs1v15::SigningKey::new(k))),
                spki.as_bytes().to_vec(),
            )
        }
        crate::oids::EC_PUBLIC_KEY => {
            let curve = pk
                .algorithm
                .parameters_oid()
                .map_err(|_| "unknown elliptic curve")?;
            match curve {
                crate::oids::P256 => {
                    let k = p256::ecdsa::SigningKey::from_pkcs8_der(kc.key())
                        .map_err(|e| e.to_string())?;
                    let spki = k
                        .verifying_key()
                        .to_public_key_der()
                        .map_err(|e| e.to_string())?;
                    (Key::P256(k), spki.as_bytes().to_vec())
                }
                crate::oids::P384 => {
                    let k = p384::ecdsa::SigningKey::from_pkcs8_der(kc.key())
                        .map_err(|e| e.to_string())?;
                    let spki = k
                        .verifying_key()
                        .to_public_key_der()
                        .map_err(|e| e.to_string())?;
                    (Key::P384(k), spki.as_bytes().to_vec())
                }
                _ => return Err("this kind of elliptic curve key is not supported".into()),
            }
        }
        _ => return Err("this kind of key is not supported".into()),
    };
    let mut certs: Vec<Cert> = kc
        .chain()
        .iter()
        .filter_map(|c| Cert::from_der(c.as_der()).ok())
        .collect();
    // Loose certificates in the file can be issuers too.
    for e in ks.entries() {
        if let (_, p12_keystore::KeyStoreEntry::Certificate(c)) = e
            && let Ok(c) = Cert::from_der(c.as_der())
            && !certs.contains(&c)
        {
            certs.push(c);
        }
    }
    let i = certs
        .iter()
        .position(|c| c.spki_der() == spki)
        .ok_or("the file has no certificate for its key")?;
    let cert = certs.remove(i);
    let chain = crate::trust::order_chain(&cert, &certs);
    Ok(P12Signer { cert, chain, key })
}

impl Signer for P12Signer {
    fn cert(&self) -> &Cert {
        &self.cert
    }
    fn chain(&self) -> &[Cert] {
        &self.chain
    }
    fn alg(&self) -> SigAlg {
        match self.key {
            Key::Rsa(_) => SigAlg::RsaSha256,
            Key::P256(_) => SigAlg::EcdsaP256Sha256,
            Key::P384(_) => SigAlg::EcdsaP384Sha384,
        }
    }
    fn sign(&self, msg: &[u8]) -> Result<Vec<u8>, String> {
        Ok(match &self.key {
            Key::Rsa(k) => k.try_sign(msg).map_err(|e| e.to_string())?.to_vec(),
            Key::P256(k) => {
                let s: p256::ecdsa::Signature = k.try_sign(msg).map_err(|e| e.to_string())?;
                s.to_der().to_vec()
            }
            Key::P384(k) => {
                let s: p384::ecdsa::Signature = k.try_sign(msg).map_err(|e| e.to_string())?;
                s.to_der().to_vec()
            }
        })
    }
}
