//! Sign release files, so pdfr and `install.sh` can tell that a release
//! came from pdfr's own CI.
//!
//!   release_sign keygen <dir>          writes <dir>/release.key, prints the public key
//!   release_sign sign <file>           writes <file>.sig, key from $RELEASE_SIGNING_KEY
//!   release_sign verify <file> [--public <base64>]
//!
//! The key file holds the 32-byte Ed25519 seed as base64, which is also what
//! goes into the `RELEASE_SIGNING_KEY` secret. `sign` refuses a key whose
//! public half isn't the one pdfr trusts (`release::PUBLIC_KEY`).

use std::path::{Path, PathBuf};
use std::process::exit;

use pdfr_license::{public_key, release};

const USAGE: &str = "release_sign keygen <dir>
release_sign sign <file>        (key in $RELEASE_SIGNING_KEY)
release_sign verify <file> [--public <base64>]";

fn fail(msg: &str) -> ! {
    eprintln!("release_sign: {msg}\n\n{USAGE}");
    exit(2)
}

fn sig_path(file: &str) -> PathBuf {
    let mut p = Path::new(file).as_os_str().to_owned();
    p.push(".sig");
    PathBuf::from(p)
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| fail(&format!("{}: {e}", path.display())))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("keygen") => {
            let dir = Path::new(args.get(1).unwrap_or_else(|| fail("keygen needs a folder")));
            make_private_dir(dir);
            let out = dir.join("release.key");
            if out.exists() {
                fail(&format!(
                    "{} exists; not overwriting a signing key",
                    out.display()
                ));
            }
            let mut seed = [0u8; 32];
            getrandom::fill(&mut seed).unwrap_or_else(|e| fail(&format!("no randomness: {e}")));
            write_private(&out, &format!("{}\n", release::encode(&seed)));
            let public = public_key(&seed);
            println!("Wrote {} (secret: never commit it).\n", out.display());
            println!("Public key (base64): {}", release::encode(&public));
            println!("Public key bytes:    {public:?}\n");
            print!("{}", release::public_pem(&public));
        }
        Some("sign") => {
            let file = args.get(1).unwrap_or_else(|| fail("sign needs a file"));
            let key = std::env::var("RELEASE_SIGNING_KEY")
                .unwrap_or_else(|_| fail("RELEASE_SIGNING_KEY is not set"));
            let seed = release::decode32(&key)
                .unwrap_or_else(|| fail("RELEASE_SIGNING_KEY is not a base64 32-byte key"));
            if public_key(&seed) != release::PUBLIC_KEY {
                fail("RELEASE_SIGNING_KEY is not the key pdfr trusts (release::PUBLIC_KEY)");
            }
            let data = read(Path::new(file));
            let sig = release::sign(&seed, &data);
            assert!(release::verify(&release::PUBLIC_KEY, &data, sig.as_bytes()));
            let out = sig_path(file);
            std::fs::write(&out, format!("{sig}\n"))
                .unwrap_or_else(|e| fail(&format!("{}: {e}", out.display())));
            println!("Wrote {}", out.display());
        }
        Some("verify") => {
            let file = args.get(1).unwrap_or_else(|| fail("verify needs a file"));
            let public = match args.iter().position(|a| a == "--public") {
                Some(i) => args
                    .get(i + 1)
                    .and_then(|k| release::decode32(k))
                    .unwrap_or_else(|| fail("--public is a base64 32-byte key")),
                None => release::PUBLIC_KEY,
            };
            let data = read(Path::new(file));
            let sig = read(&sig_path(file));
            if release::verify(&public, &data, &sig) {
                println!("valid: {file} is signed by {}", release::encode(&public));
            } else {
                println!("invalid: {file} does not match its signature");
                exit(1);
            }
        }
        _ => fail("unknown command"),
    }
}

#[cfg(unix)]
fn make_private_dir(dir: &Path) {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    if !dir.exists() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .unwrap_or_else(|e| fail(&format!("{}: {e}", dir.display())));
    }
    let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
}

#[cfg(not(unix))]
fn make_private_dir(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap_or_else(|e| fail(&format!("{}: {e}", dir.display())));
}

#[cfg(unix)]
fn write_private(path: &Path, body: &str) {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap_or_else(|e| fail(&format!("{}: {e}", path.display())));
    f.write_all(body.as_bytes())
        .unwrap_or_else(|e| fail(&format!("{}: {e}", path.display())));
}

#[cfg(not(unix))]
fn write_private(path: &Path, body: &str) {
    std::fs::write(path, body).unwrap_or_else(|e| fail(&format!("{}: {e}", path.display())));
}
