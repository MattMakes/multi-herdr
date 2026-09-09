//! Codex-specific glue: execpolicy rules, and recovering a session id after
//! launch.
//!
//! Codex differs from Claude in two ways that matter to the fleet. It loads
//! execpolicy rules at startup, so the fleet commands must be explicitly allowed
//! to run outside its sandbox or a worker can neither report back nor shut itself
//! down. And it mints its session id itself, only revealing it after the process
//! starts, so the ledger has to harvest it.
//!
//! Rules are read from `$CODEX_HOME/rules/*.rules`, and there is no flag or
//! config key that points one launch at a different set - the only lever is
//! `CODEX_HOME` itself. So [`Rules::install`] gives each codex pane a private
//! one: a directory that symlinks every entry of the real codex home except
//! `rules/`, and whose `rules/` holds this launch's rules and nothing else.
//! Auth, config, skills, plugins and `sessions/` all resolve through the
//! symlinks to the real files, so a rollout still lands where the ledger's
//! harvest looks for it.
//!
//! The alternative - appending to the shared `rules/default.rules` - grants
//! every codex session on the machine whatever any fleet ever needed, forever.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use anyhow::{Context, Result};

use crate::prompts;
use crate::teammates::ExecRule;

/// Path to codex's shared rules file: `~/.codex/rules/default.rules`.
pub fn rules_path(home: &Path) -> PathBuf {
    home.join(".codex").join("rules").join("default.rules")
}

/// The directory codex writes rollout files under.
pub fn sessions_dir(home: &Path) -> PathBuf {
    home.join(".codex").join("sessions")
}

/// The codex home a launch inherits: `$CODEX_HOME`, else `<home>/.codex`.
pub fn codex_home(home: &Path) -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".codex"))
}

/// One codex launch's execpolicy rules.
///
/// Built before the CLI starts, applied to its [`Command`], and finished after
/// it exits. Hold it across the launch - dropping it early does nothing, but
/// [`Rules::finish`] is what removes the private home again.
#[derive(Debug)]
pub enum Rules {
    /// A private `CODEX_HOME` whose `rules/` holds only this launch's rules.
    Private { dir: PathBuf },
    /// Appended to the shared rules file, because this platform cannot make the
    /// symlinks a private home is built from.
    Shared,
}

impl Rules {
    /// Install `rules` for one launch, on behalf of the pane running `role`.
    ///
    /// `home` is the user's home directory, not the codex home - the same value
    /// [`sessions_dir`] takes. `role` only names the directory, so that anything
    /// left behind says which pane it belonged to.
    pub fn install(home: &Path, role: &str, rules: &[ExecRule]) -> Result<Rules> {
        install_rules(home, role, rules)
    }

    /// Point the agent's command at these rules.
    pub fn apply(&self, cmd: &mut Command) {
        if let Rules::Private { dir } = self {
            // On the child only. This process keeps the real CODEX_HOME, so the
            // ledger's harvest still reads the real sessions directory.
            cmd.env("CODEX_HOME", dir);
        }
    }

    /// Clean up after the agent has exited.
    ///
    /// A private home is removed - unless codex created something at its top
    /// level that was not there at launch. That would be state written to a
    /// directory nobody will look in again, so the directory is kept and named
    /// rather than silently deleted.
    pub fn finish(self) {
        let Rules::Private { dir } = self else {
            return;
        };
        let stranded = stranded_entries(&dir);
        if !stranded.is_empty() {
            eprintln!(
                "horch: codex wrote {} into its private CODEX_HOME, which was not linked \
                 there at launch; keeping {} rather than deleting it. If the real \
                 ~/.codex was empty or missing, that is codex's first-run state and \
                 belongs there.",
                stranded.join(", "),
                dir.display()
            );
            return;
        }
        // Every other entry is a symlink, and remove_dir_all removes the links
        // rather than following them - the real codex home is untouched.
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Top-level entries of a private home that are not symlinks and not `rules/`.
///
/// Everything the overlay creates is a symlink or `rules/`, so anything else is
/// something codex made: a file that did not exist in the real codex home when
/// the overlay was built.
fn stranded_entries(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_name() != "rules")
        .filter(|e| {
            !e.path()
                .symlink_metadata()
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false)
        })
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    out.sort();
    out
}

