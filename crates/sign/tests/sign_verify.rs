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
        name_beside: None,
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
    assert_eq!(r.status, Status::ValidUnchecked, "{:?}", r.problem);
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
    assert_eq!(r.status, Status::ValidUnchecked, "{:?}", r.problem);
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
    assert!(
        rs.iter().all(|r| r.status == Status::ValidUnchecked),
        "{rs:?}"
    );
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
    assert_eq!(r.status, Status::ValidUnchecked, "{:?}", r.problem);
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
    assert_eq!(r.status, Status::ValidUnchecked, "{:?}", r.problem);
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

#[test]
fn looking_for_certificates_never_fails() {
    // Keychain (macOS), NSS database and tokens: whatever the machine has,
    // listing returns (possibly nothing) and every choice is described.
    for c in pdfr_sign::signer::discover() {
        assert!(!c.source.describe().is_empty());
        assert!(!c.cert.name().is_empty());
    }
}

// --- Forgeries that must not pass (security regressions) ----------------------

/// An object written as PDF syntax (no streams).
fn ser(o: &lopdf::Object) -> String {
    use lopdf::Object::*;
    match o {
        Null => "null".into(),
        Boolean(b) => b.to_string(),
        Integer(i) => i.to_string(),
        Real(r) => format!("{r}"),
        Name(n) => format!("/{}", std::string::String::from_utf8_lossy(n)),
        String(s, _) => format!(
            "<{}>",
            s.iter()
                .map(|b| format!("{b:02X}"))
                .collect::<std::string::String>()
        ),
        Array(a) => format!("[{}]", a.iter().map(ser).collect::<Vec<_>>().join(" ")),
        Dictionary(d) => format!(
            "<<{}>>",
            d.iter()
                .map(|(k, v)| format!("/{} {}", std::string::String::from_utf8_lossy(k), ser(v)))
                .collect::<Vec<_>>()
                .join(" ")
        ),
        Reference((a, b)) => format!("{a} {b} R"),
        Stream(_) => panic!("streams are written by hand"),
    }
}

/// `base` with a classic incremental update holding `objs` (id, body).
fn append(base: &[u8], root: (u32, u16), size: u32, objs: &[((u32, u16), Vec<u8>)]) -> Vec<u8> {
    let s = std::string::String::from_utf8_lossy(base);
    let sx = s.rfind("startxref").unwrap();
    let prev: usize = s[sx + 9..]
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let mut out = base.to_vec();
    out.extend(b"\n");
    let mut offs = vec![];
    for (id, body) in objs {
        offs.push((id.0, id.1, out.len()));
        out.extend(format!("{} {} obj\n", id.0, id.1).as_bytes());
        out.extend(body);
        out.extend(b"\nendobj\n");
    }
    let x = out.len();
    out.extend(b"xref\n");
    for (n, g, o) in &offs {
        out.extend(format!("{n} 1\n{o:010} {g:05} n \n").as_bytes());
    }
    out.extend(
        format!(
            "trailer\n<</Size {size} /Root {} {} R /Prev {prev}>>\nstartxref\n{x}\n%%EOF\n",
            root.0, root.1
        )
        .as_bytes(),
    );
    out
}

fn stream_body(content: &[u8]) -> Vec<u8> {
    let mut b = format!("<</Length {}>>\nstream\n", content.len()).into_bytes();
    b.extend(content);
    b.extend(b"\nendstream");
    b
}

fn check_one(bytes: &[u8], t: &TrustStore) -> pdfr_sign::verify::Report {
    let fd = FormDoc::open(bytes.to_vec()).unwrap();
    verify_all(bytes, &fd.digital, t).remove(0)
}

fn signed_sample(pki: &Pki) -> Vec<u8> {
    let signer = pdfr_sign::signer::p12::open(&pki.p12, "secret").unwrap();
    let fd = FormDoc::open(sample("real_form.pdf")).unwrap();
    sign_with(&fd, &fd.fields, &signer, &request(None))
}

