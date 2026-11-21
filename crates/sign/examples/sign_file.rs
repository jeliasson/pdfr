//! Signs a PDF with a certificate from the NSS database or a token, for
//! checking pdfr's signatures with other tools (pdfsig, pyHanko, Okular).
//!
//! `cargo run -p pdfr-sign --example sign_file -- list`
//! `cargo run -p pdfr-sign --example sign_file -- <in.pdf> <out.pdf> <name part> [tsa-url]`
use pdfr_core::digital::{DEFAULT_RESERVE, SignRequest};
use pdfr_core::forms::FormDoc;
use pdfr_core::geom::Rect;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let certs = pdfr_sign::signer::discover();
    if args.first().map(String::as_str) == Some("list") || args.len() < 3 {
        for c in &certs {
            println!(
                "{} | issuer {} | {} | {}",
                c.cert.name(),
                c.cert.issuer_name(),
                c.source.describe(),
                c.problem().unwrap_or_else(|| "can sign".into())
            );
        }
        return;
    }
    let choice = certs
        .iter()
        .find(|c| c.cert.name().contains(&args[2]))
        .expect("no such certificate");
    let signer = choice.signer(None).expect("signer");
    let fd = FormDoc::open(std::fs::read(&args[0]).unwrap()).unwrap();
    let req = SignRequest {
        name_beside: None,
        spot: Some((0, Rect::new(320.0, 700.0, 560.0, 760.0))),
        ink: Some(std::sync::Arc::new(pdfr_core::signature::Ink::typed(
            &choice.cert.name(),
        ))),
        signer: choice.cert.name(),
        reason: Some("pdfr test".into()),
        location: None,
        time: pdfr_sign::now(),
        reserve: DEFAULT_RESERVE,
    };
    let tsa_url = args.get(3).cloned();
    let post = move |body: &[u8]| -> Result<Vec<u8>, String> {
        let url = tsa_url.as_deref().unwrap();
        let tmp = std::env::temp_dir().join("pdfr-tsq");
        std::fs::write(&tmp, body).map_err(|e| e.to_string())?;
        let out = std::process::Command::new("curl")
            .args([
                "-s",
                "-f",
                "-H",
                "Content-Type: application/timestamp-query",
                "--data-binary",
            ])
            .arg(format!("@{}", tmp.display()))
            .arg(url)
            .output()
            .map_err(|e| e.to_string())?;
        out.status
            .success()
            .then_some(out.stdout)
            .ok_or("TSA request failed".into())
    };
    let ts = |d: &[u8]| pdfr_sign::tsa::timestamp(d, &post);
    let use_ts = args.len() > 3;
    let bytes = pdfr_core::digital::sign(&fd, &fd.fields, &req, |content| {
        let s = pdfr_sign::cades::sign(content, signer.as_ref(), use_ts.then_some(&ts as _))?;
        if let Some(e) = s.timestamp_error {
            eprintln!("timestamp failed: {e}");
        }
        Ok(s.der)
    })
    .unwrap();
    std::fs::write(&args[1], &bytes).unwrap();
    let fd2 = FormDoc::open(bytes.clone()).unwrap();
    let store = pdfr_sign::trust::TrustStore::standard();
    for r in pdfr_sign::verify::verify_all(&bytes, &fd2.digital, &store) {
        println!(
            "{:?} {} {} ts={:?} {:?} {:?}",
            r.status, r.signer, r.kind, r.timestamp, r.anchor, r.problem
        );
    }
}
