//! Object identifiers used in CMS, PAdES and X.509.
use const_oid::ObjectIdentifier as Oid;

pub const DATA: Oid = Oid::new_unwrap("1.2.840.113549.1.7.1");
pub const SIGNED_DATA: Oid = Oid::new_unwrap("1.2.840.113549.1.7.2");
pub const CONTENT_TYPE: Oid = Oid::new_unwrap("1.2.840.113549.1.9.3");
pub const MESSAGE_DIGEST: Oid = Oid::new_unwrap("1.2.840.113549.1.9.4");
pub const SIGNING_TIME: Oid = Oid::new_unwrap("1.2.840.113549.1.9.5");
pub const SIGNING_CERT: Oid = Oid::new_unwrap("1.2.840.113549.1.9.16.2.12");
pub const SIGNING_CERT_V2: Oid = Oid::new_unwrap("1.2.840.113549.1.9.16.2.47");
pub const TIMESTAMP_TOKEN: Oid = Oid::new_unwrap("1.2.840.113549.1.9.16.2.14");
pub const TST_INFO: Oid = Oid::new_unwrap("1.2.840.113549.1.9.16.1.4");

pub const SHA1: Oid = Oid::new_unwrap("1.3.14.3.2.26");
pub const SHA256: Oid = Oid::new_unwrap("2.16.840.1.101.3.4.2.1");
pub const SHA384: Oid = Oid::new_unwrap("2.16.840.1.101.3.4.2.2");
pub const SHA512: Oid = Oid::new_unwrap("2.16.840.1.101.3.4.2.3");

pub const RSA: Oid = Oid::new_unwrap("1.2.840.113549.1.1.1");
pub const RSA_SHA1: Oid = Oid::new_unwrap("1.2.840.113549.1.1.5");
pub const RSA_PSS: Oid = Oid::new_unwrap("1.2.840.113549.1.1.10");
pub const RSA_SHA256: Oid = Oid::new_unwrap("1.2.840.113549.1.1.11");
pub const RSA_SHA384: Oid = Oid::new_unwrap("1.2.840.113549.1.1.12");
pub const RSA_SHA512: Oid = Oid::new_unwrap("1.2.840.113549.1.1.13");

pub const EC_PUBLIC_KEY: Oid = Oid::new_unwrap("1.2.840.10045.2.1");
pub const P256: Oid = Oid::new_unwrap("1.2.840.10045.3.1.7");
pub const P384: Oid = Oid::new_unwrap("1.3.132.0.34");
pub const ECDSA_SHA1: Oid = Oid::new_unwrap("1.2.840.10045.4.1");
pub const ECDSA_SHA256: Oid = Oid::new_unwrap("1.2.840.10045.4.3.2");
pub const ECDSA_SHA384: Oid = Oid::new_unwrap("1.2.840.10045.4.3.3");
pub const ECDSA_SHA512: Oid = Oid::new_unwrap("1.2.840.10045.4.3.4");

pub const KEY_USAGE: Oid = Oid::new_unwrap("2.5.29.15");
pub const SUBJECT_KEY_ID: Oid = Oid::new_unwrap("2.5.29.14");
pub const SUBJECT_ALT_NAME: Oid = Oid::new_unwrap("2.5.29.17");
pub const ISSUER_ALT_NAME: Oid = Oid::new_unwrap("2.5.29.18");
pub const CERTIFICATE_POLICIES: Oid = Oid::new_unwrap("2.5.29.32");
pub const POLICY_CONSTRAINTS: Oid = Oid::new_unwrap("2.5.29.36");
pub const AUTHORITY_KEY_ID: Oid = Oid::new_unwrap("2.5.29.35");
pub const INHIBIT_ANY_POLICY: Oid = Oid::new_unwrap("2.5.29.54");
pub const QC_STATEMENTS: Oid = Oid::new_unwrap("1.3.6.1.5.5.7.1.3");
pub const BASIC_CONSTRAINTS: Oid = Oid::new_unwrap("2.5.29.19");
pub const CN: Oid = Oid::new_unwrap("2.5.4.3");
pub const ORG: Oid = Oid::new_unwrap("2.5.4.10");
pub const AUTHORITY_INFO_ACCESS: Oid = Oid::new_unwrap("1.3.6.1.5.5.7.1.1");
pub const AD_OCSP: Oid = Oid::new_unwrap("1.3.6.1.5.5.7.48.1");
pub const OCSP_BASIC: Oid = Oid::new_unwrap("1.3.6.1.5.5.7.48.1.1");
pub const CRL_DISTRIBUTION_POINTS: Oid = Oid::new_unwrap("2.5.29.31");
pub const EXT_KEY_USAGE: Oid = Oid::new_unwrap("2.5.29.37");
pub const KP_OCSP_SIGNING: Oid = Oid::new_unwrap("1.3.6.1.5.5.7.3.9");
pub const KP_SERVER_AUTH: Oid = Oid::new_unwrap("1.3.6.1.5.5.7.3.1");
pub const KP_CLIENT_AUTH: Oid = Oid::new_unwrap("1.3.6.1.5.5.7.3.2");
pub const KP_EMAIL_PROTECTION: Oid = Oid::new_unwrap("1.3.6.1.5.5.7.3.4");
pub const KP_TIME_STAMPING: Oid = Oid::new_unwrap("1.3.6.1.5.5.7.3.8");
/// id-kp-documentSigning (RFC 9336).
pub const KP_DOCUMENT_SIGNING: Oid = Oid::new_unwrap("1.3.6.1.5.5.7.3.36");
/// Adobe Authentic Documents Trust.
pub const KP_ADOBE_AUTHENTIC_DOCUMENTS: Oid = Oid::new_unwrap("1.2.840.113583.1.1.5");
pub const ANY_EXTENDED_KEY_USAGE: Oid = Oid::new_unwrap("2.5.29.37.0");
pub const CRL_REASON: Oid = Oid::new_unwrap("2.5.29.21");
/// Adobe's revocation data inside a CMS signed attribute.
pub const ADBE_REVOCATION: Oid = Oid::new_unwrap("1.2.840.113583.1.1.8");
/// An OCSP response among a CMS's `crls` (RFC 5940).
pub const RI_OCSP_RESPONSE: Oid = Oid::new_unwrap("1.3.6.1.5.5.7.16.2");
pub const SERIAL_NUMBER_ATTR: Oid = Oid::new_unwrap("2.5.4.5");

/// The named curve of an elliptic-curve public key.
pub fn curve(spki: &spki::SubjectPublicKeyInfoOwned) -> Option<Oid> {
    spki.algorithm.parameters.as_ref()?.decode_as::<Oid>().ok()
}
