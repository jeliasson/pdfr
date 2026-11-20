//! CMS SignedData for PDF signatures, as PAdES baseline asks
//! (`ETSI.CAdES.detached`): detached content, SHA-256, signed attributes
//! content-type, message-digest and signing-certificate-v2 (no
//! signing-time: the PDF's `/M` holds it), the certificate chain, and
//! optionally an RFC 3161 timestamp over the signature (PAdES B-T).

use crate::{Cert, SigAlg, Signer, oids};
use cms::cert::{CertificateChoices, IssuerAndSerialNumber};
use cms::content_info::{CmsVersion, ContentInfo};
use cms::signed_data::{
    CertificateSet, EncapsulatedContentInfo, SignedData, SignerIdentifier, SignerInfo, SignerInfos,
};
use der::asn1::{Null, OctetString, SetOfVec};
use der::{Any, Decode, Encode};
use spki::AlgorithmIdentifierOwned;
use x509_cert::attr::Attribute;

/// The CMS to embed, and a note if the timestamp could not be added.
pub struct Signed {
    pub der: Vec<u8>,
    pub timestamp_error: Option<String>,
}

/// Something that timestamps a signature value: returns an RFC 3161
/// TimeStampToken (DER) for `data`.
pub type Timestamper<'a> = &'a dyn Fn(&[u8]) -> Result<Vec<u8>, String>;

fn alg(oid: const_oid::ObjectIdentifier, null: bool) -> AlgorithmIdentifierOwned {
    AlgorithmIdentifierOwned {
        oid,
        parameters: null.then(|| Any::encode_from(&Null).expect("NULL encodes")),
    }
}

fn attr(oid: const_oid::ObjectIdentifier, value: Any) -> Result<Attribute, String> {
    Ok(Attribute {
        oid,
        values: SetOfVec::try_from(vec![value]).map_err(|e| e.to_string())?,
    })
}

/// DER TLV.
fn tlv(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    let n = content.len();
    if n < 0x80 {
        out.push(n as u8);
    } else {
        let bytes: Vec<u8> = n
            .to_be_bytes()
            .into_iter()
            .skip_while(|b| *b == 0)
            .collect();
        out.push(0x80 | bytes.len() as u8);
        out.extend(bytes);
    }
    out.extend_from_slice(content);
    out
}

/// `SigningCertificateV2` for `cert` (ESSCertIDv2 with the default
/// SHA-256 and the issuer and serial).
fn signing_cert_v2(cert: &Cert) -> Result<Vec<u8>, String> {
    let tbs = &cert.x.tbs_certificate;
    let issuer = tbs.issuer.to_der().map_err(|e| e.to_string())?;
    let serial = tbs.serial_number.to_der().map_err(|e| e.to_string())?;
    // GeneralNames { directoryName [4] EXPLICIT Name }
    let general_names = tlv(0x30, &tlv(0xA4, &issuer));
    let issuer_serial = tlv(0x30, &[general_names, serial].concat());
    let cert_hash = tlv(0x04, &crate::sha256(&cert.der));
    let ess_cert_id = tlv(0x30, &[cert_hash, issuer_serial].concat());
    Ok(tlv(0x30, &tlv(0x30, &ess_cert_id)))
}

/// Signs `content` (the PDF's byte ranges) with `signer`.
pub fn sign(
    content: &[u8],
    signer: &dyn Signer,
    tsa: Option<Timestamper>,
) -> Result<Signed, String> {
    let cert = signer.cert();
    let digest = crate::sha256(content);
    let signed_attrs: SetOfVec<Attribute> = SetOfVec::try_from(vec![
        attr(
            oids::CONTENT_TYPE,
            Any::encode_from(&oids::DATA).map_err(|e| e.to_string())?,
        )?,
        attr(
            oids::MESSAGE_DIGEST,
            Any::encode_from(&OctetString::new(digest.to_vec()).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?,
        )?,
        attr(
            oids::SIGNING_CERT_V2,
            Any::from_der(&signing_cert_v2(cert)?).map_err(|e| e.to_string())?,
        )?,
    ])
    .map_err(|e| e.to_string())?;
    // The signature covers the attributes encoded as a SET.
    let to_sign = signed_attrs.to_der().map_err(|e| e.to_string())?;
    let signature = signer.sign(&to_sign)?;

    let sig_alg = match signer.alg() {
        SigAlg::RsaSha256 => alg(oids::RSA_SHA256, true),
        SigAlg::EcdsaP256Sha256 => alg(oids::ECDSA_SHA256, false),
        SigAlg::EcdsaP384Sha384 => alg(oids::ECDSA_SHA384, false),
    };
    // Check our own signature before going further: a token that signed
    // with another key than its certificate's must not produce a file.
    crate::trust::verify(
        &cert.x.tbs_certificate.subject_public_key_info,
        sig_alg.oid,
        None,
        Some(oids::SHA256),
        &to_sign,
        &signature,
    )
    .map_err(|e| format!("the key did not produce a valid signature ({e})"))?;

    let mut timestamp_error = None;
    let unsigned_attrs = match tsa {
        Some(ts) => match ts(&signature) {
            Ok(token) => Some(
                SetOfVec::try_from(vec![attr(
                    oids::TIMESTAMP_TOKEN,
                    Any::from_der(&token).map_err(|e| e.to_string())?,
                )?])
                .map_err(|e| e.to_string())?,
            ),
            Err(e) => {
                timestamp_error = Some(e);
                None
            }
        },
        None => None,
    };

    let si = SignerInfo {
        version: CmsVersion::V1,
        sid: SignerIdentifier::IssuerAndSerialNumber(IssuerAndSerialNumber {
            issuer: cert.x.tbs_certificate.issuer.clone(),
            serial_number: cert.x.tbs_certificate.serial_number.clone(),
        }),
        digest_alg: alg(oids::SHA256, false),
        signed_attrs: Some(signed_attrs),
        signature_algorithm: sig_alg,
        signature: OctetString::new(signature).map_err(|e| e.to_string())?,
        unsigned_attrs,
    };
    let mut certs = vec![CertificateChoices::Certificate(cert.x.clone())];
    for c in signer.chain() {
        certs.push(CertificateChoices::Certificate(c.x.clone()));
    }
    let sd = SignedData {
        version: CmsVersion::V1,
        digest_algorithms: SetOfVec::try_from(vec![alg(oids::SHA256, false)])
            .map_err(|e| e.to_string())?,
        encap_content_info: EncapsulatedContentInfo {
            econtent_type: oids::DATA,
            econtent: None,
        },
        certificates: Some(CertificateSet(
            SetOfVec::try_from(certs).map_err(|e| e.to_string())?,
        )),
        crls: None,
        signer_infos: SignerInfos(SetOfVec::try_from(vec![si]).map_err(|e| e.to_string())?),
    };
    let ci = ContentInfo {
        content_type: oids::SIGNED_DATA,
        content: Any::encode_from(&sd).map_err(|e| e.to_string())?,
    };
    Ok(Signed {
        der: ci.to_der().map_err(|e| e.to_string())?,
        timestamp_error,
    })
}
