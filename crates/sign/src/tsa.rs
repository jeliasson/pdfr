//! RFC 3161 timestamps: asking a timestamp authority (TSA) to sign the
//! time together with a hash. Only the hash of the signature is sent.

use crate::oids;
use cms::signed_data::SignedData;
use der::asn1::{Int, OctetString};
use der::{Any, Decode, Encode};
use spki::AlgorithmIdentifier;
use x509_tsp::{MessageImprint, TimeStampReq, TimeStampResp, TspVersion, TstInfo};

/// The timestamp authority used unless the user picks another.
pub const DEFAULT_TSA: &str = "http://timestamp.digicert.com";

/// Sends a request body to the timestamp server and returns its answer.
pub type Post<'a> = &'a dyn Fn(&[u8]) -> Result<Vec<u8>, String>;

/// Sends `post` a timestamp request for `data` and returns the checked
/// TimeStampToken (DER). `post` sends the request body to the TSA
/// (`Content-Type: application/timestamp-query`) and returns the response
/// body.
pub fn timestamp(data: &[u8], post: Post) -> Result<Vec<u8>, String> {
    let hash = crate::sha256(data);
    let mut nonce = [0u8; 8];
    getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;
    nonce[0] = (nonce[0] & 0x7f) | 0x01; // positive, no leading zero byte
    let nonce_int = Int::new(&nonce).map_err(|e| e.to_string())?;
    let req = TimeStampReq {
        version: TspVersion::V1,
        message_imprint: MessageImprint {
            hash_algorithm: AlgorithmIdentifier::<Any> {
                oid: oids::SHA256,
                parameters: None,
            },
            hashed_message: OctetString::new(hash.to_vec()).map_err(|e| e.to_string())?,
        },
        req_policy: None,
        nonce: Some(nonce_int.clone()),
        cert_req: true,
        extensions: None,
    };
    let body = post(&req.to_der().map_err(|e| e.to_string())?)?;
    let resp = TimeStampResp::from_der(&body)
        .map_err(|e| format!("the timestamp server's answer could not be read ({e})"))?;
    use cmpv2::status::PkiStatus;
    if !matches!(
        resp.status.status,
        PkiStatus::Accepted | PkiStatus::GrantedWithMods
    ) {
        return Err(format!(
            "the timestamp server refused ({:?})",
            resp.status.status
        ));
    }
    let token = resp
        .time_stamp_token
        .ok_or("the timestamp server sent no timestamp")?;
    let info = tst_info(&token.to_der().map_err(|e| e.to_string())?)?;
    if info.message_imprint.hashed_message.as_bytes() != hash {
        return Err("the timestamp is for something else".into());
    }
    if info.nonce.as_ref() != Some(&nonce_int) {
        return Err("the timestamp does not answer this request".into());
    }
    token.to_der().map_err(|e| e.to_string())
}

/// The TSTInfo inside a TimeStampToken.
pub fn tst_info(token: &[u8]) -> Result<TstInfo, String> {
    let ci = cms::content_info::ContentInfo::from_der(token).map_err(|e| e.to_string())?;
    let sd: SignedData = ci.content.decode_as().map_err(|e| e.to_string())?;
    if sd.encap_content_info.econtent_type != oids::TST_INFO {
        return Err("not a timestamp".into());
    }
    let content = sd
        .encap_content_info
        .econtent
        .ok_or("empty timestamp")?
        .decode_as::<OctetString>()
        .map_err(|e| e.to_string())?;
    TstInfo::from_der(content.as_bytes()).map_err(|e| e.to_string())
}
