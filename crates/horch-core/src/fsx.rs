use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use horch_platform::fs::{rename_noreplace, BreakerGuard};

pub const PRIVATE_FILE: u32 = 0o600;
pub(crate) const PRIVATE_DIR: u32 = 0o700;

#[derive(Debug)]
pub enum FsxError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Conflict {
        path: PathBuf,
    },
    BadPath {
        path: PathBuf,
    },
    LockTimeout {
        path: PathBuf,
        waited: Duration,
    },
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
    match horch_platform::fs::set_mode(path, mode) {
        Err(e) if is_not_supported(&e) => Ok(()),
        r => io(path, r),
    }
}

fn is_not_supported(e: &std::io::Error) -> bool {
    e.get_ref()
        .is_some_and(|inner| inner.is::<horch_platform::NotSupported>())
}

fn create_new_file(path: &Path, mode: u32) -> std::io::Result<std::fs::File> {
    if mode & 0o077 == 0 {
        return horch_platform::fs::create_new_owner_only(path);
    }
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    let _ = horch_platform::fs::create_mode(&mut o, mode);
    o.open(path)
}

fn sync_dir(dir: &Path) -> Result<()> {
    match horch_platform::fs::sync_dir(dir) {
        Err(e) if is_not_supported(&e) => Ok(()),
        r => io(dir, r),
    }
}

pub(crate) const READ_LIMIT: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refused {
    NotRegular,
    TooLarge,
}

impl Refused {
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Refused::NotRegular => "not a regular file",
            Refused::TooLarge => "over 1 MiB",
        }
    }
}

pub(crate) fn read_regular_bounded(path: &Path) -> std::result::Result<Option<String>, Refused> {
    match open_regular(path)?.and_then(read_bounded) {
        None => Ok(None),
        Some(bytes) if bytes.len() as u64 > READ_LIMIT => Err(Refused::TooLarge),
        Some(bytes) => Ok(String::from_utf8(bytes).ok()),
    }
}

fn open_regular(path: &Path) -> std::result::Result<Option<std::fs::File>, Refused> {
    let mut o = std::fs::OpenOptions::new();
    o.read(true);
    let _ = horch_platform::fs::open_no_block(&mut o);
    match o
        .open(path)
        .and_then(|file| file.metadata().map(|m| (file, m.is_file())))
    {
        Ok((file, true)) => Ok(Some(file)),
        Ok((_, false)) => Err(Refused::NotRegular),
        Err(_) => Ok(None),
    }
}

fn read_bounded(file: std::fs::File) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    file.take(READ_LIMIT + 1)
        .read_to_end(&mut bytes)
        .ok()
        .map(|_| bytes)
}

pub fn write_atomic(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    let (parent, name) = parent_and_name(path)?;
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let tmp = parent.join(format!(
        ".{name}.tmp-{}-{}",
        std::process::id(),
        &nonce[..12]
    ));
    let written =
        write_temp(&tmp, bytes, mode).and_then(|()| io(path, std::fs::rename(&tmp, path)));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written?;
    sync_dir(parent)
}

