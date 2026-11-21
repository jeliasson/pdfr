//! Checking the digital signatures in a PDF.
//!
//! For each signature: do the signed bytes still hash to what was signed,
//! does the signature match the signer's certificate, does that
//! certificate lead to a trusted root, is there a valid timestamp, and was
//! the file changed after signing (and if so, only in allowed ways: form
//! fill, annotations, further signatures), and was the signer's
//! certificate revoked (see `revocation`). Document timestamps
//! (`ETSI.RFC3161`) are checked too, and give the PAdES level B-LTA.

use crate::revocation::{Evidence, Online, Revocation, State};
use crate::trust::{Anchor, TrustStore, check_chain, hash, hash_name};
use crate::{Cert, oids};
use cms::content_info::ContentInfo;
use cms::signed_data::{SignedData, SignerIdentifier, SignerInfo};
use der::asn1::OctetString;
use der::{Decode, Encode};
use pdfr_core::digital::DigitalSig;

/// Overall verdict, for the badge and the summary line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Intact, signer trusted, no disallowed changes.
    Valid,
    /// Intact but the signer's certificate doesn't lead to a trusted root
    /// (or has a problem such as having expired at signing time).
    Untrusted,
    /// The signed part is intact but the file was changed afterwards in a
    /// way other than form fill, annotations or further signatures.
    Modified,
    /// The signature doesn't match the document, or can't be read.
    Broken,
}

/// What happened to the file after this signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum After {
    /// Nothing: the signature covers the whole file, or only validation
    /// data (DSS, document timestamps) was added after it.
    Nothing,
    /// Changes were added; `allowed` if they are only form fill,
    /// annotations and signatures. `what` describes them.
    Changes { allowed: bool, what: Vec<String> },
}

#[derive(Clone, Debug)]
pub struct Report {
    pub field: String,
    pub page: Option<usize>,
    pub status: Status,
    /// The signer's name (certificate CN, else the PDF's `/Name`).
    pub signer: String,
    pub cert: Option<Cert>,
    /// Issuer of the signer's certificate.
    pub issuer: Option<String>,
    /// Time the signer's computer claimed (Unix seconds).
    pub claimed_time: Option<u64>,
    /// Time from a valid timestamp (Unix seconds).
    pub timestamp: Option<u64>,
    pub timestamp_problem: Option<String>,
    pub anchor: Option<Anchor>,
    /// Why it is broken or untrusted, if it is.
    pub problem: Option<String>,
    pub after: After,
    /// "PAdES B-LTA", "PAdES B-LT", "PAdES B-T", "PAdES B-B", "PKCS#7"
    /// or "Document timestamp".
    pub kind: String,
    pub digest: String,
    pub reason: Option<String>,
    pub location: Option<String>,
    /// Whether the signer's certificate was revoked (`None`: not got that
    /// far, e.g. the signature is broken).
    pub revocation: Option<Revocation>,
    /// A document timestamp rather than a person's signature.
    pub doc_timestamp: bool,
}

/// The document timestamp sub-filter.
pub const DOC_TIMESTAMP: &str = "ETSI.RFC3161";

/// Size limit for a signature's CMS (real ones are well below).
const MAX_CMS: usize = 1 << 20;

/// Checks every signature of the file `bytes`, given what core read,
/// using only revocation data in the file.
pub fn verify_all(bytes: &[u8], sigs: &[DigitalSig], store: &TrustStore) -> Vec<Report> {
    verify_all_with(bytes, sigs, store, None)
}

/// Like `verify_all`, asking certificate issuers about revocation when the
/// file has no answer and `online` is given.
pub fn verify_all_with(
    bytes: &[u8],
    sigs: &[DigitalSig],
    store: &TrustStore,
    online: Option<Online>,
) -> Vec<Report> {
    let dss = dss_of(bytes);
    let ctx = Ctx {
        dss: &dss,
        online,
        now: crate::now(),
    };
    let mut reports: Vec<Report> = sigs
        .iter()
        .map(|s| verify_one_in(bytes, s, sigs, store, &ctx))
        .collect();
    // B-LTA: a valid document timestamp over the validation data.
    for (i, s) in sigs.iter().enumerate() {
        let covered = sigs.iter().zip(&reports).any(|(o, r)| {
            r.doc_timestamp && r.status == Status::Valid && o.covered_end() > s.covered_end()
        });
        if covered && reports[i].kind == "PAdES B-LT" {
            reports[i].kind = "PAdES B-LTA".into();
        }
    }
    reports
}

