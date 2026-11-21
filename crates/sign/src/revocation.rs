//! Was a certificate revoked? OCSP (RFC 6960) and CRLs (RFC 5280).
//!
//! Revocation data kept in the file (the PDF's DSS, or inside the
//! signature) is used first. Only when there is none, and the caller
//! allows it, is the certificate's issuer asked: its OCSP responder, else
//! its CRL, both named in the certificate. Asking tells the issuer which
//! certificate is being checked, so it is a setting. The network is the
//! caller's: this crate has no HTTP client (see [`Fetch`]).

use crate::{Cert, oids, trust};
use cms::signed_data::{SignedData, SignerInfo};
use der::asn1::{Null, OctetString};
use der::{Any, Decode, Encode};
use spki::AlgorithmIdentifierOwned;
use std::collections::HashMap;
use std::sync::Mutex;
use x509_cert::crl::CertificateList;
use x509_cert::ext::pkix::name::{DistributionPointName, GeneralName};
use x509_cert::ext::pkix::{
    AuthorityInfoAccessSyntax, CrlDistributionPoints, CrlReason, ExtendedKeyUsage,
};
use x509_ocsp::{
    BasicOcspResponse, CertId, CertStatus, OcspRequest, OcspResponse, OcspResponseStatus, Request,
    TbsRequest, Version,
};

/// What is known about a certificate's revocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// Not revoked.
    Good,
    /// Revoked at `at` (Unix seconds), for `reason` in plain words.
    Revoked { at: u64, reason: Option<String> },
    /// Couldn't be checked, and why.
    Unknown(String),
}

/// Where the answer came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// Revocation data kept in the PDF.
    File,
    /// Asked the issuer just now (or within the last day).
    Online,
}

/// The answer for a signature: about its signer's certificate, or about a
/// certificate authority above it when that one was revoked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Revocation {
    pub state: State,
    pub source: Option<Source>,
    /// When the issuer gave the answer (OCSP / CRL `thisUpdate`).
    pub at: Option<u64>,
    /// The certificate authority whose certificate the answer is about,
    /// when it is not the signer's.
    pub ca: Option<String>,
}

impl Revocation {
    pub fn unknown(why: impl Into<String>) -> Revocation {
        Revocation {
            state: State::Unknown(why.into()),
            source: None,
            at: None,
            ca: None,
        }
    }

    pub fn is_revoked(&self) -> bool {
        matches!(self.state, State::Revoked { .. })
    }
}

/// Validation data: DER certificates, OCSP responses and CRLs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Evidence {
    pub certs: Vec<Vec<u8>>,
    pub ocsps: Vec<Vec<u8>>,
    pub crls: Vec<Vec<u8>>,
}

impl Evidence {
    /// Adds `o`'s items not already here.
    pub fn merge(&mut self, o: &Evidence) {
        let add = |to: &mut Vec<Vec<u8>>, from: &[Vec<u8>]| {
            for x in from {
                if !to.contains(x) {
                    to.push(x.clone());
                }
            }
        };
        add(&mut self.certs, &o.certs);
        add(&mut self.ocsps, &o.ocsps);
        add(&mut self.crls, &o.crls);
    }

    pub fn has_revocation_data(&self) -> bool {
        !self.ocsps.is_empty() || !self.crls.is_empty()
    }

    /// The certificates, parsed.
    pub fn cert_pool(&self) -> Vec<Cert> {
        self.certs
            .iter()
            .filter_map(|d| Cert::from_der(d).ok())
            .collect()
    }
}

/// The network, for asking issuers. Implementations should time out
/// quickly and limit the size of what they read.
pub trait Fetch: Sync {
    /// POSTs `body` to `url` with `content_type`; returns the response body.
    fn post(&self, url: &str, content_type: &str, body: &[u8]) -> Result<Vec<u8>, String>;
    /// GETs `url`; returns the response body.
    fn get(&self, url: &str) -> Result<Vec<u8>, String>;
}

