//! X.509 certificates: parsing and what pdfr shows about them.
use crate::oids;
use der::{Decode, Encode};
use x509_cert::Certificate;
use x509_cert::ext::pkix::{BasicConstraints, ExtendedKeyUsage, KeyUsage, KeyUsages};
use x509_cert::name::Name;

/// A certificate with its DER bytes.
#[derive(Clone, Debug)]
pub struct Cert {
    pub der: Vec<u8>,
    pub x: Certificate,
}

impl PartialEq for Cert {
    fn eq(&self, o: &Self) -> bool {
        self.der == o.der
    }
}

impl Cert {
    pub fn from_der(der: &[u8]) -> Result<Cert, String> {
        let x = Certificate::from_der(der).map_err(|e| format!("not a certificate: {e}"))?;
        Ok(Cert {
            der: der.to_vec(),
            x,
        })
    }

    /// All certificates in a PEM bundle (others skipped).
    pub fn from_pem_bundle(pem: &str) -> Vec<Cert> {
        use base64::Engine;
        let mut out = Vec::new();
        let mut cur: Option<String> = None;
        for line in pem.lines() {
            let l = line.trim();
            if l == "-----BEGIN CERTIFICATE-----" {
                cur = Some(String::new());
            } else if l == "-----END CERTIFICATE-----" {
                if let Some(b) = cur.take()
                    && let Ok(der) = base64::engine::general_purpose::STANDARD.decode(b)
                    && let Ok(c) = Cert::from_der(&der)
                {
                    out.push(c);
                }
            } else if let Some(b) = cur.as_mut() {
                b.push_str(l);
            }
        }
        out
    }

    pub fn subject(&self) -> &Name {
        &self.x.tbs_certificate.subject
    }

    pub fn issuer(&self) -> &Name {
        &self.x.tbs_certificate.issuer
    }

    /// The subject's common name, or the whole name if it has none.
    pub fn name(&self) -> String {
        name_attr(self.subject(), oids::CN).unwrap_or_else(|| self.subject().to_string())
    }

    /// The issuer's common name (or organisation, or whole name).
    pub fn issuer_name(&self) -> String {
        name_attr(self.issuer(), oids::CN)
            .or_else(|| name_attr(self.issuer(), oids::ORG))
            .unwrap_or_else(|| self.issuer().to_string())
    }

    pub fn not_before(&self) -> u64 {
        self.x
            .tbs_certificate
            .validity
            .not_before
            .to_unix_duration()
            .as_secs()
    }

    pub fn not_after(&self) -> u64 {
        self.x
            .tbs_certificate
            .validity
            .not_after
            .to_unix_duration()
            .as_secs()
    }

    pub fn valid_at(&self, t: u64) -> bool {
        self.not_before() <= t && t <= self.not_after()
    }