/// What all signatures of a file share while checking.
struct Ctx<'a> {
    /// Validation data in the file's DSS.
    dss: &'a Evidence,
    online: Option<Online<'a>>,
    now: u64,
}

fn dss_of(bytes: &[u8]) -> Evidence {
    lopdf::Document::load_mem(bytes)
        .map(|d| crate::revocation::from_dss(&d))
        .unwrap_or_default()
}

pub fn verify_one(bytes: &[u8], s: &DigitalSig, all: &[DigitalSig], store: &TrustStore) -> Report {
    let dss = dss_of(bytes);
    let ctx = Ctx {
        dss: &dss,
        online: None,
        now: crate::now(),
    };
    verify_one_in(bytes, s, all, store, &ctx)
}

fn verify_one_in(
    bytes: &[u8],
    s: &DigitalSig,
    all: &[DigitalSig],
    store: &TrustStore,
    ctx: &Ctx,
) -> Report {
    let mut r = Report {
        field: s.field.clone(),
        page: s.page,
        status: Status::Broken,
        signer: s.name.clone().unwrap_or_else(|| "Unknown signer".into()),
        cert: None,
        issuer: None,
        claimed_time: s.time.as_deref().and_then(parse_pdf_date),
        timestamp: None,
        timestamp_problem: None,
        anchor: None,
        problem: None,
        after: After::Nothing,
        kind: if s.sub_filter == "ETSI.CAdES.detached" {
            "PAdES B-B".into()
        } else {
            "PKCS#7".into()
        },
        digest: String::new(),
        reason: s.reason.clone(),
        location: s.location.clone(),
        revocation: None,
        doc_timestamp: s.sub_filter == DOC_TIMESTAMP,
    };
    let res = if r.doc_timestamp {
        r.kind = "Document timestamp".into();
        r.signer = "Document timestamp".into();
        check_doc_timestamp(bytes, s, all, store, &mut r)
    } else {
        check(bytes, s, all, store, ctx, &mut r)
    };
    match res {
        Ok(()) => {}
        Err(e) => {
            r.status = Status::Broken;
            r.problem = Some(e);
        }
    }
    r
}

/// The signed bytes of `s` and its CMS (DER, padding removed).
fn signed_part<'a>(bytes: &[u8], s: &'a DigitalSig) -> Result<(Vec<u8>, &'a [u8]), String> {
    let signed = s
        .signed_bytes(bytes)
        .ok_or("the signed part of the file doesn't match the file")?;
    // The hole must be exactly the /Contents string.
    let (_, l1) = s.byte_range[0];
    let (o2, _) = s.byte_range[1];
    if bytes.get(l1) != Some(&b'<') || bytes.get(o2 - 1) != Some(&b'>') {
        return Err("the signature doesn't cover the document properly".into());
    }
    let der = der_prefix(&s.contents).ok_or("the signature can't be read")?;
    if der.len() > MAX_CMS {
        return Err("the signature is too large".into());
    }
    Ok((signed, der))
}

/// The SignedData of a CMS signature.
pub(crate) fn signed_data(der: &[u8]) -> Result<SignedData, String> {
    let ci = ContentInfo::from_der(der).map_err(|_| "the signature can't be read")?;
    if ci.content_type != oids::SIGNED_DATA {
        return Err("the signature can't be read".into());
    }
    ci.content
        .decode_as()
        .map_err(|_| "the signature can't be read".into())
}