/// Answers from issuers, kept in memory for a day per certificate.
#[derive(Default)]
pub struct Cache(Mutex<HashMap<String, (u64, Found)>>);

const CACHE_FOR: u64 = 24 * 3600;

impl Cache {
    fn get(&self, key: &str, now: u64) -> Option<Found> {
        let m = self.0.lock().ok()?;
        m.get(key)
            .filter(|(t, _)| now.saturating_sub(*t) < CACHE_FOR)
            .map(|(_, f)| f.clone())
    }

    fn put(&self, key: String, now: u64, f: Found) {
        if let Ok(mut m) = self.0.lock() {
            m.retain(|_, (t, _)| now.saturating_sub(*t) < CACHE_FOR);
            m.insert(key, (now, f));
        }
    }
}

/// Permission and means to ask issuers.
#[derive(Clone, Copy)]
pub struct Online<'a> {
    pub fetch: &'a dyn Fetch,
    pub cache: &'a Cache,
}

/// One certificate's answer and the data that gave it.
#[derive(Clone, Debug)]
pub struct Found {
    pub rev: Revocation,
    pub data: Evidence,
}

/// What one OCSP response or CRL says about a certificate.
struct Answer {
    state: State,
    this_update: u64,
    next_update: Option<u64>,
}

/// Checks `leaf` and the certificate authorities above it. `chain` is
/// the issuers, nearest first; a self-issued (root) certificate is not
/// checked. Returns the answer and the data used.
pub fn check_path(
    leaf: &Cert,
    chain: &[Cert],
    file: &Evidence,
    online: Option<Online>,
    now: u64,
) -> (Revocation, Evidence) {
    let Some(issuer) = chain.first() else {
        return (
            Revocation::unknown("the issuer's certificate was not found"),
            Evidence::default(),
        );
    };
    let first = check(leaf, issuer, file, online, now);
    let mut data = first.data.clone();
    let mut out = first.rev;
    for pair in chain.windows(2) {
        if pair[0].self_issued() {
            break;
        }
        let f = check(&pair[0], &pair[1], file, online, now);
        data.merge(&f.data);
        if f.rev.is_revoked() && !out.is_revoked() {
            out = Revocation {
                ca: Some(pair[0].name()),
                ..f.rev
            };
        }
    }
    (out, data)
}

/// Checks `cert` (issued by `issuer`): from `file`'s data if it has an
/// answer, else by asking the issuer if `online` allows.
pub fn check(
    cert: &Cert,
    issuer: &Cert,
    file: &Evidence,
    online: Option<Online>,
    now: u64,
) -> Found {
    if let Some(f) = from_evidence(cert, issuer, file) {
        return f;
    }
    let Some(on) = online else {
        return Found {
            rev: Revocation::unknown("the file has no revocation data, and online checks are off"),
            data: Evidence::default(),
        };
    };
    let key = format!("{}/{}", cert.fingerprint(), issuer.fingerprint());
    if let Some(f) = on.cache.get(&key, now) {
        return f;
    }
    let f = match ask(cert, issuer, on.fetch, now) {
        Ok((a, data)) => Found {
            rev: Revocation {
                state: a.state,
                source: Some(Source::Online),
                at: Some(a.this_update),
                ca: None,
            },
            data,
        },
        Err(e) => Found {
            rev: Revocation::unknown(e),
            data: Evidence::default(),
        },
    };
    if !matches!(f.rev.state, State::Unknown(_)) {
        on.cache.put(key, now, f.clone());
    }
    f
}

