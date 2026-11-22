//! Checking the digital signatures in a PDF.
//!
//! For each signature: do the signed bytes still hash to what was signed,
//! does the signature match the signer's certificate, is that certificate
//! meant for signing documents and does it lead to a trusted root through
//! real certificate authorities, is there a valid timestamp, and was the
//! file changed after signing (and if so, only in allowed ways: form
//! fill, annotations, further signatures, validation data, as far as the
//! author's certification and field locks allow), and was the signer's
//! certificate revoked (see `revocation`). Document timestamps
//! (`ETSI.RFC3161`) are checked too, and give the PAdES level B-LTA.
//!
//! What the signer wrote in the signature dictionary (reason, place,
//! time) is read from the revision they signed, not from later updates.

use crate::revocation::{Evidence, KEY_COMPROMISE, Online, Revocation, State};
use crate::trust::{Anchor, TrustStore, check_chain, hash, hash_name};
use crate::{Cert, oids};
use cms::content_info::ContentInfo;
use cms::signed_data::{SignedData, SignerIdentifier, SignerInfo};
use der::asn1::OctetString;
use der::{Decode, Encode};
use pdfr_core::digital::{DigitalSig, SignedDict};

/// Overall verdict, for the badge and the summary line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Intact, signer trusted, no disallowed changes, not revoked.
    Valid,
    /// Like `Valid`, but whether the signer's certificate was revoked
    /// could not be checked (no answer in the file, online checks off or
    /// the issuer unreachable).
    ValidUnchecked,
    /// Intact but the signer's certificate doesn't lead to a trusted root
    /// (or has a problem such as having expired at signing time).
    Untrusted,
    /// The signed part is intact but the file was changed afterwards in a
    /// way other than form fill, annotations or further signatures.
    Modified,
    /// The signature doesn't match the document, or can't be read.
    Broken,
}

impl Status {
    /// Valid, whether or not revocation could be checked.
    pub fn is_valid(self) -> bool {
        matches!(self, Status::Valid | Status::ValidUnchecked)
    }
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
    /// Why the signer's root is trusted: the EU Trusted List, or the
    /// computer's own trust store.
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

const WEAK: &str = "it uses the weak SHA-1 algorithm";

/// Checks every signature of the file `bytes`, given what core read,
/// using only revocation data in the file.
pub fn verify_all(bytes: &[u8], sigs: &[DigitalSig], store: &TrustStore) -> Vec<Report> {
    verify_all_with(bytes, sigs, store, None)
}

/// Like `verify_all`, asking certificate issuers about revocation when the
/// file has no answer and `online` is given. Issuers are asked only about
/// certificates that lead to a trusted root.
pub fn verify_all_with(
    bytes: &[u8],
    sigs: &[DigitalSig],
    store: &TrustStore,
    online: Option<Online>,
) -> Vec<Report> {
    let ctx = Ctx::new(bytes, online);
    let mut reports: Vec<Report> = sigs
        .iter()
        .map(|s| verify_one_in(bytes, s, sigs, store, &ctx))
        .collect();
    // B-LTA: a valid document timestamp over the validation data.
    for (i, s) in sigs.iter().enumerate() {
        let covered = sigs.iter().zip(&reports).any(|(o, r)| {
            r.doc_timestamp && r.status.is_valid() && o.covered_end() > s.covered_end()
        });
        if covered && reports[i].kind == "PAdES B-LT" {
            reports[i].kind = "PAdES B-LTA".into();
        }
    }
    reports
}

/// What all signatures of a file share while checking.
struct Ctx<'a> {
    /// The whole file, read once.
    cur: Option<lopdf::Document>,
    /// Validation data in the file's DSS.
    dss: Evidence,
    online: Option<Online<'a>>,
    now: u64,
}

