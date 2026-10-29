//! Tree checks and the tree digest of a staged skill directory.

use std::fs::{self, DirEntry, FileType, Metadata};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::{MarketplaceError, Result};

pub const MAX_FILES: usize = 512;
pub const MAX_FILE_BYTES: u64 = 1 << 20;
pub const MAX_TOTAL_BYTES: u64 = 8 << 20;

/// Reject a path that is absolute, has a `..` or empty component, or is
/// not `/`-separated text. Used on every path that comes from outside
/// before it is joined to a directory.
pub fn check_relative_path(path: &str) -> Result<()> {
    if path.starts_with('/') || path.starts_with('\\') {
        return Err(MarketplaceError::AbsolutePath(PathBuf::from(path)));
    }
    let clean = path
        .split('/')
        .all(|part| !matches!(part, "" | "." | "..") && !part.contains(['\\', '\0']));
    if clean {
        Ok(())
    } else {
        Err(MarketplaceError::Traversal(PathBuf::from(path)))
    }
}

/// Apply the per-file and total limits to one more file of `bytes`. `total`
/// grows only when the file passes.
pub(crate) fn check_limits(path: &str, bytes: u64, count: usize, total: &mut u64) -> Result<()> {
    let new_total = *total + bytes;
    if count > MAX_FILES {
        Err(MarketplaceError::TooManyFiles(MAX_FILES))
    } else if bytes > MAX_FILE_BYTES {
        Err(MarketplaceError::FileTooLarge {
            path: PathBuf::from(path),
            bytes,
        })
    } else if new_total > MAX_TOTAL_BYTES {
        Err(MarketplaceError::TotalTooLarge(MAX_TOTAL_BYTES))
    } else {
        *total = new_total;
        Ok(())
    }
}

/// Walk `dir` without following links, apply the limits, and return the
/// tree digest `sha256:<hex>`: sha256 over the sorted lines
/// `<relative path>\0<hex sha256 of the bytes>\n`.
pub fn tree_digest(dir: &Path) -> Result<String> {
    let mut files = Vec::new();
    let mut total = 0u64;
    walk(dir, "", &mut files, &mut total)?;
    files.sort();
    let mut tree = Sha256::new();
    for (rel, file_hash) in &files {
        tree.update(rel.as_bytes());
        tree.update([0u8]);
        tree.update(file_hash.as_bytes());
        tree.update(b"\n");
    }
    Ok(format!("sha256:{}", hex(&tree.finalize())))
}

fn walk(dir: &Path, rel: &str, files: &mut Vec<(String, String)>, total: &mut u64) -> Result<()> {
    for entry in read_entries(dir)? {
        let (child, kind, meta) = classify(&entry, rel)?;
        match kind {
            Kind::Dir => walk(&entry.path(), &format!("{child}/"), files, total)?,
            Kind::File => hash_file(&entry, child, &meta, files, total)?,
        }
    }
    Ok(())
}

/// The entries of `dir`; an error names `dir`.
pub(crate) fn read_entries(dir: &Path) -> Result<Vec<DirEntry>> {
    fs::read_dir(dir)
        .and_then(|entries| entries.collect())
        .map_err(|e| MarketplaceError::io(dir.display(), e))
}

/// What a skill tree entry is. Links and special files are refused, so
/// these are the only 2 kinds.
pub(crate) enum Kind {
    Dir,
    File,
}

/// The entry rules of a skill tree, shared by the digest walk and the copy:
/// the name is UTF-8 and clean, and the entry is a directory or a regular
/// file. Returns the path relative to the tree root, the kind and the
/// metadata. `rel` is the parent's path with its trailing `/`.
pub(crate) fn classify(entry: &DirEntry, rel: &str) -> Result<(String, Kind, Metadata)> {
    let os_name = entry.file_name();
    let name = os_name
        .to_str()
        .ok_or_else(|| MarketplaceError::Traversal(PathBuf::from(rel).join(&os_name)))?;
    let child = format!("{rel}{name}");
    check_relative_path(&child)?;
    let meta = fs::symlink_metadata(entry.path()).map_err(|e| MarketplaceError::io(&child, e))?;
    let kind = kind_of(meta.file_type(), &child)?;
    Ok((child, kind, meta))
}

fn kind_of(file_type: FileType, child: &str) -> Result<Kind> {
    if file_type.is_symlink() {
        Err(MarketplaceError::Symlink(PathBuf::from(child)))
    } else if file_type.is_dir() {
        Ok(Kind::Dir)
    } else if file_type.is_file() {
        Ok(Kind::File)
    } else {
        Err(MarketplaceError::NotRegularFile(PathBuf::from(child)))
    }
}

