//! No greedy kills (docs/adr/0002, #35): nothing in this repository stops a
//! process by a name, a pattern or text from a transcript. A worker is
//! stopped only by `horch ledger stop`, which signals the exact PIDs its
//! ledger row records.
//!
//! The scan covers the Rust sources, the scripts, the justfile, the teammate
//! briefings and the bundled skills. A line that only forbids a greedy kill
//! is listed in [`ALLOWED`] with its reason; the list only shrinks.

use std::path::{Path, PathBuf};

/// (path from the repo root, a substring of the line, why it may stay).
const ALLOWED: &[(&str, &str, &str)] = &[
    (
        "skills/godot-build-verify/SKILL.md",
        "Never use `pkill` or `killall` with a",
        "forbids the pattern kill",
    ),
    (
        "teammates/_base/fleet-worker.md",
        "Never run pkill or killall.",
        "forbids the pattern kill",
    ),
    (
        "teammates/_base/fleet-orchestrator.md",
        "Never run pkill, killall",
        "forbids the pattern kill",
    ),
    (
        "crates/horch-core/tests/golden_prompts.rs",
        "Never run pkill",
        "pins the briefing lines that forbid the pattern kill",
    ),
    (
        "crates/horch-core/tests/no_greedy_kills.rs",
        "",
        "this test names the patterns it bans",
    ),
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/horch-core is 2 levels below the root")
        .to_path_buf()
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if name == "target" || name == ".git" || name == "node_modules" || name == "worktrees" {
            continue;
        }
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// Why `line` is a greedy kill, or `None`.
fn greedy(line: &str) -> Option<&'static str> {
    let l = line.to_lowercase();
    if l.contains("pkill") {
        return Some("pkill matches processes by name or pattern");
    }
    if l.contains("killall") {
        return Some("killall matches processes by name");
    }
    if l.contains("taskkill") && (l.contains("/im") || l.contains("/fi")) {
        return Some("taskkill /IM or /FI matches processes by name");
    }
    if l.contains("xargs kill") || l.contains("xargs -r kill") {
        return Some("xargs kill stops whatever a search printed");
    }
    let finds = [
        "$(pgrep", "`pgrep", "$(pidof", "`pidof", "pgrep -f", "| grep",
    ];
    if has_word(&l, "kill") && finds.iter().any(|f| l.contains(f)) && !l.contains("pgrep -p") {
        return Some("a kill of PIDs found by a name or pattern search");
    }
    None
}

/// Whether `word` stands alone in `l` (not inside `skill`).
fn has_word(l: &str, word: &str) -> bool {
    l.match_indices(word).any(|(i, _)| {
        let before = l[..i].chars().next_back();
        let after = l[i + word.len()..].chars().next();
        !before.is_some_and(|c| c.is_alphanumeric() || c == '_')
            && !after.is_some_and(|c| c.is_alphanumeric() || c == '_')
    })
}

fn allowed(rel: &str, line: &str) -> bool {
    ALLOWED
        .iter()
        .any(|(p, needle, _)| *p == rel && line.contains(needle))
}

fn scanned(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for dir in ["crates", "scripts", "teammates", "skills", ".claude/skills"] {
        walk(&root.join(dir), &mut files);
    }
    files.push(root.join("justfile"));
    files.retain(|p| {
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        matches!(
            ext,
            "rs" | "sh" | "md" | "py" | "toml" | "yaml" | "yml" | "json" | "ps1" | "cfg"
        ) || name == "justfile"
            || ext.is_empty()
    });
    files
}

#[test]
fn nothing_stops_a_process_by_name_or_pattern() {
    let root = repo_root();
    let mut found = Vec::new();
    let mut used = vec![false; ALLOWED.len()];
    for path in scanned(&root) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let rel = path
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        for (n, line) in text.lines().enumerate() {
            if let Some(why) = greedy(line) {
                if let Some(i) = ALLOWED
                    .iter()
                    .position(|(p, needle, _)| *p == rel && line.contains(needle))
                {
                    used[i] = true;
                    continue;
                }
                found.push(format!("{rel}:{}: {why}: {}", n + 1, line.trim()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "greedy kills (stop 1 recorded PID instead; see docs/adr/0002):\n{}",
        found.join("\n")
    );
    for (i, (p, needle, _)) in ALLOWED.iter().enumerate() {
        assert!(
            used[i],
            "ALLOWED entry {p} '{needle}' matches nothing any more; remove it"
        );
    }
}

#[test]
fn the_scan_catches_a_planted_pattern_kill() {
    assert!(greedy("pkill -f worker").is_some());
    assert!(greedy("killall codex").is_some());
    assert!(greedy("taskkill /IM claude.exe /F").is_some());
    assert!(greedy("kill $(pgrep -f horch)").is_some());
    assert!(greedy("ps aux | grep codex | awk '{print $2}' | xargs kill").is_some());
    assert!(!allowed("teammates/x.md", "pkill -f worker"));
    // Stopping 1 recorded PID is fine.
    assert!(greedy("kill \"$pid\"").is_none());
    assert!(greedy("kill \"$(cat \"{temp_dir}/game.pid\")\"").is_none());
    assert!(greedy("oc=\"$(pgrep -P \"$sp\" | head -1)\"").is_none());
    assert!(greedy("ls skills | grep godot").is_none());
}
