//! Revocation checks (OCSP, CRL) and long-term validation (PAdES B-LT and
//! B-LTA), against a throwaway PKI with an in-test OCSP responder, CRL and
//! timestamp authority. No network: the issuers are a `Fetch` in the test.

use cms::cert::{CertificateChoices, IssuerAndSerialNumber};
use cms::content_info::{CmsVersion, ContentInfo};
use cms::signed_data::{
    CertificateSet, EncapsulatedContentInfo, SignedData, SignerIdentifier, SignerInfo, SignerInfos,
};
use der::asn1::{GeneralizedTime, OctetString, SetOfVec};
use der::{Any, Decode, Encode};
use pdfr_core::digital::{SignRequest, sign};
use pdfr_core::field::Origin;
use pdfr_core::forms::FormDoc;
use pdfr_sign::ltv::{Level, extend};
use pdfr_sign::revocation::{Cache, Fetch, Online, Source, State};
use pdfr_sign::trust::{Anchor, TrustStore};
use pdfr_sign::verify::{After, Report, Status, verify_all, verify_all_with};
use pdfr_sign::{Cert, SigAlg, Signer, cades, oids};
use signature::Signer as _;
use spki::{AlgorithmIdentifierOwned, SubjectPublicKeyInfoOwned};
use std::str::FromStr;
use std::sync::Mutex;
use std::time::{Duration, UNIX_EPOCH};
use x509_cert::attr::Attribute;
use x509_cert::builder::{Builder, CertificateBuilder, Profile};
use x509_cert::crl::{CertificateList, RevokedCert, TbsCertList};
use x509_cert::ext::Extension;
use x509_cert::ext::pkix::name::{DistributionPointName, GeneralName};
use x509_cert::ext::pkix::{
    AccessDescription, AuthorityInfoAccessSyntax, CrlDistributionPoints, CrlReason,
};
use x509_cert::name::Name;
use x509_cert::serial_number::SerialNumber;
use x509_cert::time::{Time, Validity};
use x509_ocsp::builder::OcspResponseBuilder;
use x509_ocsp::{CertId, CertStatus, OcspGeneralizedTime, RevokedInfo, SingleResponse};

const DAY: u64 = 24 * 3600;
const OCSP_URL: &str = "http://ocsp.pdfr.test/";
const CRL_URL: &str = "http://crl.pdfr.test/root.crl";

fn now() -> u64 {
    pdfr_sign::now()
}

