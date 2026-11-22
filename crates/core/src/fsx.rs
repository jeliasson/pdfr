//! Writing files safely: a crash, a full disk or another program never
//! leaves a half-written file, a symlink keeps pointing at the file it
//! pointed at, and the file keeps its permissions.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Most symlinks followed to find the real file (as the kernel does).
const MAX_LINKS: usize = 40;

/// Writes `bytes` to `path` all at once: into a new file next to it, which
/// then replaces it. Readers see the old contents or the new, never a mix.
///
/// - A symlink is followed: the file it points at is replaced, the link
///   stays.
/// - The new file gets the old one's permissions; a new file is readable by
///   its owner only (0600 on Unix).
/// - A read-only file is not replaced.
/// - On failure the original is untouched and the temporary file removed.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let target = resolve(path)?;
    let dir = match target.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let name = target
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not a file name"))?;
    let old = fs::metadata(&target).ok();
    if let Some(m) = &old {
        if !m.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} is not a regular file", target.display()),
            ));
        }
        if m.permissions().readonly() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("{} is read-only", target.display()),
            ));
        }
    }
    let (tmp, mut file) = create_temp(&dir, name, old.as_ref())?;
    let res = (|| {
        file.write_all(bytes)?;
        if let Some(m) = &old {
            // `create_new`'s mode is reduced by the umask: set it exactly.
            file.set_permissions(m.permissions())?;
        }
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, &target)?;
        sync_dir(&dir);
        Ok(())
    })();
    if res.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    res
}

/// The real file `path` names: symlinks followed, also when the file they
/// point at does not exist yet.
fn resolve(path: &Path) -> io::Result<PathBuf> {
    if let Ok(p) = fs::canonicalize(path) {
        return Ok(p);
    }
    let mut p = path.to_path_buf();
    for _ in 0..MAX_LINKS {
        match fs::symlink_metadata(&p) {
            Ok(m) if m.file_type().is_symlink() => {
                let to = fs::read_link(&p)?;
                p = match p.parent() {
                    Some(d) if to.is_relative() => d.join(to),
                    _ => to,
                };
            }
            // A new file: its folder must exist (and is resolved itself).
            _ => {
                let name = p.file_name().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "not a file name")
                })?;
                let dir = match p.parent() {
                    Some(d) if !d.as_os_str().is_empty() => d,
                    _ => Path::new("."),
                };
                return Ok(fs::canonicalize(dir)?.join(name));
            }
        }
    }
    Err(io::Error::other("too many levels of symbolic links"))
}

/// A new, empty file in `dir` with a name nobody can guess beforehand.
fn create_temp(
    dir: &Path,
    name: &std::ffi::OsStr,
    old: Option<&fs::Metadata>,
) -> io::Result<(PathBuf, File)> {
    let mut last = None;
    for _ in 0..16 {
        let mut tmp_name = std::ffi::OsString::from(".");
        tmp_name.push(name);
        tmp_name.push(format!(".{:016x}.tmp", random()));
        let tmp = dir.join(tmp_name);
        let mut o = OpenOptions::new();
        o.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            o.mode(old.map_or(0o600, |m| m.permissions().mode() & 0o7777));
        }
        #[cfg(not(unix))]
        let _ = old;
        match o.open(&tmp) {
            Ok(f) => return Ok((tmp, f)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| io::Error::other("could not create a temporary file")))
}

/// 64 random bits from the standard library's randomly keyed hasher.
fn random() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
    );
    h.write_u32(std::process::id());
    h.finish()
}

/// Makes the rename itself durable (Unix; elsewhere there is no way to).
fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pdfr-fsx-{name}-{:x}", random()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn leftovers(dir: &Path) -> Vec<String> {
        fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect()
    }

    #[test]
    fn writes_new_and_replaces() {
        let d = temp_dir("new");
        let f = d.join("a.pdf");
        write_atomic(&f, b"one").unwrap();
        assert_eq!(fs::read(&f).unwrap(), b"one");
        write_atomic(&f, b"two").unwrap();
        assert_eq!(fs::read(&f).unwrap(), b"two");
        assert!(leftovers(&d).is_empty());
        fs::remove_dir_all(&d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn keeps_mode_and_new_files_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let d = temp_dir("mode");
        let f = d.join("new.pdf");
        write_atomic(&f, b"x").unwrap();
        assert_eq!(
            fs::metadata(&f).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let g = d.join("shared.pdf");
        fs::write(&g, b"old").unwrap();
        fs::set_permissions(&g, fs::Permissions::from_mode(0o640)).unwrap();
        write_atomic(&g, b"new").unwrap();
        assert_eq!(
            fs::metadata(&g).unwrap().permissions().mode() & 0o777,
            0o640
        );
        fs::set_permissions(&g, fs::Permissions::from_mode(0o666)).unwrap();
        write_atomic(&g, b"newer").unwrap();
        // Not reduced by the umask either.
        assert_eq!(
            fs::metadata(&g).unwrap().permissions().mode() & 0o777,
            0o666
        );
        fs::remove_dir_all(&d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlink_stays_a_symlink() {
        let d = temp_dir("link");
        let real = d.join("real.list");
        fs::write(&real, b"old").unwrap();
        let link = d.join("link.list");
        std::os::unix::fs::symlink("real.list", &link).unwrap();
        write_atomic(&link, b"new").unwrap();
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(&real).unwrap(), b"new");
        // A link to a file that does not exist yet creates that file.
        let dangling = d.join("dangling");
        std::os::unix::fs::symlink("later.list", &dangling).unwrap();
        write_atomic(&dangling, b"made").unwrap();
        assert!(
            fs::symlink_metadata(&dangling)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(d.join("later.list")).unwrap(), b"made");
        assert!(leftovers(&d).is_empty());
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn failure_leaves_the_original() {
        let d = temp_dir("fail");
        let f = d.join("a.pdf");
        fs::write(&f, b"original").unwrap();
        // A read-only file is not replaced.
        let mut p = fs::metadata(&f).unwrap().permissions();
        p.set_readonly(true);
        fs::set_permissions(&f, p.clone()).unwrap();
        assert!(write_atomic(&f, b"new").is_err());
        assert_eq!(fs::read(&f).unwrap(), b"original");
        #[allow(clippy::permissions_set_readonly_false)]
        p.set_readonly(false);
        fs::set_permissions(&f, p).unwrap();
        // A folder in the way is not replaced, and no temp file is left.
        let sub = d.join("dir.pdf");
        fs::create_dir(&sub).unwrap();
        assert!(write_atomic(&sub, b"new").is_err());
        assert!(sub.is_dir());
        // A missing folder fails without creating anything.
        assert!(write_atomic(&d.join("missing/a.pdf"), b"x").is_err());
        assert!(leftovers(&d).is_empty());
        assert_eq!(fs::read(&f).unwrap(), b"original");
        fs::remove_dir_all(&d).unwrap();
    }
}
