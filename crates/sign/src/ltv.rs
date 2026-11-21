//! Long-term validation for a new signature (PAdES B-LT and B-LTA): after
//! signing, the certificate chain and the issuers' revocation answers go
//! into the file's DSS, then a document timestamp seals it all. Anyone can
//! then check the signature years later, offline, after the certificate
//! expired or the issuer stopped answering.
//!
//! Best effort: whatever can't be fetched leaves the signature at B-T.

use crate::cades::Timestamper;
use crate::revocation::{Evidence, Online, State, check_path};
use crate::trust::{TrustStore, check_chain};
use crate::verify::{
    DOC_TIMESTAMP, certs_of, der_prefix, find_signer, signed_data, timestamp_token,
};
use pdfr_core::digital::{DssData, TIMESTAMP_RESERVE, add_dss, doc_timestamp};
use pdfr_core::forms::FormDoc;

/// How long-lived a new signature turned out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// No timestamp.
    B,
    /// Timestamped.
    T,
    /// Timestamped, with validation data in the file.
    Lt,
    /// And a document timestamp over that.
    Lta,
}

impl Level {
    pub fn name(self) -> &'static str {
        match self {
            Level::B => "PAdES B-B",
            Level::T => "PAdES B-T",
            Level::Lt => "PAdES B-LT",
            Level::Lta => "PAdES B-LTA",
        }
    }
}

pub struct Extended {
    pub bytes: Vec<u8>,
    pub level: Level,
    /// Why it stopped short of B-LTA, if it did.
    pub problem: Option<String>,
}

/// Adds validation data for the newest signature of the signed file
/// `bytes`, asking issuers through `online`, then (with `tsa`) a document
/// timestamp. Never fails: on a problem the file is returned as far as it
/// got.
pub fn extend(
    bytes: Vec<u8>,
    store: &TrustStore,
    online: Online,
    tsa: Option<Timestamper>,
) -> Extended {
    let stop = |bytes, level, why: String| Extended {
        bytes,
        level,
        problem: Some(why),
    };
    let data = match validation_data(&bytes, store, online) {
        Ok(d) => d,
        Err((level, why)) => return stop(bytes, level, why),
    };
    let dss = DssData {
        certs: data.certs,
        ocsps: data.ocsps,
        crls: data.crls,
    };
    let lt = match add_dss(&bytes, &dss) {
        Ok(b) => b,
        Err(e) => return stop(bytes, Level::T, e.to_string()),
    };
    let Some(tsa) = tsa else {
        return Extended {
            bytes: lt,
            level: Level::Lt,
            problem: None,
        };
    };
    match doc_timestamp(&lt, TIMESTAMP_RESERVE, |covered| tsa(covered)) {
        Ok(b) => Extended {
            bytes: b,
            level: Level::Lta,
            problem: None,
        },
        Err(e) => stop(lt, Level::Lt, format!("no document timestamp ({e})")),
    }
}

/// The chain and revocation answers for the newest signature of `bytes`
/// and for its timestamp authority. Errs with the level reached and why
/// it can't go further.
fn validation_data(
    bytes: &[u8],
    store: &TrustStore,
    online: Online,
) -> Result<Evidence, (Level, String)> {
    let at_t = |e: String| (Level::T, e);
    let fd = FormDoc::open(bytes.to_vec()).map_err(|e| at_t(format!("{e:?}")))?;
    let sig = fd
        .digital
        .iter()
        .filter(|s| s.sub_filter != DOC_TIMESTAMP)
        .max_by_key(|s| s.covered_end())
        .ok_or_else(|| at_t("the signature was not found".into()))?;
    let der = der_prefix(&sig.contents).ok_or_else(|| at_t("unreadable signature".into()))?;
    let sd = signed_data(der).map_err(at_t)?;
    let pool = certs_of(&sd);
    let si = sd
        .signer_infos
        .0
        .iter()
        .next()
        .ok_or_else(|| at_t("unreadable signature".into()))?;
    let cert = find_signer(si, &pool).ok_or_else(|| at_t("unreadable signature".into()))?;
    let Some(token) = timestamp_token(si) else {
        return Err((Level::B, "it has no timestamp".into()));
    };
    let now = crate::now();
    let chain = check_chain(&cert, &pool, store, now).chain;
    let none = Evidence::default();
    let (rev, mut data) = check_path(&cert, &chain, &none, Some(online), now);
    match rev.state {
        State::Good => {}
        State::Revoked { .. } => return Err(at_t("the certificate has been revoked".into())),
        State::Unknown(why) => return Err(at_t(why)),
    }
    data.certs
        .extend(std::iter::once(&cert).chain(&chain).map(|c| c.der.clone()));
    // The timestamp authority's chain too, if its issuer answers.
    if let Ok(tsd) = signed_data(&token)
        && let Some(tsi) = tsd.signer_infos.0.iter().next()
    {
        let tpool = certs_of(&tsd);
        if let Some(tsa) = find_signer(tsi, &tpool) {
            let tchain = check_chain(&tsa, &tpool, store, now).chain;
            let (trev, tdata) = check_path(&tsa, &tchain, &none, Some(online), now);
            if trev.state == State::Good {
                data.merge(&tdata);
            }
            for c in std::iter::once(&tsa).chain(&tchain) {
                if !data.certs.contains(&c.der) {
                    data.certs.push(c.der.clone());
                }
            }
        }
    }
    Ok(data)
}
