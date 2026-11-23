use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const START_WAIT: Duration = Duration::from_secs(120);
const HEALTHY_MACHINE: &str = r#"{"os":"linux","arch":"x86_64","cpus":8,"load_average_1m":1.0,"mem_total_bytes":17179869184,"mem_available_bytes":8589934592,"disk_free_bytes":214748364800,"disk_total_bytes":536870912000,"gpu":"none","max_open_files":1048576,"max_processes":100000}"#;

pub fn healthy_machine_file(dir: &Path) -> PathBuf {
    let file = dir.join("healthy-machine.json");
    std::fs::write(&file, HEALTHY_MACHINE).unwrap();
    file
}

pub struct TestFleetd {
    child: Child,
}

impl TestFleetd {
    pub fn start(mut cmd: Command, fleet_root: &Path) -> TestFleetd {
        std::fs::create_dir_all(fleet_root).unwrap();
        if !cmd.get_envs().any(|(k, _)| k == "HORCH_MACHINE_FILE") {
            cmd.env("HORCH_MACHINE_FILE", healthy_machine_file(fleet_root));
        }
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
    let path = std::env::var_os("PATH");
    let pathext = std::env::var("PATHEXT").ok();
    horch_core::runtime::process::which(path.as_deref(), pathext.as_deref(), "git")
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
