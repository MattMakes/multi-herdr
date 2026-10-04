//! Small filesystem helpers: atomic replace, random names, recursive copy.

use std::collections::hash_map::RandomState;
use std::fs::{self, File};
use std::hash::{BuildHasher, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{MarketplaceError, Result};

/// 32 random hex characters. `RandomState` is seeded from the OS; the time,
/// pid and a counter keep two calls in one process distinct.
pub fn random_hex() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut out = String::with_capacity(32);
    for salt in [0u64, 1] {
        let mut h = RandomState::new().build_hasher();
        h.write_u128(nanos);
        h.write_u32(std::process::id());
        h.write_u64(n);
        h.write_u64(salt);
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out
}

/// Replace `path` with `bytes`: write a temp file in the same directory,
/// fsync it, rename it over `path`, then fsync the directory.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| MarketplaceError::io(path.display().to_string(), no_parent()))?;
    fs::create_dir_all(dir).map_err(|e| MarketplaceError::io(dir.display().to_string(), e))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = dir.join(format!(".{name}.tmp-{}", random_hex()));
    let write = || -> std::io::Result<()> {
        let mut f = File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        fs::rename(&tmp, path)?;
        sync_dir(dir)
    };
    write().map_err(|e| {
        let _ = fs::remove_file(&tmp);
        MarketplaceError::io(format!("write {}", path.display()), e)
    })
}

pub fn sync_dir(dir: &Path) -> std::io::Result<()> {
    File::open(dir)?.sync_all()
}

fn no_parent() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no parent")
}

pub fn remove_dir_if_exists(dir: &Path) -> Result<()> {
    match fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(MarketplaceError::io(format!("remove {}", dir.display()), e)),
    }
}

/// Copy the regular files and directories under `from` into `to`. A symlink
/// or any other file type is rejected; no link is ever followed.
pub fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    copy_tree_at(from, to, "")
}

fn copy_tree_at(from: &Path, to: &Path, rel: &str) -> Result<()> {
    fs::create_dir_all(to).map_err(|e| MarketplaceError::io(to.display().to_string(), e))?;
    let entries =
        fs::read_dir(from).map_err(|e| MarketplaceError::io(from.display().to_string(), e))?;
    for entry in entries {
        let entry = entry.map_err(|e| MarketplaceError::io(from.display().to_string(), e))?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            return Err(MarketplaceError::Traversal(PathBuf::from(rel).join(name)));
        };
        let child_rel = format!("{rel}{name}");
        let meta = fs::symlink_metadata(entry.path())
            .map_err(|e| MarketplaceError::io(child_rel.clone(), e))?;
        let target = to.join(name);
        if meta.file_type().is_symlink() {
            return Err(MarketplaceError::Symlink(PathBuf::from(child_rel)));
        } else if meta.is_dir() {
            copy_tree_at(&entry.path(), &target, &format!("{child_rel}/"))?;
        } else if meta.is_file() {
            fs::copy(entry.path(), &target)
                .map_err(|e| MarketplaceError::io(format!("copy {child_rel}"), e))?;
        } else {
            return Err(MarketplaceError::NotRegularFile(PathBuf::from(child_rel)));
        }
    }
    Ok(())
}

/// The current UTC time as RFC 3339 with second precision.
pub fn now_rfc3339() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Howard Hinnant's days-to-civil algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_728), (2026, 10, 2));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
    }

    #[test]
    fn random_names_differ() {
        assert_ne!(random_hex(), random_hex());
        assert_eq!(random_hex().len(), 32);
    }
}
