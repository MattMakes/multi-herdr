//! Durable file writes and a cross-process directory lock.
//!
//! Every write that must survive a crash goes through here: a temp file in
//! the same directory, `sync_all`, `rename`, then an fsync of the parent
//! directory so the rename itself is on disk. [`create_immutable`] is the
//! write-once variant for content-addressed files.
//!
//! `DirLock` generalizes the ledger's mkdir spinlock and the telemetry pid
//! lock: `create_dir` is atomic on every platform, the holder records who it
//! is in an `owner` file, and a lock whose owner is dead or that is older
//! than `stale_after` is broken.

use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Mode for private files (owner read and write).
pub const PRIVATE_FILE: u32 = 0o600;
/// Mode for private directories (owner only).
pub(crate) const PRIVATE_DIR: u32 = 0o700;

/// What can go wrong in this module.
#[derive(Debug)]
pub enum FsxError {
    /// An I/O call failed on `path`.
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    /// `create_immutable` found `path` with different content.
    Conflict { path: PathBuf },
    /// `path` has no parent directory or no file name.
    BadPath { path: PathBuf },
    /// `DirLock::acquire` gave up after `waited`.
    LockTimeout { path: PathBuf, waited: Duration },
}

impl fmt::Display for FsxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FsxError::Io { path, source } => write!(f, "{}: {source}", path.display()),
            FsxError::Conflict { path } => write!(
                f,
                "{} already exists with different content",
                path.display()
            ),
            FsxError::BadPath { path } => {
                write!(f, "{} has no parent directory or file name", path.display())
            }
            FsxError::LockTimeout { path, waited } => write!(
                f,
                "timed out after {:.1}s waiting for lock {}",
                waited.as_secs_f64(),
                path.display()
            ),
        }
    }
}

impl std::error::Error for FsxError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            FsxError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Result type of this module.
pub type Result<T> = std::result::Result<T, FsxError>;

fn io<T>(path: &Path, r: std::io::Result<T>) -> Result<T> {
    r.map_err(|source| FsxError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn parent_and_name(path: &Path) -> Result<(&Path, String)> {
    let bad = || FsxError::BadPath {
        path: path.to_path_buf(),
    };
    let name = path
        .file_name()
        .ok_or_else(bad)?
        .to_string_lossy()
        .into_owned();
    let parent = match path.parent() {
        Some(p) if p.as_os_str().is_empty() => Path::new("."),
        Some(p) => p,
        None => return Err(bad()),
    };
    Ok((parent, name))
}

fn set_mode(path: &Path, mode: u32) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        io(
            path,
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)),
        )
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
        Ok(())
    }
}

/// Create a new file that is private from its first moment: on unix the
/// mode is set by `open`, so the file never exists with umask permissions.
fn create_new_file(path: &Path, mode: u32) -> std::io::Result<std::fs::File> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;
    o.open(path)
}

/// Fsync a directory so a rename or create inside it is durable. Windows
/// cannot open a directory for sync; there it is a no-op.
fn sync_dir(dir: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let f = io(dir, std::fs::File::open(dir))?;
        io(dir, f.sync_all())
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        Ok(())
    }
}

/// Replace `path` with `bytes` atomically and durably, with file mode `mode`.
/// This is the "replace" discipline of
/// `ai_docs/designs/2026-10-02-dataset-competition-design.md` §2.3 (MEA-09).
pub fn write_atomic(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    let (parent, name) = parent_and_name(path)?;
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let tmp = parent.join(format!(
        ".{name}.tmp-{}-{}",
        std::process::id(),
        &nonce[..12]
    ));
    let written = (|| {
        let mut f = io(&tmp, create_new_file(&tmp, mode))?;
        // The umask may have cleared bits the caller asked for.
        set_mode(&tmp, mode)?;
        io(&tmp, f.write_all(bytes))?;
        io(&tmp, f.sync_all())?;
        drop(f);
        io(path, std::fs::rename(&tmp, path))
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written?;
    sync_dir(parent)
}

/// What [`create_immutable`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Created {
    /// The file did not exist and is now written.
    Written,
    /// The file already existed with exactly these bytes.
    AlreadyIdentical,
}

/// Write `path` once. An existing file with the same bytes is accepted; one
/// with different bytes is a [`FsxError::Conflict`].
pub fn create_immutable(path: &Path, bytes: &[u8], mode: u32) -> Result<Created> {
    let (parent, _) = parent_and_name(path)?;
    let mut f = match create_new_file(path, mode) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let existing = io(path, std::fs::read(path))?;
            return if existing == bytes {
                Ok(Created::AlreadyIdentical)
            } else {
                Err(FsxError::Conflict {
                    path: path.to_path_buf(),
                })
            };
        }
        Err(e) => return io(path, Err(e)),
    };
    let written = (|| {
        set_mode(path, mode)?;
        io(path, f.write_all(bytes))?;
        io(path, f.sync_all())
    })();
    if written.is_err() {
        // A half-written immutable file would read as a conflict forever.
        drop(f);
        let _ = std::fs::remove_file(path);
    }
    written?;
    sync_dir(parent)?;
    Ok(Created::Written)
}

