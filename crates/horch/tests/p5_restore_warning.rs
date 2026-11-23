use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fake_herdr(dir: &Path) -> PathBuf {
    let bin = dir.join("herdr");
    std::fs::write(
        &bin,
        "#!/bin/sh\nprintf '%s\\n' '{\"result\":{\"workspaces\":[],\"panes\":[]}}'\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    bin
}

fn doctor(root: &Path, herdr_config: &Path) -> Output {
    let teammates = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../teammates");
    Command::new(env!("CARGO_BIN_EXE_horch"))
        .arg("doctor")
        .current_dir(root)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", root.join("home"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("HORCH_DATA_DIR", root.join("data"))
        .env("HORCH_STATE_DIR", root.join("state"))
        .env("HORCH_LEDGER_DB", root.join("ledger.sqlite3"))
        .env("HORCH_PROJECT_DIR", root.join("project"))
        .env("HORCH_TEAMMATES_DIR", teammates)
        .env("HORCH_GIT_BIN", "/nonexistent")
        .env("HORCH_HERDR_BIN", fake_herdr(root))
        .env("HERDR_CONFIG_PATH", herdr_config)
        .output()
        .unwrap()
}

const WARNING: &str = "warning: herdr resumes agent panes by itself after a herdr server restart";

#[test]
fn pres_06_s2_warns_on_herdr_resume_agents_on_restore() {
    let tmp = tempfile::tempdir().unwrap();
    let root = horch_platform::path::canonicalize(tmp.path()).unwrap();
    std::fs::create_dir_all(root.join("project")).unwrap();

    let on = root.join("on.toml");
    std::fs::write(&on, "[session]\nresume_agents_on_restore = true\n").unwrap();
    let out = doctor(&root, &on);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(WARNING),
        "no restore warning with the restore on; stderr:\n{stderr}"
    );
    assert!(stderr.contains("resume_agents_on_restore = false under [session]"));

    let off = root.join("off.toml");
    std::fs::write(&off, "[session]\nresume_agents_on_restore = false\n").unwrap();
    let out = doctor(&root, &off);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains(WARNING),
        "a restore warning with the restore off; stderr:\n{stderr}"
    );
}