/// The answer from data already at hand: a revocation wins, else the
/// newest "good".
fn from_evidence(cert: &Cert, issuer: &Cert, ev: &Evidence) -> Option<Found> {
    let ocsps = ev.ocsps.iter().filter_map(|d| {
        let a = ocsp_answer(d, cert, issuer)?;
        Some((
            a,
            Evidence {
                ocsps: vec![d.clone()],
                ..Default::default()
            },
        ))
    });
    let crls = ev.crls.iter().filter_map(|d| {
        let a = crl_answer(d, cert, issuer)?;
        Some((
            a,
            Evidence {
                crls: vec![d.clone()],
                ..Default::default()
            },
        ))
    });
    let all: Vec<_> = ocsps
        .chain(crls)
        .filter(|(a, _)| !matches!(a.state, State::Unknown(_)))
        .collect();
    let (a, data) = all
        .iter()
        .find(|(a, _)| matches!(a.state, State::Revoked { .. }))
        .or_else(|| all.iter().max_by_key(|(a, _)| a.this_update))?;
    Some(Found {
        rev: Revocation {
            state: a.state.clone(),
            source: Some(Source::File),
            at: Some(a.this_update),
            ca: None,
        },
        data: data.clone(),
    })
}

/// Asks `cert`'s issuer: OCSP first, then the CRL.
fn ask(
    cert: &Cert,
    issuer: &Cert,
    fetch: &dyn Fetch,
    now: u64,
) -> Result<(Answer, Evidence), String> {
    let ocsp = ocsp_urls(cert);
    let crl = crl_urls(cert);
    if ocsp.is_empty() && crl.is_empty() {
        return Err("the certificate doesn't say where to check it".into());
    }
    let mut problem = None;
    if !ocsp.is_empty() {
        let req = ocsp_request(cert, issuer)?;
        for url in &ocsp {
            match fetch.post(url, "application/ocsp-request", &req) {
                Ok(body) => match ocsp_answer(&body, cert, issuer) {
                    Some(a) if fresh(&a, now) => {
                        let data = Evidence {
                            ocsps: vec![body],
                            ..Default::default()
                        };
                        return Ok((a, data));
                    }
                    Some(a) if matches!(a.state, State::Unknown(_)) => {
                        problem = Some("the issuer doesn't know this certificate".to_string());
                    }
                    _ => problem = Some("the issuer's answer could not be checked".into()),
                },
                Err(e) => problem = Some(e),
            }
        }
    }
    for url in &crl {
        match fetch.get(url) {
            Ok(body) => match crl_answer(&body, cert, issuer) {
                Some(a) if fresh(&a, now) => {
                    let data = Evidence {
                        crls: vec![body],
                        ..Default::default()
                    };
                    return Ok((a, data));
                }
                _ => problem = Some("the issuer's revocation list could not be checked".into()),
            },
            Err(e) => problem = Some(e),
        }
    }
    Err(problem.unwrap_or_else(|| "the issuer could not be asked".into()))
}

/// A known answer that is current: issued already, not past its next
/// update (or, without one, issued within the last month).
fn fresh(a: &Answer, now: u64) -> bool {
    !matches!(a.state, State::Unknown(_))
        && a.this_update <= now + 600
        && match a.next_update {
            Some(n) => n + 24 * 3600 >= now,
            None => a.this_update + 30 * 24 * 3600 >= now,
        }
}

// --- OCSP --------------------------------------------------------------------

/// An OCSP request for `cert` (SHA-1 CertID, as responders expect).
pub fn ocsp_request(cert: &Cert, issuer: &Cert) -> Result<Vec<u8>, String> {
    let e = |e: der::Error| e.to_string();
    let spki = &issuer.x.tbs_certificate.subject_public_key_info;
    let id = CertId {
        hash_algorithm: AlgorithmIdentifierOwned {
            oid: oids::SHA1,
            parameters: Some(Any::encode_from(&Null).map_err(e)?),
        },
        issuer_name_hash: OctetString::new(
            trust::hash(oids::SHA1, &issuer.subject().to_der().map_err(e)?).unwrap_or_default(),
        )
        .map_err(e)?,
        issuer_key_hash: OctetString::new(
            trust::hash(oids::SHA1, spki.subject_public_key.raw_bytes()).unwrap_or_default(),
        )
        .map_err(e)?,
        serial_number: cert.x.tbs_certificate.serial_number.clone(),
    };
    OcspRequest {
        tbs_request: TbsRequest {
            version: Version::V1,
            requestor_name: None,
            request_list: vec![Request {
                req_cert: id,
                single_request_extensions: None,
            }],
            request_extensions: None,
        },
        optional_signature: None,
    }
    .to_der()
    .map_err(e)
}