/// Create `path` and its parents; set `path` itself to mode 0700 on unix.
pub fn ensure_private_dir(path: &Path) -> Result<()> {
    io(path, std::fs::create_dir_all(path))?;
    set_mode(path, PRIVATE_DIR)
}

/// Whether process `pid` exists. A pid alone can name a later program:
/// `DirLock` and the telemetry collector lock decide liveness with
/// [`crate::procid::alive`], which also checks the start time.
pub fn pid_alive(pid: u32) -> bool {
    // 0 and values above `i32::MAX` name a group or every process, not one.
    let Some(os) = crate::procid::os_pid(pid) else {
        return false;
    };
    #[cfg(unix)]
    {
        // Signal 0 checks existence and permission without sending anything.
        let r = unsafe { libc::kill(os, 0) };
        r == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(windows)]
    {
        let _ = os;
        std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
            .unwrap_or(false)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = os;
        true
    }
}

/// This machine's host name, from the OS rather than the environment.
fn host_name() -> String {
    #[cfg(unix)]
    {
        let mut buf = [0u8; 256];
        let r = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
        if r != 0 {
            return String::new();
        }
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        String::from_utf8_lossy(&buf[..end]).into_owned()
    }
    #[cfg(not(unix))]
    {
        String::new()
    }
}

/// The holder of a [`DirLock`], as written to `<name>.lock/owner`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct LockOwner {
    /// Process id of the holder.
    pub pid: u32,
    /// Host name of the holder; a pid is only checked on the same host.
    pub host: String,
    /// RFC 3339 UTC time the lock was taken.
    pub acquired_at: String,
    /// Random per acquisition, so a holder only ever releases its own lock.
    #[serde(default)]
    pub nonce: String,
    /// The holder's [`crate::procid::start_time`]: a pid that now names
    /// another process is a dead holder. `None` in an owner file written
    /// before it existed; then the pid alone decides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started: Option<u64>,
}

/// A cross-process lock at `<dir>/<name>.lock/`.
pub(crate) struct DirLock;

/// The held lock. Dropping it releases the lock.
#[derive(Debug)]
pub struct DirLockGuard {
    path: PathBuf,
    /// `None` until the owner file is written.
    nonce: Option<String>,
}

impl DirLockGuard {
    /// The lock directory.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Release now (the same as dropping it).
    pub fn release(self) {}
}

impl Drop for DirLockGuard {
    fn drop(&mut self) {
        // A holder that outlived `stale_after` may have had its lock broken
        // and taken by another process; that lock is not ours to remove.
        if let Some(nonce) = &self.nonce {
            if read_owner(&self.path).is_none_or(|o| &o.nonce != nonce) {
                return;
            }
        }
        remove_lock(&self.path);
    }
}