fn check(
    bytes: &[u8],
    s: &DigitalSig,
    all: &[DigitalSig],
    store: &TrustStore,
    ctx: &Ctx,
    r: &mut Report,
) -> Result<(), String> {
    if !matches!(
        s.sub_filter.as_str(),
        "adbe.pkcs7.detached" | "ETSI.CAdES.detached"
    ) {
        return Err(format!(
            "this kind of signature ({}) can't be checked",
            s.sub_filter
        ));
    }
    let (signed, der) = signed_part(bytes, s)?;
    let sd = signed_data(der)?;
    let mut pool = certs_of(&sd);
    let si = sd
        .signer_infos
        .0
        .iter()
        .next()
        .ok_or("the signature has no signer")?;
    let cert = find_signer(si, &pool).ok_or("the signer's certificate is missing")?;
    r.signer = cert.name();
    r.issuer = Some(cert.issuer_name());
    r.cert = Some(cert.clone());

    let digest = check_signer_info(si, &cert, &signed)?;
    r.digest = hash_name(digest).into();

    if let Some(t) = signing_time_attr(si) {
        r.claimed_time.get_or_insert(t);
    }

    // Timestamp over the signature value.
    if let Some(token) = timestamp_token(si) {
        match check_timestamp(&token, si.signature.as_bytes(), store) {
            Ok((t, _, Some(_))) => {
                r.timestamp = Some(t);
                if s.sub_filter == "ETSI.CAdES.detached" {
                    r.kind = "PAdES B-T".into();
                }
            }
            Ok((_, tsa, None)) => {
                r.timestamp_problem = Some(format!(
                    "the timestamp authority \"{}\" is not trusted",
                    tsa.name()
                ))
            }
            Err(e) => r.timestamp_problem = Some(e),
        }
    }

    // Changes after this signature.
    r.after = changes_after(bytes, s, all);

    // Validation data in the file: the DSS and inside the signature.
    let mut ev = ctx.dss.clone();
    ev.merge(&crate::revocation::from_cms(&sd, si));
    for c in ev.cert_pool() {
        if !pool.contains(&c) {
            pool.push(c);
        }
    }

    // Trust, at the timestamp's time if there is one.
    let at = r.timestamp.or(r.claimed_time).unwrap_or_else(crate::now);
    let chain = check_chain(&cert, &pool, store, at);
    r.anchor = chain.anchor;
    let weak = (digest == oids::SHA1).then(|| "it uses the weak SHA-1 algorithm".to_string());
    r.status = match &r.after {
        After::Changes { allowed: false, .. } => Status::Modified,
        _ if chain.anchor.is_some() && chain.problem.is_none() && weak.is_none() => Status::Valid,
        _ => Status::Untrusted,
    };
    r.problem = match r.status {
        Status::Modified => Some("the document was changed after it was signed".into()),
        Status::Untrusted => Some(chain.problem.or(weak).unwrap_or_else(|| {
            format!(
                "the signer's certificate authority (\"{}\") is not one this computer trusts",
                chain
                    .chain
                    .last()
                    .map_or_else(|| cert.issuer_name(), |c| c.name())
            )
        })),
        _ => None,
    };

    let (rev, used) = crate::revocation::check_path(&cert, &chain.chain, &ev, ctx.online, ctx.now);
    apply_revocation(r, &rev);
    let from_dss = used.has_revocation_data()
        && used.ocsps.iter().all(|o| ctx.dss.ocsps.contains(o))
        && used.crls.iter().all(|c| ctx.dss.crls.contains(c));
    if r.kind == "PAdES B-T" && from_dss {
        r.kind = "PAdES B-LT".into();
    }
    r.revocation = Some(rev);
    Ok(())
}

