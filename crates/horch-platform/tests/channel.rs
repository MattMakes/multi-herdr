//! The concern channel: the transport of the fleetd command channel, with
//! the user and the process of the peer.

use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use horch_platform::channel::{Listener, Stream, User};

/// The env var that makes this test binary run [`child_client_helper`].
const HELPER: &str = "HORCH_PLATFORM_CHANNEL_HELPER";

/// The name of a new channel. On unix it is a socket path in a new folder,
/// short because `sun_path` holds about 104 bytes on macOS. On Windows it
/// is a pipe name that no other test uses.
struct Place {
    #[cfg(unix)]
    _dir: tempfile::TempDir,
    name: PathBuf,
}

#[cfg(unix)]
fn place() -> Place {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::Builder::new()
        .prefix("ch")
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in("/tmp")
        .unwrap();
    let name = dir.path().join("c.sock");
    Place { _dir: dir, name }
}

#[cfg(windows)]
fn place() -> Place {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let name = format!("horch-channel-test-{}-{n}", std::process::id());
    Place { name: name.into() }
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

#[test]
fn connect_and_read_a_line() {
    let place = place();
    let listener = Listener::bind(&place.name).unwrap();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line).unwrap();
        (&stream).write_all(b"pong\n").unwrap();
        line
    });

    let mut stream = Stream::connect(&place.name, deadline()).unwrap();
    stream.write_all(b"ping\n").unwrap();
    let mut answer = String::new();
    BufReader::new(&mut stream).read_line(&mut answer).unwrap();
    assert_eq!(answer, "pong\n");
    assert_eq!(server.join().unwrap(), "ping\n");
}

#[test]
fn peer_is_this_user_and_this_process() {
    let place = place();
    let listener = Listener::bind(&place.name).unwrap();
    let client = Stream::connect(&place.name, deadline()).unwrap();
    let (_stream, peer) = listener.accept().unwrap();
    drop(client);

    let this_user = User::current().unwrap();
    assert_eq!(peer.user, Ok(this_user.clone()));
    assert_eq!(peer.pid, Ok(std::process::id()));
    #[cfg(unix)]
    // SAFETY: plain call with no arguments.
    assert_eq!(this_user, User::Uid(unsafe { libc::geteuid() }));
    #[cfg(windows)]
    assert!(
        matches!(&this_user, User::Sid(sid) if sid.starts_with("S-1-")),
        "{this_user}"
    );
}

#[test]
fn peer_pid_of_a_child_client_is_the_child() {
    let place = place();
    let listener = Listener::bind(&place.name).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "child_client_helper",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(HELPER, &place.name)
        .spawn()
        .expect("start the child client");

    let (stream, peer) = listener.accept().unwrap();
    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line).unwrap();
    drop(stream);
    let status = child.wait().unwrap();

    assert_eq!(line, "child\n");
    assert_eq!(peer.pid, Ok(child.id()));
    assert_eq!(peer.user, Ok(User::current().unwrap()));
    assert!(status.success(), "the child client ends well: {status:?}");
}

/// The client of the test above. It does nothing in a normal test run.
#[test]
fn child_client_helper() {
    let Some(name) = std::env::var_os(HELPER) else {
        return;
    };
    let mut stream = Stream::connect(&PathBuf::from(name), deadline()).unwrap();
    stream.write_all(b"child\n").unwrap();
    // Stay connected until the server closes the stream.
    let mut rest = Vec::new();
    stream.read_to_end(&mut rest).unwrap();
}

#[test]
fn connect_to_a_missing_channel_fails_with_not_found() {
    let place = place();
    let error = Stream::connect(&place.name, deadline()).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::NotFound);
}

#[test]
fn a_second_listener_with_the_same_name_is_refused() {
    let place = place();
    let _first = Listener::bind(&place.name).unwrap();
    assert!(Listener::bind(&place.name).is_err());
}

#[test]
fn the_listener_accepts_a_next_client_after_the_first() {
    let place = place();
    let listener = Listener::bind(&place.name).unwrap();
    for _ in 0..2 {
        let client = Stream::connect(&place.name, deadline()).unwrap();
        let (_stream, peer) = listener.accept().unwrap();
        assert_eq!(peer.pid, Ok(std::process::id()));
        drop(client);
    }
}

#[cfg(unix)]
mod unix {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn mode(path: &std::path::Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn socket_file_has_mode_0600() {
        let place = place();
        let _listener = Listener::bind(&place.name).unwrap();
        assert_eq!(mode(&place.name), 0o600);
    }

    #[test]
    fn bind_makes_a_missing_folder_with_mode_0700() {
        let place = place();
        let name = place.name.with_file_name("run").join("c.sock");
        let _listener = Listener::bind(&name).unwrap();
        assert_eq!(mode(name.parent().unwrap()), 0o700);
        assert_eq!(mode(&name), 0o600);
    }

    #[test]
    fn bind_refuses_a_folder_that_others_can_enter() {
        let place = place();
        let folder = place.name.parent().unwrap();
        std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o755)).unwrap();
        let error = Listener::bind(&place.name).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::PermissionDenied);
        assert!(!place.name.exists(), "no socket file is made");
    }
}

#[cfg(windows)]
mod windows {
    use super::*;

    #[test]
    fn a_name_with_a_backslash_is_a_valid_pipe_name() {
        let name = PathBuf::from(format!(
            r"C:\horch\channel-test-{}\fleetd",
            std::process::id()
        ));
        let listener = Listener::bind(&name).unwrap();
        let client = Stream::connect(&name, deadline()).unwrap();
        let (_stream, peer) = listener.accept().unwrap();
        assert_eq!(peer.pid, Ok(std::process::id()));
        drop(client);
    }
}
