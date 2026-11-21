//! End to end: sign a real PDF with a key from a .p12 file, read the file
//! back, and check the signature; then tamper with it, fill a form field
//! after signing, sign twice, and run out of reserved space.

use der::{Decode, Encode};
use lopdf::dictionary;
use pdfr_core::digital::{SignRequest, sign};
use pdfr_core::field::{Field, Origin};
use pdfr_core::forms::FormDoc;
use pdfr_core::geom::Rect;
use pdfr_sign::trust::{Anchor, TrustStore};
use pdfr_sign::verify::{After, Status, verify_all};
use pdfr_sign::{Cert, Signer, cades};
use pkcs8::EncodePrivateKey;
use std::str::FromStr;
use std::time::Duration;
use x509_cert::builder::{Builder, CertificateBuilder, Profile};
use x509_cert::name::Name;
use x509_cert::serial_number::SerialNumber;
use x509_cert::spki::SubjectPublicKeyInfoOwned;
use x509_cert::time::Validity;

fn sample(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../../samples/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

struct Pki {
    ca: Cert,
    p12: Vec<u8>,
}

/// A throwaway root CA and a signer certificate it issued, the signer's
/// key and both certificates packed in a .p12 with password "secret".
fn pki() -> Pki {
    let mut rng = rand_core::OsRng;
    let ca_key = p256::ecdsa::SigningKey::random(&mut rng);
    let ca_name = Name::from_str("CN=pdfr Test Root,O=pdfr tests,C=SE").unwrap();
    let ca_spki = SubjectPublicKeyInfoOwned::from_key(*ca_key.verifying_key()).unwrap();
    let ca = CertificateBuilder::new(
        Profile::Root,
        SerialNumber::from(1u32),
        Validity::from_now(Duration::from_secs(3600 * 24 * 365)).unwrap(),
        ca_name.clone(),
        ca_spki,
        &ca_key,
    )
    .unwrap()
    .build::<p256::ecdsa::DerSignature>()
    .unwrap();

    let key = p256::ecdsa::SigningKey::random(&mut rng);
    let spki = SubjectPublicKeyInfoOwned::from_key(*key.verifying_key()).unwrap();
    let leaf = CertificateBuilder::new(
        Profile::Leaf {
            issuer: ca_name,
            enable_key_agreement: false,
            enable_key_encipherment: false,
        },
        SerialNumber::from(42u32),
        Validity::from_now(Duration::from_secs(3600 * 24 * 30)).unwrap(),
        Name::from_str("CN=ÅSA ÖBERG NÚÑEZ - 12345678Z,C=ES").unwrap(),
        spki,
        &ca_key,
    )
    .unwrap()
    .build::<p256::ecdsa::DerSignature>()
    .unwrap();

    let p12c = |c: &x509_cert::Certificate| {
        p12_keystore::Certificate::from_der(&c.to_der().unwrap()).unwrap()
    };
    let mut ks = p12_keystore::KeyStore::new();
    ks.add_entry(
        "signer",
        p12_keystore::KeyStoreEntry::PrivateKeyChain(p12_keystore::PrivateKeyChain::new(
            key.to_pkcs8_der().unwrap().as_bytes(),
            [1u8; 20],
            vec![p12c(&leaf), p12c(&ca)],
        )),
    );
    let p12 = ks.writer("secret").write().unwrap();
    Pki {
        ca: Cert::from_der(&ca.to_der().unwrap()).unwrap(),
        p12,
    }
}

fn store(ca: &Cert) -> TrustStore {
    let mut t = TrustStore::default();
    t.add(ca.clone(), Anchor::EuQualified);
    t
}

fn request(spot: Option<(usize, Rect)>) -> SignRequest {
    SignRequest {
        spot,
        ink: Some(std::sync::Arc::new(pdfr_core::signature::Ink::typed(
            "Åsa Öberg",
        ))),
        signer: "ÅSA ÖBERG NÚÑEZ".into(),
        reason: Some("I agree".into()),
        location: Some("Madrid".into()),
        time: pdfr_sign::now(),
        reserve: pdfr_core::digital::DEFAULT_RESERVE,
    }
}

fn sign_with(fd: &FormDoc, fields: &[Field], signer: &dyn Signer, req: &SignRequest) -> Vec<u8> {
    sign(fd, fields, req, |content| {
        cades::sign(content, signer, None).map(|s| s.der)
    })
    .unwrap()
}

#[test]
fn sign_then_verify_then_tamper() {
    let pki = pki();
    assert!(pdfr_sign::signer::p12::open(&pki.p12, "wrong").is_err());
    let signer = pdfr_sign::signer::p12::open(&pki.p12, "secret").unwrap();
    assert_eq!(signer.cert().name(), "ÅSA ÖBERG NÚÑEZ - 12345678Z");
    assert_eq!(signer.chain().len(), 1, "the CA is found as issuer");
    assert!(signer.cert().can_sign_documents());

    let fd = FormDoc::open(sample("real_form.pdf")).unwrap();
    assert!(!fd.is_digitally_signed());
    let mut fields = fd.fields.clone();
    let name = fields
        .iter_mut()
        .find(|f| matches!(&f.origin, Origin::AcroForm { name, .. } if name == "fullname"))
        .unwrap();
    name.value = "Åsa Öberg".into();
    let req = request(Some((0, Rect::new(300.0, 650.0, 520.0, 700.0))));
    let signed = sign_with(&fd, &fields, &signer, &req);

    let fd2 = FormDoc::open(signed.clone()).unwrap();
    assert!(fd2.is_digitally_signed());
    assert_eq!(fd2.digital.len(), 1);
    let d = &fd2.digital[0];
    assert_eq!(d.sub_filter, "ETSI.CAdES.detached");
    assert_eq!(d.reason.as_deref(), Some("I agree"));
    assert_eq!(d.page, Some(0));
    assert_eq!(d.covered_end(), signed.len());
    // The value filled in before signing was saved too.
    assert!(fd2.fields.iter().any(|f| f.value == "Åsa Öberg"));

    let t = store(&pki.ca);
    let r = &verify_all(&signed, &fd2.digital, &t)[0];
    assert_eq!(r.status, Status::Valid, "{:?}", r.problem);
    assert_eq!(r.signer, "ÅSA ÖBERG NÚÑEZ - 12345678Z");
    assert_eq!(r.after, After::Nothing);
    assert_eq!(r.kind, "PAdES B-B");
    assert_eq!(r.anchor, Some(Anchor::EuQualified));

    // Without the root trusted: intact but untrusted.
    let r = &verify_all(&signed, &fd2.digital, &TrustStore::default())[0];
    assert_eq!(r.status, Status::Untrusted);

    // Change one byte in the signed part.
    let mut bad = signed.clone();
    let i = bad
        .windows(4)
        .position(|w| w == b"/MediaBox".get(..4).unwrap())
        .unwrap();
    bad[i + 1] = b'm';
    let r = &verify_all(&bad, &fd2.digital, &t)[0];
    assert_eq!(r.status, Status::Broken, "{:?}", r.problem);

    // Fill in a field after signing: saved as an update, signature intact.
    let mut f2 = fd2.fields.clone();
    let email = f2
        .iter_mut()
        .find(|f| matches!(&f.origin, Origin::AcroForm { name, .. } if name == "contact.email"))
        .unwrap();
    email.value = "asa@example.se".into();
    let after = pdfr_core::save::save(&fd2, &f2).unwrap();
    assert!(
        after.starts_with(&signed),
        "the signed file is kept as it was"
    );
    let fd3 = FormDoc::open(after.clone()).unwrap();
    assert!(fd3.fields.iter().any(|f| f.value == "asa@example.se"));
    let r = &verify_all(&after, &fd3.digital, &t)[0];
    assert_eq!(r.status, Status::Valid, "{:?}", r.problem);
    match &r.after {
        After::Changes { allowed, what } => {
            assert!(allowed, "{what:?}");
            assert!(what.iter().any(|w| w.contains("form")), "{what:?}");
        }
        a => panic!("{a:?}"),
    }

    // Sign again: both signatures check out; the first was "signed again".
    let twice = sign_with(&fd3, &fd3.fields, &signer, &request(None));
    let fd4 = FormDoc::open(twice.clone()).unwrap();
    assert_eq!(fd4.digital.len(), 2);
    let rs = verify_all(&twice, &fd4.digital, &t);
    assert!(rs.iter().all(|r| r.status == Status::Valid), "{rs:?}");
    assert!(rs.iter().any(|r| matches!(&r.after,
        After::Changes { allowed: true, what } if what.iter().any(|w| w == "signed again"))));
}

#[test]
fn comments_after_signing_keep_the_signature_valid() {
    use pdfr_core::markup::{MarkKind, Markup};
    let pki = pki();
    let signer = pdfr_sign::signer::p12::open(&pki.p12, "secret").unwrap();
    let fd = FormDoc::open(sample("real_form.pdf")).unwrap();
    let signed = sign_with(&fd, &fd.fields, &signer, &request(None));
    let fd2 = FormDoc::open(signed.clone()).unwrap();
    let g = fd2.pages[0];
    let mut note = Markup::note(0, &g, 100.0, 100.0, [255, 212, 0]);
    note.text = "Looks good".into();
    let marks = vec![
        note,
        Markup::shape(
            MarkKind::Square,
            0,
            &g,
            (50.0, 50.0),
            (150.0, 90.0),
            [255, 0, 0],
        ),
    ];
    let after = pdfr_core::save::save_with(&fd2, &fd2.fields, &marks).unwrap();
    assert!(after.starts_with(&signed), "added as an update");
    let fd3 = FormDoc::open(after.clone()).unwrap();
    assert_eq!(fd3.markups.len(), 2);
    let r = &verify_all(&after, &fd3.digital, &store(&pki.ca))[0];
    assert_eq!(r.status, Status::Valid, "{:?}", r.problem);
    match &r.after {
        After::Changes { allowed, what } => {
            assert!(allowed, "{what:?}");
            assert!(what.iter().any(|w| w.contains("comments")), "{what:?}");
        }
        a => panic!("{a:?}"),
    }
}

#[test]
fn changing_the_page_after_signing_is_reported() {
    let pki = pki();
    let signer = pdfr_sign::signer::p12::open(&pki.p12, "secret").unwrap();
    let fd = FormDoc::open(sample("real_form.pdf")).unwrap();
    let signed = sign_with(&fd, &fd.fields, &signer, &request(None));
    let fd2 = FormDoc::open(signed.clone()).unwrap();
    // Rotate a page: allowed by pdfr (as an update), but not a form change.
    let rotated = fd2.rotated(&[0], 1).unwrap();
    assert!(rotated.bytes.starts_with(&signed));
    let r = &verify_all(&rotated.bytes, &rotated.digital, &store(&pki.ca))[0];
    assert_eq!(r.status, Status::Modified, "{:?}", r.after);
}

#[test]
fn too_little_reserved_space_is_an_error() {
    let pki = pki();
    let signer = pdfr_sign::signer::p12::open(&pki.p12, "secret").unwrap();
    let fd = FormDoc::open(sample("real_form.pdf")).unwrap();
    let mut req = request(None);
    req.reserve = 64;
    let e = sign(&fd, &fd.fields, &req, |c| {
        cades::sign(c, &signer, None).map(|s| s.der)
    })
    .unwrap_err();
    assert!(
        matches!(e, pdfr_core::digital::SignError::TooLarge { .. }),
        "{e}"
    );
}

#[test]
fn signs_on_the_files_own_sign_here_field() {
    // A form with an empty /FT /Sig field.
    let pki = pki();
    let signer = pdfr_sign::signer::p12::open(&pki.p12, "secret").unwrap();
    let mut d = lopdf::Document::load_mem(&sample("real_form.pdf")).unwrap();
    let page = *d.get_pages().values().next().unwrap();
    let w = d.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "FT" => "Sig",
        "T" => lopdf::Object::string_literal("ApplicantSignature"),
        "Rect" => vec![100.into(), 100.into(), 300.into(), 150.into()],
        "P" => lopdf::Object::Reference(page),
    });
    let annots = d
        .get_dictionary(page)
        .unwrap()
        .get(b"Annots")
        .unwrap()
        .clone();
    match annots {
        lopdf::Object::Reference(r) => d
            .get_object_mut(r)
            .unwrap()
            .as_array_mut()
            .unwrap()
            .push(w.into()),
        _ => d
            .get_dictionary_mut(page)
            .unwrap()
            .get_mut(b"Annots")
            .unwrap()
            .as_array_mut()
            .unwrap()
            .push(w.into()),
    }
    let cat = d
        .catalog()
        .unwrap()
        .get(b"AcroForm")
        .unwrap()
        .as_reference()
        .unwrap();
    d.get_dictionary_mut(cat)
        .unwrap()
        .get_mut(b"Fields")
        .unwrap()
        .as_array_mut()
        .unwrap()
        .push(w.into());
    let mut bytes = Vec::new();
    d.save_to(&mut bytes).unwrap();

    let fd = FormDoc::open(bytes).unwrap();
    assert_eq!(fd.sign_here.len(), 1);
    let (p, r) = fd.sign_here[0];
    let signed = sign_with(&fd, &fd.fields, &signer, &request(Some((p, r))));
    let fd2 = FormDoc::open(signed.clone()).unwrap();
    assert_eq!(fd2.digital.len(), 1);
    assert_eq!(fd2.digital[0].field, "ApplicantSignature");
    assert!(fd2.sign_here.is_empty(), "the spot is signed now");
    let r = &verify_all(&signed, &fd2.digital, &store(&pki.ca))[0];
    assert_eq!(r.status, Status::Valid, "{:?}", r.problem);
}

#[test]
fn cms_parses_back() {
    let pki = pki();
    let signer = pdfr_sign::signer::p12::open(&pki.p12, "secret").unwrap();
    let s = cades::sign(b"hello", &signer, None).unwrap();
    let ci = cms::content_info::ContentInfo::from_der(&s.der).unwrap();
    let sd: cms::signed_data::SignedData = ci.content.decode_as().unwrap();
    assert_eq!(sd.certificates.unwrap().0.len(), 2);
    let si = sd.signer_infos.0.iter().next().unwrap();
    assert_eq!(si.signed_attrs.as_ref().unwrap().len(), 3);
}