impl<'a> Ctx<'a> {
    fn new(bytes: &[u8], online: Option<Online<'a>>) -> Ctx<'a> {
        let cur = lopdf::Document::load_mem(bytes).ok();
        let dss = cur
            .as_ref()
            .map(crate::revocation::from_dss)
            .unwrap_or_default();
        Ctx {
            cur,
            dss,
            online,
            now: crate::now(),
        }
    }
}

pub fn verify_one(bytes: &[u8], s: &DigitalSig, all: &[DigitalSig], store: &TrustStore) -> Report {
    verify_one_in(bytes, s, all, store, &Ctx::new(bytes, None))
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
        claimed_time: None,
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
        reason: None,
        location: None,
        revocation: None,
        doc_timestamp: s.sub_filter == DOC_TIMESTAMP,
    };
    let res = if r.doc_timestamp {
        r.kind = "Document timestamp".into();
        r.signer = "Document timestamp".into();
        check_doc_timestamp(bytes, s, all, store, ctx, &mut r)
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

/// A signature's signed part, as found in the file.
struct Signed {
    /// The bytes the signature covers.
    bytes: Vec<u8>,
    /// The CMS (DER, padding removed).
    der: Vec<u8>,
    /// The revision the signature covers, read on its own.
    rev: lopdf::Document,
    /// The signature dictionary in that revision.
    dict: SignedDict,
}

/// The signed bytes of `s`, its CMS and the revision it signed. The hole
/// left by `/ByteRange` must be exactly the `/Contents` string of the
/// signature dictionary in that revision: the CMS in hex, then zeros.
fn signed_part(bytes: &[u8], s: &DigitalSig) -> Result<Signed, String> {
    const ODD: &str = "the signature doesn't cover the document properly";
    let signed = s
        .signed_bytes(bytes)
        .ok_or("the signed part of the file doesn't match the file")?;
    let (_, l1) = s.byte_range[0];
    let (o2, _) = s.byte_range[1];
    if o2 < l1 + 2 || bytes.get(l1) != Some(&b'<') || bytes.get(o2 - 1) != Some(&b'>') {
        return Err(ODD.into());
    }
    let hole = hex_decode(&bytes[l1 + 1..o2 - 1]).ok_or(ODD)?;
    let der = der_prefix(&hole).ok_or("the signature can't be read")?;
    if der.len() > MAX_CMS {
        return Err("the signature is too large".into());
    }
    if hole[der.len()..].iter().any(|b| *b != 0) {
        return Err(ODD.into());
    }
    let rev = lopdf::Document::load_mem(&bytes[..s.covered_end().min(bytes.len())])
        .map_err(|_| "the signed version of the file can't be read")?;
    let dict = pdfr_core::digital::signed_dict(&rev, s).ok_or(ODD)?;
    if dict.contents != hole {
        return Err(ODD.into());
    }
    Ok(Signed {
        bytes: signed,
        der: der.to_vec(),
        rev,
        dict,
    })
}

/// Strict hex (no spaces), as signatures write `/Contents`.
fn hex_decode(h: &[u8]) -> Option<Vec<u8>> {
    if !h.len().is_multiple_of(2) {
        return None;
    }
    let digit = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    h.chunks(2)
        .map(|p| Some(digit(p[0])? << 4 | digit(p[1])?))
        .collect()
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

/// What the signer wrote, from the revision they signed.
fn take_signed_dict(r: &mut Report, d: &SignedDict) {
    if let Some(n) = &d.name {
        r.signer = n.clone();
    }
    r.claimed_time = d.time.as_deref().and_then(parse_pdf_date);
    r.reason = d.reason.clone();
    r.location = d.location.clone();
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
    let part = signed_part(bytes, s)?;
    take_signed_dict(r, &part.dict);
    let sd = signed_data(&part.der)?;
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

    let (digest, sha1) = check_signer_info(si, &cert, &part.bytes)?;
    r.digest = hash_name(digest).into();

    let signing_time = signing_time_attr(si);
    if let Some(t) = signing_time {
        r.claimed_time.get_or_insert(t);
    }

    // Validation data in the file: the DSS and inside the signature.
    let mut ev = ctx.dss.clone();
    ev.merge(&crate::revocation::from_cms(&sd, si));
    for c in ev.cert_pool() {
        if !pool.contains(&c) {
            pool.push(c);
        }
    }

    // Timestamp over the signature value.
    if let Some(token) = timestamp_token(si) {
        match check_timestamp(&token, si.signature.as_bytes(), store, &ev, ctx) {
            Ok(ts) => match ts.trusted {
                Ok(_) => {
                    r.timestamp = Some(ts.time);
                    if s.sub_filter == "ETSI.CAdES.detached" {
                        r.kind = "PAdES B-T".into();
                    }
                }
                Err(p) => r.timestamp_problem = Some(p),
            },
            Err(e) => r.timestamp_problem = Some(e),
        }
    }

    // Changes after this signature.
    r.after = changes_after(bytes, s, all, &part.rev, ctx.cur.as_ref(), part.dict.id);

    // Trust, at the trusted timestamp's time, else at the signing time
    // the signer signed, else now. Never at the PDF's `/M` alone.
    let at = r.timestamp.or(signing_time).unwrap_or(ctx.now);
    let chain = check_chain(&cert, &pool, store, at);
    r.anchor = chain.anchor;
    let purpose = (!cert.can_sign_documents())
        .then(|| "the signer's certificate is not meant for signing documents".to_string());
    let weak = sha1.then(|| WEAK.to_string());
    let trusted = chain.trusted() && purpose.is_none() && weak.is_none();
    r.status = match &r.after {
        After::Changes { allowed: false, .. } => Status::Modified,
        _ if trusted => Status::Valid,
        _ => Status::Untrusted,
    };
    r.problem = match r.status {
        Status::Modified => Some("the document was changed after it was signed".into()),
        Status::Untrusted => Some(chain.problem.clone().or(purpose).or(weak).unwrap_or_else(
            || {
                format!(
                    "the signer's certificate authority (\"{}\") is not one this computer trusts",
                    chain
                        .chain
                        .last()
                        .map_or_else(|| cert.issuer_name(), |c| c.name())
                )
            },
        )),
        _ => None,
    };

    // Revocation: only for a certificate worth asking about. An untrusted
    // chain could name any address to send the question to.
    if !trusted {
        r.revocation = Some(Revocation::unknown(
            "not checked, because the signer's certificate is not trusted",
        ));
        return Ok(());
    }
    let (rev, used) = crate::revocation::check_path_since(
        &cert,
        &chain.chain,
        &ev,
        ctx.online,
        ctx.now,
        r.timestamp,
    );
    apply_revocation(r, &rev);
    let from_dss = used.has_revocation_data()
        && used.ocsps.iter().all(|o| ctx.dss.ocsps.contains(o))
        && used.crls.iter().all(|c| ctx.dss.crls.contains(c));
    if r.kind == "PAdES B-T" && from_dss {
        r.kind = "PAdES B-LT".into();
    }
    if r.status == Status::Valid && matches!(rev.state, State::Unknown(_)) {
        r.status = Status::ValidUnchecked;
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
            if r.status.is_valid() {
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
    ctx: &Ctx,
    r: &mut Report,
) -> Result<(), String> {
    let part = signed_part(bytes, s)?;
    let info = crate::tsa::tst_info(&part.der).map_err(|_| "the timestamp can't be read")?;
    r.digest = hash_name(info.message_imprint.hash_algorithm.oid).into();
    let ts = check_timestamp(&part.der, &part.bytes, store, &ctx.dss, ctx)
        .map_err(|_| "the document was changed: it no longer matches its timestamp")?;
    r.signer = ts.tsa.name();
    r.issuer = Some(ts.tsa.issuer_name());
    r.cert = Some(ts.tsa.clone());
    r.timestamp = Some(ts.time);
    r.anchor = ts.trusted.as_ref().ok().copied();
    r.after = changes_after(bytes, s, all, &part.rev, ctx.cur.as_ref(), part.dict.id);
    r.status = match (&r.after, &ts.trusted) {
        (After::Changes { allowed: false, .. }, _) => Status::Modified,
        (_, Ok(_)) => Status::Valid,
        _ => Status::Untrusted,
    };
    r.problem = match (r.status, ts.trusted) {
        (Status::Modified, _) => Some("the document was changed after it was timestamped".into()),
        (Status::Untrusted, Err(p)) => Some(p),
        _ => None,
    };
    Ok(())
}

/// Checks `si` against `content`: the message digest, then the signature
/// with `cert`. Returns the digest algorithm, and whether SHA-1 was used
/// anywhere (as the digest or inside the signature algorithm).
fn check_signer_info(
    si: &SignerInfo,
    cert: &Cert,
    content: &[u8],
) -> Result<(const_oid::ObjectIdentifier, bool), String> {
    let digest_oid = si.digest_alg.oid;
    let h = hash(digest_oid, content).ok_or("the signature uses an unknown digest algorithm")?;
    let spki = &cert.x.tbs_certificate.subject_public_key_info;
    let params = si
        .signature_algorithm
        .parameters
        .as_ref()
        .and_then(|p| p.to_der().ok());
    let used = match &si.signed_attrs {
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
            .map_err(|_| "the signature does not match the signer's certificate".to_string())?
        }
        None => crate::trust::verify(
            spki,
            si.signature_algorithm.oid,
            params.as_deref(),
            Some(digest_oid),
            content,
            si.signature.as_bytes(),
        )
        .map_err(|_| "the document was changed: it no longer matches its signature".to_string())?,
    };
    Ok((digest_oid, digest_oid == oids::SHA1 || used == oids::SHA1))
}

/// A timestamp token that matches its data.
struct TimestampCheck {
    time: u64,
    /// The timestamp authority's certificate.
    tsa: Cert,
    /// The root it is trusted through, or why it can't be relied on.
    trusted: Result<Anchor, String>,
}

/// Checks an RFC 3161 token over `data`. Errs when it doesn't match
/// `data` or can't be read. It is trusted only if its authority's
/// certificate is for time stamping, leads to a trusted root through real
/// certificate authorities, was not revoked by the time it stamped
/// (from `ev` or, if allowed, by asking), and nothing in it uses SHA-1.
fn check_timestamp(
    token: &[u8],
    data: &[u8],
    store: &TrustStore,
    ev: &Evidence,
    ctx: &Ctx,
) -> Result<TimestampCheck, String> {
    let info = crate::tsa::tst_info(token)?;
    let imprint_alg = info.message_imprint.hash_algorithm.oid;
    let h = hash(imprint_alg, data).ok_or("the timestamp uses an unknown digest")?;
    if info.message_imprint.hashed_message.as_bytes() != h.as_slice() {
        return Err("the timestamp is for another signature".into());
    }
    let sd = signed_data(token)?;
    let content = sd
        .encap_content_info
        .econtent
        .as_ref()
        .ok_or("empty timestamp")?
        .decode_as::<OctetString>()
        .map_err(|e| e.to_string())?;
    let mut pool = certs_of(&sd);
    let si = sd
        .signer_infos
        .0
        .iter()
        .next()
        .ok_or("unsigned timestamp")?;
    let tsa = find_signer(si, &pool).ok_or("the timestamp authority's certificate is missing")?;
    let (_, sha1) = check_signer_info(si, &tsa, content.as_bytes())
        .map_err(|_| "the timestamp's own signature does not match".to_string())?;
    // The signed content-type must say it is a TSTInfo.
    let content_type = si
        .signed_attrs
        .as_ref()
        .and_then(|a| a.iter().find(|x| x.oid == oids::CONTENT_TYPE))
        .and_then(|a| a.values.iter().next())
        .and_then(|v| v.decode_as::<const_oid::ObjectIdentifier>().ok());
    let time = info.gen_time.to_unix_duration().as_secs();
    let mut ev = ev.clone();
    ev.merge(&crate::revocation::from_cms(&sd, si));
    for c in ev.cert_pool() {
        if !pool.contains(&c) {
            pool.push(c);
        }
    }
    let name = tsa.name();
    let trusted = (|| {
        if sha1 || imprint_alg == oids::SHA1 {
            return Err("the timestamp uses the weak SHA-1 algorithm".to_string());
        }
        if content_type != Some(oids::TST_INFO) {
            return Err("the timestamp is not signed as a timestamp".to_string());
        }
        if !tsa.is_timestamp_authority() {
            return Err(format!("\"{name}\" is not a timestamp authority"));
        }
        let chain = check_chain(&tsa, &pool, store, time);
        if let Some(p) = chain.problem {
            return Err(format!("the timestamp authority can't be trusted: {p}"));
        }
        let Some(anchor) = chain.anchor else {
            return Err(format!(
                "the timestamp authority \"{name}\" is not one this computer trusts"
            ));
        };
        let (rev, _) = crate::revocation::check_path_since(
            &tsa,
            &chain.chain,
            &ev,
            ctx.online,
            ctx.now,
            Some(time),
        );
        if let State::Revoked { at, reason } = &rev.state
            && (*at <= time || reason.as_deref() == Some(KEY_COMPROMISE))
        {
            return Err(format!(
                "the timestamp authority \"{name}\" had its certificate revoked"
            ));
        }
        Ok(anchor)
    })();
    Ok(TimestampCheck { time, tsa, trusted })
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

use lopdf::{Document, Object, ObjectId};
use std::collections::{BTreeSet, HashSet};

/// What changed after `s`, given the revision it signed (`rev`, where its
/// signature dictionary is `sig_id`) and the whole file (`cur`).
fn changes_after(
    bytes: &[u8],
    s: &DigitalSig,
    all: &[DigitalSig],
    rev: &Document,
    cur: Option<&Document>,
    sig_id: ObjectId,
) -> After {
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
    let Some(cur) = cur else {
        return After::Changes {
            allowed: false,
            what: vec!["changes that could not be read".into()],
        };
    };
    let found = classify(rev, cur);
    let kinds = &found.kinds;
    if what.is_empty() && !kinds.is_empty() && kinds.iter().all(|k| *k == Kind::Validation) {
        return After::Nothing;
    }
    let mut allowed = !kinds.contains(&Kind::Other);
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
    // The author's certification (DocMDP): 1 allows no changes, 2 form
    // fill and signing, 3 also annotations. Validation data always.
    if let Some(p) = doc_mdp(rev) {
        let permitted: &[Kind] = match p {
            1 => &[Kind::Validation],
            2 => &[Kind::Form, Kind::Sig, Kind::Validation],
            _ => &[Kind::Form, Kind::Annot, Kind::Sig, Kind::Validation],
        };
        if kinds.iter().any(|k| !permitted.contains(k)) {
            allowed = false;
            what.push("changes the author's certification does not allow".into());
        }
    }
    // Fields this signature locked (FieldMDP).
    let locks = field_locks(rev, sig_id);
    if found
        .fields
        .iter()
        .any(|f| locks.iter().any(|l| l.covers(f)))
    {
        allowed = false;
        what.push("fields locked by this signature changed".into());
    }
    if what.is_empty() {
        what.push("saved again without visible changes".into());
    }
    After::Changes { allowed, what }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Form,
    Annot,
    Sig,
    /// Validation data (DSS) and document timestamps.
    Validation,
    Other,
}

/// What changed between the signed revision and now.
struct Found {
    kinds: BTreeSet<Kind>,
    /// Fully qualified names of form fields that existed when signed and
    /// changed since.
    fields: BTreeSet<String>,
}

fn deref<'a>(d: &'a Document, o: &'a Object) -> &'a Object {
    match o {
        Object::Reference(r) => d.get_object(*r).unwrap_or(o),
        o => o,
    }
}

fn dict_of(o: &Object) -> Option<&lopdf::Dictionary> {
    match o {
        Object::Dictionary(d) => Some(d),
        Object::Stream(s) => Some(&s.dict),
        _ => None,
    }
}

fn name_of<'a>(d: &'a lopdf::Dictionary, k: &[u8]) -> Option<&'a [u8]> {
    d.get(k).ok().and_then(|v| v.as_name().ok())
}

/// `key` of the field dictionary `id` or the fields above it.
fn inherited<'a>(d: &'a Document, id: ObjectId, key: &[u8]) -> Option<&'a Object> {
    let mut cur = id;
    for _ in 0..32 {
        let dict = d.get_dictionary(cur).ok()?;
        if let Ok(v) = dict.get(key) {
            return Some(v);
        }
        cur = dict.get(b"Parent").ok()?.as_reference().ok()?;
    }
    None
}

/// A field or widget of a signature field.
fn is_sig_field(d: &Document, id: ObjectId) -> bool {
    inherited(d, id, b"FT").and_then(|o| o.as_name().ok()) == Some(b"Sig")
}

/// A field or widget dictionary.
fn is_field(dict: &lopdf::Dictionary) -> bool {
    name_of(dict, b"Subtype") == Some(b"Widget")
        || dict.has(b"FT")
        || dict.has(b"T") && dict.has(b"Kids")
}

/// The fully qualified name of the field `id` (its `/T` and its parents').
fn field_name(d: &Document, id: ObjectId) -> String {
    let mut parts = Vec::new();
    let mut cur = Some(id);
    for _ in 0..32 {
        let Some(dict) = cur.and_then(|c| d.get_dictionary(c).ok()) else {
            break;
        };
        if let Ok(Object::String(t, _)) = dict.get(b"T") {
            parts.push(text(t));
        }
        cur = dict.get(b"Parent").ok().and_then(|p| p.as_reference().ok());
    }
    parts.reverse();
    parts.join(".")
}

/// A PDF text string (UTF-16 with a byte order mark, else bytes as Latin-1).
fn text(b: &[u8]) -> String {
    match b.strip_prefix(&[0xfe, 0xff]) {
        Some(u) => String::from_utf16_lossy(
            &u.chunks(2)
                .map(|c| u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)]))
                .collect::<Vec<_>>(),
        ),
        None => b.iter().map(|&c| c as char).collect(),
    }
}

