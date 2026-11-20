use std::path::{Path, PathBuf};
use std::process::Command;

/// The variables cargo sets for a test process. A nested build that sees
/// them has another fingerprint, and builds each crate again.
const CARGO_RUN_VARS: &[&str] = &[
    "CARGO_MANIFEST_DIR",
    "CARGO_MANIFEST_PATH",
    "CARGO_CRATE_NAME",
    "CARGO_BIN_NAME",
    "CARGO_PRIMARY_PACKAGE",
    "CARGO_TARGET_TMPDIR",
    "CARGO_RUSTC_CURRENT_DIR",
    "OUT_DIR",
];

/// cargo 1.100 runs a test from `<profile>/build/<package>/<hash>/out/`,
/// an older cargo from `<profile>/deps/` (T-59).
fn built_dir_of(exe: &Path) -> PathBuf {
    let dir = exe.parent().expect("exe dir");
    let name = |p: &Path| p.file_name().and_then(|n| n.to_str()).map(str::to_owned);
    if name(dir).as_deref() == Some("deps") {
        return dir.parent().expect("profile dir").to_path_buf();
    }
    let build = dir.ancestors().nth(3);
    if name(dir).as_deref() == Some("out") && build.and_then(name).as_deref() == Some("build") {
        return build
            .and_then(Path::parent)
            .expect("profile dir")
            .to_path_buf();
    }
    dir.to_path_buf()
}

fn cargo_profile(built: &Path) -> &str {
    match built.file_name().and_then(|n| n.to_str()) {
        Some("debug") | None => "dev",
        Some(profile) => profile,
    }
}

fn build_command(built: &Path, fakes: &[&str]) -> Command {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut cmd = Command::new(cargo);
    for (key, _) in std::env::vars_os() {
        let key = key.to_string_lossy();
        if key.starts_with("CARGO_PKG_") || CARGO_RUN_VARS.contains(&key.as_ref()) {
            cmd.env_remove(key.as_ref());
        }
    }
    cmd.current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .args(["build", "--locked", "-p", "horch-e2e"]);
    for bin in fakes {
        cmd.args(["--bin", bin]);
    }
    match cargo_profile(built) {
        "dev" => {}
        "release" => {
            cmd.arg("--release");
        }
        profile => {
            cmd.args(["--profile", profile]);
        }
    }
    cmd.arg("--target-dir")
        .arg(built.parent().expect("target dir"));
    cmd
}

/// The first call builds `fakes` and starts each 1 time: macOS holds a new
/// binary at its first launch, for minutes, and no bounded wait of a test
/// counts that hold.
pub fn fake(fakes: &[&str], name: &str) -> PathBuf {
    static BUILT: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    let dir = built_dir_of(&std::env::current_exe().unwrap());
    let path = |bin: &str| dir.join(format!("{bin}{}", std::env::consts::EXE_SUFFIX));
    BUILT.get_or_init(|| {
        let mut cmd = build_command(&dir, fakes);
        let out = cmd.output().unwrap();
        assert!(
            out.status.success(),
            "the build of the fakes failed: {cmd:?}\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        for bin in fakes {
            let status = Command::new(path(bin))
                .arg("--version")
                .env_remove("HORCH_FAKE_LOG")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .status()
                .unwrap();
            assert!(status.success(), "{bin} --version: {status}");
        }
    });
    path(name)
}

#[test]
fn the_fakes_take_the_profile_from_the_target_layout() {
    for (root, want) in [
        ("/w/target/debug", "dev"),
        ("/w/target/release", "release"),
        ("/w/target/ci", "ci"),
        ("/w/out/debug", "dev"),
    ] {
        for exe in [
            format!("{root}/build/horch-core/ce99708c1aeeb00b/out/p7_conformance-ce99708c1aeeb00b"),
            format!("{root}/deps/p7_conformance-ce99708c1aeeb00b"),
        ] {
            let built = built_dir_of(Path::new(&exe));
            assert_eq!(built, PathBuf::from(root), "{exe}");
            assert_eq!(cargo_profile(&built), want, "{exe}");
        }
    }
    let new =
        "/w/target/debug/build/horch-core/ce99708c1aeeb00b/out/p7_conformance-ce99708c1aeeb00b";
    let args: Vec<String> = build_command(&built_dir_of(Path::new(new)), &["fake-claude"])
        .get_args()
        .skip(6)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(args, ["--target-dir", "/w/target"]);
}