fn read_owner(lock: &Path) -> Option<LockOwner> {
    std::fs::read(lock.join("owner"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
}

/// Remove a lock directory. It is renamed first, so a process that takes
/// the lock in the meantime is not removed with it.
fn remove_lock(path: &Path) -> bool {
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let mut tomb = path.as_os_str().to_owned();
    tomb.push(format!(".gone-{}", &nonce[..12]));
    let tomb = PathBuf::from(tomb);
    let moved = std::fs::rename(path, &tomb).is_ok();
    if moved {
        let _ = std::fs::remove_dir_all(&tomb);
    }
    moved
}

/// How long a lock without an `owner` file counts as being set up.
const OWNERLESS_GRACE: Duration = Duration::from_secs(5);

impl DirLock {
    /// Take `<dir>/<name>.lock/`, waiting up to `timeout`. A lock whose owner
    /// pid is dead on this host, or that is older than `stale_after`, is
    /// broken.
    pub(crate) fn acquire(
        dir: &Path,
        name: &str,
        stale_after: Duration,
        timeout: Duration,
    ) -> Result<DirLockGuard> {
        io(dir, std::fs::create_dir_all(dir))?;
        let path = dir.join(format!("{name}.lock"));
        let start = Instant::now();
        let mut backoff = Duration::from_millis(5);
        loop {
            match std::fs::create_dir(&path) {
                Ok(()) => {
                    let mut guard = DirLockGuard { path, nonce: None };
                    let owner = LockOwner {
                        pid: std::process::id(),
                        host: host_name(),
                        acquired_at: crate::clock::stamp(chrono::Utc::now()),
                        nonce: uuid::Uuid::new_v4().simple().to_string(),
                        started: crate::procid::start_time(std::process::id()),
                    };
                    let json = serde_json::to_vec(&owner).expect("LockOwner serializes");
                    write_atomic(&guard.path.join("owner"), &json, PRIVATE_FILE)?;
                    guard.nonce = Some(owner.nonce);
                    return Ok(guard);
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if is_stale(&path, stale_after) && break_lock(&path, stale_after) {
                        continue;
                    }
                }
                Err(e) => return io(&path, Err(e)),
            }
            let waited = start.elapsed();
            if waited >= timeout {
                return Err(FsxError::LockTimeout { path, waited });
            }
            std::thread::sleep(backoff.min(timeout - waited));
            backoff = (backoff * 2).min(Duration::from_millis(100));
        }
    }
}

/// Whether the lock at `path` may be broken.
fn is_stale(path: &Path, stale_after: Duration) -> bool {
    let age = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok());
    if age.is_some_and(|a| a >= stale_after) {
        return true;
    }
    match read_owner(path) {
        // A pid on another host cannot be checked from here.
        Some(o) => o.host == host_name() && !crate::procid::alive(o.pid, o.started),
        // A holder between `create_dir` and writing its owner is live for a
        // moment; only an old, ownerless lock is stale.
        None => age.is_some_and(|a| a >= OWNERLESS_GRACE),
    }
}

/// How long a breaker directory may live. Breaking takes microseconds, so
/// an older one was left by a killed process and is removed.
const BREAKER_STALE_AFTER: Duration = Duration::from_secs(5);

/// An exclusive `flock` on the directory of a lock, held while one process
/// breaks that lock. The kernel drops it when the process dies, so unlike a
/// breaker directory it is never left behind.
struct BreakerGuard {
    _dir: Option<std::fs::File>,
}

impl BreakerGuard {
    /// Take the guard of the lock at `lock`. `None` while another process
    /// holds it. Where `flock` is not available (not unix, or a filesystem
    /// without it) the guard holds nothing and the breaker directory alone
    /// excludes, as before the guard.
    fn take(lock: &Path) -> Option<BreakerGuard> {
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let Some(dir) = lock.parent().and_then(|d| std::fs::File::open(d).ok()) else {
                return Some(BreakerGuard { _dir: None });
            };
            if unsafe { libc::flock(dir.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                return Some(BreakerGuard { _dir: Some(dir) });
            }
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::EWOULDBLOCK) {
                return None;
            }
        }
        #[cfg(not(unix))]
        let _ = lock;
        Some(BreakerGuard { _dir: None })
    }
}