/// Objects reachable from `starts`, not following `skip` keys nor going
/// into objects `stop` refuses.
fn reach(
    d: &Document,
    starts: Vec<Object>,
    skip: &[&[u8]],
    stop: impl Fn(ObjectId, &Object) -> bool,
) -> HashSet<ObjectId> {
    let mut seen = HashSet::new();
    let mut stack = starts;
    while let Some(o) = stack.pop() {
        match o {
            Object::Reference(id) => {
                let Ok(x) = d.get_object(id) else { continue };
                if stop(id, x) || !seen.insert(id) {
                    continue;
                }
                stack.push(x.clone());
            }
            Object::Array(a) => stack.extend(a),
            Object::Dictionary(dict) => stack.extend(
                dict.into_iter()
                    .filter(|(k, _)| !skip.contains(&k.as_slice()))
                    .map(|(_, v)| v),
            ),
            Object::Stream(s) => stack.extend(
                s.dict
                    .into_iter()
                    .filter(|(k, _)| !skip.contains(&k.as_slice()))
                    .map(|(_, v)| v),
            ),
            _ => {}
        }
    }
    seen
}

/// What kinds of objects changed between the signed revision and now.
fn classify(rev: &Document, cur: &Document) -> Found {
    let mut kinds = BTreeSet::new();
    let mut fields = BTreeSet::new();
    let catalog_id = cur.trailer.get(b"Root").and_then(|o| o.as_reference()).ok();
    let catalog = cur.catalog().ok();
    let acroform = catalog.and_then(|c| c.get(b"AcroForm").ok()).cloned();
    let acroform_id = acroform.as_ref().and_then(|o| o.as_reference().ok());
    let pages: HashSet<_> = cur.get_pages().values().copied().collect();
    let page_dicts = || pages.iter().filter_map(|p| cur.get_dictionary(*p).ok());
    let content_streams: HashSet<ObjectId> = page_dicts()
        .filter_map(|p| p.get(b"Contents").ok())
        .flat_map(|c| match c {
            Object::Reference(r) => vec![*r],
            Object::Array(a) => a.iter().filter_map(|o| o.as_reference().ok()).collect(),
            _ => vec![],
        })
        .collect();
    // Pages' /Annots arrays kept as objects of their own.
    let annot_arrays: HashSet<ObjectId> = page_dicts()
        .filter_map(|p| p.get(b"Annots").ok()?.as_reference().ok())
        .collect();
    // What the pages draw: contents and resources (fonts, images, form
    // XObjects), not their annotations.
    let page_used = reach(
        cur,
        catalog
            .and_then(|c| c.get(b"Pages").ok())
            .cloned()
            .into_iter()
            .collect(),
        &[b"Annots", b"Parent", b"P", b"B"],
        |_, _| false,
    );
    // The form: fields, their widgets and appearances, the form's own
    // resources; not signature fields, which have rules of their own.
    let form_used = reach(
        cur,
        acroform.clone().into_iter().collect(),
        &[b"Parent", b"P"],
        |id, o| {
            dict_of(o).is_some_and(|d| {
                matches!(name_of(d, b"Type"), Some(b"Page" | b"Pages"))
                    || is_field(d) && is_sig_field(cur, id)
            })
        },
    );
    // Comments: annotations other than widgets, and what they draw.
    let annots: Vec<Object> = page_dicts()
        .filter_map(|p| p.get(b"Annots").ok())
        .filter_map(|a| deref(cur, a).as_array().ok())
        .flatten()
        .filter(|o| {
            o.as_reference()
                .ok()
                .and_then(|id| cur.get_dictionary(id).ok())
                .is_some_and(|d| name_of(d, b"Subtype") != Some(b"Widget"))
        })
        .cloned()
        .collect();
    let annot_used = reach(cur, annots, &[b"Parent", b"P"], |_, o| {
        dict_of(o).is_some_and(|d| matches!(name_of(d, b"Type"), Some(b"Page" | b"Pages")))
    });
    let validation = validation_objects(rev, cur);
    for (id, o) in &cur.objects {
        let old = rev.objects.get(id);
        if old == Some(o) {
            continue;
        }
        let dict = dict_of(o);
        let name = |k: &[u8]| dict.and_then(|d| name_of(d, k));
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
                let keys: BTreeSet<&[u8]> = a
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
            } else if changed_only(&[b"AcroForm", b"DSS", b"Extensions"]) {
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
        } else if content_streams.contains(id)
            || name(b"Type") == Some(b"Pages")
            || page_used.contains(id)
        {
            Kind::Other
        } else if validation.contains(id) {
            Kind::Validation
        } else if matches!(name(b"Type"), Some(b"XRef" | b"ObjStm")) {
            continue;
        } else if name(b"Type") == Some(b"Sig") {
            // A new signature; never one that was there when signing.
            if old.is_none() {
                Kind::Sig
            } else {
                Kind::Other
            }
        } else if dict.is_some_and(is_field) && is_sig_field(cur, *id) {
            // A signature field: new, or empty when signed and signed now.
            if old.is_none() || newly_signed(rev, cur, *id) {
                Kind::Sig
            } else {
                Kind::Other
            }
        } else if dict.is_some_and(is_field) {
            if old.is_some() {
                fields.insert(field_name(cur, *id));
            }
            Kind::Form
        } else if name(b"Type") == Some(b"Annot") {
            Kind::Annot
        } else if form_used.contains(id) {
            Kind::Form
        } else if annot_used.contains(id) {
            Kind::Annot
        } else if old.is_none() {
            // New objects nothing on the pages uses (appearances, fonts).
            Kind::Form
        } else {
            Kind::Other
        };
        kinds.insert(kind);
    }
    // Objects taken out since signing.
    for (id, o) in &rev.objects {
        if !cur.objects.contains_key(id)
            && !matches!(
                dict_of(o).and_then(|d| name_of(d, b"Type")),
                Some(b"XRef" | b"ObjStm")
            )
        {
            kinds.insert(Kind::Other);
        }
    }
    Found { kinds, fields }
}

