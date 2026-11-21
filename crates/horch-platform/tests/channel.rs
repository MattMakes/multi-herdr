use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use horch_platform::channel::{Listener, Stream, User};

const HELPER: &str = "HORCH_PLATFORM_CHANNEL_HELPER";

const SERVER_HELPER: &str = "HORCH_PLATFORM_CHANNEL_SERVER_HELPER";

const SERVER_READY: &str = "horch-channel-server-ready";

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

#[test]
fn child_client_helper() {
    let Some(name) = std::env::var_os(HELPER) else {
        return;
    };
    let mut stream = Stream::connect(&PathBuf::from(name), deadline()).unwrap();
    stream.write_all(b"child\n").unwrap();
    let mut rest = Vec::new();
    stream.read_to_end(&mut rest).unwrap();
}

#[test]
fn client_gets_the_pid_of_the_server() {
    let place = place();
    let listener = Listener::bind(&place.name).unwrap();
    let client = Stream::connect(&place.name, deadline()).unwrap();
    let (_stream, _) = listener.accept().unwrap();

    assert_eq!(client.server_pid(), Ok(std::process::id()));
}

#[test]
fn client_gets_the_pid_of_a_server_in_a_child_process() {
    let place = place();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "child_server_helper",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(SERVER_HELPER, &place.name)
        .stderr(Stdio::piped())
        .spawn()
        .expect("start the child server");
    let mut lines = BufReader::new(child.stderr.take().unwrap()).lines();
    let ready = lines.any(|line| line.is_ok_and(|line| line == SERVER_READY));
    assert!(ready, "the child server binds its listener");

    let client = Stream::connect(&place.name, deadline()).unwrap();
    let server_pid = client.server_pid();
    let mut line = String::new();
    BufReader::new(&client).read_line(&mut line).unwrap();
    drop(client);
    let status = child.wait().unwrap();
    let rest: Vec<String> = lines.map_while(Result::ok).collect();

    assert_eq!(line, "server\n");
    assert_eq!(server_pid, Ok(child.id()));
    assert_ne!(child.id(), std::process::id());
    assert!(
        status.success(),
        "the child server ends well: {status:?} {rest:?}"
    );
}

#[test]
fn child_server_helper() {
    let Some(name) = std::env::var_os(SERVER_HELPER) else {
        return;
    };
    let listener = Listener::bind(&PathBuf::from(name)).unwrap();
    eprintln!("{SERVER_READY}");
    let (mut stream, _) = listener.accept().unwrap();
    stream.write_all(b"server\n").unwrap();
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

const LARGE_REPLY: usize = 1024 * 1024;

fn serve_large_reply(
    listener: &Listener,
    timeout: Duration,
) -> (Stream, String, std::io::Result<()>) {
    let (stream, _) = listener.accept().unwrap();
    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line).unwrap();
    stream.set_write_timeout(timeout).unwrap();
    let mut reply = vec![b'x'; LARGE_REPLY];
    reply.push(b'\n');
    let written = (&stream).write_all(&reply);
    (stream, line, written)
}

#[test]
fn large_reply_to_non_reading_client_times_out_and_does_not_block() {
    let place = place();
    let listener = Listener::bind(&place.name).unwrap();
    let timeout = Duration::from_millis(300);

    let silent = Stream::connect(&place.name, deadline()).unwrap();
    (&silent).write_all(b"silent\n").unwrap();
    let started = Instant::now();
    let (stream, line, written) = serve_large_reply(&listener, timeout);
    let error = written.unwrap_err();
    assert_eq!(line, "silent\n");
    assert!(
        matches!(error.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock),
        "{error:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "the write ends near its timeout: {:?}",
        started.elapsed()
    );
    let closing = Instant::now();
    drop(stream);
    assert!(
        closing.elapsed() < Duration::from_secs(1),
        "the end of the connection does not wait for the client: {:?}",
        closing.elapsed()
    );

    let next = std::thread::spawn(move || {
        let stream = Stream::connect(&place.name, deadline()).unwrap();
        (&stream).write_all(b"next\n").unwrap();
        let mut reply = Vec::new();
        (&stream).read_to_end(&mut reply).unwrap();
        reply.len()
    });
    let (stream, line, written) = serve_large_reply(&listener, timeout * 10);
    assert_eq!(line, "next\n");
    written.unwrap();
    drop(stream);
    assert_eq!(next.join().unwrap(), LARGE_REPLY + 1);

    let closing = Instant::now();
    drop(silent);
    drop(listener);
    assert!(closing.elapsed() < Duration::from_secs(1));
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