fn hash_file(
    entry: &DirEntry,
    child: String,
    meta: &Metadata,
    files: &mut Vec<(String, String)>,
    total: &mut u64,
) -> Result<()> {
    check_limits(&child, meta.len(), files.len() + 1, total)?;
    let bytes = fs::read(entry.path())
        .map_err(|e| MarketplaceError::io(format_args!("read {child}"), e))?;
    files.push((child, hex(&Sha256::digest(&bytes))));
    Ok(())
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The first 12 hex characters of a `sha256:<hex>` digest.
pub(crate) fn digest12(digest: &str) -> &str {
    let hex = digest.strip_prefix("sha256:").unwrap_or(digest);
    &hex[..hex.len().min(12)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_path_rules() {
        assert!(check_relative_path("SKILL.md").is_ok());
        assert!(check_relative_path("refs/a.md").is_ok());
        assert!(matches!(
            check_relative_path("/etc/passwd"),
            Err(MarketplaceError::AbsolutePath(_))
        ));
        for bad in ["a/../../x", "..", "", "a//b", "./a", "a/"] {
            assert!(
                matches!(
                    check_relative_path(bad),
                    Err(MarketplaceError::Traversal(_))
                ),
                "{bad}"
            );
        }
    }

    #[test]
    fn digest_is_order_independent_and_content_sensitive() {
        let a = tempfile::tempdir().unwrap();
        fs::create_dir(a.path().join("d")).unwrap();
        fs::write(a.path().join("d/x"), "1").unwrap();
        fs::write(a.path().join("y"), "2").unwrap();
        let first = tree_digest(a.path()).unwrap();
        assert!(first.starts_with("sha256:") && first.len() == 71);
        assert_eq!(first, tree_digest(a.path()).unwrap());
        fs::write(a.path().join("y"), "3").unwrap();
        assert_ne!(first, tree_digest(a.path()).unwrap());
    }

    fn limits(path: &str, bytes: u64, count: usize, total: u64) -> (Result<()>, u64) {
        let mut total = total;
        let r = check_limits(path, bytes, count, &mut total);
        (r, total)
    }

    #[test]
    fn limits_accept_the_edge_and_add_to_the_total() {
        let (r, total) = limits("a", MAX_FILE_BYTES, MAX_FILES, 100);
        r.unwrap();
        assert_eq!(total, 100 + MAX_FILE_BYTES);
    }

    #[test]
    fn limits_refuse_each_overrun_with_its_own_error() {
        let (r, _) = limits("a", 1, MAX_FILES + 1, 0);
        assert!(matches!(r, Err(MarketplaceError::TooManyFiles(n)) if n == MAX_FILES));
        let (r, _) = limits("big", MAX_FILE_BYTES + 1, 1, 0);
        assert!(matches!(
            r,
            Err(MarketplaceError::FileTooLarge { ref path, bytes })
                if path == Path::new("big") && bytes == MAX_FILE_BYTES + 1
        ));
        let (r, _) = limits("a", 2, 1, MAX_TOTAL_BYTES - 1);
        assert!(matches!(r, Err(MarketplaceError::TotalTooLarge(n)) if n == MAX_TOTAL_BYTES));
        // The total limit is exact: reaching it is allowed.
        limits("a", 1, 1, MAX_TOTAL_BYTES - 1).0.unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn digest_refuses_a_symlink_and_a_socket() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("real"), "x").unwrap();
        std::os::unix::fs::symlink("real", dir.path().join("link")).unwrap();
        let err = tree_digest(dir.path()).unwrap_err();
        assert!(matches!(err, MarketplaceError::Symlink(p) if p == Path::new("link")));
        fs::remove_file(dir.path().join("link")).unwrap();
        // A socket file is neither a regular file nor a link.
        let _socket = std::os::unix::net::UnixListener::bind(dir.path().join("sock")).unwrap();
        let err = tree_digest(dir.path()).unwrap_err();
        assert!(matches!(err, MarketplaceError::NotRegularFile(p) if p == Path::new("sock")));
    }

    #[cfg(unix)]
    #[test]
    fn digest_refuses_unclean_and_non_utf8_names_and_nested_links() {
        use std::os::unix::ffi::OsStrExt;
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a\\b"), "x").unwrap();
        let err = tree_digest(dir.path()).unwrap_err();
        assert!(matches!(err, MarketplaceError::Traversal(_)), "{err:?}");
        fs::remove_file(dir.path().join("a\\b")).unwrap();
        let bad = std::ffi::OsStr::from_bytes(b"bad\xff");
        if fs::write(dir.path().join(bad), "x").is_ok() {
            let err = tree_digest(dir.path()).unwrap_err();
            assert!(matches!(err, MarketplaceError::Traversal(_)), "{err:?}");
            fs::remove_file(dir.path().join(bad)).unwrap();
        }
        fs::create_dir(dir.path().join("d")).unwrap();
        std::os::unix::fs::symlink("/", dir.path().join("d/l")).unwrap();
        let err = tree_digest(dir.path()).unwrap_err();
        assert!(matches!(err, MarketplaceError::Symlink(p) if p == Path::new("d/l")));
    }

    #[test]
    fn digest_refuses_an_oversize_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("big"),
            vec![0u8; MAX_FILE_BYTES as usize + 1],
        )
        .unwrap();
        let err = tree_digest(dir.path()).unwrap_err();
        assert!(
            matches!(err, MarketplaceError::FileTooLarge { .. }),
            "{err:?}"
        );
    }

    #[test]
    fn digest_refuses_too_many_files() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..=MAX_FILES {
            fs::write(dir.path().join(format!("f{i}")), "").unwrap();
        }
        let err = tree_digest(dir.path()).unwrap_err();
        assert!(matches!(err, MarketplaceError::TooManyFiles(_)), "{err:?}");
    }
}
