//! A0 behavior oracles for the `horch` binary: `horch skills` and `horch
//! sessions` output, frozen in `tests/oracles/` (ARC-01).
//!
//! The oracle files never change. `HORCH_BLESS=1` writes them; that is for
//! the A0 baseline only, and a diff here means user-visible output changed.

use std::path::{Path, PathBuf};
use std::process::Command;

fn oracles() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/oracles")
}

/// Write `actual` under `HORCH_BLESS=1`; otherwise compare it with the
/// frozen file, which must exist.
fn check_oracle(rel: &str, actual: &str) {
    let path = oracles().join(rel);
    if std::env::var("HORCH_BLESS").ok().as_deref() == Some("1") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{}: {e}. Oracles are frozen at A0; an absent file is a failure",
            path.display()
        )
    });
    if want != actual {
        let line = want
            .lines()
            .zip(actual.lines())
            .position(|(a, b)| a != b)
            .unwrap_or_else(|| want.lines().count().min(actual.lines().count()));
        panic!(
            "{rel} differs from the A0 oracle at line {}:\n  want: {:?}\n  got:  {:?}\n\
             The oracle is frozen: fix the code, not the file.",
            line + 1,
            want.lines().nth(line),
            actual.lines().nth(line)
        );
    }
}

const PHASES: [&str; 4] = ["research", "plan", "implementation", "validation"];

/// The skills of the A0 oracle. Skills added later appear in the full
/// listing (`horch skills` without `--phase`) but belong to no phase, so the
/// frozen full-listing oracle holds these 16 skills only, with its metadata
/// totals blanked. A bless writes that same view, never the host listing.
const A0_SKILLS: [&str; 16] = [
    "brainstorm",
    "check",
    "code-analysis",
    "code-review",
    "create-plan",
    "debug",
    "document",
    "execute",
    "handoff",
    "orchestrate",
    "pre-flight",
    "research-codebase",
    "security-review",
    "skill-creator",
    "tdd",
    "trace",
];

/// `text` (a full `horch skills` listing, text or JSON) reduced to the A0
/// skills, with the metadata totals blanked.
fn a0_skills_view(text: &str) -> String {
    if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(text) {
        if let Some(skills) = v["skills"].as_array_mut() {
            skills.retain(|s| A0_SKILLS.contains(&s["name"].as_str().unwrap_or("")));
        }
        assert!(v["metadata_bytes"].is_u64() && v["metadata_tokens_estimate"].is_u64());
        v["metadata_bytes"] = serde_json::Value::Null;
        v["metadata_tokens_estimate"] = serde_json::Value::Null;
        return serde_json::to_string_pretty(&v).unwrap();
    }
    text.lines()
        .filter(|l| match l.strip_prefix("  ") {
            Some(rest) => A0_SKILLS.contains(&rest.split_whitespace().next().unwrap_or("")),
            None => true,
        })
        .map(|l| {
            if l.starts_with("Metadata: ") {
                let rest = l.split_once(" bytes (~").expect("metadata line").1;
                let tail = rest.split_once(" tokens").expect("metadata line").1;
                format!("Metadata: N bytes (~N tokens{tail}")
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The project path every session fixture is installed under.
const PROJECT: &str = "/oracle/project";

/// A temp home and state root, and a `horch` command that sees only them.
struct World {
    _tmp: tempfile::TempDir,
    root: PathBuf,
}

impl World {
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("home")).unwrap();
        std::fs::create_dir_all(root.join("state")).unwrap();
        World { _tmp: tmp, root }
    }

    fn state(&self) -> PathBuf {
        self.root.join("state")
    }

    fn horch(&self, args: &[&str]) -> String {
        let teammates = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../teammates");
        let out = Command::new(env!("CARGO_BIN_EXE_horch"))
            .args(args)
            .current_dir(&self.root)
            .env("HOME", self.root.join("home"))
            .env("HORCH_STATE_DIR", self.state())
            .env("HORCH_PROJECT_DIR", PROJECT)
            .env("HORCH_TEAMMATES_DIR", teammates)
            .env_remove("ANTHROPIC_API_KEY")
            .env_remove("XDG_STATE_HOME")
            .env_remove("HORCH_NOW")
            .env_remove("HORCH_BALANCE")
            .env_remove("HORCH_WORKSPACE_ID")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "horch {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let text = String::from_utf8(out.stdout).unwrap();
        text.replace(&*self.root.to_string_lossy(), "<TMP>")
    }
}

#[test]
fn oracle_cli_skills_match() {
    let w = World::new();
    for (rel, args) in [
        ("skills/skills.txt", &["skills"][..]),
        ("skills/skills-json.txt", &["skills", "--json"][..]),
    ] {
        check_oracle(rel, &a0_skills_view(&w.horch(args)));
    }
    for phase in PHASES {
        check_oracle(
            &format!("skills/skills-phase-{phase}.txt"),
            &w.horch(&["skills", "--phase", phase]),
        );
        check_oracle(
            &format!("skills/skills-phase-{phase}-json.txt"),
            &w.horch(&["skills", "--phase", phase, "--json"]),
        );
    }
}

#[test]
fn oracle_cli_sessions_match() {
    let fixtures =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../horch-core/tests/oracles/ledgers");
    for name in ["bash-era", "pre-effort", "pr14-substituted", "orchestrator"] {
        let w = World::new();
        let ledger = horch_core::execution::records::Ledger::for_project(w.state(), PROJECT);
        std::fs::copy(fixtures.join(format!("{name}.json")), ledger.path()).unwrap();
        check_oracle(
            &format!("sessions/sessions-{name}.txt"),
            &w.horch(&["sessions"]),
        );
        check_oracle(
            &format!("sessions/sessions-{name}-json.txt"),
            &w.horch(&["sessions", "--json"]),
        );
    }
}
