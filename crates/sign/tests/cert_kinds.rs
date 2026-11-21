//! An ID card holds a certificate to log in with and one to sign with;
//! pdfr offers the signing one first (signer::discover).

use der::Encode;
use pdfr_sign::Cert;
use std::str::FromStr;
use std::time::Duration;
use x509_cert::builder::{Builder, CertificateBuilder, Profile};
use x509_cert::name::Name;
use x509_cert::serial_number::SerialNumber;
use x509_cert::spki::SubjectPublicKeyInfoOwned;
use x509_cert::time::Validity;

fn cert(profile: Profile) -> Cert {
    let mut rng = rand_core::OsRng;
    let key = p256::ecdsa::SigningKey::random(&mut rng);
    let spki = SubjectPublicKeyInfoOwned::from_key(*key.verifying_key()).unwrap();
    let b = CertificateBuilder::new(
        profile,
        SerialNumber::from(7u32),
        Validity::from_now(Duration::from_secs(3600)).unwrap(),
        Name::from_str("CN=MARIA GARCIA,C=ES").unwrap(),
        spki,
        &key,
    )
    .unwrap();
    let c = b.build::<p256::ecdsa::DerSignature>().unwrap();
    Cert::from_der(&c.to_der().unwrap()).unwrap()
}

#[test]
fn a_signing_certificate_is_recognised() {
    // An end-entity certificate with non-repudiation, as on an ID card's
    // signing certificate.
    let issuer = Name::from_str("CN=Test CA,C=ES").unwrap();
    let signing = cert(Profile::Leaf {
        issuer,
        enable_key_agreement: false,
        enable_key_encipherment: false,
    });
    assert!(signing.is_signature_cert());
    assert!(signing.can_sign_documents());
    // A CA (key cert sign only) is not one.
    let ca = cert(Profile::Root);
    assert!(!ca.is_signature_cert());
}