#[test]
fn page_content_hidden_under_the_dss_is_a_change() {
    // The catalog's /DSS points at the page's content stream, which a
    // later update replaces: not validation data, a changed page.
    let pki = pki();
    let signed = signed_sample(&pki);
    let t = store(&pki.ca);
    assert_eq!(check_one(&signed, &t).status, Status::ValidUnchecked);
    let d = lopdf::Document::load_mem(&signed).unwrap();
    let root = d.trailer.get(b"Root").unwrap().as_reference().unwrap();
    let page = *d.get_pages().values().next().unwrap();
    let cid = match d.get_dictionary(page).unwrap().get(b"Contents").unwrap() {
        lopdf::Object::Reference(r) => *r,
        lopdf::Object::Array(a) => a[0].as_reference().unwrap(),
        o => panic!("{o:?}"),
    };
    let mut cat = d.get_dictionary(root).unwrap().clone();
    cat.set(
        "DSS",
        dictionary! { "Certs" => vec![lopdf::Object::Reference(cid)] },
    );
    let forged = append(
        &signed,
        root,
        d.max_id + 1,
        &[
            (
                cid,
                stream_body(b"BT /F1 40 Tf 50 400 Td (FORGED CONTENT) Tj ET"),
            ),
            (root, ser(&lopdf::Object::Dictionary(cat)).into_bytes()),
        ],
    );
    let r = check_one(&forged, &t);
    assert_eq!(r.status, Status::Modified, "{:?}", r.after);
}

#[test]
fn an_end_entity_certificate_cannot_issue_certificates() {
    // A real CA issues "Mallory" an ordinary certificate; Mallory mints
    // one for "Alice Victim" and signs as her.
    let mut rng = rand_core::OsRng;
    let ca_key = p256::ecdsa::SigningKey::random(&mut rng);
    let ca_name = Name::from_str("CN=Root").unwrap();
    let ca = build(Profile::Root, &ca_key, &ca_key, ca_name.clone(), 1, |_| {});
    let m_key = p256::ecdsa::SigningKey::random(&mut rng);
    let m_name = Name::from_str("CN=Mallory").unwrap();
    let mallory = build(
        leaf_of(&ca_name),
        &m_key,
        &ca_key,
        m_name.clone(),
        2,
        |_| {},
    );
    let v_key = p256::ecdsa::SigningKey::random(&mut rng);
    let victim = build(
        leaf_of(&m_name),
        &v_key,
        &m_key,
        Name::from_str("CN=Alice Victim").unwrap(),
        3,
        |_| {},
    );
    assert!(!mallory.is_ca());
    let k = KeySigner {
        cert: victim,
        chain: vec![mallory],
        key: v_key,
    };
    let fd = FormDoc::open(sample("real_form.pdf")).unwrap();
    let signed = sign_with(&fd, &fd.fields, &k, &request(None));
    let r = check_one(&signed, &store(&ca));
    assert_eq!(r.status, Status::Untrusted, "{:?}", r.problem);
    assert!(
        r.problem.as_deref().unwrap().contains("Mallory"),
        "{:?}",
        r.problem
    );
}

#[test]
fn a_rewritten_reason_is_a_change_and_the_signed_one_is_shown() {
    let pki = pki();
    let signed = signed_sample(&pki);
    let d = lopdf::Document::load_mem(&signed).unwrap();
    let root = d.trailer.get(b"Root").unwrap().as_reference().unwrap();
    let (sid, mut sig) = d
        .objects
        .iter()
        .find_map(|(i, o)| {
            let dict = o.as_dict().ok()?;
            (dict.get(b"Type").ok()?.as_name().ok()? == b"Sig").then(|| (*i, dict.clone()))
        })
        .unwrap();
    sig.set(
        "Reason",
        lopdf::Object::string_literal("I REJECT these terms"),
    );
    sig.set("M", lopdf::Object::string_literal("D:19990101000000Z"));
    let forged = append(
        &signed,
        root,
        d.max_id + 1,
        &[(sid, ser(&lopdf::Object::Dictionary(sig)).into_bytes())],
    );
    let r = check_one(&forged, &store(&pki.ca));
    assert_eq!(r.status, Status::Modified, "{:?}", r.after);
    assert_eq!(r.reason.as_deref(), Some("I agree"));
    assert_ne!(
        r.claimed_time,
        pdfr_sign::verify::parse_pdf_date("D:19990101000000Z")
    );
}

