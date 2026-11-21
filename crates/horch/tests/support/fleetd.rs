//! A test fleetd: the real `horch fleetd` of 1 test, on the temporary
//! ledger and fleet root of that test.
//!
//! The `horch` commands of Phase 5 send their writes to fleetd as requests
//! (the channel, or a request file when fleetd is down). A CLI test of such
//! a command starts 1 [`TestFleetd`] for its temporary ledger. fleetd
//! serves the channel and applies the request files, and a drop stops it,
//! also when the test fails. 1 fleetd for each ledger: the `fleetd` row
//! has 1 row only.
//!
//! The socket path must be short (`sun_path` holds about 104 bytes on
//! macOS), so the fleet root of a test is in a short folder under `/tmp`.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The longest wait for fleetd to serve its channel. A slow host only makes
/// the wait longer: the test waits for the event, not for a time.
const START_WAIT: Duration = Duration::from_secs(120);

/// 1 `horch fleetd` process of a test.
pub struct TestFleetd {
    child: Child,
}

impl TestFleetd {
    /// Start `horch fleetd` with `cmd`, a command of the test's `horch`
    /// binary with the environment of the test (`HORCH_LEDGER_DB` or
    /// `HOME`, `FLEET_ROOT` when set). `fleet_root` is the fleet root that
    /// this environment gives. The output of fleetd goes to
    /// `<fleet_root>/fleetd.stdout` and `fleetd.stderr`. Returns when
    /// fleetd accepts a connection on its channel.
    ///
    /// The `PATH` of a test is short (`/usr/bin:/bin`), and there a bare
    /// `git` can be the Apple git, older than the 2.41 that the rescue and
    /// the start of fleetd need. Unless `cmd` sets `HORCH_GIT_BIN`, fleetd
    /// gets the first git of the real `PATH`, as the e2e harness does.
    pub fn start(mut cmd: Command, fleet_root: &Path) -> TestFleetd {
        std::fs::create_dir_all(fleet_root).unwrap();
        if !cmd.get_envs().any(|(k, _)| k == "HORCH_GIT_BIN") {
            if let Some(git) = git_on_real_path() {
                cmd.env("HORCH_GIT_BIN", git);
            }
        }
        let file = |ext: &str| std::fs::File::create(fleet_root.join(format!("fleetd.{ext}")));
        let child = cmd
            .arg("fleetd")
            .stdin(Stdio::null())
            .stdout(file("stdout").unwrap())
            .stderr(file("stderr").unwrap())
            .spawn()
            .expect("starting horch fleetd");
        let mut fleetd = TestFleetd { child };
        fleetd.wait_for_channel(fleet_root);
        fleetd
    }

    fn wait_for_channel(&mut self, fleet_root: &Path) {
        let socket = socket(fleet_root);
        let deadline = Instant::now() + START_WAIT;
        loop {
            let probe = Instant::now() + Duration::from_millis(100);
            if horch_platform::channel::Stream::connect(&socket, probe).is_ok() {
                return;
            }
            if let Some(status) = self.child.try_wait().unwrap() {
                let err = std::fs::read_to_string(fleet_root.join("fleetd.stderr"));
                panic!("the test fleetd exited ({status}) before it served its channel: {err:?}");
            }
            assert!(
                Instant::now() < deadline,
                "the test fleetd did not serve {} in {START_WAIT:?}",
                socket.display()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for TestFleetd {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The first `git` of the `PATH` of this test process.
fn git_on_real_path() -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|d| d.join("git"))
        .find(|p| p.is_file())
}

/// The channel of the fleet at `fleet_root`.
fn socket(fleet_root: &Path) -> PathBuf {
    fleet_root.join("run").join("fleetd.sock")
}

/// A new short temporary folder under `/tmp` (mode 0700 on unix), for a
/// ledger and a fleet root whose socket path fits `sun_path`.
pub fn short_tempdir() -> tempfile::TempDir {
    let mut b = tempfile::Builder::new();
    b.prefix("hf");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        b.permissions(std::fs::Permissions::from_mode(0o700));
        b.tempdir_in("/tmp").unwrap()
    }
    #[cfg(not(unix))]
    b.tempdir().unwrap()
}