fn write_temp(tmp: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    let mut f = io(tmp, create_new_file(tmp, mode))?;
    set_mode(tmp, mode)?;
    io(tmp, f.write_all(bytes))?;
    io(tmp, f.sync_all())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Created {
    Written,
    AlreadyIdentical,
}

pub fn create_immutable(path: &Path, bytes: &[u8], mode: u32) -> Result<Created> {
    create_immutable_with(path, bytes, mode, || {})
}

fn create_immutable_with(
    path: &Path,
    bytes: &[u8],
    mode: u32,
    written: impl FnOnce(),
) -> Result<Created> {
    let (parent, name) = parent_and_name(path)?;
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let tmp = parent.join(format!(
        ".{name}.new-{}-{}",
        std::process::id(),
        &nonce[..12]
    ));
    let linked = write_and_link(&tmp, path, bytes, mode, written);
    let _ = std::fs::remove_file(&tmp);
    match linked? {
        Ok(()) => sync_dir(parent).map(|()| Created::Written),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => compare_existing(path, bytes),
        Err(e) => io(path, Err(e)),
    }
}

fn write_and_link(
    tmp: &Path,
    path: &Path,
    bytes: &[u8],
    mode: u32,
    written: impl FnOnce(),
) -> Result<std::io::Result<()>> {
    write_temp(tmp, bytes, mode)?;
    written();
    Ok(std::fs::hard_link(tmp, path))
}

fn compare_existing(path: &Path, bytes: &[u8]) -> Result<Created> {
    if io(path, std::fs::read(path))? == bytes {
        Ok(Created::AlreadyIdentical)
    } else {
        Err(FsxError::Conflict {
            path: path.to_path_buf(),
        })
    }
}

pub fn ensure_private_dir(path: &Path) -> Result<()> {
    io(path, std::fs::create_dir_all(path))?;
    io(path, horch_platform::fs::create_dir_owner_only(path))?;
    set_mode(path, PRIVATE_DIR)
}

pub fn pid_alive(pid: u32) -> bool {
    horch_platform::process::alive(pid)
}

fn host_name() -> String {
    horch_platform::machine::host_name()
        .ok()
        .flatten()
        .unwrap_or_default()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct LockOwner {
    pub pid: u32,
    pub host: String,
    pub acquired_at: String,
    #[serde(default)]
    pub nonce: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale_after_s: Option<u64>,
}

pub(crate) struct DirLock;

#[derive(Debug)]
pub struct DirLockGuard {
    path: PathBuf,
    nonce: Option<String>,
}

impl DirLockGuard {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn refresh(&self) -> Result<()> {
        let nonce = self
            .nonce
            .as_deref()
            .ok_or_else(|| self.lock_error("lock owner is not initialized"))?;
        self.ensure_owned(nonce, "lock is no longer owned by this guard")?;
        let heartbeat = self.path.join(format!("heartbeat-{nonce}"));
        write_atomic(&heartbeat, b"live\n", PRIVATE_FILE)?;
        self.ensure_owned(nonce, "lock changed while it was refreshed")
            .inspect_err(|_| {
                let _ = std::fs::remove_file(&heartbeat);
            })
    }

    fn ensure_owned(&self, nonce: &str, why: &'static str) -> Result<()> {
        if read_owner(&self.path).is_some_and(|owner| owner.nonce == nonce) {
            Ok(())
        } else {
            Err(self.lock_error(why))
        }
    }

    fn lock_error(&self, why: &'static str) -> FsxError {
        FsxError::Io {
            path: self.path.clone(),
            source: std::io::Error::other(why),
        }
    }

    pub fn release(self) {}
}

impl Drop for DirLockGuard {
    fn drop(&mut self) {
        release_lock(&self.path, self.nonce.as_deref(), BREAKER_STALE_AFTER);
    }
}

fn release_lock(path: &Path, nonce: Option<&str>, guard_wait: Duration) {
    let Some(_breakers_excluded) = wait_for_guard(path, guard_wait) else {
        if let Some(nonce) = nonce {
            mark_released(path, nonce);
        }
        return;
    };
    if let Some(nonce) = nonce {
        if read_owner(path).is_none_or(|o| o.nonce != nonce) {
            return;
        }
    }
    remove_lock(
        path,
        |owner| nonce.is_none() || owner.is_some_and(|o| Some(o.nonce.as_str()) == nonce),
        || {},
    );
}

fn mark_released(path: &Path, nonce: &str) {
    let _ = create_new_file(&released_mark(path, nonce), PRIVATE_FILE);
}

fn released_mark(path: &Path, nonce: &str) -> PathBuf {
    path.join(format!("released-{nonce}"))
}

fn wait_for_guard(lock: &Path, wait: Duration) -> Option<BreakerGuard> {
    let start = Instant::now();
    loop {
        if let Some(guard) = BreakerGuard::take(lock) {
            return Some(guard);
        }
        if start.elapsed() >= wait {
            return None;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn read_owner(lock: &Path) -> Option<LockOwner> {
    std::fs::read(lock.join("owner"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
}

fn remove_lock(
    path: &Path,
    is_ours: impl FnOnce(Option<&LockOwner>) -> bool,
    renamed: impl FnOnce(),
) -> bool {
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let mut tomb = path.as_os_str().to_owned();
    tomb.push(format!(".gone-{}", &nonce[..12]));
    let tomb = PathBuf::from(tomb);
    if std::fs::rename(path, &tomb).is_err() {
        return false;
    }
    renamed();
    if is_ours(read_owner(&tomb).as_ref()) {
        let _ = std::fs::remove_dir_all(&tomb);
        return true;
    }
    if rename_noreplace(&tomb, path).is_err() {
        eprintln!(
            "horch: lock {} was taken while another lock was put back",
            path.display()
        );
        let _ = std::fs::remove_dir_all(&tomb);
    }
    false
}

const OWNERLESS_GRACE: Duration = Duration::from_secs(5);

impl DirLock {
    pub(crate) fn acquire(
        dir: &Path,
        name: &str,
        stale_after: Duration,
        timeout: Duration,
    ) -> Result<DirLockGuard> {
        Self::acquire_with(dir, name, stale_after, timeout, no_hook)
    }

    fn acquire_with(
        dir: &Path,
        name: &str,
        stale_after: Duration,
        timeout: Duration,
        writing: impl FnOnce(&Path),
    ) -> Result<DirLockGuard> {
        let mut writing = Some(writing);
        io(dir, std::fs::create_dir_all(dir))?;
        let path = dir.join(format!("{name}.lock"));
        let start = Instant::now();
        let mut backoff = Duration::from_millis(5);
        loop {
            if let Some(done) = publish(&path, stale_after, &mut writing).transpose() {
                break done;
            }
            let broke = is_stale(&path, stale_after) && break_lock(&path, stale_after);
            if let Err(e) = wait_or_timeout(broke, &path, start, timeout, &mut backoff) {
                break Err(e);
            }
        }
    }
}

fn wait_or_timeout(
    broke: bool,
    path: &Path,
    start: Instant,
    timeout: Duration,
    backoff: &mut Duration,
) -> Result<()> {
    if broke {
        Ok(())
    } else if start.elapsed() >= timeout {
        let waited = start.elapsed();
        Err(FsxError::LockTimeout {
            path: path.to_path_buf(),
            waited,
        })
    } else {
        std::thread::sleep((*backoff).min(timeout.saturating_sub(start.elapsed())));
        *backoff = (*backoff * 2).min(Duration::from_millis(100));
        Ok(())
    }
}

fn no_hook(_lock: &Path) {}

fn publish(
    path: &Path,
    stale_after: Duration,
    writing: &mut Option<impl FnOnce(&Path)>,
) -> Result<Option<DirLockGuard>> {
    if std::fs::symlink_metadata(path).is_ok() {
        return Ok(None);
    }
    let (parent, name) = parent_and_name(path)?;
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let stage = parent.join(format!(".{name}.new-{}", &nonce[..12]));
    io(&stage, std::fs::create_dir(&stage))?;
    let owner = LockOwner {
        pid: std::process::id(),
        host: host_name(),
        acquired_at: crate::clock::stamp(chrono::Utc::now()),
        nonce,
        started: crate::procid::start_time(std::process::id()),
        stale_after_s: Some(stale_after.as_secs()),
    };
    let json = serde_json::to_vec(&owner).expect("LockOwner serializes");
    if let Some(writing) = writing.take() {
        writing(path);
    }
    let staged = write_owner(&stage.join("owner"), &json).map(|()| rename_noreplace(&stage, path));
    match staged {
        Ok(Ok(())) => Ok(Some(DirLockGuard {
            path: path.to_path_buf(),
            nonce: Some(owner.nonce),
        })),
        Ok(Err(e)) => {
            let _ = std::fs::remove_dir_all(&stage);
            let taken = matches!(
                e.kind(),
                std::io::ErrorKind::AlreadyExists | std::io::ErrorKind::DirectoryNotEmpty
            );
            if taken || std::fs::symlink_metadata(path).is_ok() {
                return Ok(None);
            }
            io(path, Err(e))
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&stage);
            Err(e)
        }
    }
}

fn write_owner(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut f = io(path, create_new_file(path, PRIVATE_FILE))?;
    set_mode(path, PRIVATE_FILE)?;
    io(path, f.write_all(bytes))
}

fn age(path: &Path) -> Option<Duration> {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
}

fn is_stale(path: &Path, stale_after: Duration) -> bool {
    stale_lock(path, stale_after).is_some()
}

struct StaleLock {
    nonce: Option<String>,
}

fn stale_lock(path: &Path, stale_after: Duration) -> Option<StaleLock> {
    let age = age(path);
    let owner = read_owner(path);
    let limit = owner
        .as_ref()
        .and_then(|o| o.stale_after_s)
        .map_or(stale_after, |s| stale_after.max(Duration::from_secs(s)));
    let released = owner
        .as_ref()
        .is_some_and(|o| !o.nonce.is_empty() && released_mark(path, &o.nonce).exists());
    let stale = released
        || age.is_some_and(|a| a >= limit)
        || match &owner {
            Some(o) => o.host == host_name() && !crate::procid::alive(o.pid, o.started),
            None => age.is_some_and(|a| a >= OWNERLESS_GRACE),
        };
    stale.then(|| StaleLock {
        nonce: owner.map(|o| o.nonce),
    })
}

const BREAKER_STALE_AFTER: Duration = Duration::from_secs(5);

fn break_lock(path: &Path, stale_after: Duration) -> bool {
    break_lock_with(path, stale_after, || {}, || {})
}

fn break_lock_with(
    path: &Path,
    stale_after: Duration,
    judged: impl FnOnce(),
    renamed: impl FnOnce(),
) -> bool {
    let Some(_guard) = BreakerGuard::take(path) else {
        return false;
    };
    let mut breaker = path.as_os_str().to_owned();
    breaker.push(".break");
    let breaker = PathBuf::from(breaker);
    let broke = if std::fs::create_dir(&breaker).is_ok() {
        let broke = stale_lock(path, stale_after).is_some_and(|stale| {
            judged();
            remove_lock(
                path,
                |owner| owner.map(|o| &o.nonce) == stale.nonce.as_ref(),
                renamed,
            )
        });
        let _ = std::fs::remove_dir(&breaker);
        broke
    } else {
        clear_stale_breaker(&breaker);
        false
    };
    if broke {
        eprintln!("horch: broke stale lock {}", path.display());
    }
    broke
}

fn clear_stale_breaker(breaker: &Path) {
    if age(breaker).is_some_and(|a| a >= BREAKER_STALE_AFTER) {
        let _ = std::fs::remove_dir(breaker);
    }
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
    fn create_immutable_shows_no_partial_file() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("output.json");
        let mut seen = None;
        let made = create_immutable_with(&p, b"answer", PRIVATE_FILE, || {
            seen = Some(p.exists());
        })
        .unwrap();
        assert_eq!(made, Created::Written);
        assert_eq!(seen, Some(false), "the file was visible before its bytes");
        assert_eq!(std::fs::read(&p).unwrap(), b"answer");
        let names: Vec<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["output.json"], "no temp file is left behind");
        let err = create_immutable(&p, b"other", PRIVATE_FILE).unwrap_err();
        assert!(matches!(err, FsxError::Conflict { .. }), "{err}");
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 1);
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

    #[test]
    fn private_files_and_folders_pass_the_owner_only_check() {
        use horch_platform::channel::User;
        use horch_platform::fs as platform;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("a").join("private");
        ensure_private_dir(&dir).unwrap();
        let replaced = dir.join("r.json");
        write_atomic(&replaced, b"x", PRIVATE_FILE).unwrap();
        let created = dir.join("c.json");
        create_immutable(&created, b"x", PRIVATE_FILE).unwrap();
        let me = User::current().unwrap();
        for (path, rule) in [
            (&dir, PRIVATE_DIR),
            (&replaced, PRIVATE_FILE),
            (&created, PRIVATE_FILE),
        ] {
            let file = platform::open_no_follow(path).unwrap();
            assert_eq!(platform::owner_of(&file).unwrap(), me, "{}", path.display());
            platform::check_owner_only(&file, &file.metadata().unwrap(), rule).unwrap();
        }
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
        let dead = LockOwner {
            pid: 999_999_999,
            host: host_name(),
            acquired_at: "2026-09-28T17:00:00Z".into(),
            nonce: "dead".into(),
            started: None,
            stale_after_s: None,
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

    #[cfg(unix)]
    #[test]
    fn trf_06_a_live_holders_lock_is_stale_only_after_its_own_age() {
        let tmp = tempfile::tempdir().unwrap();
        let hour = Duration::from_secs(3600);
        let held = DirLock::acquire(tmp.path(), "land-x", hour, Duration::from_secs(1)).unwrap();
        let lock = held.path().to_path_buf();
        assert_eq!(read_owner(&lock).unwrap().stale_after_s, Some(3600));
        let aged = |by: Duration| {
            std::fs::File::open(&lock)
                .unwrap()
                .set_modified(std::time::SystemTime::now() - by)
                .unwrap();
        };
        let second = Duration::from_secs(1);
        aged(Duration::from_secs(60));
        assert!(!is_stale(&lock, second), "the holder's hour is not over");
        let err =
            DirLock::acquire(tmp.path(), "land-x", second, Duration::from_millis(50)).unwrap_err();
        assert!(matches!(err, FsxError::LockTimeout { .. }), "{err}");
        assert!(
            lock.join("owner").exists(),
            "the live holder keeps its lock"
        );
        aged(2 * hour);
        assert!(is_stale(&lock, second), "the holder's hour is over");
        let mut old = read_owner(&lock).unwrap();
        old.stale_after_s = None;
        std::fs::write(lock.join("owner"), serde_json::to_vec(&old).unwrap()).unwrap();
        aged(Duration::from_secs(60));
        assert!(is_stale(&lock, second));
        assert!(!is_stale(&lock, hour));
    }

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
            stale_after_s: None,
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
            return;
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
                stale_after_s: None,
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
            assert!(stages(tmp.path()).is_empty(), "{:?}", stages(tmp.path()));
        }
    }

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
            stale_after_s: None,
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
            stale_after_s: None,
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

    #[cfg(unix)]
    #[test]
    fn dirlock_guard_is_free_while_a_forked_child_shares_it() {
        let tmp = tempfile::tempdir().unwrap();
        let lock = tmp.path().join("events.lock");
        let held = BreakerGuard::take(&lock).expect("the guard is free");
        // SAFETY: the child only calls `pause` and `_exit`, which are
        // async-signal-safe; it stands for a child between fork and exec.
        let child = unsafe { libc::fork() };
        assert!(child >= 0, "fork failed");
        if child == 0 {
            // SAFETY: the forked child runs only `pause` and `_exit`, both async-signal-safe.
            unsafe {
                libc::pause();
                libc::_exit(0);
            }
        }
        drop(held);
        let free = BreakerGuard::take(&lock).is_some();
        // SAFETY: `child` is the pid of the child forked above, which this test has not yet reaped;
        // SIGKILL and `waitpid` with a null status pointer touch no memory.
        unsafe {
            libc::kill(child, libc::SIGKILL);
            libc::waitpid(child, std::ptr::null_mut(), 0);
        }
        assert!(free, "the dropped guard is free while the child lives");
    }

    fn stages(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(".events.lock.new-"))
            .collect()
    }

    #[cfg(unix)]
    #[test]
    fn dirlock_slow_owner_write_keeps_the_lock() {
        let tmp = tempfile::tempdir().unwrap();
        let hour = Duration::from_secs(3600);
        let mut hook_ran = false;
        let first = DirLock::acquire_with(tmp.path(), "events", hour, hour, |lock| {
            hook_ran = true;
            assert!(
                std::fs::symlink_metadata(lock).is_err(),
                "no lock is visible before its owner is written"
            );
            DirLock::acquire(tmp.path(), "events", hour, Duration::from_millis(50))
                .expect("the lock is free until it is published");
        })
        .expect("the writer gets the lock after the second holder released it");
        assert!(hook_ran, "the checks above run while the owner is written");
        let owner = read_owner(first.path()).expect("the owner file is written");
        assert_eq!(owner.pid, std::process::id());
        assert_eq!(Some(&owner.nonce), first.nonce.as_ref());
        assert!(stages(tmp.path()).is_empty(), "{:?}", stages(tmp.path()));
    }

    #[cfg(unix)]
    #[test]
    fn dirlock_writer_that_loses_the_rename_waits() {
        let tmp = tempfile::tempdir().unwrap();
        let hour = Duration::from_secs(3600);
        let mut held = None;
        let lost = DirLock::acquire_with(
            tmp.path(),
            "events",
            hour,
            Duration::from_millis(100),
            |_| {
                held = Some(
                    DirLock::acquire(tmp.path(), "events", hour, Duration::from_millis(50))
                        .unwrap(),
                );
            },
        )
        .unwrap_err();
        assert!(matches!(lost, FsxError::LockTimeout { .. }), "{lost}");
        let held = held.unwrap();
        let owner = read_owner(held.path()).expect("the holder keeps its owner file");
        assert_eq!(Some(&owner.nonce), held.nonce.as_ref());
        assert!(stages(tmp.path()).is_empty(), "{:?}", stages(tmp.path()));
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn rename_noreplace_keeps_an_empty_lock() {
        let tmp = tempfile::tempdir().unwrap();
        let lock = tmp.path().join("events.lock");
        std::fs::create_dir(&lock).unwrap();
        let stage = tmp.path().join(".events.lock.new-test");
        std::fs::create_dir(&stage).unwrap();
        std::fs::write(stage.join("owner"), b"{}").unwrap();
        let err = rename_noreplace(&stage, &lock).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists, "{err}");
        assert_eq!(std::fs::read_dir(&lock).unwrap().count(), 0);
        assert!(stage.join("owner").is_file());
    }

    #[test]
    fn dirlock_fresh_ownerless_lock_is_held() {
        let tmp = tempfile::tempdir().unwrap();
        let lock = tmp.path().join("events.lock");
        std::fs::create_dir(&lock).unwrap();
        let err = DirLock::acquire(
            tmp.path(),
            "events",
            Duration::from_secs(3600),
            Duration::from_millis(50),
        )
        .unwrap_err();
        assert!(matches!(err, FsxError::LockTimeout { .. }), "{err}");
        assert!(lock.is_dir());
        assert_eq!(std::fs::read_dir(&lock).unwrap().count(), 0);
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

    #[test]
    fn read_regular_bounded_reads_text_up_to_the_limit() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("settings.json");
        assert_eq!(
            read_regular_bounded(&p),
            Ok(None),
            "no file: nothing to read"
        );
        std::fs::write(&p, "{\"a\":1}").unwrap();
        assert_eq!(read_regular_bounded(&p), Ok(Some("{\"a\":1}".to_string())));
        std::fs::write(&p, vec![b'x'; READ_LIMIT as usize]).unwrap();
        let full = read_regular_bounded(&p)
            .unwrap()
            .expect("exactly the limit is read");
        assert_eq!(full.len() as u64, READ_LIMIT);
        std::fs::write(&p, vec![b'x'; READ_LIMIT as usize + 1]).unwrap();
        assert_eq!(read_regular_bounded(&p), Err(Refused::TooLarge));
        std::fs::write(&p, [0xff, 0xfe, 0x00]).unwrap();
        assert_eq!(
            read_regular_bounded(&p),
            Ok(None),
            "not UTF-8: nothing to read"
        );
    }

    #[cfg(unix)]
    #[test]
    fn read_regular_bounded_refuses_a_directory_and_a_fifo() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(read_regular_bounded(tmp.path()), Err(Refused::NotRegular));
        let fifo = tmp.path().join("pipe");
        let c = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
        // SAFETY: a NUL-terminated path that outlives the call.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        assert_eq!(read_regular_bounded(&fifo), Err(Refused::NotRegular));
    }

    #[test]
    fn refresh_writes_a_heartbeat_only_while_owned() {
        let tmp = tempfile::tempdir().unwrap();
        let stale = Duration::from_secs(60);
        let held = DirLock::acquire(tmp.path(), "events", stale, Duration::from_secs(1)).unwrap();
        held.refresh().expect("the holder refreshes its own lock");
        let nonce = held.nonce.clone().unwrap();
        assert!(held.path().join(format!("heartbeat-{nonce}")).is_file());

        let mut other = read_owner(held.path()).unwrap();
        other.nonce = "someone-else".into();
        std::fs::write(
            held.path().join("owner"),
            serde_json::to_vec(&other).unwrap(),
        )
        .unwrap();
        let err = held.refresh().unwrap_err();
        assert!(
            err.to_string()
                .ends_with("lock is no longer owned by this guard"),
            "{err}"
        );
        drop(held);
        assert!(
            tmp.path().join("events.lock").exists(),
            "a guard does not remove a lock it no longer owns"
        );

        let unowned = DirLockGuard {
            path: tmp.path().join("other.lock"),
            nonce: None,
        };
        let err = unowned.refresh().unwrap_err();
        assert!(
            err.to_string().ends_with("lock owner is not initialized"),
            "{err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn breaker_guard_without_a_parent_dir_holds_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let lock = tmp.path().join("missing").join("events.lock");
        let guard = BreakerGuard::take(&lock).expect("no directory to lock: free");
        let held = format!("{guard:?}");
        assert!(held.contains("dir: None"), "{held}");
    }

    #[cfg(unix)]
    #[test]
    fn dirlock_break_keeps_a_lock_taken_after_its_check() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let lock = dir.join("events.lock");
        let hour = Duration::from_secs(3600);
        let old = DirLock::acquire(&dir, "events", Duration::ZERO, Duration::from_secs(1)).unwrap();
        let (taken, next_took_lock) = std::sync::mpsc::channel();
        let mut next = None;
        break_lock_with(
            &lock,
            Duration::ZERO,
            || {
                next = Some(std::thread::spawn(move || {
                    drop(old);
                    let held = DirLock::acquire(&dir, "events", hour, Duration::from_secs(20));
                    let _ = taken.send(());
                    held
                }));
                let _ = next_took_lock.recv_timeout(Duration::from_millis(500));
            },
            || {},
        );
        let next = next
            .unwrap()
            .join()
            .unwrap()
            .expect("the next holder gets the lock");
        let owner = read_owner(&lock).expect("the next holder keeps its lock");
        assert_eq!(Some(&owner.nonce), next.nonce.as_ref());
        drop(next);
        assert!(!lock.exists());
        assert!(stages(tmp.path()).is_empty(), "{:?}", stages(tmp.path()));
    }

    #[test]
    fn break_lock_keeps_a_fresh_breaker_and_the_lock() {
        let tmp = tempfile::tempdir().unwrap();
        let lock = tmp.path().join("events.lock");
        std::fs::create_dir(&lock).unwrap();
        let breaker = tmp.path().join("events.lock.break");
        std::fs::create_dir(&breaker).unwrap();
        assert!(!break_lock(&lock, Duration::ZERO));
        assert!(
            breaker.is_dir(),
            "a fresh breaker belongs to a live process"
        );
        assert!(lock.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn dirlock_release_timeout_does_not_overlap_a_new_holder() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let lock = dir.join("events.lock");
        let hour = Duration::from_secs(3600);
        let short = Duration::from_millis(50);
        let a = std::mem::ManuallyDrop::new(
            DirLock::acquire(&dir, "events", Duration::ZERO, Duration::from_secs(1)).unwrap(),
        );
        let (mut b, mut c) = (None, None);
        break_lock_with(
            &lock,
            Duration::ZERO,
            || {
                let started = Instant::now();
                release_lock(&a.path, a.nonce.as_deref(), short);
                assert!(started.elapsed() < BREAKER_STALE_AFTER);
                b = Some(DirLock::acquire(&dir, "events", hour, short));
            },
            || c = Some(DirLock::acquire(&dir, "events", hour, short)),
        );
        let holders: Vec<DirLockGuard> = [b, c].into_iter().flatten().flatten().collect();
        assert_eq!(holders.len(), 1, "1 holder at a time: {holders:?}");
        let owner = read_owner(&lock).expect("the holder keeps its lock");
        assert_eq!(Some(&owner.nonce), holders[0].nonce.as_ref());
    }

    #[cfg(unix)]
    #[test]
    fn dirlock_release_at_the_guard_timeout_frees_the_lock() {
        let tmp = tempfile::tempdir().unwrap();
        let hour = Duration::from_secs(3600);
        let short = Duration::from_millis(50);
        let a = std::mem::ManuallyDrop::new(
            DirLock::acquire(tmp.path(), "events", hour, Duration::from_secs(1)).unwrap(),
        );
        let lock = a.path.clone();
        let breaker = BreakerGuard::take(&lock).expect("the guard is free");
        let started = Instant::now();
        release_lock(&a.path, a.nonce.as_deref(), short);
        assert!(started.elapsed() < BREAKER_STALE_AFTER);
        let owner = read_owner(&lock).expect("a release without the guard removes nothing");
        assert_eq!(Some(&owner.nonce), a.nonce.as_ref());
        assert!(is_stale(&lock, hour), "the released lock is free to break");
        let err = DirLock::acquire(tmp.path(), "events", hour, short).unwrap_err();
        assert!(matches!(err, FsxError::LockTimeout { .. }), "{err}");
        drop(breaker);
        let b = DirLock::acquire(tmp.path(), "events", hour, Duration::from_secs(20))
            .expect("the next taker breaks the released lock");
        let late = released_mark(b.path(), &owner.nonce);
        create_new_file(&late, PRIVATE_FILE).unwrap();
        assert!(!is_stale(&lock, hour), "A's mark does not free B's lock");
    }
}