/// The rendered `prefix_rule` blocks for one launch.
fn render(rules: &[ExecRule]) -> String {
    rules.iter().map(prompts::codex_rule_block).collect()
}

#[cfg(not(windows))]
fn install_rules(home: &Path, role: &str, rules: &[ExecRule]) -> Result<Rules> {
    let dir = build_private_home(&codex_home(home), &crate::ledger::state_root(), role)?;
    std::fs::write(dir.join("rules").join("horch.rules"), render(rules))
        .with_context(|| format!("writing this launch's rules under {}", dir.display()))?;
    Ok(Rules::Private { dir })
}

/// Assemble a private codex home under `state_root`, mirroring `source`.
///
/// Rebuilt on every launch rather than cached, so an entry added to the real
/// codex home since the last fleet is linked rather than missing.
///
/// A pane killed outright never reaches [`Rules::finish`] and leaves its
/// directory behind. That is why `role` is in the name: the leftovers are
/// symlinks and one rules file, harmless to delete, and legible about which
/// pane they came from.
#[cfg(not(windows))]
fn build_private_home(source: &Path, state_root: &Path, role: &str) -> Result<PathBuf> {
    let slug: String = role
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
        .collect();
    let dir = state_root
        .join("codex-home")
        .join(format!("{slug}-{}", crate::mint_uuid()));
    std::fs::create_dir_all(dir.join("rules"))
        .with_context(|| format!("creating {}", dir.join("rules").display()))?;

    // A missing source is not an error: a machine with no codex home yet gets a
    // private one holding only the rules, which is what codex would have made.
    if let Ok(entries) = std::fs::read_dir(source) {
        for entry in entries.flatten() {
            if entry.file_name() == "rules" {
                continue;
            }
            let link = dir.join(entry.file_name());
            std::os::unix::fs::symlink(entry.path(), &link).with_context(|| {
                format!("linking {} into the private codex home", entry.path().display())
            })?;
        }
    }
    Ok(dir)
}

/// Windows has no private home: symlinks there need Developer Mode or an
/// elevated process, and a fleet that refused to start without one would be
/// worse than a shared rules file. So the rules go where they always went, and
/// the pane says so rather than implying an isolation it does not have.
#[cfg(windows)]
fn install_rules(home: &Path, _role: &str, rules: &[ExecRule]) -> Result<Rules> {
    ensure_rules(home, rules)?;
    eprintln!(
        "horch: codex execpolicy rules were added to the shared {} - on Windows they \
         apply to every codex session, not just this pane.",
        rules_path(home).display()
    );
    Ok(Rules::Shared)
}

/// Append any missing fleet `prefix_rule`s to codex's SHARED rules file.
///
/// Only Windows uses this, and only because it cannot build a private home. On
/// every other platform the rules go into one, and this file is left as the
/// user wrote it.
///
/// Idempotent: a rule whose pattern is already present is left alone, so the
/// user's own rules and formatting survive.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn ensure_rules(home: &Path, rules: &[ExecRule]) -> Result<()> {
    let path = rules_path(home);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let existing = std::fs::read_to_string(&path).unwrap_or_default();

    let mut appended = String::new();
    for rule in rules {
        // Match on the rendered pattern list, exactly as the bash `grep -qF` did.
        if existing.contains(&format!("[{}]", rule.pattern)) {
            continue;
        }
        appended.push_str(&prompts::codex_rule_block(rule));
    }
    if appended.is_empty() {
        return Ok(());
    }

    let mut next = existing;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(&appended);
    std::fs::write(&path, next).with_context(|| format!("writing {}", path.display()))
}

