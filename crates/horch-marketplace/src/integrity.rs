//! Tree checks and the tree digest of a staged skill directory.

use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::error::{IntegrityViolation, MarketplaceError, Result};

pub const MAX_FILES: usize = 512;
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;
pub const MAX_TOTAL_BYTES: u64 = 8 * 1024 * 1024;

/// Reject a path that is absolute, has a `..` or empty component, or is
/// not `/`-separated text. Used on every path that comes from outside
/// before it is joined to a directory.
pub fn check_relative_path(path: &str) -> std::result::Result<(), IntegrityViolation> {
    if path.starts_with('/') || path.starts_with('\\') {
        return Err(IntegrityViolation::AbsolutePath(path.to_owned()));
    }
    let mut parts = path.split('/').peekable();
    if parts.peek().is_none() {
        return Err(IntegrityViolation::InvalidPath(path.to_owned()));
    }
    for part in parts {
        match part {
            ".." => return Err(IntegrityViolation::ParentComponent(path.to_owned())),
            "" | "." => return Err(IntegrityViolation::InvalidPath(path.to_owned())),
            p if p.contains('\\') || p.contains('\0') => {
                return Err(IntegrityViolation::InvalidPath(path.to_owned()))
            }
            _ => {}
        }
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
    let entries =
        fs::read_dir(dir).map_err(|e| MarketplaceError::io(dir.display().to_string(), e))?;
    for entry in entries {
        let entry = entry.map_err(|e| MarketplaceError::io(dir.display().to_string(), e))?;
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or_else(|| IntegrityViolation::InvalidPath(format!("{rel}{name:?}")))?;
        let child = format!("{rel}{name}");
        check_relative_path(&child)?;
        let meta = fs::symlink_metadata(entry.path())
            .map_err(|e| MarketplaceError::io(child.clone(), e))?;
        let kind = meta.file_type();
        if kind.is_symlink() {
            return Err(IntegrityViolation::Symlink(child).into());
        } else if kind.is_dir() {
            walk(&entry.path(), &format!("{child}/"), files, total)?;
        } else if kind.is_file() {
            if files.len() >= MAX_FILES {
                return Err(IntegrityViolation::TooManyFiles { limit: MAX_FILES }.into());
            }
            if meta.len() > MAX_FILE_BYTES {
                return Err(IntegrityViolation::FileTooLarge {
                    path: child,
                    bytes: meta.len(),
                    limit: MAX_FILE_BYTES,
                }
                .into());
            }
            *total += meta.len();
            if *total > MAX_TOTAL_BYTES {
                return Err(IntegrityViolation::TotalTooLarge {
                    limit: MAX_TOTAL_BYTES,
                }
                .into());
            }
            let bytes = fs::read(entry.path())
                .map_err(|e| MarketplaceError::io(format!("read {child}"), e))?;
            files.push((child, hex(&Sha256::digest(&bytes))));
        } else {
            return Err(IntegrityViolation::NotRegularFile(child).into());
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
        assert_eq!(
            check_relative_path("/etc/passwd"),
            Err(IntegrityViolation::AbsolutePath("/etc/passwd".into()))
        );
        assert_eq!(
            check_relative_path("a/../../x"),
            Err(IntegrityViolation::ParentComponent("a/../../x".into()))
        );
        for bad in ["", "a//b", "./a", "a/"] {
            assert!(check_relative_path(bad).is_err(), "{bad}");
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
