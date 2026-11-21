//! Checking the digital signatures in a PDF.
//!
//! For each signature: do the signed bytes still hash to what was signed,
//! does the signature match the signer's certificate, does that
//! certificate lead to a trusted root, is there a valid timestamp, and was
//! the file changed after signing (and if so, only in allowed ways: form
//! fill, annotations, further signatures).

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
    /// Nothing: the signature covers the whole file.
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
    /// "PAdES B-T", "PAdES B-B" or "PKCS#7".
    pub kind: String,
    pub digest: String,
    pub reason: Option<String>,
    pub location: Option<String>,
}

/// Size limit for a signature's CMS (real ones are well below).
const MAX_CMS: usize = 1 << 20;

/// Checks every signature of the file `bytes`, given what core read.
pub fn verify_all(bytes: &[u8], sigs: &[DigitalSig], store: &TrustStore) -> Vec<Report> {
    sigs.iter()
        .map(|s| verify_one(bytes, s, sigs, store))
        .collect()
}

pub fn verify_one(bytes: &[u8], s: &DigitalSig, all: &[DigitalSig], store: &TrustStore) -> Report {
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
    };
    match check(bytes, s, all, store, &mut r) {
        Ok(()) => {}
        Err(e) => {
            r.status = Status::Broken;
            r.problem = Some(e);
        }
    }
    r
}

fn check(
    bytes: &[u8],
    s: &DigitalSig,
    all: &[DigitalSig],
    store: &TrustStore,
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
    let ci = ContentInfo::from_der(der).map_err(|_| "the signature can't be read")?;
    if ci.content_type != oids::SIGNED_DATA {
        return Err("the signature can't be read".into());
    }
    let sd: SignedData = ci
        .content
        .decode_as()
        .map_err(|_| "the signature can't be read")?;
    let pool = certs_of(&sd);
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
            Ok(t) => {
                r.timestamp = Some(t);
                if s.sub_filter == "ETSI.CAdES.detached" {
                    r.kind = "PAdES B-T".into();
                }
            }
            Err(e) => r.timestamp_problem = Some(e),
        }
    }

    // Changes after this signature.
    r.after = changes_after(bytes, s, all);

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

/// Checks an RFC 3161 token over `sig_value`; returns its time.
fn check_timestamp(token: &[u8], sig_value: &[u8], store: &TrustStore) -> Result<u64, String> {
    let info = crate::tsa::tst_info(token)?;
    let h = hash(info.message_imprint.hash_algorithm.oid, sig_value)
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
    if chain.anchor.is_none() {
        return Err(format!(
            "the timestamp authority \"{}\" is not trusted",
            tsa.name()
        ));
    }
    Ok(t)
}

fn certs_of(sd: &SignedData) -> Vec<Cert> {
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

fn find_signer(si: &SignerInfo, pool: &[Cert]) -> Option<Cert> {
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

fn timestamp_token(si: &SignerInfo) -> Option<Vec<u8>> {
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
fn der_prefix(b: &[u8]) -> Option<&[u8]> {
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
    if all.iter().any(|o| o.covered_end() > end) {
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
    let allowed = !kinds.contains(&Kind::Other);
    for k in kinds {
        let w = match k {
            Kind::Form => "form fields filled in",
            Kind::Annot => "comments or annotations added",
            Kind::Sig => continue,
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
            Kind::Form
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
            if changed_only(&[b"AcroForm", b"DSS", b"Perms", b"Extensions"]) {
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