/// Pull the session uuid out of a rollout filename.
///
/// Codex names them `rollout-<timestamp>-<uuid>.jsonl`. Returns `None` for
/// anything that does not end in a well-formed uuid, which is what stops a
/// partially written or unrelated file from being recorded as a session.
pub fn session_id_from_rollout(file_name: &str) -> Option<&str> {
    let stem = file_name
        .strip_prefix("rollout-")?
        .strip_suffix(".jsonl")?;
    // The uuid is the trailing 36 characters, preceded by the separating dash.
    let candidate = stem.get(stem.len().checked_sub(36)?..)?;
    if stem.len() > 36 && !stem[..stem.len() - 36].ends_with('-') {
        return None;
    }
    is_uuid(candidate).then_some(candidate)
}

fn is_uuid(s: &str) -> bool {
    let groups: Vec<&str> = s.split('-').collect();
    if groups.len() != 5 {
        return false;
    }
    [8, 4, 4, 4, 12]
        .iter()
        .zip(&groups)
        .all(|(len, g)| g.len() == *len && g.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
}

/// A rollout file that could belong to this worker.
#[derive(Debug, Clone)]
pub struct RolloutCandidate {
    pub path: PathBuf,
    pub session_id: String,
    pub modified: SystemTime,
}

/// Rollout files created after `since` whose head names `project_dir` as its cwd,
/// newest first.
///
/// The cwd check is what keeps two projects' concurrent codex sessions apart; the
/// bash version grepped the first 16KB for the same JSON fragment.
pub fn find_rollouts(
    sessions_dir: &Path,
    project_dir: &str,
    since: SystemTime,
) -> Vec<RolloutCandidate> {
    let needle = format!("\"cwd\":{}", serde_json::Value::from(project_dir));
    let mut found = Vec::new();
    collect_rollouts(sessions_dir, since, &needle, &mut found);
    // Newest first, matching `ls -t`.
    found.sort_by(|a, b| b.modified.cmp(&a.modified));
    found
}

fn collect_rollouts(
    dir: &Path,
    since: SystemTime,
    needle: &str,
    out: &mut Vec<RolloutCandidate>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            collect_rollouts(&path, since, needle, out);
            continue;
        }
        if !meta.is_file() {
            continue;
        }
        let Ok(modified) = meta.modified() else { continue };
        if modified <= since {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(session_id) = session_id_from_rollout(name) else {
            continue;
        };
        if !head_contains(&path, needle) {
            continue;
        }
        out.push(RolloutCandidate {
            session_id: session_id.to_string(),
            path,
            modified,
        });
    }
}