    /// SHA-256 of the certificate, hex.
    pub fn fingerprint(&self) -> String {
        crate::sha256(&self.der)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    pub(crate) fn ext<'a, T: Decode<'a>>(&'a self, oid: const_oid::ObjectIdentifier) -> Option<T> {
        self.x
            .tbs_certificate
            .extensions
            .as_ref()?
            .iter()
            .find(|e| e.extn_id == oid)
            .and_then(|e| T::from_der(e.extn_value.as_bytes()).ok())
    }

    /// Whether the key may sign documents: key usage allows digital
    /// signature or non-repudiation (or the certificate doesn't restrict
    /// it), and the extended key usage, if there is one, includes a
    /// document-signing purpose.
    pub fn can_sign_documents(&self) -> bool {
        let usage = match self.ext::<KeyUsage>(oids::KEY_USAGE) {
            None => !self.is_ca(),
            Some(ku) => {
                ku.0.contains(KeyUsages::DigitalSignature)
                    || ku.0.contains(KeyUsages::NonRepudiation)
            }
        };
        usage && self.purpose_allows_documents()
    }

    /// The extended key usage is absent or names a purpose that covers
    /// signing documents (email protection, document signing, Adobe's
    /// authentic documents, or any purpose). A certificate only for
    /// websites (server or client authentication) is not one.
    pub fn purpose_allows_documents(&self) -> bool {
        match self.extended_key_usage() {
            None => true,
            Some(ekus) => ekus.iter().any(|o| {
                [
                    oids::KP_EMAIL_PROTECTION,
                    oids::KP_DOCUMENT_SIGNING,
                    oids::KP_ADOBE_AUTHENTIC_DOCUMENTS,
                    oids::ANY_EXTENDED_KEY_USAGE,
                ]
                .contains(o)
            }),
        }
    }

    /// The extended key usage purposes, if the certificate has the
    /// extension.
    pub fn extended_key_usage(&self) -> Option<Vec<const_oid::ObjectIdentifier>> {
        self.ext::<ExtendedKeyUsage>(oids::EXT_KEY_USAGE)
            .map(|e| e.0)
    }

    /// A timestamp authority's certificate: its extended key usage
    /// includes time stamping.
    pub fn is_timestamp_authority(&self) -> bool {
        self.extended_key_usage()
            .is_some_and(|e| e.contains(&oids::KP_TIME_STAMPING))
    }

    /// May issue certificates: key usage, if present, includes
    /// certificate signing.
    pub fn may_sign_certs(&self) -> bool {
        self.ext::<KeyUsage>(oids::KEY_USAGE)
            .is_none_or(|ku| ku.0.contains(KeyUsages::KeyCertSign))
    }

    /// The basic constraints' path length: how many certificate
    /// authorities may sit below this one.
    pub fn path_len(&self) -> Option<u8> {
        self.ext::<BasicConstraints>(oids::BASIC_CONSTRAINTS)
            .and_then(|b| b.path_len_constraint)
    }

    /// Has an extension marked critical that pdfr doesn't know, so the
    /// certificate must not be relied on.
    pub fn has_unknown_critical(&self) -> bool {
        const KNOWN: &[const_oid::ObjectIdentifier] = &[
            oids::BASIC_CONSTRAINTS,
            oids::KEY_USAGE,
            oids::EXT_KEY_USAGE,
            oids::SUBJECT_KEY_ID,
            oids::AUTHORITY_KEY_ID,
            oids::SUBJECT_ALT_NAME,
            oids::ISSUER_ALT_NAME,
            oids::CERTIFICATE_POLICIES,
            oids::POLICY_CONSTRAINTS,
            oids::INHIBIT_ANY_POLICY,
            oids::CRL_DISTRIBUTION_POINTS,
            oids::AUTHORITY_INFO_ACCESS,
            oids::QC_STATEMENTS,
        ];
        self.x
            .tbs_certificate
            .extensions
            .iter()
            .flatten()
            .any(|e| e.critical && !KNOWN.contains(&e.extn_id))
    }

    /// Signed by its issuer with SHA-1.
    pub fn signed_with_sha1(&self) -> bool {
        matches!(
            self.x.signature_algorithm.oid,
            oids::RSA_SHA1 | oids::ECDSA_SHA1
        )
    }

    /// A certificate meant for signing documents: its key usage includes
    /// non-repudiation (content commitment), as on the signing certificate
    /// of an ID card, not only digital signature (which logging in uses).
    pub fn is_signature_cert(&self) -> bool {
        self.ext::<KeyUsage>(oids::KEY_USAGE)
            .is_some_and(|ku| ku.0.contains(KeyUsages::NonRepudiation))
    }

    pub fn is_ca(&self) -> bool {
        self.ext::<BasicConstraints>(oids::BASIC_CONSTRAINTS)
            .is_some_and(|b| b.ca)
    }

    pub fn self_issued(&self) -> bool {
        self.subject() == self.issuer()
    }

    /// The SubjectPublicKeyInfo, DER.
    pub fn spki_der(&self) -> Vec<u8> {
        self.x
            .tbs_certificate
            .subject_public_key_info
            .to_der()
            .unwrap_or_default()
    }

    /// Why this certificate can't sign now, if it can't.
    pub fn sign_problem(&self, now: u64) -> Option<String> {
        if now < self.not_before() {
            return Some("not valid yet".into());
        }
        if now > self.not_after() {
            return Some("expired".into());
        }
        if !self.can_sign_documents() {
            return Some("not meant for signing".into());
        }
        None
    }
}

fn name_attr(n: &Name, oid: const_oid::ObjectIdentifier) -> Option<String> {
    for rdn in n.0.iter() {
        for atv in rdn.0.iter() {
            if atv.oid == oid {
                if let Ok(s) = atv.value.decode_as::<String>() {
                    return Some(s);
                }
                if let Ok(s) = atv.value.decode_as::<der::asn1::PrintableStringRef>() {
                    return Some(s.to_string());
                }
                if let Ok(s) = atv.value.decode_as::<der::asn1::Utf8StringRef>() {
                    return Some(s.to_string());
                }
                if let Ok(s) = atv.value.decode_as::<der::asn1::Ia5StringRef>() {
                    return Some(s.to_string());
                }
                return Some(String::from_utf8_lossy(atv.value.value()).into_owned());
            }
        }
    }
    None
}
