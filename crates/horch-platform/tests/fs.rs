//! The concern fs: file locks, file permissions and file identity (PL-2),
//! with the gaps of Windows as `NotSupported` (PL-6).

use std::path::Path;

use horch_platform::fs;

#[cfg(unix)]
mod unix {
    use std::fs::{DirBuilder, OpenOptions};
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::sync::Mutex;
    use std::time::Duration;

    use horch_platform::fs;

    /// The umask is of the process: the tests that change it run 1 at a time.
    static UMASK: Mutex<()> = Mutex::new(());

    /// Run `f` with the current umask, then again with the umask `0o022`.
    fn with_both_umasks(f: impl Fn(&str)) {
        let _held = UMASK.lock().unwrap_or_else(|e| e.into_inner());
        f("current");
        // SAFETY: umask only swaps the mask of this process; the lock above
        // keeps the other umask tests out.
        let old = unsafe { libc::umask(0o022) };
        f("022");
        // SAFETY: as above.
        unsafe { libc::umask(old) };
    }

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn create_mode_gives_0600() {
        let dir = tempfile::tempdir().unwrap();
        with_both_umasks(|tag| {
            let path = dir.path().join(format!("private-{tag}"));
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            fs::create_mode(&mut options, 0o600).unwrap();
            options.open(&path).unwrap();
            assert_eq!(mode(&path), 0o600, "umask {tag}");
        });
    }

    #[test]
    fn create_dir_mode_gives_0700() {
        let dir = tempfile::tempdir().unwrap();
        with_both_umasks(|tag| {
            let path = dir.path().join(format!("private-{tag}"));
            let mut builder = DirBuilder::new();
            fs::create_dir_mode(&mut builder, 0o700).unwrap();
            builder.create(&path).unwrap();
            assert_eq!(mode(&path), 0o700, "umask {tag}");
        });
    }

    #[test]
    fn owner_and_mode_name_the_current_user() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mine");
        std::fs::write(&path, b"x").unwrap();
        fs::set_mode(&path, 0o640).unwrap();
        let facts = fs::owner_and_mode(&std::fs::metadata(&path).unwrap()).unwrap();
        assert_eq!(facts.uid, fs::current_uid().unwrap());
        assert_eq!(facts.mode & 0o777, 0o640);
        assert_eq!(
            fs::mode_of(&std::fs::metadata(&path).unwrap()).unwrap() & 0o777,
            0o640
        );
    }

    #[test]
    fn breaker_guard_is_exclusive() {
        let dir = tempfile::tempdir().unwrap();
        let lock = dir.path().join("ledger.lock");
        let first = fs::BreakerGuard::take(&lock).expect("the first take holds the guard");
        assert!(
            fs::BreakerGuard::take(&lock).is_none(),
            "a second take must wait while the first guard lives"
        );
        drop(first);
        assert!(
            fs::BreakerGuard::take(&lock).is_some(),
            "the drop releases the guard"
        );
    }

    #[test]
    fn open_no_block_does_not_block_on_a_fifo() {
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("fifo");
        let c_path = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
        // SAFETY: `c_path` is a NUL-terminated path that outlives the call.
        assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
        let (tx, rx) = std::sync::mpsc::channel();
        let path = fifo.clone();
        std::thread::spawn(move || {
            let mut options = OpenOptions::new();
            options.read(true);
            fs::open_no_block(&mut options).unwrap();
            let _ = tx.send(options.open(&path).map(|_| ()));
        });
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(opened) => opened.unwrap(),
            Err(_) => {
                // Release the blocked reader, then fail.
                let _writer = OpenOptions::new().write(true).open(&fifo);
                panic!("open of a FIFO with no writer blocked");
            }
        }
    }

    #[test]
    fn is_executable_follows_the_exec_bit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tool");
        std::fs::write(&path, b"#!/bin/sh\n").unwrap();
        fs::set_mode(&path, 0o644).unwrap();
        assert!(!fs::is_executable(&std::fs::metadata(&path).unwrap()));
        fs::set_mode(&path, 0o744).unwrap();
        assert!(fs::is_executable(&std::fs::metadata(&path).unwrap()));
        fs::set_mode(&path, 0o644).unwrap();
        fs::make_executable(&path).unwrap();
        assert_eq!(mode(&path), 0o755);
        assert!(fs::is_executable(&std::fs::metadata(&path).unwrap()));
        assert!(
            !fs::is_executable(&std::fs::metadata(dir.path()).unwrap()),
            "a dir is not an executable file"
        );
    }

    #[test]
    fn sync_dir_syncs_a_dir() {
        let dir = tempfile::tempdir().unwrap();
        fs::sync_dir(dir.path()).unwrap();
    }

    #[test]
    fn remove_dir_below_removes_the_folder_and_all_in_it() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("tmp").join("t1");
        std::fs::create_dir_all(folder.join("sub").join("deeper")).unwrap();
        std::fs::write(folder.join("index"), b"x").unwrap();
        std::fs::write(folder.join("sub").join("deeper").join("f"), b"x").unwrap();
        let outside = dir.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("work.txt"), b"work").unwrap();
        std::os::unix::fs::symlink(&outside, folder.join("link")).unwrap();

        fs::remove_dir_below(dir.path(), &["tmp", "t1"]).unwrap();

        assert!(!folder.exists());
        assert!(dir.path().join("tmp").is_dir(), "only the last name goes");
        assert_eq!(std::fs::read(outside.join("work.txt")).unwrap(), b"work");
    }

    #[test]
    fn remove_dir_below_follows_no_link_on_the_way() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("fleet");
        let outside = dir.path().join("elsewhere");
        std::fs::create_dir_all(outside.join("t1")).unwrap();
        std::fs::create_dir(&root).unwrap();
        std::fs::write(outside.join("t1").join("work.txt"), b"work").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("tmp")).unwrap();
        std::os::unix::fs::symlink(outside.join("t1"), dir.path().join("t2")).unwrap();

        assert!(fs::remove_dir_below(&root, &["tmp", "t1"]).is_err());
        assert!(fs::remove_dir_below(dir.path(), &["t2"]).is_err());

        assert_eq!(
            std::fs::read(outside.join("t1").join("work.txt")).unwrap(),
            b"work"
        );
    }

    #[test]
    fn remove_dir_below_refuses_a_name_that_is_not_1_folder_name() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("a").join("b")).unwrap();
        for names in [&[][..], &["a/b"], &[".."], &["."], &[""]] {
            let e = fs::remove_dir_below(&dir.path().join("a"), names).unwrap_err();
            assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput, "{names:?}");
        }
        assert!(dir.path().join("a").join("b").is_dir());
    }
}