/// What the OCSP response `der` says about `cert`, if it is a response
/// about it signed by its issuer (or a responder the issuer authorised).
fn ocsp_answer(der: &[u8], cert: &Cert, issuer: &Cert) -> Option<Answer> {
    let resp = OcspResponse::from_der(der).ok()?;
    if resp.response_status != OcspResponseStatus::Successful {
        return None;
    }
    let rb = resp.response_bytes?;
    if rb.response_type != oids::OCSP_BASIC {
        return None;
    }
    let basic = BasicOcspResponse::from_der(rb.response.as_bytes()).ok()?;
    let single = basic
        .tbs_response_data
        .responses
        .iter()
        .find(|s| cert_id_matches(&s.cert_id, cert, issuer))?;
    if !ocsp_signed(&basic, issuer) {
        return None;
    }
    let secs = |t: &x509_ocsp::OcspGeneralizedTime| t.0.to_unix_duration().as_secs();
    let state = match &single.cert_status {
        CertStatus::Good(_) => State::Good,
        CertStatus::Revoked(ri) => match ri.revocation_reason {
            Some(CrlReason::RemoveFromCRL) => State::Good,
            r => State::Revoked {
                at: secs(&ri.revocation_time),
                reason: r.and_then(reason_words),
            },
        },
        CertStatus::Unknown(_) => State::Unknown("the issuer doesn't know this certificate".into()),
    };
    Some(Answer {
        state,
        this_update: secs(&single.this_update),
        next_update: single.next_update.as_ref().map(secs),
    })
}

fn cert_id_matches(id: &CertId, cert: &Cert, issuer: &Cert) -> bool {
    let alg = id.hash_algorithm.oid;
    let Ok(name) = issuer.subject().to_der() else {
        return false;
    };
    let key = issuer
        .x
        .tbs_certificate
        .subject_public_key_info
        .subject_public_key
        .raw_bytes();
    id.serial_number == cert.x.tbs_certificate.serial_number
        && trust::hash(alg, &name).as_deref() == Some(id.issuer_name_hash.as_bytes())
        && trust::hash(alg, key).as_deref() == Some(id.issuer_key_hash.as_bytes())
}

/// Signed by `issuer`, or by a responder certificate `issuer` issued for
/// OCSP signing (included in the response).
fn ocsp_signed(basic: &BasicOcspResponse, issuer: &Cert) -> bool {
    let (Ok(tbs), Some(sig)) = (basic.tbs_response_data.to_der(), basic.signature.as_bytes())
    else {
        return false;
    };
    let alg = &basic.signature_algorithm;
    let params = alg.parameters.as_ref().and_then(|p| p.to_der().ok());
    let by = |c: &Cert| {
        trust::verify(
            &c.x.tbs_certificate.subject_public_key_info,
            alg.oid,
            params.as_deref(),
            None,
            &tbs,
            sig,
        )
        .is_ok()
    };
    if by(issuer) {
        return true;
    }
    let produced = basic
        .tbs_response_data
        .produced_at
        .0
        .to_unix_duration()
        .as_secs();
    basic
        .certs
        .iter()
        .flatten()
        .filter_map(|c| Cert::from_der(&c.to_der().ok()?).ok())
        .any(|r| {
            r.issuer() == issuer.subject()
                && trust::verify_cert(&r, issuer)
                && r.valid_at(produced)
                && r.ext::<ExtendedKeyUsage>(oids::EXT_KEY_USAGE)
                    .is_some_and(|k| k.0.contains(&oids::KP_OCSP_SIGNING))
                && by(&r)
        })
}

// --- CRLs --------------------------------------------------------------------