#[test]
fn swapping_a_font_the_page_uses_is_a_change() {
    let pki = pki();
    let signed = signed_sample(&pki);
    let d = lopdf::Document::load_mem(&signed).unwrap();
    let root = d.trailer.get(b"Root").unwrap().as_reference().unwrap();
    // A font the first page's text is drawn with.
    let page = *d.get_pages().values().next().unwrap();
    let deref = |o: &lopdf::Object| match o {
        lopdf::Object::Reference(r) => d.get_dictionary(*r).unwrap().clone(),
        lopdf::Object::Dictionary(x) => x.clone(),
        o => panic!("{o:?}"),
    };
    let res = deref(d.get_dictionary(page).unwrap().get(b"Resources").unwrap());
    let fonts = deref(res.get(b"Font").unwrap());
    let fid = fonts.iter().next().unwrap().1.as_reference().unwrap();
    let mut font = d.get_dictionary(fid).unwrap().clone();
    // Glyphs that draw other letters than the text says.
    font.set(
        "Encoding",
        dictionary! { "Type" => "Encoding", "Differences" => vec![65.into(), "Z".into()] },
    );
    let forged = append(
        &signed,
        root,
        d.max_id + 1,
        &[(fid, ser(&lopdf::Object::Dictionary(font)).into_bytes())],
    );
    let r = check_one(&forged, &store(&pki.ca));
    assert_eq!(r.status, Status::Modified, "{:?}", r.after);
}

#[test]
fn a_certifying_signature_that_allows_no_changes_refuses_a_form_fill() {
    let pki = pki();
    let signer = pdfr_sign::signer::p12::open(&pki.p12, "secret").unwrap();
    let t = store(&pki.ca);
    let fill = |bytes: Vec<u8>| {
        let fd = FormDoc::open(bytes).unwrap();
        let mut fields = fd.fields.clone();
        let f = fields
            .iter_mut()
            .find(|f| matches!(&f.origin, Origin::AcroForm { name, .. } if name == "contact.email"))
            .unwrap();
        f.value = "x@example.se".into();
        pdfr_core::save::save(&fd, &fields).unwrap()
    };

    let certified = certify(&signer, 1);
    let r = check_one(&certified, &t);
    assert_eq!(r.status, Status::ValidUnchecked, "{:?}", r.problem);
    let r = check_one(&fill(certified), &t);
    assert_eq!(r.status, Status::Modified, "{:?}", r.after);

    // Permission 2 allows the same form fill.
    let r = check_one(&fill(certify(&signer, 2)), &t);
    assert_eq!(r.status, Status::ValidUnchecked, "{:?}", r.after);
}

#[test]
fn a_website_certificate_cannot_sign_documents() {
    use x509_cert::ext::pkix::ExtendedKeyUsage;
    let mut rng = rand_core::OsRng;
    let ca_key = p256::ecdsa::SigningKey::random(&mut rng);
    let ca_name = Name::from_str("CN=Root").unwrap();
    let ca = build(Profile::Root, &ca_key, &ca_key, ca_name.clone(), 1, |_| {});
    let key = p256::ecdsa::SigningKey::random(&mut rng);
    let leaf = build(
        leaf_of(&ca_name),
        &key,
        &ca_key,
        Name::from_str("CN=www.example.com").unwrap(),
        2,
        |b| {
            b.add_extension(&ExtendedKeyUsage(vec![pdfr_sign::oids::KP_SERVER_AUTH]))
                .unwrap()
        },
    );
    assert!(!leaf.can_sign_documents());
    let k = KeySigner {
        cert: leaf,
        chain: vec![],
        key,
    };
    let fd = FormDoc::open(sample("real_form.pdf")).unwrap();
    let signed = sign_with(&fd, &fd.fields, &k, &request(None));
    let r = check_one(&signed, &store(&ca));
    assert_eq!(r.status, Status::Untrusted, "{:?}", r.problem);
    assert!(
        r.problem
            .as_deref()
            .unwrap()
            .contains("not meant for signing"),
        "{:?}",
        r.problem
    );
}