/// The signature field `id` had no value when signed and now holds a new
/// signature.
fn newly_signed(rev: &Document, cur: &Document, id: ObjectId) -> bool {
    let had = rev.objects.contains_key(&id) && inherited(rev, id, b"V").is_some();
    let now_new = inherited(cur, id, b"V")
        .and_then(|v| v.as_reference().ok())
        .is_some_and(|v| {
            !rev.objects.contains_key(&v)
                && cur
                    .get_dictionary(v)
                    .is_ok_and(|d| matches!(name_of(d, b"Type"), Some(b"Sig" | b"DocTimeStamp")))
        });
    !had && now_new
}

/// The objects that make up a file's DSS: (dictionaries and arrays,
/// streams of certificates, OCSP responses, CRLs and VRI timestamps).
fn dss_objects(d: &Document) -> (HashSet<ObjectId>, HashSet<ObjectId>) {
    let mut structure = HashSet::new();
    let mut streams = HashSet::new();
    let Some(dss) = d.catalog().ok().and_then(|c| c.get(b"DSS").ok()) else {
        return (structure, streams);
    };
    // A dictionary or array, by reference or written in place.
    let open = |o: &Object, structure: &mut HashSet<ObjectId>| -> Option<Object> {
        if let Object::Reference(id) = o {
            structure.insert(*id);
        }
        let x = deref(d, o);
        matches!(x, Object::Dictionary(_) | Object::Array(_)).then(|| x.clone())
    };
    let stream_list =
        |o: &Object, structure: &mut HashSet<ObjectId>, streams: &mut HashSet<ObjectId>| {
            if let Some(Object::Array(a)) = open(o, structure) {
                streams.extend(a.iter().filter_map(|x| x.as_reference().ok()));
            }
        };
    let Some(Object::Dictionary(top)) = open(dss, &mut structure) else {
        return (structure, streams);
    };
    for k in [&b"Certs"[..], b"OCSPs", b"CRLs"] {
        if let Ok(v) = top.get(k) {
            stream_list(v, &mut structure, &mut streams);
        }
    }
    if let Ok(v) = top.get(b"VRI")
        && let Some(Object::Dictionary(vri)) = open(v, &mut structure)
    {
        for (_, entry) in vri.iter() {
            let Some(Object::Dictionary(e)) = open(entry, &mut structure) else {
                continue;
            };
            for k in [&b"Cert"[..], b"OCSP", b"CRL"] {
                if let Ok(v) = e.get(k) {
                    stream_list(v, &mut structure, &mut streams);
                }
            }
            if let Ok(Object::Reference(ts)) = e.get(b"TS") {
                streams.insert(*ts);
            }
        }
    }
    (structure, streams)
}

