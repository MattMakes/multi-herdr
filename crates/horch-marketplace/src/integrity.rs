//! Tree checks and the tree digest of a staged skill directory.

use std::fs;
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

/// Apply the per-file and total limits to one more file of `bytes`.
pub(crate) fn check_limits(path: &str, bytes: u64, count: usize, total: &mut u64) -> Result<()> {
    if count > MAX_FILES {
        return Err(MarketplaceError::TooManyFiles(MAX_FILES));
    }
    if bytes > MAX_FILE_BYTES {
        return Err(MarketplaceError::FileTooLarge {
            path: PathBuf::from(path),
            bytes,
        });
    }
    *total += bytes;
    if *total > MAX_TOTAL_BYTES {
        return Err(MarketplaceError::TotalTooLarge(MAX_TOTAL_BYTES));
    }
    Ok(())
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
    let entries = fs::read_dir(dir).map_err(|e| MarketplaceError::io(dir.display(), e))?;
    for entry in entries {
        let entry = entry.map_err(|e| MarketplaceError::io(dir.display(), e))?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            return Err(MarketplaceError::Traversal(PathBuf::from(rel).join(name)));
        };
        let child = format!("{rel}{name}");
        check_relative_path(&child)?;
        let meta =
            fs::symlink_metadata(entry.path()).map_err(|e| MarketplaceError::io(&child, e))?;
        let kind = meta.file_type();
        if kind.is_symlink() {
            return Err(MarketplaceError::Symlink(PathBuf::from(child)));
        } else if kind.is_dir() {
            walk(&entry.path(), &format!("{child}/"), files, total)?;
        } else if kind.is_file() {
            check_limits(&child, meta.len(), files.len() + 1, total)?;
            let bytes = fs::read(entry.path())
                .map_err(|e| MarketplaceError::io(format_args!("read {child}"), e))?;
            files.push((child, hex(&Sha256::digest(&bytes))));
        } else {
            return Err(MarketplaceError::NotRegularFile(PathBuf::from(child)));
        }
    }
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
}