/// Remove a stale lock, and say so. Returns whether this call removed it.
///
/// Two processes can both see the same stale lock. Without coordination the
/// slower one removes the lock the faster one has just taken in its place.
/// So a breaker first takes the [`BreakerGuard`], then `<name>.lock.break/`,
/// and checks again that the lock is stale; a process that cannot take
/// either waits and retries.
///
/// A breaker killed inside its window leaves `<name>.lock.break/`. It is
/// removed once it is older than [`BREAKER_STALE_AFTER`], and only under the
/// guard: 2 processes that removed it at once could remove a fresh breaker
/// a third had just made, and 2 breakers would run again. The directory
/// stays so that binaries from before the guard are still excluded.
fn break_lock(path: &Path, stale_after: Duration) -> bool {
    let Some(_guard) = BreakerGuard::take(path) else {
        return false;
    };
    let mut breaker = path.as_os_str().to_owned();
    breaker.push(".break");
    let breaker = PathBuf::from(breaker);
    if std::fs::create_dir(&breaker).is_err() {
        let age = std::fs::metadata(&breaker)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok());
        if age.is_some_and(|a| a >= BREAKER_STALE_AFTER) {
            let _ = std::fs::remove_dir(&breaker);
        }
        return false;
    }
    let broke = is_stale(path, stale_after) && remove_lock(path);
    let _ = std::fs::remove_dir(&breaker);
    if broke {
        eprintln!("horch: broke stale lock {}", path.display());
    }
    broke
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mea_09_create_immutable_refuses_overwrite() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("blob.json");
        assert_eq!(
            create_immutable(&p, b"one", PRIVATE_FILE).unwrap(),
            Created::Written
        );
        assert_eq!(
            create_immutable(&p, b"one", PRIVATE_FILE).unwrap(),
            Created::AlreadyIdentical
        );
        let err = create_immutable(&p, b"two", PRIVATE_FILE).unwrap_err();
        assert!(matches!(err, FsxError::Conflict { .. }), "{err}");
        assert_eq!(std::fs::read(&p).unwrap(), b"one");
    }

    #[test]
    fn mea_09_write_atomic_replaces_durably() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("state.json");
        write_atomic(&p, b"first", PRIVATE_FILE).unwrap();
        write_atomic(&p, b"second", PRIVATE_FILE).unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"second");
        let names: Vec<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["state.json"], "no temp file is left behind");
    }

    #[cfg(unix)]
    #[test]
    fn sec_05_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("a").join("dataset");
        ensure_private_dir(&dir).unwrap();
        assert_eq!(mode(&dir), 0o700);
        let replaced = dir.join("r.json");
        write_atomic(&replaced, b"x", PRIVATE_FILE).unwrap();
        assert_eq!(mode(&replaced), 0o600);
        let created = dir.join("c.json");
        create_immutable(&created, b"x", PRIVATE_FILE).unwrap();
        assert_eq!(mode(&created), 0o600);
        let lock = DirLock::acquire(
            &dir,
            "events",
            Duration::from_secs(60),
            Duration::from_secs(1),
        )
        .unwrap();
        assert_eq!(mode(&lock.path().join("owner")), 0o600);
    }

    #[test]
    fn dirlock_excludes_second_holder() {
        let tmp = tempfile::tempdir().unwrap();
        let stale = Duration::from_secs(60);
        let held = DirLock::acquire(tmp.path(), "events", stale, Duration::from_secs(1)).unwrap();
        let owner: LockOwner =
            serde_json::from_slice(&std::fs::read(held.path().join("owner")).unwrap()).unwrap();
        assert_eq!(owner.pid, std::process::id());
        let err =
            DirLock::acquire(tmp.path(), "events", stale, Duration::from_millis(50)).unwrap_err();
        assert!(matches!(err, FsxError::LockTimeout { .. }), "{err}");
        drop(held);
        assert!(!tmp.path().join("events.lock").exists(), "released on drop");
        DirLock::acquire(tmp.path(), "events", stale, Duration::from_secs(1))
            .expect("free again after release");
    }

    #[test]
    fn dirlock_breaks_dead_owner() {
        let tmp = tempfile::tempdir().unwrap();
        let lock = tmp.path().join("events.lock");
        std::fs::create_dir(&lock).unwrap();
        // A pid that cannot be running: beyond every platform's pid range.
        let dead = LockOwner {
            pid: 999_999_999,
            host: host_name(),
            acquired_at: "2026-09-28T17:00:00Z".into(),
            nonce: "dead".into(),
            started: None,
        };
        std::fs::write(lock.join("owner"), serde_json::to_vec(&dead).unwrap()).unwrap();
        let held = DirLock::acquire(
            tmp.path(),
            "events",
            Duration::from_secs(3600),
            Duration::from_secs(1),
        )
        .expect("the dead owner's lock is broken");
        let owner: LockOwner =
            serde_json::from_slice(&std::fs::read(held.path().join("owner")).unwrap()).unwrap();
        assert_eq!(owner.pid, std::process::id());
    }

    /// The owner's pid runs (it is this process) but with another start
    /// time: the pid was given to a later program, so the owner is dead and
    /// the lock is broken. An owner file without a start time keeps the old
    /// rule: the pid runs, so the lock is held.
    #[test]
    fn dirlock_breaks_an_owner_whose_pid_was_reused() {
        let tmp = tempfile::tempdir().unwrap();
        let lock = tmp.path().join("events.lock");
        let me = std::process::id();
        let owner = |started| LockOwner {
            pid: me,
            host: host_name(),
            acquired_at: "2026-09-28T17:00:00Z".into(),
            nonce: "old".into(),
            started,
        };
        std::fs::create_dir(&lock).unwrap();
        std::fs::write(
            lock.join("owner"),
            serde_json::to_vec(&owner(None)).unwrap(),
        )
        .unwrap();
        let hour = Duration::from_secs(3600);
        assert!(!is_stale(&lock, hour), "an old owner file: the pid decides");
        let Some(mine) = crate::procid::start_time(me) else {
            return; // No start times on this platform.
        };
        std::fs::write(
            lock.join("owner"),
            serde_json::to_vec(&owner(Some(mine))).unwrap(),
        )
        .unwrap();
        assert!(!is_stale(&lock, hour), "the same process holds it");
        std::fs::write(
            lock.join("owner"),
            serde_json::to_vec(&owner(Some(mine + 1))).unwrap(),
        )
        .unwrap();
        assert!(is_stale(&lock, hour), "the pid now names another process");
        let held = DirLock::acquire(tmp.path(), "events", hour, Duration::from_secs(1))
            .expect("the reused pid's lock is broken");
        let new: LockOwner =
            serde_json::from_slice(&std::fs::read(held.path().join("owner")).unwrap()).unwrap();
        assert_eq!(new.started, Some(mine));
    }

    #[test]
    fn dirlock_stale_holder_does_not_release_the_next_holder() {
        let tmp = tempfile::tempdir().unwrap();
        let first =
            DirLock::acquire(tmp.path(), "events", Duration::ZERO, Duration::from_secs(1)).unwrap();
        // With stale_after zero, the first lock is stale at once.
        let second =
            DirLock::acquire(tmp.path(), "events", Duration::ZERO, Duration::from_secs(1)).unwrap();
        drop(first);
        assert!(
            second.path().join("owner").is_file(),
            "the second holder keeps its lock"
        );
        drop(second);
        assert!(!tmp.path().join("events.lock").exists());
    }

    /// Many processes find one dead owner's lock at once. Only one of them
    /// breaks it, so no new holder loses its lock to a second breaker. In
    /// every other round a killed breaker's directory is there too: only
    /// one process removes it, so 2 breakers never run at once.
    #[test]
    fn dirlock_concurrent_breakers_keep_one_holder() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        for round in 0..10 {
            let tmp = tempfile::tempdir().unwrap();
            let lock = tmp.path().join("events.lock");
            std::fs::create_dir(&lock).unwrap();
            let dead = LockOwner {
                pid: 999_999_999,
                host: host_name(),
                acquired_at: "2026-09-28T17:00:00Z".into(),
                nonce: "dead".into(),
                started: None,
            };
            std::fs::write(lock.join("owner"), serde_json::to_vec(&dead).unwrap()).unwrap();
            #[cfg(unix)]
            if round % 2 == 1 {
                let breaker = tmp.path().join("events.lock.break");
                std::fs::create_dir(&breaker).unwrap();
                let old = std::time::SystemTime::now() - BREAKER_STALE_AFTER * 2;
                std::fs::File::open(&breaker)
                    .unwrap()
                    .set_modified(old)
                    .unwrap();
            }
            let inside = Arc::new(AtomicUsize::new(0));
            let most = Arc::new(AtomicUsize::new(0));
            let threads: Vec<_> = (0..8)
                .map(|_| {
                    let dir = tmp.path().to_path_buf();
                    let (inside, most) = (inside.clone(), most.clone());
                    std::thread::spawn(move || {
                        let held = DirLock::acquire(
                            &dir,
                            "events",
                            Duration::from_secs(3600),
                            Duration::from_secs(20),
                        )?;
                        let now = inside.fetch_add(1, Ordering::SeqCst) + 1;
                        most.fetch_max(now, Ordering::SeqCst);
                        std::thread::sleep(Duration::from_millis(1));
                        inside.fetch_sub(1, Ordering::SeqCst);
                        drop(held);
                        Ok::<(), FsxError>(())
                    })
                })
                .collect();
            for t in threads {
                t.join().unwrap().expect("every thread gets the lock");
            }
            assert_eq!(most.load(Ordering::SeqCst), 1, "one holder at a time");
        }
    }

    /// A breaker directory left by a killed process is removed once it is
    /// older than its own stale limit, so it never blocks the lock.
    #[cfg(unix)]
    #[test]
    fn dirlock_left_breaker_does_not_block() {
        let tmp = tempfile::tempdir().unwrap();
        let lock = tmp.path().join("events.lock");
        std::fs::create_dir(&lock).unwrap();
        let dead = LockOwner {
            pid: 999_999_999,
            host: host_name(),
            acquired_at: "2026-09-28T17:00:00Z".into(),
            nonce: "dead".into(),
            started: None,
        };
        std::fs::write(lock.join("owner"), serde_json::to_vec(&dead).unwrap()).unwrap();
        let breaker = tmp.path().join("events.lock.break");
        std::fs::create_dir(&breaker).unwrap();
        let old = std::time::SystemTime::now() - BREAKER_STALE_AFTER * 2;
        std::fs::File::open(&breaker)
            .unwrap()
            .set_modified(old)
            .unwrap();
        DirLock::acquire(
            tmp.path(),
            "events",
            Duration::from_secs(3600),
            Duration::from_secs(2),
        )
        .expect("the left breaker is removed and the dead lock broken");
        assert!(!breaker.exists());
    }

    /// A breaker paused inside its window for longer than
    /// [`BREAKER_STALE_AFTER`] still holds the guard: no other process
    /// removes its breaker directory or breaks the lock a second time.
    #[cfg(unix)]
    #[test]
    fn dirlock_paused_breaker_blocks_other_breakers() {
        let tmp = tempfile::tempdir().unwrap();
        let lock = tmp.path().join("events.lock");
        std::fs::create_dir(&lock).unwrap();
        let dead = LockOwner {
            pid: 999_999_999,
            host: host_name(),
            acquired_at: "2026-09-28T17:00:00Z".into(),
            nonce: "dead".into(),
            started: None,
        };
        std::fs::write(lock.join("owner"), serde_json::to_vec(&dead).unwrap()).unwrap();
        let breaker = tmp.path().join("events.lock.break");
        std::fs::create_dir(&breaker).unwrap();
        let old = std::time::SystemTime::now() - BREAKER_STALE_AFTER * 2;
        std::fs::File::open(&breaker)
            .unwrap()
            .set_modified(old)
            .unwrap();
        let stale = Duration::from_secs(3600);

        let paused = BreakerGuard::take(&lock).expect("the guard is free");
        for _ in 0..3 {
            assert!(!break_lock(&lock, stale));
        }
        assert!(breaker.exists(), "the paused breaker keeps its directory");
        assert!(lock.join("owner").is_file(), "the lock is not broken twice");

        drop(paused);
        assert!(
            !break_lock(&lock, stale),
            "the left breaker is removed first"
        );
        assert!(!breaker.exists());
        assert!(break_lock(&lock, stale), "then the dead lock is broken");
        assert!(!lock.exists());
    }

    #[test]
    fn dirlock_breaks_old_lock() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("events.lock")).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        DirLock::acquire(
            tmp.path(),
            "events",
            Duration::from_millis(10),
            Duration::from_secs(1),
        )
        .expect("a lock older than stale_after is broken");
    }
}
