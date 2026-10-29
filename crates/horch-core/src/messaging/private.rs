//! Messaging dirs and files only this user can read or change (#77).
//!
//! The registry, briefs, spool and mail hold task text, reports and the
//! binaries a worker runs. A dir is created mode 0700 and a file mode 0600.
//! An existing dir that another user owns, or that others can write to, is
//! refused: whoever can write it can plant a brief or an `orchestrator.id`.

use std::path::Path;

use anyhow::{bail, Context, Result};

/// The owner and the permission bits of a dir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Facts {
    pub uid: u32,
    pub mode: u32,
}

/// Refuse a dir that `me` does not own, or that group or others can write.
pub(crate) fn judge(path: &Path, facts: Facts, me: u32) -> Result<()> {
    if facts.uid != me {
        bail!(
            "{} is owned by uid {}, not by this user (uid {me}); horch does not use it",
            path.display(),
            facts.uid
        );
    }
    if facts.mode & 0o022 != 0 {
        bail!(
            "{} is writable by other users (mode {:o}); horch does not use it",
            path.display(),
            facts.mode & 0o777
        );
    }
    Ok(())
}

#[cfg(unix)]
fn facts(path: &Path) -> std::io::Result<Facts> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(path)?;
    Ok(Facts {
        uid: meta.uid(),
        mode: meta.mode(),
    })
}

#[cfg(unix)]
fn me() -> u32 {
    // SAFETY: getuid has no preconditions and cannot fail.
    unsafe { libc::getuid() }
}

/// [`judge`] the existing dir `path`. A missing dir passes.
pub(crate) fn check(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        match facts(path) {
            Ok(facts) => judge(path, facts, me()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

/// Create `path` and its missing parents mode 0700, or check the existing
/// dir and make it 0700.
pub(crate) fn ensure_dir(path: &Path) -> Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .with_context(|| format!("creating {}", path.display()))?;
    check(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .with_context(|| format!("setting the mode of {}", path.display()))?;
    }
    Ok(())
}

/// Replace `path` with `bytes`, mode 0600, atomically.
pub(crate) fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    crate::fsx::write_atomic(path, bytes, crate::fsx::PRIVATE_FILE)
        .with_context(|| format!("writing {}", path.display()))
}

/// Create the new file `path` mode 0600; it must not exist.
pub(crate) fn create_new(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dir_of_another_user_is_refused_with_its_path() {
        let path = Path::new("/state/herdr-orchestration-w1");
        let err = judge(
            path,
            Facts {
                uid: 501,
                mode: 0o40700,
            },
            502,
        )
        .unwrap_err();
        let text = format!("{err:#}");
        assert!(text.contains("/state/herdr-orchestration-w1"), "{text}");
        assert!(text.contains("owned by uid 501"), "{text}");
    }

    #[test]
    fn a_dir_others_can_write_is_refused_with_its_path() {
        let path = Path::new("/tmp/herdr-orchestration-w7");
        for mode in [0o40777, 0o40770, 0o40702] {
            let err = judge(path, Facts { uid: 7, mode }, 7).unwrap_err();
            let text = format!("{err:#}");
            assert!(text.contains("/tmp/herdr-orchestration-w7"), "{text}");
            assert!(text.contains("writable by other users"), "{text}");
        }
    }

    #[test]
    fn an_own_dir_others_cannot_write_is_accepted() {
        let path = Path::new("/state/mail/w1");
        for mode in [0o40700, 0o40755, 0o40750] {
            judge(path, Facts { uid: 7, mode }, 7).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn ensure_dir_creates_0700_and_tightens_an_own_0755_dir() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let new = tmp.path().join("a/b");
        ensure_dir(&new).unwrap();
        assert_eq!(mode(&new), 0o700);
        assert_eq!(mode(&tmp.path().join("a")), 0o700);

        let old = tmp.path().join("old");
        std::fs::create_dir(&old).unwrap();
        std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o755)).unwrap();
        ensure_dir(&old).unwrap();
        assert_eq!(mode(&old), 0o700);

        let open = tmp.path().join("open");
        std::fs::create_dir(&open).unwrap();
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o777)).unwrap();
        let err = ensure_dir(&open).unwrap_err();
        assert!(format!("{err:#}").contains("writable by other users"));
        assert_eq!(mode(&open), 0o777, "a refused dir is left as it is");
    }

    #[cfg(unix)]
    #[test]
    fn private_files_are_0600() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let a = tmp.path().join("a");
        write(&a, b"x").unwrap();
        assert_eq!(mode(&a), 0o600);
        let b = tmp.path().join("b");
        create_new(&b).unwrap();
        assert_eq!(mode(&b), 0o600);
    }
}