/// Does the first 16KB of `path` contain `needle`?
fn head_contains(path: &Path, needle: &str) -> bool {
    use std::io::Read;
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut buf = vec![0u8; 16 * 1024];
    let Ok(read) = file.read(&mut buf) else {
        return false;
    };
    buf.truncate(read);
    String::from_utf8_lossy(&buf).contains(needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::teammates::Roster;
    use std::time::Duration;

    #[test]
    fn extracts_session_ids_from_rollout_filenames() {
        let uuid = "0f10e145-3a7f-4b21-9c8d-2552aabbccdd";
        assert_eq!(
            session_id_from_rollout(&format!("rollout-2026-07-21T10-30-00-{uuid}.jsonl")),
            Some(uuid)
        );
    }

    #[test]
    fn rejects_filenames_without_a_wellformed_uuid() {
        for name in [
            "rollout-2026-07-21-notauuid.jsonl",
            "rollout-.jsonl",
            "rollout-2026-07-21T10-30-00-0F10E145-3A7F-4B21-9C8D-2552AABBCCDD.jsonl",
            "session-0f10e145-3a7f-4b21-9c8d-2552aabbccdd.jsonl",
            "rollout-0f10e145-3a7f-4b21-9c8d-2552aabbccdd.json",
            // A truncated uuid must not be accepted.
            "rollout-x-0f10e145-3a7f-4b21-9c8d-2552aabbccd.jsonl",
        ] {
            assert_eq!(session_id_from_rollout(name), None, "should reject {name}");
        }
    }

    /// The bash version required the uuid to be dash-separated from the
    /// timestamp; a run-on prefix is not a session id.
    #[test]
    fn requires_a_separator_before_the_uuid() {
        let uuid = "0f10e145-3a7f-4b21-9c8d-2552aabbccdd";
        assert_eq!(
            session_id_from_rollout(&format!("rollout-stamp{uuid}.jsonl")),
            None
        );
    }

    fn write_rollout(dir: &Path, uuid: &str, cwd: &str) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(format!("rollout-2026-08-06T00-00-00-{uuid}.jsonl"));
        std::fs::write(&path, format!(r#"{{"cwd":"{cwd}","id":"x"}}"#)).unwrap();
        path
    }

    #[test]
    fn finds_only_rollouts_for_this_project() {
        let tmp = tempfile::tempdir().unwrap();
        let sessions = tmp.path().join("sessions");
        let since = SystemTime::now() - Duration::from_secs(60);

        write_rollout(&sessions.join("2026/08"), "aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa", "/proj/mine");
        write_rollout(&sessions.join("2026/08"), "bbbbbbbb-bbbb-4bbb-bbbb-bbbbbbbbbbbb", "/proj/other");

        let found = find_rollouts(&sessions, "/proj/mine", since);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].session_id, "aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa");
    }

    /// Sessions that predate this worker's launch belong to someone else.
    #[test]
    fn ignores_rollouts_older_than_the_launch_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let sessions = tmp.path().join("sessions");
        write_rollout(&sessions, "aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa", "/proj/mine");

        let future = SystemTime::now() + Duration::from_secs(3600);
        assert!(find_rollouts(&sessions, "/proj/mine", future).is_empty());
    }

    /// A project path containing characters that need JSON escaping must still
    /// match, which is why the needle is built with a JSON encoder.
    #[test]
    fn matches_project_paths_that_need_json_escaping() {
        let tmp = tempfile::tempdir().unwrap();
        let sessions = tmp.path().join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        let uuid = "cccccccc-cccc-4ccc-cccc-cccccccccccc";
        std::fs::write(
            sessions.join(format!("rollout-2026-08-06T00-00-00-{uuid}.jsonl")),
            r#"{"cwd":"C:\\Users\\a b\\proj"}"#,
        )
        .unwrap();

        let since = SystemTime::now() - Duration::from_secs(60);
        let found = find_rollouts(&sessions, r"C:\Users\a b\proj", since);
        assert_eq!(found.len(), 1, "{found:#?}");
    }

    /// A fake codex home with the entries a real one has: a config file, an
    /// auth file, a directory codex writes into, and the user's own rules.
    fn fake_codex_home() -> tempfile::TempDir {
        let home = tempfile::tempdir().unwrap();
        let codex = home.path().join(".codex");
        std::fs::create_dir_all(codex.join("sessions")).unwrap();
        std::fs::create_dir_all(codex.join("rules")).unwrap();
        std::fs::write(codex.join("config.toml"), "model = \"gpt-5.6-sol\"\n").unwrap();
        std::fs::write(codex.join("auth.json"), "{}").unwrap();
        std::fs::write(codex.join("rules/default.rules"), "# the user's own\n").unwrap();
        home
    }

    /// The private home mirrors everything EXCEPT rules, so codex finds the
    /// operator's auth, config and skills but only this launch's rules.
    #[test]
    fn a_private_home_shares_everything_but_the_rules() {
        let home = fake_codex_home();
        let state = tempfile::tempdir().unwrap();
        let dir = build_private_home(&home.path().join(".codex"), state.path(), "codex-sol-1").unwrap();

        for shared in ["config.toml", "auth.json", "sessions"] {
            assert!(
                dir.join(shared).symlink_metadata().unwrap().file_type().is_symlink(),
                "{shared} must be a symlink to the real codex home"
            );
        }
        assert!(dir.join("rules").is_dir());
        assert!(
            !dir.join("rules").symlink_metadata().unwrap().file_type().is_symlink(),
            "rules must be this launch's own directory, not the shared one"
        );
        assert!(!dir.join("rules/default.rules").exists(), "the user's rules must not load");
        // Reading through the link reaches the real file.
        assert_eq!(
            std::fs::read_to_string(dir.join("config.toml")).unwrap(),
            "model = \"gpt-5.6-sol\"\n"
        );
    }

    /// Two launches get different rules, and neither can see the other's.
    #[test]
    fn each_launch_gets_only_its_own_rules() {
        let home = fake_codex_home();
        let state = tempfile::tempdir().unwrap();
        let roster = Roster::builtin().unwrap();

        let make = |rules: &[ExecRule]| {
            let dir = build_private_home(&home.path().join(".codex"), state.path(), "codex-sol-1").unwrap();
            std::fs::write(dir.join("rules/horch.rules"), render(rules)).unwrap();
            dir
        };
        let worker = make(roster.exec_rules());
        let orchestrator = make(roster.orchestrator_exec_rules());
        assert_ne!(worker, orchestrator, "each launch needs its own directory");

        let read = |d: &Path| std::fs::read_to_string(d.join("rules/horch.rules")).unwrap();
        assert!(read(&worker).contains(r#"pattern = ["horch", "done"]"#));
        assert!(!read(&worker).contains(r#"pattern = ["horch", "spawn"]"#));
        assert!(read(&orchestrator).contains(r#"pattern = ["horch", "spawn"]"#));
        assert!(!read(&orchestrator).contains(r#"pattern = ["horch", "done"]"#));

        // And the shared file the operator owns was never touched.
        assert_eq!(
            std::fs::read_to_string(home.path().join(".codex/rules/default.rules")).unwrap(),
            "# the user's own\n"
        );
    }

    /// The load-bearing safety property: cleanup removes the LINKS, never what
    /// they point at. If this regressed it would delete the operator's real
    /// codex home - auth, sessions and all - when a codex pane exited.
    #[test]
    fn finishing_a_private_home_cannot_delete_the_real_one() {
        let home = fake_codex_home();
        let state = tempfile::tempdir().unwrap();
        let real = home.path().join(".codex");
        let dir = build_private_home(&real, state.path(), "codex-sol-1").unwrap();
        std::fs::write(dir.join("rules/horch.rules"), "# rules\n").unwrap();

        Rules::Private { dir: dir.clone() }.finish();

        assert!(!dir.exists(), "the private home should be gone");
        assert!(real.join("config.toml").is_file(), "the real config survived");
        assert!(real.join("auth.json").is_file(), "the real auth survived");
        assert!(real.join("sessions").is_dir(), "the real sessions dir survived");
        assert_eq!(
            std::fs::read_to_string(real.join("rules/default.rules")).unwrap(),
            "# the user's own\n"
        );
    }

    /// If codex writes something the overlay did not link, deleting the private
    /// home would throw that state away. Keep it and say where it is.
    #[test]
    fn state_codex_invented_is_kept_rather_than_deleted() {
        let home = fake_codex_home();
        let state = tempfile::tempdir().unwrap();
        let dir = build_private_home(&home.path().join(".codex"), state.path(), "codex-sol-1").unwrap();
        assert!(stranded_entries(&dir).is_empty(), "a fresh overlay strands nothing");

        std::fs::write(dir.join("brand_new.sqlite"), b"x").unwrap();
        assert_eq!(stranded_entries(&dir), vec!["brand_new.sqlite".to_string()]);

        Rules::Private { dir: dir.clone() }.finish();
        assert!(dir.join("brand_new.sqlite").is_file(), "stranded state must survive");
    }

    /// A machine that has never run codex has no home to mirror. That is not an
    /// error: the launch still gets its rules.
    #[test]
    fn a_missing_codex_home_still_yields_rules() {
        let state = tempfile::tempdir().unwrap();
        let dir = build_private_home(Path::new("/nonexistent/.codex"), state.path(), "codex-sol-1").unwrap();
        assert!(dir.join("rules").is_dir());
    }
}