/// What the CRL `der` says about `cert`, if `issuer` signed it.
fn crl_answer(der: &[u8], cert: &Cert, issuer: &Cert) -> Option<Answer> {
    let crl = CertificateList::from_der(der).ok()?;
    let tbs = &crl.tbs_cert_list;
    if tbs.issuer != *cert.issuer() || tbs.issuer != *issuer.subject() {
        return None;
    }
    // Delta CRLs list only changes.
    let delta = const_oid::ObjectIdentifier::new_unwrap("2.5.29.27");
    if tbs
        .crl_extensions
        .iter()
        .flatten()
        .any(|e| e.extn_id == delta)
    {
        return None;
    }
    let msg = tbs.to_der().ok()?;
    let alg = &crl.signature_algorithm;
    let params = alg.parameters.as_ref().and_then(|p| p.to_der().ok());
    trust::verify(
        &issuer.x.tbs_certificate.subject_public_key_info,
        alg.oid,
        params.as_deref(),
        None,
        &msg,
        crl.signature.as_bytes()?,
    )
    .ok()?;
    let serial = &cert.x.tbs_certificate.serial_number;
    let state = match tbs
        .revoked_certificates
        .iter()
        .flatten()
        .find(|r| r.serial_number == *serial)
    {
        None => State::Good,
        Some(r) => {
            let reason = r
                .crl_entry_extensions
                .iter()
                .flatten()
                .find(|e| e.extn_id == oids::CRL_REASON)
                .and_then(|e| CrlReason::from_der(e.extn_value.as_bytes()).ok());
            if reason == Some(CrlReason::RemoveFromCRL) {
                State::Good
            } else {
                State::Revoked {
                    at: r.revocation_date.to_unix_duration().as_secs(),
                    reason: reason.and_then(reason_words),
                }
            }
        }
    };
    Some(Answer {
        state,
        this_update: tbs.this_update.to_unix_duration().as_secs(),
        next_update: tbs.next_update.map(|t| t.to_unix_duration().as_secs()),
    })
}

fn reason_words(r: CrlReason) -> Option<String> {
    Some(
        match r {
            CrlReason::Unspecified | CrlReason::RemoveFromCRL => return None,
            CrlReason::KeyCompromise => "the key was compromised",
            CrlReason::CaCompromise | CrlReason::AaCompromise => "the issuer was compromised",
            CrlReason::AffiliationChanged => "the holder's details changed",
            CrlReason::Superseded => "replaced by a newer certificate",
            CrlReason::CessationOfOperation => "no longer in use",
            CrlReason::CertificateHold => "put on hold",
            CrlReason::PrivilegeWithdrawn => "permission withdrawn",
        }
        .into(),
    )
}

// --- Where to ask --------------------------------------------------------------

fn http(n: &GeneralName) -> Option<String> {
    match n {
        GeneralName::UniformResourceIdentifier(u) => {
            let s = u.as_str();
            (s.starts_with("http://") || s.starts_with("https://")).then(|| s.to_string())
        }
        _ => None,
    }
}

/// The OCSP responders named in `cert` (http only).
pub fn ocsp_urls(cert: &Cert) -> Vec<String> {
    cert.ext::<AuthorityInfoAccessSyntax>(oids::AUTHORITY_INFO_ACCESS)
        .map(|a| {
            a.0.iter()
                .filter(|d| d.access_method == oids::AD_OCSP)
                .filter_map(|d| http(&d.access_location))
                .collect()
        })
        .unwrap_or_default()
}

/// The CRLs named in `cert` (http only).
pub fn crl_urls(cert: &Cert) -> Vec<String> {
    cert.ext::<CrlDistributionPoints>(oids::CRL_DISTRIBUTION_POINTS)
        .map(|d| {
            d.0.iter()
                .filter_map(|p| match &p.distribution_point {
                    Some(DistributionPointName::FullName(names)) => Some(names),
                    _ => None,
                })
                .flatten()
                .filter_map(http)
                .collect()
        })
        .unwrap_or_default()
}

// --- Data inside a signature ---------------------------------------------------

