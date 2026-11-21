use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const START_WAIT: Duration = Duration::from_secs(120);

pub struct TestFleetd {
    child: Child,
}

impl TestFleetd {
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

fn git_on_real_path() -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|d| d.join("git"))
        .find(|p| p.is_file())
}

fn socket(fleet_root: &Path) -> PathBuf {
    fleet_root.join("run").join("fleetd.sock")
}

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
