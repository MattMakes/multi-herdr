//! `horch skills path`: stable paths for bundled and installed skill files.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct World {
    _tmp: tempfile::TempDir,
    root: PathBuf,
}

impl World {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        Self { _tmp: tmp, root }
    }

    fn run(&self, args: &[&str]) -> Output {
        let teammates = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../teammates");
        Command::new(env!("CARGO_BIN_EXE_horch"))
            .args(args)
            .current_dir(&self.root)
            .env_clear()
            .env("HOME", self.root.join("home"))
            .env("XDG_DATA_HOME", self.root.join("data"))
            .env("HORCH_STATE_DIR", self.root.join("state"))
            .env("HORCH_PROJECT_DIR", "/oracle/project")
            .env("HORCH_TEAMMATES_DIR", teammates)
            .env("HORCH_GIT_BIN", "/nonexistent")
            .output()
            .unwrap()
    }

    fn path(&self, args: &[&str]) -> PathBuf {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "horch {args:?} failed: {}",
            text(&out)
        );
        PathBuf::from(String::from_utf8(out.stdout).unwrap().trim())
    }
}

fn text(out: &Output) -> String {
    format!(
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn bundled_skill_has_a_stable_content_addressed_path() {
    let w = World::new();
    let first = w.path(&["skills", "path", "tdd"]);
    assert!(first.is_absolute(), "{}", first.display());
    assert!(first.join("SKILL.md").is_file(), "{}", first.display());
    assert!(first.starts_with(w.root.join("state/skills/tdd")));
    let digest = first.file_name().unwrap().to_string_lossy();
    assert_eq!(digest.len(), 64);
    assert!(digest.bytes().all(|byte| byte.is_ascii_hexdigit()));

    let second = w.path(&["skills", "path", "tdd"]);
    assert_eq!(second, first);
}

#[test]
fn bundled_script_path_exists_and_is_executable() {
    let w = World::new();
    let script = w.path(&[
        "skills",
        "path",
        "video-audio",
        "--script",
        "scripts/check_audio.py",
    ]);
    assert!(script.is_file(), "{}", script.display());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_ne!(script.metadata().unwrap().permissions().mode() & 0o111, 0);
    }
}

#[test]
fn missing_skill_or_script_exits_one_without_printing_a_path() {
    let w = World::new();
    for (args, message) in [
        (
            &["skills", "path", "no-such-skill"][..],
            "no skill 'no-such-skill'",
        ),
        (
            &[
                "skills",
                "path",
                "video-audio",
                "--script",
                "scripts/no-such-script.py",
            ][..],
            "has no file 'scripts/no-such-script.py'",
        ),
    ] {
        let out = w.run(args);
        assert_eq!(out.status.code(), Some(1), "{}", text(&out));
        assert!(out.stdout.is_empty(), "{}", text(&out));
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(message),
            "{}",
            text(&out)
        );
    }
}

#[test]
fn installed_skill_returns_its_install_path() {
    let w = World::new();
    let source = w.root.join("source/demo");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(
        source.join("SKILL.md"),
        "---\nname: demo\ndescription: A demo skill.\n---\nBody\n",
    )
    .unwrap();
    let source_arg = source.to_str().unwrap();
    let installed = w.run(&["skills", "install", source_arg]);
    assert!(installed.status.success(), "{}", text(&installed));

    let path = w.path(&["skills", "path", "demo"]);
    assert!(path.is_absolute(), "{}", path.display());
    assert!(path.join("SKILL.md").is_file(), "{}", path.display());
    assert!(path.starts_with(w.root.join("data/horch/skills/demo")));
}