fn sample(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../../samples/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn time(t: u64) -> Time {
    Time::try_from(UNIX_EPOCH + Duration::from_secs(t)).unwrap()
}

fn gtime(t: u64) -> OcspGeneralizedTime {
    OcspGeneralizedTime(GeneralizedTime::from_unix_duration(Duration::from_secs(t)).unwrap())
}

fn cert_of(x: &x509_cert::Certificate) -> Cert {
    Cert::from_der(&x.to_der().unwrap()).unwrap()
}

/// A root CA, a signer it issued (valid from 30 days ago, naming an
/// OCSP responder and a CRL) and a timestamp authority.
struct Pki {
    ca_key: p256::ecdsa::SigningKey,
    ca: Cert,
    leaf_key: p256::ecdsa::SigningKey,
    leaf: Cert,
    tsa_key: p256::ecdsa::SigningKey,
    tsa: Cert,
}

fn pki() -> Pki {
    let mut rng = rand_core::OsRng;
    let ca_key = p256::ecdsa::SigningKey::random(&mut rng);
    let ca_name = Name::from_str("CN=pdfr Revocation Test Root,O=pdfr tests").unwrap();
    let validity = |days_back: u64, days: u64| Validity {
        not_before: time(now() - days_back * DAY),
        not_after: time(now() + days * DAY),
    };
    let ca = CertificateBuilder::new(
        Profile::Root,
        SerialNumber::from(1u32),
        validity(60, 365),
        ca_name.clone(),
        SubjectPublicKeyInfoOwned::from_key(*ca_key.verifying_key()).unwrap(),
        &ca_key,
    )
    .unwrap()
    .build::<p256::ecdsa::DerSignature>()
    .unwrap();

    let leaf_cert = |key: &p256::ecdsa::SigningKey, serial: u32, cn: &str, urls: bool| {
        let mut b = CertificateBuilder::new(
            Profile::Leaf {
                issuer: ca_name.clone(),
                enable_key_agreement: false,
                enable_key_encipherment: false,
            },
            SerialNumber::from(serial),
            validity(30, 30),
            Name::from_str(cn).unwrap(),
            SubjectPublicKeyInfoOwned::from_key(*key.verifying_key()).unwrap(),
            &ca_key,
        )
        .unwrap();
        if urls {
            let uri = |u: &str| {
                GeneralName::UniformResourceIdentifier(der::asn1::Ia5String::new(u).unwrap())
            };
            b.add_extension(&AuthorityInfoAccessSyntax(vec![AccessDescription {
                access_method: oids::AD_OCSP,
                access_location: uri(OCSP_URL),
            }]))
            .unwrap();
            b.add_extension(&CrlDistributionPoints(vec![
                x509_cert::ext::pkix::crl::dp::DistributionPoint {
                    distribution_point: Some(DistributionPointName::FullName(vec![uri(CRL_URL)])),
                    reasons: None,
                    crl_issuer: None,
                },
            ]))
            .unwrap();
        }
        cert_of(&b.build::<p256::ecdsa::DerSignature>().unwrap())
    };
    let leaf_key = p256::ecdsa::SigningKey::random(&mut rng);
    let leaf = leaf_cert(&leaf_key, 42, "CN=Anna Ek,C=SE", true);
    let tsa_key = p256::ecdsa::SigningKey::random(&mut rng);
    let tsa = leaf_cert(&tsa_key, 7, "CN=pdfr Test TSA,C=SE", false);
    Pki {
        ca_key,
        ca: cert_of(&ca),
        leaf_key,
        leaf,
        tsa_key,
        tsa,
    }
}

impl Pki {
    fn store(&self) -> TrustStore {
        let mut t = TrustStore::default();
        t.add(self.ca.clone(), Anchor::EuQualified);
        t
    }

    /// An OCSP response from the root about the signer.
    fn ocsp(&self, status: CertStatus) -> Vec<u8> {
        let id = CertId::from_cert::<sha1::Sha1>(&self.ca.x, &self.leaf.x).unwrap();
        let single = SingleResponse::new(id, status, gtime(now() - 60))
            .with_next_update(gtime(now() + 7 * DAY));
        OcspResponseBuilder::new(self.ca.x.tbs_certificate.subject.clone())
            .with_single_response(single)
            .sign::<_, p256::ecdsa::DerSignature>(&mut self.ca_key.clone(), None, gtime(now()))
            .unwrap()
            .to_der()
            .unwrap()
    }

    fn revoked(&self, at: u64) -> CertStatus {
        CertStatus::revoked(RevokedInfo {
            revocation_time: gtime(at),
            revocation_reason: Some(CrlReason::KeyCompromise),
        })
    }

    /// A CRL from the root; `revoked_at` lists the signer as revoked.
    fn crl(&self, revoked_at: Option<u64>) -> Vec<u8> {
        let alg = AlgorithmIdentifierOwned {
            oid: oids::ECDSA_SHA256,
            parameters: None,
        };
        let revoked = revoked_at.map(|t| {
            vec![RevokedCert {
                serial_number: self.leaf.x.tbs_certificate.serial_number.clone(),
                revocation_date: time(t),
                crl_entry_extensions: Some(vec![Extension {
                    extn_id: oids::CRL_REASON,
                    critical: false,
                    extn_value: OctetString::new(CrlReason::Superseded.to_der().unwrap()).unwrap(),
                }]),
            }]
        });
        let tbs = TbsCertList {
            version: x509_cert::Version::V2,
            signature: alg.clone(),
            issuer: self.ca.x.tbs_certificate.subject.clone(),
            this_update: time(now() - 3600),
            next_update: Some(time(now() + 7 * DAY)),
            revoked_certificates: revoked,
            crl_extensions: None,
        };
        let sig: p256::ecdsa::DerSignature = self.ca_key.sign(&tbs.to_der().unwrap());
        CertificateList {
            tbs_cert_list: tbs,
            signature_algorithm: alg,
            signature: der::asn1::BitString::from_bytes(sig.as_bytes()).unwrap(),
        }
        .to_der()
        .unwrap()
    }

    /// An RFC 3161 token from the test TSA for `data`, dated `at`.
    fn token(&self, data: &[u8], at: u64) -> Vec<u8> {
        let info = x509_tsp::TstInfo {
            version: x509_tsp::TspVersion::V1,
            policy: der::asn1::ObjectIdentifier::new_unwrap("1.2.3.4"),
            message_imprint: x509_tsp::MessageImprint {
                hash_algorithm: spki::AlgorithmIdentifier {
                    oid: oids::SHA256,
                    parameters: None,
                },
                hashed_message: OctetString::new(pdfr_sign::sha256(data).to_vec()).unwrap(),
            },
            serial_number: der::asn1::Int::new(&[1]).unwrap(),
            gen_time: GeneralizedTime::from_unix_duration(Duration::from_secs(at)).unwrap(),
            accuracy: None,
            ordering: false,
            nonce: None,
            tsa: None,
            extensions: None,
        }
        .to_der()
        .unwrap();
        let attr = |oid, v: Any| Attribute {
            oid,
            values: SetOfVec::try_from(vec![v]).unwrap(),
        };
        let attrs = SetOfVec::try_from(vec![
            attr(
                oids::CONTENT_TYPE,
                Any::encode_from(&oids::TST_INFO).unwrap(),
            ),
            attr(
                oids::MESSAGE_DIGEST,
                Any::encode_from(&OctetString::new(pdfr_sign::sha256(&info).to_vec()).unwrap())
                    .unwrap(),
            ),
        ])
        .unwrap();
        let sig: p256::ecdsa::DerSignature = self.tsa_key.sign(&attrs.to_der().unwrap());
        let alg = |oid| AlgorithmIdentifierOwned {
            oid,
            parameters: None,
        };
        let si = SignerInfo {
            version: CmsVersion::V1,
            sid: SignerIdentifier::IssuerAndSerialNumber(IssuerAndSerialNumber {
                issuer: self.tsa.x.tbs_certificate.issuer.clone(),
                serial_number: self.tsa.x.tbs_certificate.serial_number.clone(),
            }),
            digest_alg: alg(oids::SHA256),
            signed_attrs: Some(attrs),
            signature_algorithm: alg(oids::ECDSA_SHA256),
            signature: OctetString::new(sig.as_bytes()).unwrap(),
            unsigned_attrs: None,
        };
        let sd = SignedData {
            version: CmsVersion::V3,
            digest_algorithms: SetOfVec::try_from(vec![alg(oids::SHA256)]).unwrap(),
            encap_content_info: EncapsulatedContentInfo {
                econtent_type: oids::TST_INFO,
                econtent: Some(Any::encode_from(&OctetString::new(info).unwrap()).unwrap()),
            },
            certificates: Some(CertificateSet(
                SetOfVec::try_from(vec![CertificateChoices::Certificate(self.tsa.x.clone())])
                    .unwrap(),
            )),
            crls: None,
            signer_infos: SignerInfos(SetOfVec::try_from(vec![si]).unwrap()),
        };
        ContentInfo {
            content_type: oids::SIGNED_DATA,
            content: Any::encode_from(&sd).unwrap(),
        }
        .to_der()
        .unwrap()
    }
}

/// The signer's key, in memory.
struct KeySigner {
    key: p256::ecdsa::SigningKey,
    cert: Cert,
    chain: Vec<Cert>,
}

impl Signer for KeySigner {
    fn cert(&self) -> &Cert {
        &self.cert
    }
    fn chain(&self) -> &[Cert] {
        &self.chain
    }
    fn alg(&self) -> SigAlg {
        SigAlg::EcdsaP256Sha256
    }
    fn sign(&self, msg: &[u8]) -> Result<Vec<u8>, String> {
        let s: p256::ecdsa::DerSignature = self.key.sign(msg);
        Ok(s.as_bytes().to_vec())
    }
}

/// The issuers, as the test sets them up; counts the requests.
#[derive(Default)]
struct Issuers {
    ocsp: Option<Vec<u8>>,
    crl: Option<Vec<u8>>,
    calls: Mutex<Vec<String>>,
}

impl Fetch for Issuers {
    fn post(&self, url: &str, content_type: &str, body: &[u8]) -> Result<Vec<u8>, String> {
        self.calls.lock().unwrap().push(url.into());
        assert_eq!(content_type, "application/ocsp-request");
        x509_ocsp::OcspRequest::from_der(body).expect("a valid OCSP request");
        self.ocsp.clone().ok_or_else(|| "offline".into())
    }
    fn get(&self, url: &str) -> Result<Vec<u8>, String> {
        self.calls.lock().unwrap().push(url.into());
        self.crl.clone().ok_or_else(|| "offline".into())
    }
}

/// `real_form.pdf` signed by the test signer at `signed_at`, timestamped
/// at `stamped_at` if given.
fn signed(p: &Pki, signed_at: u64, stamped_at: Option<u64>) -> Vec<u8> {
    let signer = KeySigner {
        key: p.leaf_key.clone(),
        cert: p.leaf.clone(),
        chain: vec![p.ca.clone()],
    };
    let fd = FormDoc::open(sample("real_form.pdf")).unwrap();
    let req = SignRequest {
        spot: None,
        ink: None,
        signer: "Anna Ek".into(),
        reason: None,
        location: None,
        time: signed_at,
        reserve: pdfr_core::digital::DEFAULT_RESERVE,
    };
    let ts = |d: &[u8]| Ok(p.token(d, stamped_at.unwrap()));
    sign(&fd, &fd.fields, &req, |content| {
        let tsa = stamped_at.is_some().then_some(&ts as cades::Timestamper);
        cades::sign(content, &signer, tsa).map(|s| s.der)
    })
    .unwrap()
}

fn check(p: &Pki, bytes: &[u8], issuers: Option<&Issuers>, cache: &Cache) -> Vec<Report> {
    let fd = FormDoc::open(bytes.to_vec()).unwrap();
    let online = issuers.map(|i| Online { fetch: i, cache });
    verify_all_with(bytes, &fd.digital, &p.store(), online)
}

#[test]
fn good_answer_online_is_cached() {
    let p = pki();
    let bytes = signed(&p, now(), Some(now()));
    let issuers = Issuers {
        ocsp: Some(p.ocsp(CertStatus::good())),
        ..Default::default()
    };
    let cache = Cache::default();
    let r = &check(&p, &bytes, Some(&issuers), &cache)[0];
    assert_eq!(r.status, Status::Valid, "{:?}", r.problem);
    assert_eq!(r.kind, "PAdES B-T");
    let rev = r.revocation.as_ref().unwrap();
    assert_eq!(rev.state, State::Good);
    assert_eq!(rev.source, Some(Source::Online));
    assert_eq!(*issuers.calls.lock().unwrap(), vec![OCSP_URL.to_string()]);
    // Asked again: the answer comes from memory.
    check(&p, &bytes, Some(&issuers), &cache);
    assert_eq!(issuers.calls.lock().unwrap().len(), 1);
    // Online checks off: unknown, and still valid.
    let r = &check(&p, &bytes, None, &cache)[0];
    assert_eq!(r.status, Status::Valid);
    assert!(matches!(
        r.revocation.as_ref().unwrap().state,
        State::Unknown(_)
    ));
}

#[test]
fn unknown_when_the_issuer_cannot_be_reached() {
    let p = pki();
    let bytes = signed(&p, now(), Some(now()));
    let issuers = Issuers::default();
    let r = &check(&p, &bytes, Some(&issuers), &Cache::default())[0];
    assert_eq!(r.status, Status::Valid, "{:?}", r.problem);
    assert_eq!(
        r.revocation.as_ref().unwrap().state,
        State::Unknown("offline".into())
    );
    // Both the responder and the CRL were tried.
    assert_eq!(issuers.calls.lock().unwrap().len(), 2);
}

#[test]
fn revoked_before_signing_is_not_valid() {
    let p = pki();
    let bytes = signed(&p, now(), Some(now()));
    let issuers = Issuers {
        ocsp: Some(p.ocsp(p.revoked(now() - DAY))),
        ..Default::default()
    };
    let r = &check(&p, &bytes, Some(&issuers), &Cache::default())[0];
    assert_eq!(r.status, Status::Broken);
    assert!(
        r.problem.as_ref().unwrap().contains("revoked"),
        "{:?}",
        r.problem
    );
    assert!(matches!(
        &r.revocation.as_ref().unwrap().state,
        State::Revoked { reason: Some(why), .. } if why.contains("compromised")
    ));
}

#[test]
fn revoked_after_the_timestamp_stays_valid() {
    let p = pki();
    let issuers = Issuers {
        ocsp: Some(p.ocsp(p.revoked(now() - DAY))),
        ..Default::default()
    };
    // Timestamped two days ago, revoked yesterday: fine.
    let bytes = signed(&p, now() - 2 * DAY, Some(now() - 2 * DAY));
    let r = &check(&p, &bytes, Some(&issuers), &Cache::default())[0];
    assert_eq!(r.status, Status::Valid, "{:?}", r.problem);
    assert!(r.revocation.as_ref().unwrap().is_revoked());

    // Timestamped after the revocation: not valid.
    let bytes = signed(&p, now() - 2 * DAY, Some(now() - DAY / 2));
    let r = &check(&p, &bytes, Some(&issuers), &Cache::default())[0];
    assert_eq!(r.status, Status::Broken);

    // No timestamp, the signer's computer says before the revocation:
    // nothing proves it, so not confirmed.
    let bytes = signed(&p, now() - 2 * DAY, None);
    let r = &check(&p, &bytes, Some(&issuers), &Cache::default())[0];
    assert_eq!(r.status, Status::Untrusted, "{:?}", r.problem);
}

#[test]
fn crl_when_the_responder_fails() {
    let p = pki();
    let bytes = signed(&p, now(), Some(now()));
    let issuers = Issuers {
        crl: Some(p.crl(Some(now() - DAY))),
        ..Default::default()
    };
    let r = &check(&p, &bytes, Some(&issuers), &Cache::default())[0];
    assert_eq!(r.status, Status::Broken);
    assert!(matches!(
        &r.revocation.as_ref().unwrap().state,
        State::Revoked { reason: Some(why), .. } if why.contains("replaced")
    ));
    let issuers = Issuers {
        crl: Some(p.crl(None)),
        ..Default::default()
    };
    let r = &check(&p, &bytes, Some(&issuers), &Cache::default())[0];
    assert_eq!(r.revocation.as_ref().unwrap().state, State::Good);
}

#[test]
fn an_answer_signed_by_someone_else_is_ignored() {
    let p = pki();
    let other = pki();
    let bytes = signed(&p, now(), Some(now()));
    // A "revoked" answer for the same serial from another CA's key.
    let forged = Issuers {
        ocsp: Some(other.ocsp(other.revoked(now() - DAY))),
        ..Default::default()
    };
    let r = &check(&p, &bytes, Some(&forged), &Cache::default())[0];
    assert_eq!(r.status, Status::Valid);
    assert!(matches!(
        r.revocation.as_ref().unwrap().state,
        State::Unknown(_)
    ));
}

#[test]
fn long_term_validation_round_trip() {
    let p = pki();
    let bt = signed(&p, now(), Some(now()));
    let issuers = Issuers {
        ocsp: Some(p.ocsp(CertStatus::good())),
        ..Default::default()
    };
    let cache = Cache::default();
    let online = Online {
        fetch: &issuers,
        cache: &cache,
    };
    let ts = |d: &[u8]| Ok(p.token(d, now()));

    // B-LT: the DSS only.
    let lt = extend(bt.clone(), &p.store(), online, None);
    assert_eq!(lt.level, Level::Lt, "{:?}", lt.problem);
    assert!(lt.bytes.starts_with(&bt), "added as an update");
    let fd = FormDoc::open(lt.bytes.clone()).unwrap();
    let dss = pdfr_sign::revocation::from_dss(fd.doc.as_ref().unwrap());
    assert_eq!(dss.ocsps.len(), 1);
    assert!(dss.certs.contains(&p.leaf.der) && dss.certs.contains(&p.ca.der));
    let r = &verify_all(&lt.bytes, &fd.digital, &p.store())[0];
    assert_eq!(r.status, Status::Valid, "{:?}", r.problem);
    assert_eq!(r.after, After::Nothing, "validation data is not a change");
    assert_eq!(r.kind, "PAdES B-LT");
    let rev = r.revocation.as_ref().unwrap();
    assert_eq!((&rev.state, rev.source), (&State::Good, Some(Source::File)));

    // B-LTA: and a document timestamp; checked offline, from the file.
    let lta = extend(
        bt.clone(),
        &p.store(),
        online,
        Some(&ts as cades::Timestamper),
    );
    assert_eq!(lta.level, Level::Lta, "{:?}", lta.problem);
    let fd = FormDoc::open(lta.bytes.clone()).unwrap();
    assert_eq!(fd.digital.len(), 2);
    let rs = verify_all(&lta.bytes, &fd.digital, &p.store());
    let sig = rs.iter().find(|r| !r.doc_timestamp).unwrap();
    let stamp = rs.iter().find(|r| r.doc_timestamp).unwrap();
    assert_eq!(sig.status, Status::Valid, "{:?}", sig.problem);
    assert_eq!(sig.after, After::Nothing);
    assert_eq!(sig.kind, "PAdES B-LTA");
    assert_eq!(stamp.status, Status::Valid, "{:?}", stamp.problem);
    assert_eq!(stamp.kind, "Document timestamp");
    assert_eq!(stamp.signer, "pdfr Test TSA");

    // Filling in a field afterwards is still an allowed change.
    let mut fields = fd.fields.clone();
    let email = fields
        .iter_mut()
        .find(|f| matches!(&f.origin, Origin::AcroForm { name, .. } if name == "contact.email"))
        .unwrap();
    email.value = "anna@example.se".into();
    let after = pdfr_core::save::save(&fd, &fields).unwrap();
    assert!(after.starts_with(&lta.bytes));
    let fd2 = FormDoc::open(after.clone()).unwrap();
    let rs = verify_all(&after, &fd2.digital, &p.store());
    assert!(rs.iter().all(|r| r.status == Status::Valid), "{rs:?}");
    assert!(
        rs.iter()
            .all(|r| matches!(&r.after, After::Changes { allowed: true, .. })),
        "{rs:?}"
    );

    // Tampering with the timestamped part breaks the document timestamp.
    let mut bad = lta.bytes.clone();
    let i = bad.windows(9).position(|w| w == b"/MediaBox").unwrap();
    bad[i + 1] = b'm';
    let rs = verify_all(&bad, &fd.digital, &p.store());
    assert!(rs.iter().all(|r| r.status == Status::Broken), "{rs:?}");
}

#[test]
fn long_term_validation_is_best_effort() {
    let p = pki();
    let bt = signed(&p, now(), Some(now()));
    let offline = Issuers::default();
    let cache = Cache::default();
    let online = Online {
        fetch: &offline,
        cache: &cache,
    };
    let x = extend(bt.clone(), &p.store(), online, None);
    assert_eq!(x.level, Level::T);
    assert!(x.problem.is_some());
    assert_eq!(x.bytes, bt, "unchanged");

    // Without a timestamp there is nothing to build on.
    let bb = signed(&p, now(), None);
    let x = extend(bb.clone(), &p.store(), online, None);
    assert_eq!(x.level, Level::B);
    assert_eq!(x.bytes, bb);
}

#[test]
fn ocsp_request_names_the_certificate() {
    let p = pki();
    let req = pdfr_sign::revocation::ocsp_request(&p.leaf, &p.ca).unwrap();
    let r = x509_ocsp::OcspRequest::from_der(&req).unwrap();
    let id = &r.tbs_request.request_list[0].req_cert;
    assert_eq!(
        *id,
        CertId::from_cert::<sha1::Sha1>(&p.ca.x, &p.leaf.x).unwrap()
    );
    assert_eq!(pdfr_sign::revocation::ocsp_urls(&p.leaf), vec![OCSP_URL]);
    assert_eq!(pdfr_sign::revocation::crl_urls(&p.leaf), vec![CRL_URL]);
}