/// A revoked certificate makes the signature not valid, unless a trusted
/// timestamp proves it was made before the revocation.
fn apply_revocation(r: &mut Report, rev: &Revocation) {
    let State::Revoked { at, reason } = &rev.state else {
        return;
    };
    let whose = match &rev.ca {
        Some(ca) => format!("the certificate of \"{ca}\", which issued the signer's,"),
        None => "the signer's certificate".into(),
    };
    let why = reason
        .as_ref()
        .map(|x| format!(" ({x})"))
        .unwrap_or_default();
    let when = pdfr_core::digital::display_date(*at);
    match (r.timestamp, r.claimed_time) {
        (Some(ts), _) if *at > ts => {}
        (None, Some(t)) if *at > t => {
            if r.status == Status::Valid {
                r.status = Status::Untrusted;
            }
            r.problem.get_or_insert(format!(
                "{whose} was revoked on {when}{why}, and without a trusted timestamp nothing proves it was signed before that"
            ));
        }
        _ => {
            r.status = Status::Broken;
            r.problem = Some(format!(
                "{whose} was revoked on {when}{why}, before it was signed"
            ));
        }
    }
}

/// A document timestamp (`ETSI.RFC3161`): an RFC 3161 token over the
/// file up to it.
fn check_doc_timestamp(
    bytes: &[u8],
    s: &DigitalSig,
    all: &[DigitalSig],
    store: &TrustStore,
    r: &mut Report,
) -> Result<(), String> {
    let (signed, der) = signed_part(bytes, s)?;
    let info = crate::tsa::tst_info(der).map_err(|_| "the timestamp can't be read")?;
    r.digest = hash_name(info.message_imprint.hash_algorithm.oid).into();
    let (t, tsa, anchor) = check_timestamp(der, &signed, store)
        .map_err(|_| "the document was changed: it no longer matches its timestamp")?;
    r.signer = tsa.name();
    r.issuer = Some(tsa.issuer_name());
    r.cert = Some(tsa.clone());
    r.timestamp = Some(t);
    r.anchor = anchor;
    r.after = changes_after(bytes, s, all);
    r.status = match (&r.after, anchor) {
        (After::Changes { allowed: false, .. }, _) => Status::Modified,
        (_, Some(_)) => Status::Valid,
        _ => Status::Untrusted,
    };
    r.problem = match r.status {
        Status::Modified => Some("the document was changed after it was timestamped".into()),
        Status::Untrusted => Some(format!(
            "the timestamp authority \"{}\" is not one this computer trusts",
            tsa.name()
        )),
        _ => None,
    };
    Ok(())
}

/// Checks `si` against `content`: the message digest, then the signature
/// with `cert`. Returns the digest algorithm.
fn check_signer_info(
    si: &SignerInfo,
    cert: &Cert,
    content: &[u8],
) -> Result<const_oid::ObjectIdentifier, String> {
    let digest_oid = si.digest_alg.oid;
    let h = hash(digest_oid, content).ok_or("the signature uses an unknown digest algorithm")?;
    let spki = &cert.x.tbs_certificate.subject_public_key_info;
    let params = si
        .signature_algorithm
        .parameters
        .as_ref()
        .and_then(|p| p.to_der().ok());
    match &si.signed_attrs {
        Some(attrs) => {
            let md = attrs
                .iter()
                .find(|a| a.oid == oids::MESSAGE_DIGEST)
                .and_then(|a| a.values.iter().next())
                .and_then(|v| v.decode_as::<OctetString>().ok())
                .ok_or("the signature has no message digest")?;
            if md.as_bytes() != h.as_slice() {
                return Err("the document was changed: it no longer matches its signature".into());
            }
            let msg = attrs.to_der().map_err(|e| e.to_string())?;
            crate::trust::verify(
                spki,
                si.signature_algorithm.oid,
                params.as_deref(),
                Some(digest_oid),
                &msg,
                si.signature.as_bytes(),
            )
            .map_err(|_| "the signature does not match the signer's certificate".to_string())?;
        }
        None => {
            crate::trust::verify(
                spki,
                si.signature_algorithm.oid,
                params.as_deref(),
                Some(digest_oid),
                content,
                si.signature.as_bytes(),
            )
            .map_err(|_| {
                "the document was changed: it no longer matches its signature".to_string()
            })?;
        }
    }
    Ok(digest_oid)
}

