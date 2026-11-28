use std::path::{Path, PathBuf};
use std::process::Command;

#[path = "../common/warm.rs"]
mod warm;

const FAKE: &str = "#!/bin/bash\nfor a in \"$@\"; do\n  if [ \"$a\" = warm ]; then exit 0; fi\n  if [ \"$a\" = --version ]; then echo 4.7.2.stable.fake; exit 0; fi\ndone\necho \"ARGS $*\"\nif [ -n \"${FAKE_LINES:-}\" ]; then printf '%b\\n' \"$FAKE_LINES\"; fi\nexit \"${FAKE_EXIT:-0}\"\n";

fn script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../skills/godot-build-verify/scripts/godot-check.sh")
}

fn run(project: &Path, engine: &Path, lines: &str, code: &str) -> std::process::Output {
    Command::new("bash")
        .arg(script())
        .current_dir(project)
        .env_remove("CODEX_SANDBOX")
        .env_remove("GODOT_MONO_PATH")
        .env("GODOT_PATH", engine)
        .env("FAKE_LINES", lines)
        .env("FAKE_EXIT", code)
        .output()
        .unwrap()
}

#[test]
fn grs_09_godot_check_fails_when_godot_exits_zero_with_script_or_gut_errors() {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(script()).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o755);
    let tmp = tempfile::tempdir().unwrap();
    let engine = tmp.path().join("fake-godot");
    warm::write_warmed(&engine, FAKE, &["warm"]);
    let project = tmp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(project.join("project.godot"), "config_version=5\n").unwrap();
    std::fs::write(project.join("a.gd"), "extends Node\n").unwrap();
    std::fs::write(project.join(".gutconfig.json"), "{}\n").unwrap();

    let clean = run(&project, &engine, "", "0");
    assert_eq!(clean.status.code(), Some(0), "{clean:?}");
    for line in [
        "\\x1b[31mSCRIPT ERROR\\x1b[0m: boom",
        "\\x1b[31mParse Error\\x1b[0m: bad",
        "ERROR: \\x1b[1mFailed to load script\\x1b[0m",
        "\\x1b[33mNothing\\x1b[0m was run",
    ] {
        let out = run(&project, &engine, line, "0");
        assert_eq!(out.status.code(), Some(3), "{line}: {out:?}");
    }
    let engine_fail = run(&project, &engine, "", "1");
    assert_eq!(engine_fail.status.code(), Some(1), "{engine_fail:?}");
}