#[test]
fn sha1_in_the_chain_is_flagged() {
    let mut rng = rand_core::OsRng;
    let ca_key = p256::ecdsa::SigningKey::random(&mut rng);
    let ca_name = Name::from_str("CN=Root").unwrap();
    let ca = build(Profile::Root, &ca_key, &ca_key, ca_name.clone(), 1, |_| {});
    let key = p256::ecdsa::SigningKey::random(&mut rng);
    let leaf = build(
        leaf_of(&ca_name),
        &key,
        &ca_key,
        Name::from_str("CN=Old Signer").unwrap(),
        2,
        |_| {},
    );
    let leaf = resign_sha1(leaf, &ca_key);
    assert!(leaf.signed_with_sha1());
    assert!(pdfr_sign::trust::verify_cert(&leaf, &ca));
    let k = KeySigner {
        cert: leaf,
        chain: vec![],
        key,
    };
    let fd = FormDoc::open(sample("real_form.pdf")).unwrap();
    let signed = sign_with(&fd, &fd.fields, &k, &request(None));
    let r = check_one(&signed, &store(&ca));
    assert_eq!(r.status, Status::Untrusted, "{:?}", r.problem);
    assert!(
        r.problem.as_deref().unwrap().contains("SHA-1"),
        "{:?}",
        r.problem
    );
}

// --- Helpers for the forgeries --------------------------------------------------

/// A key in memory, with its certificate.
struct KeySigner {
    cert: Cert,
    chain: Vec<Cert>,
    key: p256::ecdsa::SigningKey,
}

impl Signer for KeySigner {
    fn cert(&self) -> &Cert {
        &self.cert
    }
    fn chain(&self) -> &[Cert] {
        &self.chain
    }
    fn alg(&self) -> pdfr_sign::SigAlg {
        pdfr_sign::SigAlg::EcdsaP256Sha256
    }
    fn sign(&self, msg: &[u8]) -> Result<Vec<u8>, std::string::String> {
        use signature::Signer as _;
        let s: p256::ecdsa::DerSignature = self.key.sign(msg);
        Ok(s.as_bytes().to_vec())
    }
}

fn leaf_of(issuer: &Name) -> Profile {
    Profile::Leaf {
        issuer: issuer.clone(),
        enable_key_agreement: false,
        enable_key_encipherment: false,
    }
}

