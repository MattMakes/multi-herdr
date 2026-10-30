//! Messaging dirs and files only this user can read or change (#77).
//!
//! The registry, briefs, spool and mail hold task text, reports and the
//! binaries a worker runs. A dir is created mode 0700 and a file mode 0600.
//! An existing dir that another user owns, or that others can write to, is
//! refused: whoever can write it can plant a brief or an `orchestrator.id`.
//! An own dir that only its group can write (umask 002 made it so) is made
//! 0700 and used.

use std::path::Path;

use anyhow::{bail, Context, Result};

/// The owner and the permission bits of a dir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Facts {
    pub uid: u32,
    pub mode: u32,
}

/// What to do with an existing dir that passed [`judge`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Mode 0700 already.
    Private,
    /// This user's dir with other bits set, but others cannot write it:
    /// set it to 0700.
    Tighten,
}

/// Refuse a dir that `me` does not own, or that others can write.
pub(crate) fn judge(path: &Path, facts: Facts, me: u32) -> Result<Verdict> {
    if facts.uid != me {
        bail!(
            "{} is owned by uid {}, not by this user (uid {me}); horch does not use it",
            path.display(),
            facts.uid
        );
    }
    if facts.mode & 0o002 != 0 {
        bail!(
            "{} is writable by other users (mode {:o}); horch does not use it; \
             run chmod go-w {} if you trust its files",
            path.display(),
            facts.mode & 0o777,
            path.display()
        );
    }
    Ok(if facts.mode & 0o777 == 0o700 {
        Verdict::Private
    } else {
        Verdict::Tighten
    })
}

/// [`judge`] the old temp dir `path` more strictly: it is in a dir all
/// users share, so it must be a real dir, not a link, and its group must
/// not write it either. It is never changed.
pub(crate) fn judge_shared(path: &Path, facts: Facts, is_link: bool, me: u32) -> Result<()> {
    if is_link {
        bail!("{} is a link; horch does not follow it", path.display());
    }
    judge(path, facts, me)?;
    if facts.mode & 0o020 != 0 {
        bail!(
            "{} is writable by its group (mode {:o}); horch does not use it; \
             run chmod go-w {} if you trust its files",
            path.display(),
            facts.mode & 0o777,
            path.display()
        );
    }
    Ok(())
}

#[cfg(unix)]
fn facts(meta: &std::fs::Metadata) -> Facts {
    use std::os::unix::fs::MetadataExt;
    Facts {
        uid: meta.uid(),
        mode: meta.mode(),
    }
}

#[cfg(unix)]
fn me() -> u32 {
    // SAFETY: getuid has no preconditions and cannot fail.
    unsafe { libc::getuid() }
}

/// [`judge`] the existing dir `path`, and make an own dir 0700. A missing
/// dir passes.
pub(crate) fn check(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = match std::fs::metadata(path) {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        if judge(path, facts(&meta), me())? == Verdict::Tighten {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
                .with_context(|| format!("setting the mode of {}", path.display()))?;
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

/// [`judge_shared`] the existing old temp dir `path`, without following a
/// link.
pub(crate) fn check_shared(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let meta = std::fs::symlink_metadata(path)
            .with_context(|| format!("reading {}", path.display()))?;
        let is_link = meta.file_type().is_symlink();
        judge_shared(path, facts(&meta), is_link, me())?;
        if !meta.is_dir() {
            bail!("{} is not a dir", path.display());
        }
        Ok(())
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
    check(path)
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
        for mode in [0o40777, 0o40702, 0o40703] {
            let err = judge(path, Facts { uid: 7, mode }, 7).unwrap_err();
            let text = format!("{err:#}");
            assert!(text.contains("/tmp/herdr-orchestration-w7"), "{text}");
            assert!(text.contains("writable by other users"), "{text}");
            assert!(
                text.contains("chmod go-w /tmp/herdr-orchestration-w7"),
                "{text}"
            );
        }
    }

    /// An own dir that only its group can write is the umask 002 default of
    /// earlier releases: it is made 0700, not refused.
    #[test]
    fn an_own_dir_others_cannot_write_is_accepted() {
        let path = Path::new("/state/mail/w1");
        assert_eq!(
            judge(
                path,
                Facts {
                    uid: 7,
                    mode: 0o40700
                },
                7
            )
            .unwrap(),
            Verdict::Private
        );
        for mode in [0o40755, 0o40750, 0o40775, 0o40770] {
            assert_eq!(
                judge(path, Facts { uid: 7, mode }, 7).unwrap(),
                Verdict::Tighten,
                "{mode:o}"
            );
        }
    }

    /// The old temp dir is in a dir all users share: a link, or a dir its
    /// group can write, is refused there.
    #[test]
    fn a_shared_dir_must_be_a_real_dir_its_group_cannot_write() {
        let path = Path::new("/tmp/herdr-orchestration-w1");
        let own = |mode| Facts { uid: 7, mode };
        judge_shared(path, own(0o40700), false, 7).unwrap();
        judge_shared(path, own(0o40755), false, 7).unwrap();
        let link = format!(
            "{:#}",
            judge_shared(path, own(0o40700), true, 7).unwrap_err()
        );
        assert!(
            link.contains("/tmp/herdr-orchestration-w1 is a link"),
            "{link}"
        );
        let group = format!(
            "{:#}",
            judge_shared(path, own(0o40775), false, 7).unwrap_err()
        );
        assert!(group.contains("writable by its group"), "{group}");
        let other = format!(
            "{:#}",
            judge_shared(path, own(0o40700), false, 8).unwrap_err()
        );
        assert!(other.contains("owned by uid 7"), "{other}");
    }

    #[cfg(unix)]
    #[test]
    fn check_shared_does_not_follow_a_link_to_an_own_private_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("real");
        ensure_dir(&real).unwrap();
        check_shared(&real).unwrap();
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let err = format!("{:#}", check_shared(&link).unwrap_err());
        assert!(err.contains("is a link"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn ensure_dir_creates_0700_and_tightens_an_own_0755_or_0775_dir() {
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

        // umask 002 made the dirs of earlier releases 0775.
        let group = tmp.path().join("group");
        std::fs::create_dir(&group).unwrap();
        std::fs::set_permissions(&group, std::fs::Permissions::from_mode(0o775)).unwrap();
        ensure_dir(&group).unwrap();
        assert_eq!(mode(&group), 0o700);
        let read = tmp.path().join("read");
        std::fs::create_dir(&read).unwrap();
        std::fs::set_permissions(&read, std::fs::Permissions::from_mode(0o770)).unwrap();
        check(&read).unwrap();
        assert_eq!(mode(&read), 0o700, "a read check makes an own dir 0700 too");

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