/// Validation data added since the signed revision: the DSS (its
/// dictionaries and arrays if new or already the DSS's, its streams only
/// if new) and new document timestamps with their fields.
fn validation_objects(rev: &Document, cur: &Document) -> HashSet<ObjectId> {
    let (rev_structure, _) = dss_objects(rev);
    let (structure, streams) = dss_objects(cur);
    let is_new = |id: &ObjectId| !rev.objects.contains_key(id);
    let mut out: HashSet<ObjectId> =
        structure
            .into_iter()
            .filter(|id| is_new(id) || rev_structure.contains(id))
            .chain(streams.into_iter().filter(|id| {
                is_new(id) && cur.get_object(*id).is_ok_and(|o| o.as_stream().is_ok())
            }))
            .collect();
    let is_dts =
        |o: &Object| o.as_dict().ok().and_then(|d| name_of(d, b"Type")) == Some(b"DocTimeStamp");
    for (id, o) in &cur.objects {
        if !is_new(id) {
            continue;
        }
        let Ok(d) = o.as_dict() else { continue };
        let field_of_new_stamp = is_field(d)
            && d.get(b"V")
                .ok()
                .and_then(|v| v.as_reference().ok())
                .is_some_and(|v| is_new(&v) && cur.get_object(v).is_ok_and(is_dts));
        if is_dts(o) || field_of_new_stamp {
            out.insert(*id);
        }
    }
    out
}

