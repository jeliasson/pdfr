//! Make and check pdfr licence keys by hand.
//!
//!   pdfr-license keygen --kid 1 --out ~/secrets/pdfr-kid1.signing-key
//!   pdfr-license issue  --key ~/secrets/pdfr-kid1.signing-key --name "Jane Doe" [--plan personal]
//!   pdfr-license verify "PDFR1.…" [--public 1:<base64url>]
//!
//! pdfr.app issues the real keys; this is for testing and emergencies.

use std::process::exit;

use pdfr_license as lic;

const USAGE: &str = "pdfr-license keygen --kid <n> --out <file>
pdfr-license issue --key <file> --name <name> [--plan personal|business|founder] [--id <uuid>]
pdfr-license verify <key> [--public <kid>:<base64url>]";

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn fail(msg: &str) -> ! {
    eprintln!("pdfr-license: {msg}\n\n{USAGE}");
    exit(2)
}

/// The key file: `kid=<n>` and `seed=<base64url>` lines.
fn read_key(path: &str) -> (u8, [u8; 32]) {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| fail(&format!("{path}: {e}")));
    let field = |k: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(&format!("{k}=")))
            .map(str::trim)
    };
    let kid = field("kid").and_then(|v| v.parse().ok());
    let seed = field("seed").and_then(lic::decode32);
    match (kid, seed) {
        (Some(k), Some(s)) => (k, s),
        _ => fail(&format!("{path} is not a pdfr signing key")),
    }
}

fn random_seed() -> [u8; 32] {
    let mut s = [0u8; 32];
    getrandom::fill(&mut s).unwrap_or_else(|e| fail(&format!("no randomness: {e}")));
    s
}

fn uuid_v4() -> String {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).unwrap_or_else(|e| fail(&format!("no randomness: {e}")));
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    )
}

fn today() -> String {
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 86_400)
        .unwrap_or(0) as i64;
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("keygen") => {
            let kid: u8 = arg(&args, "--kid")
                .and_then(|k| k.parse().ok())
                .unwrap_or_else(|| fail("--kid <1-254> is required"));
            let out = arg(&args, "--out").unwrap_or_else(|| fail("--out <file> is required"));
            if std::path::Path::new(&out).exists() {
                fail(&format!("{out} exists; not overwriting a signing key"));
            }
            let seed = random_seed();
            let public = lic::public_key(&seed);
            let body = format!(
                "# pdfr licence signing key. SECRET: keep out of git, put in pdfr.app's LICENSE_SIGNING_KEY.\nkid={kid}\nseed={}\npublic={}\n",
                lic::encode(&seed),
                lic::encode(&public)
            );
            write_private(&out, &body);
            println!("Wrote {out}\n");
            println!(
                "pdfr.app secret:\n  LICENSE_KID={kid}\n  LICENSE_SIGNING_KEY={}\n  LICENSE_PUBLIC_KEY={}\n",
                lic::encode(&seed),
                lic::encode(&public)
            );
            println!(
                "pdfr TRUSTED_KEYS entry (crates/license/src/lib.rs):\n  ({kid}, {public:?}),"
            );
        }
        Some("issue") => {
            let (kid, seed) =
                read_key(&arg(&args, "--key").unwrap_or_else(|| fail("--key <file> is required")));
            let name = arg(&args, "--name").unwrap_or_else(|| fail("--name is required"));
            let payload = lic::Payload {
                v: 1,
                kid,
                id: arg(&args, "--id").unwrap_or_else(uuid_v4),
                plan: arg(&args, "--plan").unwrap_or_else(|| "personal".into()),
                name,
                issued: today(),
            };
            println!("{}", lic::issue(&payload, &seed));
        }
        Some("verify") => {
            let key = args.get(1).unwrap_or_else(|| fail("verify needs a key"));
            let mut keys = lic::trusted_keys();
            if let Some(p) = arg(&args, "--public") {
                keys.push(
                    lic::parse_public(&p).unwrap_or_else(|| fail("--public is <kid>:<base64url>")),
                );
            }
            match lic::verify(key, &keys) {
                Ok(l) => println!(
                    "valid: {} licence for {} (id {}, issued {}, key {})",
                    l.plan.label(),
                    l.name,
                    l.id,
                    l.issued,
                    l.kid
                ),
                Err(e) => {
                    println!("invalid: {e} ({e:?})");
                    exit(1);
                }
            }
        }
        _ => fail("unknown command"),
    }
}

#[cfg(unix)]
fn write_private(path: &str, body: &str) {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap_or_else(|e| fail(&format!("{path}: {e}")));
    f.write_all(body.as_bytes())
        .unwrap_or_else(|e| fail(&format!("{path}: {e}")));
}

#[cfg(not(unix))]
fn write_private(path: &str, body: &str) {
    std::fs::write(path, body).unwrap_or_else(|e| fail(&format!("{path}: {e}")));
}