/// A certificate for `key`'s public key, issued with `issuer_key`.
fn build(
    profile: Profile,
    key: &p256::ecdsa::SigningKey,
    issuer_key: &p256::ecdsa::SigningKey,
    subject: Name,
    serial: u32,
    extra: impl FnOnce(&mut CertificateBuilder<'_, p256::ecdsa::SigningKey>),
) -> Cert {
    let mut b = CertificateBuilder::new(
        profile,
        SerialNumber::from(serial),
        Validity::from_now(Duration::from_secs(86400 * 30)).unwrap(),
        subject,
        SubjectPublicKeyInfoOwned::from_key(*key.verifying_key()).unwrap(),
        issuer_key,
    )
    .unwrap();
    extra(&mut b);
    let c = b.build::<p256::ecdsa::DerSignature>().unwrap();
    Cert::from_der(&c.to_der().unwrap()).unwrap()
}

/// `c` signed again by `issuer_key` with ECDSA and SHA-1.
fn resign_sha1(c: Cert, issuer_key: &p256::ecdsa::SigningKey) -> Cert {
    use sha1::Digest;
    use signature::hazmat::PrehashSigner;
    let mut x = c.x;
    let alg = x509_cert::spki::AlgorithmIdentifierOwned {
        oid: pdfr_sign::oids::ECDSA_SHA1,
        parameters: None,
    };
    x.tbs_certificate.signature = alg.clone();
    let tbs = x.tbs_certificate.to_der().unwrap();
    let sig: p256::ecdsa::Signature = issuer_key.sign_prehash(&sha1::Sha1::digest(&tbs)).unwrap();
    x.signature_algorithm = alg;
    x.signature = der::asn1::BitString::from_bytes(sig.to_der().as_bytes()).unwrap();
    Cert::from_der(&x.to_der().unwrap()).unwrap()
}

/// `real_form.pdf` with a certifying signature (DocMDP permission `p`),
/// written by hand: pdfr itself doesn't certify.
fn certify(signer: &dyn Signer, p: i64) -> Vec<u8> {
    use lopdf::{Object, StringFormat};
    const PH: i64 = 9_999_999_999;
    const RESERVE: usize = 8192;
    let mut d = lopdf::Document::load_mem(&sample("real_form.pdf")).unwrap();
    let sig = d.add_object(dictionary! {
        "Type" => "Sig",
        "Filter" => "Adobe.PPKLite",
        "SubFilter" => "ETSI.CAdES.detached",
        "ByteRange" => vec![0.into(), PH.into(), PH.into(), PH.into()],
        "Contents" => Object::String(vec![0; RESERVE], StringFormat::Hexadecimal),
        "Reference" => vec![Object::Dictionary(dictionary! {
            "Type" => "SigRef",
            "TransformMethod" => "DocMDP",
            "TransformParams" => dictionary! { "Type" => "TransformParams", "P" => p, "V" => "1.2" },
        })],
    });
    let page = *d.get_pages().values().next().unwrap();
    let w = d.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "FT" => "Sig",
        "T" => Object::string_literal("Certification"),
        "Rect" => vec![0.into(), 0.into(), 0.into(), 0.into()],
        "F" => 132, "P" => Object::Reference(page), "V" => Object::Reference(sig),
    });
    let form = d
        .catalog()
        .unwrap()
        .get(b"AcroForm")
        .unwrap()
        .as_reference()
        .unwrap();
    let fd = d.get_dictionary_mut(form).unwrap();
    fd.get_mut(b"Fields")
        .unwrap()
        .as_array_mut()
        .unwrap()
        .push(w.into());
    fd.set("SigFlags", 3);
    d.catalog_mut()
        .unwrap()
        .set("Perms", dictionary! { "DocMDP" => Object::Reference(sig) });
    let mut out = Vec::new();
    d.save_to(&mut out).unwrap();

    let find = |hay: &[u8], n: &[u8]| hay.windows(n.len()).position(|w| w == n).unwrap();
    let mut hole = vec![b'<'];
    hole.extend(std::iter::repeat_n(b'0', RESERVE * 2));
    hole.push(b'>');
    let a = find(&out, &hole);
    let b = a + hole.len();
    let br = format!("0 {PH} {PH} {PH}");
    let at = find(&out, br.as_bytes());
    let mut ranges = format!("0 {a} {b} {}", out.len() - b).into_bytes();
    ranges.resize(br.len(), b' ');
    out[at..at + br.len()].copy_from_slice(&ranges);
    let mut covered = out[..a].to_vec();
    covered.extend_from_slice(&out[b..]);
    let der = cades::sign(&covered, signer, None).unwrap().der;
    let hex: std::string::String = der.iter().map(|x| format!("{x:02X}")).collect();
    out[a + 1..a + 1 + hex.len()].copy_from_slice(hex.as_bytes());
    out
}
