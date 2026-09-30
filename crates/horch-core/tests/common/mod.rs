//! Installing the telemetry fixture corpus into a temp home and state root.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use horch_core::usage::Locations;

pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/telemetry")
}

pub struct World {
    pub _tmp: tempfile::TempDir,
    pub home: PathBuf,
    pub state: PathBuf,
    pub loc: Locations,
}

/// Which part of every transcript to install (EXPECTED.md stages).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Whole,
    FirstHalf,
}

pub fn locations(home: &Path) -> Locations {
    Locations {
        home: home.to_path_buf(),
        claude_projects: home.join(".claude/projects"),
        codex_sessions: home.join(".codex/sessions"),
        pi_sessions: home.join(".pi/agent/sessions"),
        opencode_db: home.join(".local/share/opencode/opencode.db"),
    }
}

/// Where each fixture subtree lands.
pub fn targets(home: &Path, state: &Path) -> Vec<(PathBuf, PathBuf)> {
    let f = fixtures();
    vec![
        (f.join("claude"), home.join(".claude/projects")),
        (f.join("codex/sessions"), home.join(".codex/sessions")),
        (f.join("pi/sessions"), home.join(".pi/agent/sessions")),
        (f.join("prime"), state.join("prime")),
    ]
}

pub fn world(part: Part) -> World {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let state = tmp.path().join("state");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&state).unwrap();
    for (from, to) in targets(&home, &state) {
        copy(&from, &to, part);
    }
    install_ledgers(&state);
    let _ = build_opencode_db(&home.join(".local/share/opencode/opencode.db"));
    let loc = locations(&home);
    World {
        _tmp: tmp,
        home,
        state,
        loc,
    }
}

pub fn install_ledgers(state: &Path) {
    for name in ["-work-alpha.json", "-work-beta.json"] {
        let text = std::fs::read_to_string(fixtures().join("ledgers").join(name)).unwrap();
        let text = text.replace("{STATE}", &state.to_string_lossy());
        std::fs::write(state.join(name), text).unwrap();
    }
}

/// Copy a tree; `.jsonl` files are cut to their first floor(n/2) lines for
/// [`Part::FirstHalf`].
pub fn copy(from: &Path, to: &Path, part: Part) {
    if from.is_dir() {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap().flatten() {
            copy(&e.path(), &to.join(e.file_name()), part);
        }
        return;
    }
    let text = std::fs::read_to_string(from).unwrap();
    let text =
        if part == Part::FirstHalf && from.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            first_half(&text)
        } else {
            text
        };
    std::fs::write(to, text).unwrap();
}

pub fn first_half(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[..lines.len() / 2]
        .iter()
        .map(|l| format!("{l}\n"))
        .collect()
}

/// `sqlite3` on PATH or at `HORCH_SQLITE3_BIN`.
pub fn sqlite3() -> Option<PathBuf> {
    let bin = horch_core::agent::sqlite3_bin();
    if bin.components().count() > 1 {
        return bin.is_file().then_some(bin);
    }
    horch_core::agent::which(&bin.to_string_lossy())
}

/// Build `opencode.db` from the SQL fixture. `None` without `sqlite3`.
pub fn build_opencode_db(db: &Path) -> Option<()> {
    let bin = sqlite3()?;
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    let sql = std::fs::read_to_string(fixtures().join("opencode/opencode.sql")).unwrap();
    let status = std::process::Command::new(bin)
        .arg(db)
        .arg(sql)
        .status()
        .unwrap();
    assert!(status.success(), "building the opencode fixture database");
    Some(())
}

/// Skip a test that needs `sqlite3`, unless `HORCH_REQUIRE_SQLITE=1`.
pub fn need_sqlite(test: &str) -> bool {
    if sqlite3().is_some() {
        return true;
    }
    if std::env::var("HORCH_REQUIRE_SQLITE").ok().as_deref() == Some("1") {
        panic!("{test}: sqlite3 is required (HORCH_REQUIRE_SQLITE=1) but not found");
    }
    eprintln!("{test}: skipped, sqlite3 not found");
    false
}

pub const O1: &str = "11111111-1111-4111-8111-111111111111";
pub const A2: &str = "22222222-2222-4222-8222-222222222222";
pub const A3: &str = "01a0e61c-d73e-74a3-837c-b5aade8b1c38";
pub const A4: &str = "01c0e61c-d73e-74a3-837c-b5aade8b1c38";
pub const A5: &str = "01b0e61c-d73e-74a3-837c-b5aade8b1c38";
pub const B1: &str = "33333333-3333-4333-8333-333333333333";

pub fn prime_sid(state: &Path) -> String {
    state
        .join("prime/prime-1-4444/sessions/44444444-4444-4444-8444-444444444444.jsonl")
        .to_string_lossy()
        .into_owned()
}