/// Checks an RFC 3161 token over `data`; returns its time, the timestamp
/// authority's certificate and the root it is trusted through, if any.
fn check_timestamp(
    token: &[u8],
    data: &[u8],
    store: &TrustStore,
) -> Result<(u64, Cert, Option<Anchor>), String> {
    let info = crate::tsa::tst_info(token)?;
    let h = hash(info.message_imprint.hash_algorithm.oid, data)
        .ok_or("the timestamp uses an unknown digest")?;
    if info.message_imprint.hashed_message.as_bytes() != h.as_slice() {
        return Err("the timestamp is for another signature".into());
    }
    let ci = ContentInfo::from_der(token).map_err(|e| e.to_string())?;
    let sd: SignedData = ci.content.decode_as().map_err(|e| e.to_string())?;
    let content = sd
        .encap_content_info
        .econtent
        .as_ref()
        .ok_or("empty timestamp")?
        .decode_as::<OctetString>()
        .map_err(|e| e.to_string())?;
    let pool = certs_of(&sd);
    let si = sd
        .signer_infos
        .0
        .iter()
        .next()
        .ok_or("unsigned timestamp")?;
    let tsa = find_signer(si, &pool).ok_or("the timestamp authority's certificate is missing")?;
    check_signer_info(si, &tsa, content.as_bytes())
        .map_err(|_| "the timestamp's own signature does not match".to_string())?;
    let t = info.gen_time.to_unix_duration().as_secs();
    let chain = check_chain(&tsa, &pool, store, t);
    Ok((t, tsa, chain.anchor))
}

