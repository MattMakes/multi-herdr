//! The protected file reader (ticket 07 of fleetd-p5): SR-5, SR-6, FM-23.
//! Each rejected case asserts its reason text.

#[cfg(unix)]
use std::fs::Permissions;
use std::fs::{self, File};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};

use horch_core::fleetd::fleet_runtime::adapters::protected_files::{read_protected, Rule};
use horch_platform::channel::User;
use horch_platform::fs as platform;

struct Setup {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    owner: User,
}

impl Setup {
    fn new() -> Setup {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("outbox");
        platform::create_dir_owner_only(&root).unwrap();
        Setup {
            _tmp: tmp,
            root,
            owner: User::current().unwrap(),
        }
    }

    /// The real path of the temporary folder, outside the root.
    fn outside(&self) -> PathBuf {
        fs::canonicalize(self._tmp.path()).unwrap()
    }

    fn rule(&self, limit: u64) -> Rule<'_> {
        Rule {
            root: &self.root,
            owner: &self.owner,
            limit,
        }
    }
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    fs::set_permissions(path, Permissions::from_mode(mode)).unwrap();
}

fn write_file(path: &Path, bytes: &[u8]) {
    let mut file = platform::create_new_owner_only(path).unwrap();
    file.write_all(bytes).unwrap();
}

fn reason(path: &Path, rule: &Rule<'_>) -> String {
    match read_protected(path, rule) {
        Ok(bytes) => panic!("{} was read: {} bytes", path.display(), bytes.len()),
        Err(rejected) => rejected.reason,
    }
}

#[cfg(unix)]
#[test]
fn symlink_is_rejected() {
    let s = Setup::new();
    let target = s.root.join("target.json");
    write_file(&target, b"{}");
    let link = s.root.join("result.json");
    symlink(&target, &link).unwrap();

    assert_eq!(reason(&link, &s.rule(1024)), "the file is a link");
}

#[cfg(unix)]
#[test]
fn fifo_is_rejected() {
    let s = Setup::new();
    let fifo = s.root.join("result.json");
    let made = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap();
    assert!(made.success());
    set_mode(&fifo, 0o600);

    assert_eq!(
        reason(&fifo, &s.rule(1024)),
        "the file is not a regular file"
    );
}

#[test]
fn file_over_its_limit_is_rejected_without_reading_it_all() {
    let s = Setup::new();
    let path = s.root.join("progress.jsonl");
    write_file(&path, b"");
    // A sparse file of 1 GiB: the reason comes from the size of the open
    // handle, so the reader refuses it before a read.
    File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(1 << 30)
        .unwrap();

    assert_eq!(
        reason(&path, &s.rule(64 * 1024)),
        "size 1073741824 bytes is over the limit of 65536 bytes"
    );
}

#[cfg(unix)]
#[test]
fn file_with_mode_0644_is_rejected() {
    let s = Setup::new();
    let path = s.root.join("result.json");
    write_file(&path, b"{}");
    set_mode(&path, 0o644);

    assert_eq!(reason(&path, &s.rule(1024)), "mode 0644 is wider than 0600");
}

#[cfg(unix)]
#[test]
fn folder_with_mode_0755_is_rejected() {
    let s = Setup::new();
    let path = s.root.join("result.json");
    write_file(&path, b"{}");
    set_mode(&s.root, 0o755);

    let real_root = fs::canonicalize(&s.root).unwrap();
    assert_eq!(
        reason(&path, &s.rule(1024)),
        format!(
            "folder {}: mode 0755 is wider than 0700",
            real_root.display()
        )
    );
}

#[cfg(unix)]
#[test]
fn real_path_outside_the_root_is_rejected() {
    let s = Setup::new();
    // The file is regular, and the link is in a parent folder: `sub` points
    // out of the root.
    let elsewhere = s.outside().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    set_mode(&elsewhere, 0o700);
    write_file(&elsewhere.join("result.json"), b"{}");
    symlink(&elsewhere, s.root.join("sub")).unwrap();

    let real_root = fs::canonicalize(&s.root).unwrap();
    assert_eq!(
        reason(&s.root.join("sub/result.json"), &s.rule(1024)),
        format!(
            "real path {} is outside the root {}",
            elsewhere.join("result.json").display(),
            real_root.display()
        )
    );
}

#[cfg(unix)]
#[test]
fn file_of_another_owner_is_rejected() {
    let s = Setup::new();
    let path = s.root.join("result.json");
    write_file(&path, b"{}");
    let User::Uid(uid) = s.owner else {
        panic!("a unix user is a uid")
    };
    // A test cannot make a file of another user, so the caller gives an
    // owner that is not the owner of the file.
    let other = User::Uid(uid + 1);
    let rule = Rule {
        root: &s.root,
        owner: &other,
        limit: 1024,
    };

    assert_eq!(
        reason(&path, &rule),
        format!("owner uid {uid} is not the ledger owner uid {}", uid + 1)
    );
}

#[test]
fn valid_file_is_read() {
    let s = Setup::new();
    let path = s.root.join("result.json");
    write_file(&path, b"{\"status\":\"done\"}");

    let bytes = read_protected(&path, &s.rule(1024)).unwrap();

    assert_eq!(bytes, b"{\"status\":\"done\"}");
}

#[test]
fn valid_owner_only_file_and_request_spool_succeed_on_host_platform() {
    let s = Setup::new();
    let path = s.root.join("result.json");
    write_file(&path, b"{\"status\":\"done\"}");

    assert_eq!(
        read_protected(&path, &s.rule(1024)).unwrap(),
        b"{\"status\":\"done\"}"
    );

    let other = match &s.owner {
        User::Uid(uid) => User::Uid(uid + 1),
        User::Sid(sid) => User::Sid(format!("{sid}-1")),
    };
    let rule = Rule {
        root: &s.root,
        owner: &other,
        limit: 1024,
    };
    assert!(
        reason(&path, &rule).starts_with("owner "),
        "{}",
        reason(&path, &rule)
    );

    grant_others(&path);
    let wide = reason(&path, &s.rule(1024));
    assert!(
        wide == "mode 0604 is wider than 0600"
            || wide.starts_with("the access list allows S-1-1-0"),
        "{wide}"
    );
}

#[cfg(unix)]
fn grant_others(path: &Path) {
    set_mode(path, 0o604);
}

#[cfg(windows)]
fn grant_others(path: &Path) {
    let granted = std::process::Command::new("icacls")
        .arg(path)
        .args(["/grant", "*S-1-1-0:(R)"])
        .status()
        .unwrap();
    assert!(granted.success());
}