/// The form dictionary changed only by adding document timestamp fields.
fn only_timestamp_fields_added(
    cur: &Document,
    old: Option<&Object>,
    new: &Object,
    validation: &HashSet<ObjectId>,
) -> bool {
    let (Some(Object::Dictionary(a)), Object::Dictionary(b)) = (old, new) else {
        return false;
    };
    let fields = |d: &lopdf::Dictionary| -> Vec<Object> {
        match d.get(b"Fields") {
            Ok(o) => deref(cur, o).as_array().cloned().unwrap_or_default(),
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

/// The `/TransformParams` of the signature dictionary's references with
/// transform method `method` (DocMDP, FieldMDP).
fn transform_params<'a>(
    d: &'a Document,
    sig: &'a lopdf::Dictionary,
    method: &[u8],
) -> Vec<&'a lopdf::Dictionary> {
    let Ok(refs) = sig.get(b"Reference").map(|r| deref(d, r)) else {
        return vec![];
    };
    refs.as_array()
        .map(|a| a.as_slice())
        .unwrap_or_default()
        .iter()
        .filter_map(|r| deref(d, r).as_dict().ok())
        .filter(|r| name_of(r, b"TransformMethod") == Some(method))
        .filter_map(|r| deref(d, r.get(b"TransformParams").ok()?).as_dict().ok())
        .collect()
}

/// The DocMDP permission (1, 2 or 3) of a certified revision.
fn doc_mdp(rev: &Document) -> Option<i64> {
    let perms = rev.catalog().ok()?.get(b"Perms").ok()?;
    let sig = deref(rev, deref(rev, perms).as_dict().ok()?.get(b"DocMDP").ok()?)
        .as_dict()
        .ok()?;
    let p = transform_params(rev, sig, b"DocMDP")
        .into_iter()
        .find_map(|t| t.get(b"P").ok()?.as_i64().ok())
        .unwrap_or(2);
    Some(p.clamp(1, 3))
}

/// Which fields a signature locks.
enum Lock {
    All,
    Include(Vec<String>),
    Exclude(Vec<String>),
}

impl Lock {
    fn covers(&self, field: &str) -> bool {
        let named = |list: &[String]| {
            list.iter().any(|n| {
                field == n
                    || field
                        .strip_prefix(n.as_str())
                        .is_some_and(|r| r.starts_with('.'))
            })
        };
        match self {
            Lock::All => true,
            Lock::Include(l) => named(l),
            Lock::Exclude(l) => !named(l),
        }
    }
}

/// The field locks of the signature dictionary `sig_id` in `rev`: its
/// FieldMDP references and its field's `/Lock`.
fn field_locks(rev: &Document, sig_id: ObjectId) -> Vec<Lock> {
    let Ok(sig) = rev.get_dictionary(sig_id) else {
        return vec![];
    };
    let mut params = transform_params(rev, sig, b"FieldMDP");
    for (id, o) in &rev.objects {
        if let Ok(d) = o.as_dict()
            && d.get(b"V").ok().and_then(|v| v.as_reference().ok()) == Some(sig_id)
            && let Some(Ok(l)) = inherited(rev, *id, b"Lock").map(|l| deref(rev, l).as_dict())
        {
            params.push(l);
        }
    }
    params
        .into_iter()
        .map(|p| {
            let list = || -> Vec<String> {
                p.get(b"Fields")
                    .map(|f| deref(rev, f))
                    .ok()
                    .and_then(|f| f.as_array().ok())
                    .into_iter()
                    .flatten()
                    .filter_map(|o| match deref(rev, o) {
                        Object::String(s, _) => Some(text(s)),
                        _ => None,
                    })
                    .collect()
            };
            match name_of(p, b"Action") {
                Some(b"Include") => Lock::Include(list()),
                Some(b"Exclude") => Lock::Exclude(list()),
                _ => Lock::All,
            }
        })
        .collect()
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