#[test]
// Windows renames with a plain rename, which replaces an empty dir.
#[cfg_attr(
    windows,
    ignore = "preservation-06 section 6, row File locks: plain rename on Windows"
)]
fn rename_noreplace_refuses_an_existing_target() {
    let dir = tempfile::tempdir().unwrap();
    let from = dir.path().join("lock.tmp");
    let to = dir.path().join("lock");
    std::fs::create_dir(&from).unwrap();
    std::fs::write(from.join("owner"), b"me").unwrap();
    // An empty dir: the lock of an older binary before its owner write. A
    // plain rename replaces it.
    std::fs::create_dir(&to).unwrap();
    assert!(fs::rename_noreplace(&from, &to).is_err());
    assert!(from.join("owner").exists(), "the source stays");
    assert!(!to.join("owner").exists(), "the target stays");
}

#[test]
fn rename_noreplace_moves_to_a_free_name() {
    let dir = tempfile::tempdir().unwrap();
    let from = dir.path().join("lock.tmp");
    let to = dir.path().join("lock");
    std::fs::create_dir(&from).unwrap();
    std::fs::write(from.join("owner"), b"me").unwrap();
    fs::rename_noreplace(&from, &to).unwrap();
    assert!(!from.exists());
    assert_eq!(std::fs::read(to.join("owner")).unwrap(), b"me");
}

#[test]
fn file_identity_differs_for_2_files_and_is_stable_for_1() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a");
    let b = dir.path().join("b");
    std::fs::write(&a, b"a").unwrap();
    std::fs::write(&b, b"b").unwrap();
    let id = |p: &Path| fs::file_identity(&std::fs::metadata(p).unwrap());
    assert_ne!(id(&a), id(&b));
    let a_id = id(&a);
    assert_eq!(id(&a), a_id);
    std::fs::write(&a, b"new text").unwrap();
    assert_eq!(id(&a), a_id, "a write keeps the identity");
}

#[cfg(windows)]
#[test]
fn modes_and_owner_are_not_supported_on_windows() {
    use horch_platform::NotSupported;

    fn is_not_supported(err: &std::io::Error) -> bool {
        err.kind() == std::io::ErrorKind::Unsupported
            && err
                .get_ref()
                .is_some_and(|inner| inner.downcast_ref::<NotSupported>().is_some())
    }

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file");
    std::fs::write(&path, b"x").unwrap();
    let meta = std::fs::metadata(&path).unwrap();

    assert!(is_not_supported(&fs::set_mode(&path, 0o600).unwrap_err()));
    assert!(is_not_supported(&fs::sync_dir(dir.path()).unwrap_err()));
    let mut options = std::fs::OpenOptions::new();
    assert!(fs::create_mode(&mut options, 0o600).is_err());
    assert!(fs::open_no_block(&mut options).is_err());
    assert!(fs::create_dir_mode(&mut std::fs::DirBuilder::new(), 0o700).is_err());
    assert!(fs::owner_and_mode(&meta).is_err());
    assert!(fs::mode_of(&meta).is_err());
    assert!(fs::current_uid().is_err());
    // "Exists" is the rule for the exec bit on Windows.
    assert!(fs::is_executable(&meta));
    fs::make_executable(&path).unwrap();
    // The guard holds nothing, so a second take also succeeds.
    let lock = dir.path().join("ledger.lock");
    let _first = fs::BreakerGuard::take(&lock).unwrap();
    assert!(fs::BreakerGuard::take(&lock).is_some());
    assert!(is_not_supported(
        &fs::remove_dir_below(dir.path(), &["x"]).unwrap_err()
    ));
}