pub(crate) fn certs_of(sd: &SignedData) -> Vec<Cert> {
    sd.certificates
        .as_ref()
        .map(|set| {
            set.0
                .iter()
                .filter_map(|c| match c {
                    cms::cert::CertificateChoices::Certificate(x) => {
                        let der = x.to_der().ok()?;
                        Some(Cert { der, x: x.clone() })
                    }
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn find_signer(si: &SignerInfo, pool: &[Cert]) -> Option<Cert> {
    match &si.sid {
        SignerIdentifier::IssuerAndSerialNumber(is) => pool
            .iter()
            .find(|c| {
                c.x.tbs_certificate.issuer == is.issuer
                    && c.x.tbs_certificate.serial_number == is.serial_number
            })
            .cloned(),
        SignerIdentifier::SubjectKeyIdentifier(ski) => pool
            .iter()
            .find(|c| {
                c.x.tbs_certificate
                    .extensions
                    .as_ref()
                    .and_then(|ex| {
                        ex.iter().find(|e| {
                            e.extn_id == const_oid::ObjectIdentifier::new_unwrap("2.5.29.14")
                        })
                    })
                    .and_then(|e| OctetString::from_der(e.extn_value.as_bytes()).ok())
                    .is_some_and(|k| k.as_bytes() == ski.0.as_bytes())
            })
            .cloned(),
    }
}

pub(crate) fn timestamp_token(si: &SignerInfo) -> Option<Vec<u8>> {
    si.unsigned_attrs
        .as_ref()?
        .iter()
        .find(|a| a.oid == oids::TIMESTAMP_TOKEN)?
        .values
        .iter()
        .next()?
        .to_der()
        .ok()
}

fn signing_time_attr(si: &SignerInfo) -> Option<u64> {
    let v = si
        .signed_attrs
        .as_ref()?
        .iter()
        .find(|a| a.oid == oids::SIGNING_TIME)?
        .values
        .iter()
        .next()?
        .clone();
    if let Ok(t) = v.decode_as::<der::asn1::UtcTime>() {
        return Some(t.to_unix_duration().as_secs());
    }
    v.decode_as::<der::asn1::GeneralizedTime>()
        .ok()
        .map(|t| t.to_unix_duration().as_secs())
}

/// The first DER element of `b` (`/Contents` is padded with zeros).
pub(crate) fn der_prefix(b: &[u8]) -> Option<&[u8]> {
    if b.len() < 2 || b[0] != 0x30 {
        return None;
    }
    let (len, hdr) = match b[1] {
        n if n < 0x80 => (n as usize, 2),
        0x80 => return None,
        n => {
            let k = (n & 0x7f) as usize;
            if k > 4 || b.len() < 2 + k {
                return None;
            }
            let len = b[2..2 + k]
                .iter()
                .fold(0usize, |a, x| (a << 8) | *x as usize);
            (len, 2 + k)
        }
    };
    b.get(..hdr.checked_add(len)?)
}

/// `D:YYYYMMDDHHmmSS+HH'mm'` (later parts optional) as Unix seconds.
pub fn parse_pdf_date(s: &str) -> Option<u64> {
    let s = s.trim().strip_prefix("D:").unwrap_or(s.trim());
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.len() < 4 {
        return None;
    }
    let part = |a: usize, b: usize, def: i64| -> i64 {
        digits.get(a..b).and_then(|x| x.parse().ok()).unwrap_or(def)
    };
    let (y, mo, d) = (part(0, 4, 1970), part(4, 6, 1), part(6, 8, 1));
    let (h, mi, sec) = (part(8, 10, 0), part(10, 12, 0), part(12, 14, 0));
    let rest = &s[digits.len()..];
    let off = match rest.chars().next() {
        Some(c @ ('+' | '-')) => {
            let t: String = rest[1..].chars().filter(|c| c.is_ascii_digit()).collect();
            let oh: i64 = t.get(0..2).and_then(|x| x.parse().ok()).unwrap_or(0);
            let om: i64 = t.get(2..4).and_then(|x| x.parse().ok()).unwrap_or(0);
            let sign = if c == '-' { -1 } else { 1 };
            sign * (oh * 3600 + om * 60)
        }
        _ => 0,
    };
    // Days from civil (Howard Hinnant).
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let t = days * 86_400 + h * 3600 + mi * 60 + sec - off;
    u64::try_from(t).ok()
}

// --- Changes after signing ---------------------------------------------------------

fn changes_after(bytes: &[u8], s: &DigitalSig, all: &[DigitalSig]) -> After {
    let end = s.covered_end();
    if bytes[end.min(bytes.len())..]
        .iter()
        .all(|b| b.is_ascii_whitespace() || *b == 0)
    {
        return After::Nothing;
    }
    let mut what = Vec::new();
    if all
        .iter()
        .any(|o| o.covered_end() > end && o.sub_filter != DOC_TIMESTAMP)
    {
        what.push("signed again".to_string());
    }
    let (Ok(rev), Ok(cur)) = (
        lopdf::Document::load_mem(&bytes[..end]),
        lopdf::Document::load_mem(bytes),
    ) else {
        return After::Changes {
            allowed: false,
            what: vec!["changes that could not be read".into()],
        };
    };
    let kinds = classify(&rev, &cur);
    if what.is_empty() && !kinds.is_empty() && kinds.iter().all(|k| *k == Kind::Validation) {
        return After::Nothing;
    }
    let allowed = !kinds.contains(&Kind::Other);
    for k in kinds {
        let w = match k {
            Kind::Form => "form fields filled in",
            Kind::Annot => "comments or annotations added",
            Kind::Sig | Kind::Validation => continue,
            Kind::Other => "the pages or document changed",
        };
        if !what.iter().any(|x| x == w) {
            what.push(w.into());
        }
    }
    if what.is_empty() {
        what.push("saved again without visible changes".into());
    }
    After::Changes { allowed, what }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Form,
    Annot,
    Sig,
    /// Validation data (DSS) and document timestamps.
    Validation,
    Other,
}

/// What kinds of objects changed between the signed revision and now.
fn classify(rev: &lopdf::Document, cur: &lopdf::Document) -> std::collections::BTreeSet<Kind> {
    use lopdf::Object;
    let mut out = std::collections::BTreeSet::new();
    let catalog_id = cur.trailer.get(b"Root").and_then(|o| o.as_reference()).ok();
    let acroform_id = cur
        .catalog()
        .ok()
        .and_then(|c| c.get(b"AcroForm").ok())
        .and_then(|o| o.as_reference().ok());
    let pages: std::collections::HashSet<_> = cur.get_pages().values().copied().collect();
    let content_streams: std::collections::HashSet<lopdf::ObjectId> = pages
        .iter()
        .filter_map(|p| cur.get_dictionary(*p).ok())
        .filter_map(|p| p.get(b"Contents").ok())
        .flat_map(|c| match c {
            Object::Reference(r) => vec![*r],
            Object::Array(a) => a.iter().filter_map(|o| o.as_reference().ok()).collect(),
            _ => vec![],
        })
        .collect();
    let validation = validation_objects(cur);
    // Pages' /Annots arrays kept as objects of their own.
    let annot_arrays: std::collections::HashSet<lopdf::ObjectId> = pages
        .iter()
        .filter_map(|p| cur.get_dictionary(*p).ok())
        .filter_map(|p| p.get(b"Annots").ok()?.as_reference().ok())
        .collect();
    for (id, o) in &cur.objects {
        let old = rev.objects.get(id);
        if old == Some(o) {
            continue;
        }
        if validation.contains(id) {
            out.insert(Kind::Validation);
            continue;
        }
        let dict = match o {
            Object::Dictionary(d) => Some(d),
            Object::Stream(s) => Some(&s.dict),
            _ => None,
        };
        let name = |k: &[u8]| {
            dict.and_then(|d| d.get(k).ok())
                .and_then(|v| v.as_name().ok())
                .map(<[u8]>::to_vec)
        };
        let kind = if Some(*id) == acroform_id {
            if only_timestamp_fields_added(cur, old, o, &validation) {
                Kind::Validation
            } else {
                Kind::Form
            }
        } else if Some(*id) == catalog_id {
            // Only the form, signature permissions or validation data.
            let changed_only = |allowed: &[&[u8]]| {
                let (Some(Object::Dictionary(a)), Object::Dictionary(b)) = (old, o) else {
                    return false;
                };
                let keys: std::collections::BTreeSet<&[u8]> = a
                    .iter()
                    .chain(b.iter())
                    .map(|(k, _)| k.as_slice())
                    .collect();
                keys.into_iter()
                    .filter(|k| a.get(k).ok() != b.get(k).ok())
                    .all(|k| allowed.contains(&k))
            };
            if changed_only(&[b"DSS", b"Extensions"]) {
                Kind::Validation
            } else if changed_only(&[b"AcroForm", b"DSS", b"Perms", b"Extensions"]) {
                Kind::Form
            } else {
                Kind::Other
            }
        } else if pages.contains(id) {
            let only_annots = match (old, o) {
                (Some(Object::Dictionary(a)), Object::Dictionary(b)) => {
                    let mut a = a.clone();
                    let mut b = b.clone();
                    a.remove(b"Annots");
                    b.remove(b"Annots");
                    a == b
                }
                _ => false,
            };
            if only_annots {
                Kind::Annot
            } else {
                Kind::Other
            }
        } else if annot_arrays.contains(id) {
            Kind::Annot
        } else if content_streams.contains(id) || name(b"Type").as_deref() == Some(b"Pages") {
            Kind::Other
        } else if name(b"Type").as_deref() == Some(b"Sig") || name(b"FT").as_deref() == Some(b"Sig")
        {
            Kind::Sig
        } else if name(b"Subtype").as_deref() == Some(b"Widget")
            || dict.is_some_and(|d| d.has(b"FT") || d.has(b"T") && d.has(b"Kids"))
        {
            Kind::Form
        } else if name(b"Type").as_deref() == Some(b"Annot") {
            Kind::Annot
        } else if old.is_none() || name(b"Subtype").as_deref() == Some(b"Form") {
            // New objects (appearances, fonts) and appearance streams.
            Kind::Form
        } else if matches!(name(b"Type").as_deref(), Some(b"XRef" | b"ObjStm")) {
            continue;
        } else if old.is_some() && name(b"Type").as_deref() == Some(b"Font")
            || name(b"Type").as_deref() == Some(b"FontDescriptor")
        {
            Kind::Form
        } else {
            Kind::Other
        };
        out.insert(kind);
    }
    out
}

/// Objects of the DSS (and everything under it) and of document
/// timestamps (their signature dictionaries and fields).
fn validation_objects(cur: &lopdf::Document) -> std::collections::HashSet<lopdf::ObjectId> {
    use lopdf::Object;
    let mut out = std::collections::HashSet::new();
    let mut stack: Vec<(Object, usize)> = cur
        .catalog()
        .ok()
        .and_then(|c| c.get(b"DSS").ok())
        .map(|o| vec![(o.clone(), 0)])
        .unwrap_or_default();
    while let Some((o, depth)) = stack.pop() {
        if depth > 4 {
            continue;
        }
        match o {
            Object::Reference(id) => {
                if out.insert(id)
                    && let Ok(x) = cur.get_object(id)
                {
                    stack.push((x.clone(), depth + 1));
                }
            }
            Object::Array(a) => stack.extend(a.into_iter().map(|x| (x, depth + 1))),
            Object::Dictionary(d) => {
                stack.extend(d.into_iter().map(|(_, x)| (x, depth + 1)));
            }
            _ => {}
        }
    }
    let is_dts = |o: &Object| {
        o.as_dict()
            .ok()
            .and_then(|d| d.get(b"Type").ok())
            .and_then(|t| t.as_name().ok())
            == Some(b"DocTimeStamp")
    };
    for (id, o) in &cur.objects {
        let Ok(d) = o.as_dict() else { continue };
        let v = d.get(b"V").ok().and_then(|v| match v {
            Object::Reference(r) => cur.get_object(*r).ok(),
            v => Some(v),
        });
        if is_dts(o) || v.is_some_and(is_dts) {
            out.insert(*id);
        }
    }
    out
}

/// The form dictionary changed only by adding document timestamp fields.
fn only_timestamp_fields_added(
    cur: &lopdf::Document,
    old: Option<&lopdf::Object>,
    new: &lopdf::Object,
    validation: &std::collections::HashSet<lopdf::ObjectId>,
) -> bool {
    use lopdf::Object;
    let (Some(Object::Dictionary(a)), Object::Dictionary(b)) = (old, new) else {
        return false;
    };
    let fields = |d: &lopdf::Dictionary| -> Vec<Object> {
        match d.get(b"Fields") {
            Ok(Object::Array(x)) => x.clone(),
            Ok(Object::Reference(r)) => cur
                .get_object(*r)
                .ok()
                .and_then(|o| o.as_array().ok().cloned())
                .unwrap_or_default(),
            _ => vec![],
        }
    };
    let (fa, fb) = (fields(a), fields(b));
    let mut a2 = a.clone();
    let mut b2 = b.clone();
    a2.remove(b"Fields");
    b2.remove(b"Fields");
    a2 == b2
        && fb.starts_with(&fa)
        && fb[fa.len()..]
            .iter()
            .all(|o| o.as_reference().is_ok_and(|id| validation.contains(&id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdf_dates() {
        assert_eq!(parse_pdf_date("D:19700101000000Z"), Some(0));
        assert_eq!(parse_pdf_date("D:20261120134510Z"), Some(1_795_182_310));
        assert_eq!(
            parse_pdf_date("D:20261120144510+01'00'"),
            Some(1_795_182_310)
        );
        assert_eq!(parse_pdf_date("D:2026"), Some(1_767_225_600));
        assert_eq!(parse_pdf_date("nope"), None);
    }

    #[test]
    fn der_prefix_trims_padding() {
        let mut b = vec![0x30, 0x03, 1, 2, 3];
        b.extend([0u8; 10]);
        assert_eq!(der_prefix(&b), Some(&b[..5]));
        assert_eq!(der_prefix(&[0x30, 0x82, 0x01]), None);
        assert_eq!(der_prefix(&[0u8; 8]), None);
    }
}