/// Revocation data inside a CMS signature: its `crls` field (CRLs and
/// OCSP responses) and Adobe's revocation attribute.
pub fn from_cms(sd: &SignedData, si: &SignerInfo) -> Evidence {
    let mut ev = Evidence::default();
    for c in sd.crls.iter().flat_map(|c| c.0.iter()) {
        match c {
            cms::revocation::RevocationInfoChoice::Crl(l) => {
                ev.crls.extend(l.to_der().ok());
            }
            cms::revocation::RevocationInfoChoice::Other(o) => {
                if o.other_format.oid == oids::RI_OCSP_RESPONSE {
                    ev.ocsps.extend(o.other.to_der().ok());
                }
            }
        }
    }
    let adbe = si
        .signed_attrs
        .iter()
        .flat_map(|a| a.iter())
        .find(|a| a.oid == oids::ADBE_REVOCATION)
        .and_then(|a| a.values.iter().next())
        .and_then(|v| v.to_der().ok());
    if let Some(v) = adbe {
        // RevocationInfoArchival ::= SEQUENCE {
        //   crl [0] EXPLICIT SEQUENCE OF CRLs OPTIONAL,
        //   ocsp [1] EXPLICIT SEQUENCE OF OCSPResponse OPTIONAL, ... }
        for (tag, part) in children(&v) {
            let list = children(part).into_iter().next().map(|(_, l)| l);
            let items = list.map(children).unwrap_or_default();
            for (_, item) in items {
                match tag {
                    0xA0 => ev.crls.push(item.to_vec()),
                    0xA1 => ev.ocsps.push(item.to_vec()),
                    _ => {}
                }
            }
        }
    }
    ev
}

/// The elements inside the DER element `b`: (tag, whole element).
fn children(b: &[u8]) -> Vec<(u8, &[u8])> {
    let Some((_, hdr, len)) = header(b) else {
        return vec![];
    };
    let mut rest = b.get(hdr..hdr + len).unwrap_or_default();
    let mut out = Vec::new();
    while let Some((tag, h, l)) = header(rest) {
        let Some(el) = rest.get(..h + l) else { break };
        out.push((tag, el));
        rest = &rest[h + l..];
    }
    out
}

/// Tag, header length and content length of the DER element at `b`.
fn header(b: &[u8]) -> Option<(u8, usize, usize)> {
    let tag = *b.first()?;
    let first = *b.get(1)?;
    if first < 0x80 {
        return Some((tag, 2, first as usize));
    }
    let k = (first & 0x7f) as usize;
    if k == 0 || k > 4 {
        return None;
    }
    let len = b
        .get(2..2 + k)?
        .iter()
        .fold(0usize, |a, x| (a << 8) | *x as usize);
    Some((tag, 2 + k, len))
}

// --- The PDF's DSS ----------------------------------------------------------------

/// Largest stream read from the DSS.
const MAX_ITEM: usize = 16 << 20;

/// The validation data in the file's Document Security Store (`/DSS`).
pub fn from_dss(doc: &lopdf::Document) -> Evidence {
    use lopdf::Object;
    let deref = |o: &Object| -> Option<Object> {
        match o {
            Object::Reference(r) => doc.get_object(*r).ok().cloned(),
            o => Some(o.clone()),
        }
    };
    let mut ev = Evidence::default();
    let Some(Object::Dictionary(dss)) = doc
        .catalog()
        .ok()
        .and_then(|c| c.get(b"DSS").ok())
        .and_then(deref)
    else {
        return ev;
    };
    let read = |key: &[u8], to: &mut Vec<Vec<u8>>| {
        let Some(Object::Array(a)) = dss.get(key).ok().and_then(deref) else {
            return;
        };
        for o in &a {
            if let Some(Object::Stream(s)) = deref(o)
                && let Ok(b) = s.get_plain_content_with_limit(MAX_ITEM)
                && !to.contains(&b)
            {
                to.push(b);
            }
        }
    };
    read(b"Certs", &mut ev.certs);
    read(b"OCSPs", &mut ev.ocsps);
    read(b"CRLs", &mut ev.crls);
    ev
}
