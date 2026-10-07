//! `horch skills read` (SKL-09): a worker prints a catalog skill's file
//! that its bundle does not hold. Every command runs with a temp home and
//! data root.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A temp home, state root and data root, and a `horch` that sees only them.
struct World {
    _tmp: tempfile::TempDir,
    root: PathBuf,
}

impl World {
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        World { _tmp: tmp, root }
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

    fn horch(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "horch {args:?} failed: {}",
            text(&out)
        );
        String::from_utf8(out.stdout).unwrap()
    }
}

fn text(o: &Output) -> String {
    format!(
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn repo_skill(rel: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../skills")
            .join(rel),
    )
    .unwrap()
}

/// SKL-09: without a file, `read` prints the bundled SKILL.md byte for byte.
#[test]
fn skl_09_read_prints_skill_md() {
    let w = World::new();
    let got = w.horch(&["skills", "read", "godot-save-load"]);
    assert_eq!(got, repo_skill("godot-save-load/SKILL.md"));
}

/// SKL-09: a file relative to the skill directory prints that file, for a
/// bundled skill and for an installed marketplace skill.
#[test]
fn skl_09_read_prints_a_named_file() {
    let w = World::new();
    let got = w.horch(&[
        "skills",
        "read",
        "godot-save-load",
        "references/json-saves.md",
    ]);
    assert_eq!(got, repo_skill("godot-save-load/references/json-saves.md"));
    let got = w.horch(&[
        "skills",
        "read",
        "godot-save-load",
        "./references/json-saves.md",
    ]);
    assert_eq!(got, repo_skill("godot-save-load/references/json-saves.md"));

    let src = w.root.join("src/demo");
    std::fs::create_dir_all(src.join("references")).unwrap();
    std::fs::write(
        src.join("SKILL.md"),
        "---\nname: demo\ndescription: A local demo.\n---\nBody\n",
    )
    .unwrap();
    std::fs::write(src.join("references/notes.md"), "notes\n").unwrap();
    w.horch(&["skills", "install", src.to_str().unwrap()]);
    assert!(w.horch(&["skills", "read", "demo"]).ends_with("Body\n"));
    assert_eq!(
        w.horch(&["skills", "read", "demo", "references/notes.md"]),
        "notes\n"
    );
}

/// SKL-09: `--files` lists the skill's files, one per line, sorted.
#[test]
fn skl_09_read_lists_files() {
    let w = World::new();
    let got = w.horch(&["skills", "read", "godot-save-load", "--files"]);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/godot-save-load");
    let mut want: Vec<String> = std::fs::read_dir(dir.join("references"))
        .unwrap()
        .map(|e| format!("references/{}", e.unwrap().file_name().to_string_lossy()))
        .collect();
    want.push("SKILL.md".into());
    want.sort();
    assert_eq!(got.lines().collect::<Vec<_>>(), want);
}

/// SKL-09: an unknown skill or file, an absolute path and a `..` path exit
/// 1 with a message on stderr, and print nothing on stdout.
#[test]
fn skl_09_read_rejects_unknown_and_dotdot() {
    let w = World::new();
    for (args, needle) in [
        (
            &["skills", "read", "no-such-skill"][..],
            "no skill 'no-such-skill'",
        ),
        (
            &["skills", "read", "tdd", "nope.md"][..],
            "has no file 'nope.md'",
        ),
        (
            &["skills", "read", "tdd", "/etc/passwd"][..],
            "not a path inside",
        ),
        (
            &["skills", "read", "tdd", "../check/SKILL.md"][..],
            "not a path inside",
        ),
        (
            &["skills", "read", "tdd", "a/../SKILL.md"][..],
            "not a path inside",
        ),
    ] {
        let out = w.run(args);
        assert_eq!(out.status.code(), Some(1), "{args:?}: {}", text(&out));
        assert!(out.stdout.is_empty(), "{args:?}: {}", text(&out));
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(needle),
            "{args:?}: {}",
            text(&out)
        );
    }
}
