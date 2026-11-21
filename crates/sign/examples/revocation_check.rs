//! Asks the issuers of a real certificate chain whether it was revoked,
//! for trying the online path by hand (uses curl for the network).
//!
//! `cargo run -p pdfr-sign --example revocation_check -- chain.pem`
//! (leaf first, then its issuers, as `openssl s_client -showcerts` prints)
use pdfr_sign::Cert;
use pdfr_sign::revocation::{Cache, Evidence, Fetch, Online, check_path};

struct Curl;

impl Curl {
    fn run(args: &[&str]) -> Result<Vec<u8>, String> {
        let out = std::process::Command::new("curl")
            .args(["-s", "-f", "--max-time", "6"])
            .args(args)
            .output()
            .map_err(|e| e.to_string())?;
        out.status
            .success()
            .then_some(out.stdout)
            .ok_or_else(|| format!("curl failed ({})", out.status))
    }
}

impl Fetch for Curl {
    fn post(&self, url: &str, content_type: &str, body: &[u8]) -> Result<Vec<u8>, String> {
        let tmp = std::env::temp_dir().join("pdfr-ocsp-req");
        std::fs::write(&tmp, body).map_err(|e| e.to_string())?;
        let header = format!("Content-Type: {content_type}");
        let data = format!("@{}", tmp.display());
        eprintln!("POST {url}");
        Curl::run(&["-H", &header, "--data-binary", &data, url])
    }

    fn get(&self, url: &str) -> Result<Vec<u8>, String> {
        eprintln!("GET {url}");
        Curl::run(&[url])
    }
}

fn main() {
    let path = std::env::args().nth(1).expect("a PEM chain");
    let certs = Cert::from_pem_bundle(&std::fs::read_to_string(path).unwrap());
    let (leaf, chain) = certs.split_first().expect("certificates");
    let cache = Cache::default();
    let online = Online {
        fetch: &Curl,
        cache: &cache,
    };
    let (rev, data) = check_path(
        leaf,
        chain,
        &Evidence::default(),
        Some(online),
        pdfr_sign::now(),
    );
    println!("{}: {rev:?}", leaf.name());
    println!(
        "data: {} OCSP response(s), {} CRL(s)",
        data.ocsps.len(),
        data.crls.len()
    );
}
