#[cfg(unix)]
mod unix {
    use std::io::{BufRead, BufReader, ErrorKind, Write};
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use horch_platform::ipc::{LocalListener, LocalStream};

    fn socket_in(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().join("s.sock")
    }

    fn short_dir() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("ipc")
            .tempdir_in("/tmp")
            .unwrap()
    }

    #[test]
    fn connect_write_read_a_line_within_a_deadline() {
        let dir = short_dir();
        let path = socket_in(&dir);
        let listener = LocalListener::bind(&path).unwrap();
        let echo = std::thread::spawn(move || {
            let stream = listener.accept().unwrap();
            stream.set_timeouts(Duration::from_secs(5)).unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            (&stream).write_all(line.as_bytes()).unwrap();
        });

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut stream = LocalStream::connect_by(&path, deadline).unwrap();
        stream.write_by(b"hello\n", deadline).unwrap();
        let answer = stream.read_line_by(deadline, 64 * 1024).unwrap();
        assert_eq!(answer, "hello\n");
        echo.join().unwrap();
    }

    #[test]
    fn connect_to_a_missing_socket_fails_at_once() {
        let dir = short_dir();
        let path = socket_in(&dir);
        let start = Instant::now();
        let error = LocalStream::connect_by(&path, start + Duration::from_secs(5)).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::NotFound);
        assert!(start.elapsed() < Duration::from_secs(1));
        assert_eq!(
            LocalStream::connect(&path).unwrap_err().kind(),
            ErrorKind::NotFound
        );
    }

    #[test]
    fn read_line_by_times_out_at_the_deadline() {
        let dir = short_dir();
        let path = socket_in(&dir);
        let listener = LocalListener::bind(&path).unwrap();
        let wait = Duration::from_millis(300);
        let start = Instant::now();
        let mut stream = LocalStream::connect_by(&path, start + wait).unwrap();
        let _peer = listener.accept().unwrap();
        let error = stream.read_line_by(start + wait, 64 * 1024).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::TimedOut);
        let elapsed = start.elapsed();
        assert!(elapsed >= wait, "returned early: {elapsed:?}");
        assert!(
            elapsed < wait + Duration::from_secs(1),
            "too late: {elapsed:?}"
        );
        for _ in 0..20 {
            let deadline = Instant::now() + Duration::from_micros(1_950);
            let error = stream.read_line_by(deadline, 64 * 1024).unwrap_err();
            let end = Instant::now();
            assert_eq!(error.kind(), ErrorKind::TimedOut);
            assert!(
                end >= deadline,
                "returned {:?} early",
                deadline.duration_since(end)
            );
        }
    }

    fn peer_that_writes(bytes: &'static [u8]) -> (tempfile::TempDir, PathBuf) {
        let dir = short_dir();
        let path = socket_in(&dir);
        let listener = LocalListener::bind(&path).unwrap();
        std::thread::spawn(move || {
            let mut stream = listener.accept().unwrap();
            stream.write_all(bytes).unwrap();
            std::thread::sleep(Duration::from_secs(5));
        });
        (dir, path)
    }

    #[test]
    fn read_line_by_stops_at_the_first_newline() {
        let (_dir, path) = peer_that_writes(b"one\ntwo\n");
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut stream = LocalStream::connect_by(&path, deadline).unwrap();
        assert_eq!(stream.read_line_by(deadline, 1024).unwrap(), "one\n");
        assert_eq!(stream.read_line_by(deadline, 1024).unwrap(), "two\n");
    }

    #[test]
    fn read_line_by_returns_at_most_limit_bytes() {
        let (_dir, path) = peer_that_writes(b"abcdefghij\n");
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut stream = LocalStream::connect_by(&path, deadline).unwrap();
        assert_eq!(stream.read_line_by(deadline, 4).unwrap(), "abcd");
        assert_eq!(stream.read_line_by(deadline, 4).unwrap(), "efgh");
        assert_eq!(stream.read_line_by(deadline, 1024).unwrap(), "ij\n");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn connect_by_waits_while_the_backlog_is_full() {
        use std::os::unix::io::AsRawFd;
        use std::os::unix::net::UnixListener;

        let dir = short_dir();
        let path = socket_in(&dir);
        let listener = UnixListener::bind(&path).unwrap();
        // SAFETY: plain syscall on the socket that `listener` owns. A
        // backlog of 0 holds 1 connection that is not accepted.
        assert_eq!(unsafe { libc::listen(listener.as_raw_fd(), 0) }, 0);
        let long = Instant::now() + Duration::from_secs(5);
        let _first = LocalStream::connect_by(&path, long).unwrap();

        let deadline = Instant::now() + Duration::from_millis(200);
        let error = LocalStream::connect_by(&path, deadline).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::TimedOut, "{error:?}");
        assert!(Instant::now() >= deadline, "returned before the deadline");

        let accept = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            let accepted = listener.accept().unwrap();
            (listener, accepted)
        });
        let long = Instant::now() + Duration::from_secs(5);
        let second = LocalStream::connect_by(&path, long);
        let _open = accept.join().unwrap();
        second.unwrap();
    }

    #[test]
    fn socket_path_too_long_is_refused() {
        let dir = short_dir();
        let path = dir.path().join("x".repeat(200));
        let deadline = Instant::now() + Duration::from_secs(1);
        let error = LocalStream::connect_by(&path, deadline).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidInput);
        assert!(LocalListener::bind(&path).is_err());
    }

    #[test]
    fn a_plain_file_is_not_a_socket() {
        let dir = short_dir();
        let path = socket_in(&dir);
        std::fs::write(&path, b"").unwrap();
        let error = LocalStream::connect(&path).unwrap_err();
        #[cfg(target_os = "linux")]
        assert_eq!(error.kind(), ErrorKind::ConnectionRefused, "{error:?}");
        #[cfg(not(target_os = "linux"))]
        assert!(horch_platform::ipc::is_not_a_socket(&error), "{error:?}");
    }
}

#[cfg(windows)]
#[test]
fn local_sockets_are_not_supported_on_windows() {
    use std::time::{Duration, Instant};

    use horch_platform::ipc::{LocalListener, LocalStream};
    use horch_platform::NotSupported;

    let path = std::env::temp_dir().join("horch-ipc-test.sock");
    let deadline = Instant::now() + Duration::from_secs(1);
    let errors = [
        LocalStream::connect(&path).unwrap_err(),
        LocalStream::connect_by(&path, deadline).unwrap_err(),
        LocalListener::bind(&path).unwrap_err(),
    ];
    for error in errors {
        assert_eq!(error.kind(), std::io::ErrorKind::Unsupported);
        let gap = error
            .get_ref()
            .and_then(|e| e.downcast_ref::<NotSupported>());
        assert_eq!(gap, Some(&NotSupported::new("ipc", "windows")));
    }
}

#[test]
fn local_sockets_are_supported_only_on_unix() {
    assert_eq!(horch_platform::ipc::is_supported(), cfg!(unix));
}
