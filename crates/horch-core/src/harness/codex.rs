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

use anyhow::{bail, Context, Result};

use super::launch::model_for;
use super::{
    generic_validate, CommandSpec, Harness, HarnessKind, LaunchEnv, PrepareRequest, Prepared,
    Session, WindowInputs, Workdir,
};
use crate::compaction::window::OperatorWindow;
use crate::prompts;
use crate::roster::{ExecRule, HarnessDefault, Teammate};
use crate::runtime::RuntimeContext;

/// Path to codex's shared rules file: `~/.codex/rules/default.rules`.
pub(crate) fn rules_path(home: &Path) -> PathBuf {
    home.join(".codex").join("rules").join("default.rules")
}

/// The directory codex writes rollout files under.
pub fn sessions_dir(home: &Path) -> PathBuf {
    home.join(".codex").join("sessions")
}

/// The codex home a launch inherits: `codex_home_var` (`$CODEX_HOME`, empty
/// counting as unset), else `<home>/.codex`.
pub fn codex_home(home: &Path, codex_home_var: Option<&Path>) -> PathBuf {
    codex_home_var
        .filter(|v| !v.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| home.join(".codex"))
}

/// One codex launch's execpolicy rules.
///
/// Built before the CLI starts, applied to its [`Command`], and finished after
/// it exits. Hold it across the launch; dropping it also cleans up on errors.
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
    ///
    /// `codex_home` is the inherited codex home ([`codex_home`]) the private
    /// one mirrors, and `state_root` is where the private one is built.
    pub fn install(
        home: &Path,
        codex_home: &Path,
        state_root: &Path,
        role: &str,
        rules: &[ExecRule],
    ) -> Result<Rules> {
        install_rules(home, codex_home, state_root, role, rules)
    }

    /// Point the agent's command at these rules.
    pub fn apply(&self, cmd: &mut Command) {
        if let Rules::Private { dir } = self {
            // On the child only. This process keeps the real CODEX_HOME, so the
            // ledger's harvest still reads the real sessions directory.
            cmd.env("CODEX_HOME", dir);
        }
    }

    /// Replace only the private home's skills link. Never follow it into the
    /// operator's skills directory. Codex discovers these files natively.
    pub(crate) fn attach_skills(&self, skills_dir: &Path) -> Result<()> {
        #[cfg(not(windows))]
        if let Rules::Private { dir } = self {
            let target = dir.join("skills");
            match std::fs::symlink_metadata(&target) {
                Ok(meta) if meta.file_type().is_symlink() => std::fs::remove_file(&target)?,
                Ok(_) => anyhow::bail!("refusing to replace non-symlink {}", target.display()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
            std::os::unix::fs::symlink(skills_dir, &target)?;
            return Ok(());
        }
        let _ = skills_dir;
        anyhow::bail!("Codex phase skills require a private CODEX_HOME; use WSL on Windows")
    }

    /// Clean up after the agent has exited.
    ///
    /// A private home is removed - unless codex created something at its top
    /// level that was not there at launch. That would be state written to a
    /// directory nobody will look in again, so the directory is kept and named
    /// rather than silently deleted.
    pub fn finish(self) {
        // Drop handles both normal completion and early-return error paths.
    }

    fn cleanup(&self) {
        let Rules::Private { dir } = self else {
            return;
        };
        let stranded = stranded_entries(dir);
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
        let _ = std::fs::remove_dir_all(dir);
    }
}

impl Drop for Rules {
    fn drop(&mut self) {
        self.cleanup();
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
fn install_rules(
    _home: &Path,
    codex_home: &Path,
    state_root: &Path,
    role: &str,
    rules: &[ExecRule],
) -> Result<Rules> {
    let dir = build_private_home(codex_home, state_root, role)?;
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
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
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
                format!(
                    "linking {} into the private codex home",
                    entry.path().display()
                )
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
fn install_rules(
    home: &Path,
    _codex_home: &Path,
    _state_root: &Path,
    _role: &str,
    rules: &[ExecRule],
) -> Result<Rules> {
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
pub(crate) fn ensure_rules(home: &Path, rules: &[ExecRule]) -> Result<()> {
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
pub(crate) fn session_id_from_rollout(file_name: &str) -> Option<&str> {
    let stem = file_name.strip_prefix("rollout-")?.strip_suffix(".jsonl")?;
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
    [8, 4, 4, 4, 12].iter().zip(&groups).all(|(len, g)| {
        g.len() == *len
            && g.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}

/// A rollout file that could belong to this worker.
#[derive(Debug, Clone)]
pub struct RolloutCandidate {
    pub path: PathBuf,
    pub session_id: String,
    pub modified: SystemTime,
}

/// Rollout files modified at or after `since` whose head names `project_dir` as its cwd,
/// newest first.
///
/// The cwd check is what keeps two projects' concurrent codex sessions apart.
/// Each `"cwd"` value in the first 16KB is compared canonically
/// ([`Workdir`]), so `/var/...` finds a session recorded as `/private/var/...`.
pub(crate) fn find_rollouts(
    sessions_dir: &Path,
    project_dir: &str,
    since: SystemTime,
) -> Vec<RolloutCandidate> {
    let workdir = Workdir::new(project_dir);
    let mut found = Vec::new();
    collect_rollouts(sessions_dir, since, &workdir, &mut found);
    // Newest first, matching `ls -t`.
    found.sort_by_key(|c| std::cmp::Reverse(c.modified));
    found
}

fn collect_rollouts(
    dir: &Path,
    since: SystemTime,
    workdir: &Workdir,
    out: &mut Vec<RolloutCandidate>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            collect_rollouts(&path, since, workdir, out);
            continue;
        }
        if !meta.is_file() {
            continue;
        }
        let Ok(modified) = meta.modified() else {
            continue;
        };
        // An equal mtime is this launch's: mtimes come from a coarse clock,
        // so a rollout written in the marker's tick has the marker's mtime.
        if modified < since {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(session_id) = session_id_from_rollout(name) else {
            continue;
        };
        if !head_names_cwd(&path, workdir) {
            continue;
        }
        out.push(RolloutCandidate {
            session_id: session_id.to_string(),
            path,
            modified,
        });
    }
}

/// Does the first 16KB of `path` record `workdir` as a `"cwd"`?
fn head_names_cwd(path: &Path, workdir: &Workdir) -> bool {
    use std::io::Read;
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut buf = vec![0u8; 16 * 1024];
    let Ok(read) = file.read(&mut buf) else {
        return false;
    };
    buf.truncate(read);
    let head = String::from_utf8_lossy(&buf);
    head.match_indices("\"cwd\":").any(|(i, key)| {
        let rest = head[i + key.len()..].trim_start();
        serde_json::Deserializer::from_str(rest)
            .into_iter::<String>()
            .next()
            .and_then(|v| v.ok())
            .is_some_and(|cwd| workdir.matches(Path::new(&cwd)))
    })
}

/// The operator's `config.toml`, read bounded (a FIFO, a device or a file
/// over 1 MiB counts as absent).
fn read_config(path: &Path) -> Option<String> {
    crate::fsx::read_regular_bounded(path).ok().flatten()
}

/// The codex adapter.
pub struct Codex;

impl Harness for Codex {
    fn kind(&self) -> HarnessKind {
        HarnessKind::Codex
    }

    /// The generic checks, plus `inherit_plugins`: codex skills are installed
    /// into its private home, and it cannot disable all other operator
    /// extensions with a CLI switch.
    fn validate(&self, t: &Teammate) -> Vec<&'static str> {
        let mut out = generic_validate(self, t);
        if !t.inherit_plugins {
            out.push("inherit_plugins");
            out.sort_unstable();
        }
        out
    }

    /// Skills reach codex only through a private home, which needs symlinks.
    fn ensure_skills_supported(&self) -> Result<()> {
        if cfg!(windows) {
            anyhow::bail!("Codex phase skills require a private CODEX_HOME; use WSL on Windows, or explicitly set phase: null and skills: [] for the legacy shared-rules launcher");
        }
        Ok(())
    }

    /// A private `CODEX_HOME` holding only this pane's rules, with the
    /// skills bundle linked in. Finished once the CLI has exited.
    ///
    /// The state root is the pane's one extra writable root (`--add-dir`):
    /// `horch note` and `horch done` lock and replace the ledger directly in
    /// it. An execpolicy `allow` lifts the sandbox only for a command that
    /// matches its prefix, so `horch note x 2>&1; echo $?` ran sandboxed and
    /// failed on the lock (live check B2). `horch tell` writes under the
    /// temp dir, which the sandbox already allows; the data dir is only read.
    /// Only a `workspace-write` launch (or codex's default) gets the root:
    /// a `read-only` one exits at launch on `--add-dir`, so it gets the root
    /// as a `sandbox_workspace_write.writable_roots` pair for a later
    /// switch; full access needs none.
    fn prepare(&self, ctx: &RuntimeContext, req: &PrepareRequest<'_>) -> Result<Prepared> {
        let home = codex_home(&ctx.paths.home, ctx.inherited.codex_home.as_deref());
        let rules = Rules::install(
            &ctx.paths.home,
            &home,
            &ctx.paths.state_root,
            req.role,
            req.exec_rules,
        )?;
        if let Some(skills) = req.skills {
            rules.attach_skills(&skills.skills_dir())?;
        }
        let sandbox = launch_sandbox(req.teammate, || read_config(&home.join("config.toml")));
        let mut prepared = Prepared::default();
        // Codex 0.160.0 exits 1 when `--add-dir` meets a read-only sandbox
        // ("the effective permissions do not allow additional writable
        // roots"), and full access needs no extra root.
        let state_root = ctx.paths.state_root.to_string_lossy().into_owned();
        match sandbox.as_deref() {
            None | Some(WORKSPACE_WRITE) => {
                prepared.extra_args = vec!["--add-dir".into(), state_root];
            }
            // Named as a workspace-write root instead: codex reads it only
            // after a run-time switch to workspace-write (`horch mode <role>
            // write`), and starts normally with it (X6 repro 2).
            Some(READ_ONLY)
                if !HarnessDefault::config_keys(&req.teammate.args)
                    .contains(&WRITABLE_ROOTS_KEY) =>
            {
                prepared.extra_args = vec![
                    "-c".into(),
                    format!("{WRITABLE_ROOTS_KEY}=[{}]", toml_string(&state_root)),
                ];
            }
            _ => {}
        }
        if let Rules::Private { dir } = &rules {
            // On the child only. This process keeps the real CODEX_HOME, so the
            // session discovery still reads the real sessions directory.
            prepared
                .env
                .push(("CODEX_HOME".into(), dir.clone().into_os_string()));
        }
        prepared.on_finish(move || rules.finish());
        Ok(prepared)
    }

    /// The operator's `model_auto_compact_token_limit` in the inherited
    /// `config.toml`: the active profile, then the top level (design §6.4).
    fn operator_window(&self, inputs: &WindowInputs<'_>) -> Option<OperatorWindow> {
        let path = inputs.codex_home.join("config.toml");
        let text = read_config(&path);
        codex_operator_limit(
            text.as_deref(),
            args_profile(&inputs.teammate.args),
            &path.to_string_lossy(),
        )
    }

    /// The value of the last `-c model_auto_compact_token_limit=<n>` pair.
    fn window_in_command(&self, cmd: &Command) -> Option<u64> {
        let args: Vec<_> = cmd.get_args().filter_map(|a| a.to_str()).collect();
        args.windows(2)
            .rev()
            .filter(|w| w[0] == "-c")
            .find_map(|w| w[1].strip_prefix(LIMIT_KEY)?.strip_prefix('='))?
            .trim()
            .parse()
            .ok()
    }

    fn command(&self, env: &LaunchEnv, spec: &CommandSpec<'_>) -> Result<Command> {
        codex_command(
            env,
            spec.teammate,
            spec.session,
            spec.prompt,
            spec.model_override,
        )
    }

    /// Rollout files under the real codex home whose recorded cwd is
    /// `workdir`.
    fn discover_sessions(
        &self,
        ctx: &RuntimeContext,
        workdir: &Path,
        since: SystemTime,
        _sessions_dir: Option<&Path>,
    ) -> Vec<String> {
        find_rollouts(
            &sessions_dir(&ctx.paths.home),
            &workdir.to_string_lossy(),
            since,
        )
        .into_iter()
        .map(|c| c.session_id)
        .collect()
    }
}

/// The value of `key` (dotted, for example `tui.auto_recap`) in a codex
/// `config.toml`: under `[profiles.<p>]` first, then at the top level and in
/// its tables. `p` is `profile`, else the file's top-level `profile`. A
/// string loses its quotes and a number its `_` separators. A line scanner
/// like `trust::codex_trust`, not a TOML parser: no TOML crate.
pub fn codex_config_value(text: &str, profile: Option<&str>, key: &str) -> Option<String> {
    let entries = toml_entries(text);
    let lookup = |full: &str| {
        entries
            .iter()
            .find(|(k, _)| k == full)
            .map(|(_, v)| toml_scalar(v))
    };
    let profile = profile.map(str::to_string).or_else(|| lookup("profile"));
    if let Some(p) = profile {
        if let Some(v) = lookup(&format!("profiles.{p}.{key}")) {
            return Some(v);
        }
    }
    lookup(key)
}

/// Codex's auto-compact limit key in `config.toml` and in a `-c` pair.
const LIMIT_KEY: &str = "model_auto_compact_token_limit";

/// The operator's Codex limit over a `config.toml` text at `path`, for the
/// launch's `profile` (design §6.4). A value that is not a number gives
/// `tokens: None`.
pub(crate) fn codex_operator_limit(
    config_toml: Option<&str>,
    profile: Option<&str>,
    path: &str,
) -> Option<OperatorWindow> {
    let value = codex_config_value(config_toml?, profile, LIMIT_KEY)?;
    Some(match value.parse() {
        Ok(n) => OperatorWindow {
            tokens: Some(n),
            detail: path.to_string(),
        },
        Err(_) => OperatorWindow {
            tokens: None,
            detail: format!("{path} (not a number)"),
        },
    })
}

/// Every `key = value` line of a TOML text, as (full dotted key, raw value).
/// The key includes its `[table]`; quotes around key parts are dropped. An
/// array-of-tables (`[[x]]`) section is skipped.
fn toml_entries(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut table: Option<String> = Some(String::new());
    for line in text.lines() {
        let line = strip_toml_comment(line).trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("[[") {
            table = None;
            continue;
        }
        if let Some(header) = line.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
            table = Some(toml_key(header));
            continue;
        }
        let (Some(table), Some((key, value))) = (&table, line.split_once('=')) else {
            continue;
        };
        let key = toml_key(key);
        let full = if table.is_empty() {
            key
        } else {
            format!("{table}.{key}")
        };
        out.push((full, value.trim().to_string()));
    }
    out
}

/// A dotted TOML key with its parts unquoted and trimmed.
fn toml_key(key: &str) -> String {
    key.split('.')
        .map(|part| part.trim().trim_matches('"').trim_matches('\''))
        .collect::<Vec<_>>()
        .join(".")
}

/// `line` up to a `#` that is outside a string.
fn strip_toml_comment(line: &str) -> &str {
    let mut quote: Option<char> = None;
    for (i, c) in line.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), _) if c == q => quote = None,
            (None, '#') => return &line[..i],
            _ => {}
        }
    }
    line
}

/// A raw TOML value as text: a string without its quotes, a number without
/// `_` separators, anything else as written.
fn toml_scalar(raw: &str) -> String {
    let raw = raw.trim();
    for q in ['"', '\''] {
        if let Some(inner) = raw.strip_prefix(q).and_then(|r| r.strip_suffix(q)) {
            return inner.to_string();
        }
    }
    if raw.starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '+') {
        return raw.replace('_', "");
    }
    raw.to_string()
}

/// Codex's sandbox name that allows extra writable roots.
const WORKSPACE_WRITE: &str = "workspace-write";

/// Codex's read-only sandbox name (`permission_mode: plan`).
const READ_ONLY: &str = "read-only";

/// The config key of the workspace-write sandbox's extra writable roots.
const WRITABLE_ROOTS_KEY: &str = "sandbox_workspace_write.writable_roots";

/// `s` as 1 TOML basic string.
fn toml_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The sandbox a codex launch of `teammate` runs: `read-only`,
/// `workspace-write` or `danger-full-access`. `None`: codex's own default.
///
/// A sandbox flag in the argv wins over a `-c sandbox_mode=` pair, which
/// wins over `config` (the inherited `config.toml`, read only when needed).
/// Among flags, the teammate's args come after `permission_mode`'s and win.
fn launch_sandbox(teammate: &Teammate, config: impl FnOnce() -> Option<String>) -> Option<String> {
    let mode_args = teammate
        .permission_mode
        .and_then(|m| m.codex_args())
        .unwrap_or_default();
    let mut flag: Option<String> = None;
    let mut pair: Option<String> = None;
    let mut it = mode_args.iter().chain(&teammate.args);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-s" | "--sandbox" => flag = it.next().cloned(),
            "--full-auto" => flag = Some(WORKSPACE_WRITE.into()),
            "--dangerously-bypass-approvals-and-sandbox" | "--yolo" => {
                flag = Some("danger-full-access".into())
            }
            "-c" | "--config" => {
                let Some((key, value)) = it.next().and_then(|p| p.split_once('=')) else {
                    continue;
                };
                if key.trim() == "sandbox_mode" {
                    pair = Some(toml_scalar(value));
                }
            }
            other => {
                if let Some(s) = other.strip_prefix("--sandbox=") {
                    flag = Some(s.to_string());
                }
            }
        }
    }
    flag.or(pair)
        .or_else(|| codex_config_value(&config()?, args_profile(&teammate.args), "sandbox_mode"))
}

/// The profile a codex launch runs: the value after `-p` or `--profile` in
/// the teammate's args. `None` lets the config's own `profile` decide.
fn args_profile(args: &[String]) -> Option<&str> {
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        if arg == "-p" || arg == "--profile" {
            return it.next().map(String::as_str);
        }
        if let Some(p) = arg.strip_prefix("--profile=") {
            return Some(p);
        }
    }
    None
}

/// The harness-default args of a codex launch, which go before the
/// teammate's own. A `-c <key>=` pair is left out when the teammate's args
/// set the key, or when `config` (the inherited `config.toml`) sets it and
/// the entry has no `force`. Any other token is left out when the teammate's
/// args hold it. `config` is read only when a pair needs it.
pub(crate) fn default_args(
    teammate: &Teammate,
    config: impl FnOnce() -> Option<String>,
) -> Vec<String> {
    let teammate_keys = HarnessDefault::config_keys(&teammate.args);
    let profile = args_profile(&teammate.args);
    let mut config = Some(config);
    let mut text: Option<String> = None;
    let mut operator_sets = |key: &str| {
        if let Some(read) = config.take() {
            text = read();
        }
        text.as_deref()
            .is_some_and(|t| codex_config_value(t, profile, key).is_some())
    };
    let mut out = Vec::new();
    for entry in HarnessDefault::for_harness(&teammate.harness_defaults, HarnessKind::Codex) {
        let mut it = entry.args.iter();
        while let Some(arg) = it.next() {
            if arg == "-c" {
                let Some(pair) = it.next() else { break };
                let key = pair
                    .split_once('=')
                    .map_or(pair.as_str(), |(k, _)| k)
                    .trim();
                if teammate_keys.contains(&key) || (entry.force.is_none() && operator_sets(key)) {
                    continue;
                }
                out.push(arg.clone());
                out.push(pair.clone());
            } else if !teammate.args.contains(arg) {
                out.push(arg.clone());
            }
        }
    }
    out
}

pub(super) fn codex_command(
    env: &LaunchEnv,
    teammate: &Teammate,
    session: Session<'_>,
    prompt: &str,
    model_override: Option<&str>,
) -> Result<Command> {
    let bin = env.bins.codex.clone();
    let model = format!("model=\"{}\"", model_for(teammate, model_override)?);
    let mut cmd = Command::new(&bin);

    // `resume` is a subcommand, so it has to lead.
    if let Session::Resume(_) = session {
        cmd.arg("resume");
    }
    cmd.arg("-c").arg(&model);
    // Fleet panes must reach their briefing without startup dialogs. Keep
    // sandbox/command approval policy separate from trust for enabled hooks.
    cmd.args(["-c", "check_for_update_on_startup=false"]);
    cmd.args(["-c", "tui.resume_cwd=\"current\""]);
    if !teammate
        .args
        .iter()
        .any(|arg| arg == "--dangerously-bypass-hook-trust")
    {
        cmd.arg("--dangerously-bypass-hook-trust");
    }

    if let Some(mode) = teammate.permission_mode {
        match mode.codex_args() {
            Some(args) => {
                cmd.args(args);
            }
            None => bail!(
                "teammate '{}' sets permission_mode '{}', which has no codex equivalent",
                teammate.name,
                mode.as_str()
            ),
        }
    }
    if let Some(effort) = &teammate.effort {
        cmd.arg("-c")
            .arg(format!("model_reasoning_effort=\"{effort}\""));
    }
    // Harness defaults first, so the teammate's own args come later and win.
    cmd.args(default_args(teammate, || {
        let home = env.home()?;
        let dir = codex_home(home, env.codex_home.as_deref());
        read_config(&dir.join("config.toml"))
    }));
    // The fleet window, with the defaults: only when the decision applies
    // it, and not over the teammate's own pair.
    let window = env.compact_window.as_ref().filter(|d| d.applied);
    if let Some(n) = window.and_then(|d| d.tokens) {
        if !HarnessDefault::config_keys(&teammate.args).contains(&LIMIT_KEY) {
            cmd.arg("-c").arg(format!("{LIMIT_KEY}={n}"));
        }
    }
    cmd.args(&teammate.args);
    if let Session::Resume(id) = session {
        cmd.arg(id);
    }
    cmd.arg(prompt);
    Ok(cmd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roster::Roster;
    use std::time::Duration;

    /// Run `read` in a thread; panic when it does not return within 1 s.
    #[cfg(unix)]
    fn within_1s<T: Send + 'static>(read: impl FnOnce() -> T + Send + 'static) -> T {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(read());
        });
        rx.recv_timeout(std::time::Duration::from_secs(1))
            .expect("the read blocked")
    }

    #[cfg(unix)]
    fn mkfifo(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let made = std::process::Command::new("mkfifo")
            .arg(path)
            .status()
            .unwrap();
        assert!(made.success());
    }

    /// A FIFO at `config.toml` neither blocks the operator-window read nor
    /// counts as a value (u9b step 10).
    #[cfg(unix)]
    #[test]
    fn codex_config_fifo_does_not_block() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("codex/config.toml");
        mkfifo(&path);
        let read = path.clone();
        assert_eq!(within_1s(move || read_config(&read)), None);
        drop(tmp);
    }

    #[cfg(unix)]
    #[test]
    fn phase_skills_replace_only_the_private_link_and_cleanup_on_error() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("user-codex");
        std::fs::create_dir_all(source.join("skills/personal")).unwrap();
        std::fs::write(source.join("skills/personal/SKILL.md"), "personal").unwrap();
        let t = crate::roster::Teammate {
            phase: Some(crate::roster::Phase::Plan),
            ..Default::default()
        };
        let bundle = crate::skills::Bundle::install(tmp.path(), &t)
            .unwrap()
            .unwrap();
        let dir = build_private_home(&source, tmp.path(), "worker").unwrap();
        let rules = Rules::Private { dir: dir.clone() };
        rules.attach_skills(&bundle.skills_dir()).unwrap();
        assert!(dir.join("skills/create-plan/SKILL.md").exists());
        assert!(!dir.join("skills/personal").exists());
        assert_eq!(
            std::fs::read_to_string(source.join("skills/personal/SKILL.md")).unwrap(),
            "personal"
        );
        drop(rules); // Same cleanup as an early-return launch error.
        assert!(!dir.exists());
        assert!(bundle.skills_dir().join("create-plan/SKILL.md").exists());
        assert!(source.join("skills/personal/SKILL.md").exists());
    }

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

        write_rollout(
            &sessions.join("2026/08"),
            "aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa",
            "/proj/mine",
        );
        write_rollout(
            &sessions.join("2026/08"),
            "bbbbbbbb-bbbb-4bbb-bbbb-bbbbbbbbbbbb",
            "/proj/other",
        );

        let found = find_rollouts(&sessions, "/proj/mine", since);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].session_id, "aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa");
    }

    /// Sessions that predate this worker's launch belong to someone else.
    #[test]
    fn ignores_rollouts_older_than_the_launch_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let sessions = tmp.path().join("sessions");
        write_rollout(
            &sessions,
            "aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa",
            "/proj/mine",
        );

        let future = SystemTime::now() + Duration::from_secs(3600);
        assert!(find_rollouts(&sessions, "/proj/mine", future).is_empty());
    }

    /// A filesystem stamps mtimes from a coarse clock (1 kernel tick on
    /// Linux), so a rollout written right after the launch marker can carry
    /// the marker's exact mtime. It is this launch's rollout, not an older one.
    #[test]
    fn finds_a_rollout_with_the_launch_markers_mtime() {
        let tmp = tempfile::tempdir().unwrap();
        let sessions = tmp.path().join("sessions");
        let path = write_rollout(
            &sessions,
            "aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa",
            "/proj/mine",
        );
        let since = std::fs::metadata(&path).unwrap().modified().unwrap();

        let found = find_rollouts(&sessions, "/proj/mine", since);
        assert_eq!(found.len(), 1, "{found:#?}");
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
        let dir =
            build_private_home(&home.path().join(".codex"), state.path(), "codex-sol-1").unwrap();

        for shared in ["config.toml", "auth.json", "sessions"] {
            assert!(
                dir.join(shared)
                    .symlink_metadata()
                    .unwrap()
                    .file_type()
                    .is_symlink(),
                "{shared} must be a symlink to the real codex home"
            );
        }
        assert!(dir.join("rules").is_dir());
        assert!(
            !dir.join("rules")
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink(),
            "rules must be this launch's own directory, not the shared one"
        );
        assert!(
            !dir.join("rules/default.rules").exists(),
            "the user's rules must not load"
        );
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
            let dir = build_private_home(&home.path().join(".codex"), state.path(), "codex-sol-1")
                .unwrap();
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
        assert!(
            real.join("config.toml").is_file(),
            "the real config survived"
        );
        assert!(real.join("auth.json").is_file(), "the real auth survived");
        assert!(
            real.join("sessions").is_dir(),
            "the real sessions dir survived"
        );
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
        let dir =
            build_private_home(&home.path().join(".codex"), state.path(), "codex-sol-1").unwrap();
        assert!(
            stranded_entries(&dir).is_empty(),
            "a fresh overlay strands nothing"
        );

        std::fs::write(dir.join("brand_new.sqlite"), b"x").unwrap();
        assert_eq!(stranded_entries(&dir), vec!["brand_new.sqlite".to_string()]);

        Rules::Private { dir: dir.clone() }.finish();
        assert!(
            dir.join("brand_new.sqlite").is_file(),
            "stranded state must survive"
        );
    }

    /// Prepare one codex pane under a state root in `tmp`, for `teammate`.
    #[cfg(unix)]
    fn prepare_in(tmp: &Path, teammate: &Teammate) -> (RuntimeContext, Prepared) {
        let home = tmp.join("home");
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        let env = crate::runtime::MapEnv::new(tmp)
            .with("HOME", &home.to_string_lossy())
            .with("HORCH_STATE_DIR", &tmp.join("state").to_string_lossy());
        let ctx = RuntimeContext::from_env(&env).unwrap();
        let prepared = Codex
            .prepare(
                &ctx,
                &PrepareRequest {
                    role: "codex-1",
                    exec_rules: &[],
                    skills: None,
                    compact_window: None,
                    teammate,
                    model: "gpt-5.6-sol",
                    workdir: tmp,
                },
            )
            .unwrap();
        (ctx, prepared)
    }

    /// Live check B2: an execpolicy `allow` does not lift the sandbox for a
    /// compound command, so the ledger lock under the state root failed. The
    /// state root is the one writable root a pane gets, and nothing else.
    #[cfg(unix)]
    #[test]
    fn a_pane_gets_exactly_the_state_root_as_a_writable_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let teammate = Teammate::default();
        let (ctx, prepared) = prepare_in(tmp.path(), &teammate);
        assert_eq!(ctx.paths.state_root, tmp.path().join("state"));
        assert_eq!(
            prepared.extra_args,
            vec![
                "--add-dir".to_string(),
                tmp.path().join("state").to_string_lossy().into_owned()
            ]
        );
    }

    /// A teammate's own `--add-dir` still reaches the argv, beside the fleet's.
    #[cfg(unix)]
    #[test]
    fn a_teammates_own_add_dir_still_applies() {
        let tmp = tempfile::tempdir().unwrap();
        let mut teammate = Teammate {
            model: Some("gpt-5.6-sol".into()),
            args: vec!["--add-dir".into(), "/teammate/own".into()],
            ..Default::default()
        };
        let (ctx, prepared) = prepare_in(tmp.path(), &teammate);
        teammate.args.extend(prepared.extra_args.iter().cloned());
        let cmd = codex_command(
            &super::super::LaunchEnv::for_test(),
            &teammate,
            Session::Unmanaged,
            "P",
            None,
        )
        .unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let dirs: Vec<&str> = args
            .windows(2)
            .filter(|w| w[0] == "--add-dir")
            .map(|w| w[1].as_str())
            .collect();
        let state = ctx.paths.state_root.to_string_lossy().into_owned();
        assert_eq!(dirs, vec!["/teammate/own", state.as_str()]);
        assert_eq!(args.last().map(String::as_str), Some("P"));
    }

    /// The `--add-dir` values a prepared launch for `teammate` passes, with
    /// `config` as the codex home's `config.toml`.
    #[cfg(unix)]
    fn fleet_dirs(teammate: &Teammate, config: Option<&str>) -> Vec<String> {
        let tmp = tempfile::tempdir().unwrap();
        if let Some(text) = config {
            let dir = tmp.path().join("home").join(".codex");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("config.toml"), text).unwrap();
        }
        let (_, prepared) = prepare_in(tmp.path(), teammate);
        prepared
            .extra_args
            .windows(2)
            .filter(|w| w[0] == "--add-dir")
            .map(|w| w[1].clone())
            .collect()
    }

    /// Codex 0.160.0 exits 1 at launch when `--add-dir` meets a read-only
    /// sandbox: "Ignoring --add-dir (...) because the effective permissions
    /// do not allow additional writable roots" (the X6 repro). Full access
    /// needs no extra root. Only a workspace-write launch gets the state root.
    #[cfg(unix)]
    #[test]
    fn the_state_root_is_added_only_for_a_workspace_write_sandbox() {
        use crate::roster::PermissionMode::*;
        for (mode, expect) in [
            (Some(Plan), false),
            (Some(AcceptEdits), true),
            (Some(Auto), true),
            (Some(BypassPermissions), false),
            (None, true),
        ] {
            let teammate = Teammate {
                permission_mode: mode,
                ..Default::default()
            };
            assert_eq!(
                fleet_dirs(&teammate, None).len(),
                usize::from(expect),
                "permission_mode {mode:?}"
            );
        }
    }

    /// A read-only launch cannot take `--add-dir`, so it names the state root
    /// as a workspace-write root by config instead. Codex reads that only
    /// after a run-time switch to workspace-write (`horch mode <role> write`),
    /// and it starts normally with it (X6 repro 2). No other sandbox gets it,
    /// and a teammate's own pair is kept.
    #[cfg(unix)]
    #[test]
    fn a_read_only_launch_names_the_state_root_for_a_later_switch() {
        use crate::roster::PermissionMode::*;
        let pairs = |teammate: &Teammate| {
            let tmp = tempfile::tempdir().unwrap();
            let (ctx, prepared) = prepare_in(tmp.path(), teammate);
            let pairs: Vec<String> = prepared
                .extra_args
                .windows(2)
                .filter(|w| w[0] == "-c" && w[1].starts_with(WRITABLE_ROOTS_KEY))
                .map(|w| w[1].clone())
                .collect();
            (ctx.paths.state_root.to_string_lossy().into_owned(), pairs)
        };
        let plan = Teammate {
            permission_mode: Some(Plan),
            ..Default::default()
        };
        let (state, got) = pairs(&plan);
        assert_eq!(
            got,
            vec![format!(
                "sandbox_workspace_write.writable_roots=[\"{state}\"]"
            )]
        );
        for mode in [Some(AcceptEdits), Some(Auto), Some(BypassPermissions), None] {
            let teammate = Teammate {
                permission_mode: mode,
                ..Default::default()
            };
            assert!(pairs(&teammate).1.is_empty(), "permission_mode {mode:?}");
        }
        let own = Teammate {
            permission_mode: Some(Plan),
            args: vec![
                "-c".into(),
                "sandbox_workspace_write.writable_roots=[\"/own\"]".into(),
            ],
            ..Default::default()
        };
        assert!(pairs(&own).1.is_empty());
    }

    /// A path with a quote or a backslash stays 1 TOML string.
    #[test]
    fn a_writable_root_is_quoted_as_a_toml_string() {
        assert_eq!(toml_string("/a/b"), "\"/a/b\"");
        assert_eq!(toml_string("/a\"b\\c"), "\"/a\\\"b\\\\c\"");
    }

    /// The teammate's own sandbox args come after `permission_mode` and win,
    /// as they do in the argv.
    #[cfg(unix)]
    #[test]
    fn a_teammates_own_sandbox_args_decide_the_state_root() {
        use crate::roster::PermissionMode::*;
        for (mode, args, expect) in [
            (None, vec!["-s", "read-only"], false),
            (None, vec!["--sandbox=read-only"], false),
            (None, vec!["-c", "sandbox_mode=\"read-only\""], false),
            (
                None,
                vec!["--dangerously-bypass-approvals-and-sandbox"],
                false,
            ),
            (None, vec!["--full-auto"], true),
            (Some(Plan), vec!["--sandbox", "workspace-write"], true),
            (Some(Auto), vec!["-s", "danger-full-access"], false),
        ] {
            let teammate = Teammate {
                permission_mode: mode,
                args: args.iter().map(|s| s.to_string()).collect(),
                ..Default::default()
            };
            assert_eq!(
                fleet_dirs(&teammate, None).len(),
                usize::from(expect),
                "permission_mode {mode:?} args {args:?}"
            );
        }
    }

    /// With no sandbox in the argv, the inherited `config.toml` decides: its
    /// profile, then its top level.
    #[cfg(unix)]
    #[test]
    fn the_operator_config_sandbox_decides_when_the_argv_sets_none() {
        let plain = Teammate::default();
        assert!(fleet_dirs(&plain, Some("sandbox_mode = \"read-only\"\n")).is_empty());
        assert_eq!(
            fleet_dirs(&plain, Some("sandbox_mode = \"workspace-write\"\n")).len(),
            1
        );
        let profiled = Teammate {
            args: vec!["-p".into(), "ro".into()],
            ..Default::default()
        };
        let text =
            "sandbox_mode = \"workspace-write\"\n[profiles.ro]\nsandbox_mode = \"read-only\"\n";
        assert!(fleet_dirs(&profiled, Some(text)).is_empty());
        let plan_over_config = Teammate {
            permission_mode: Some(crate::roster::PermissionMode::Auto),
            ..Default::default()
        };
        assert_eq!(
            fleet_dirs(&plan_over_config, Some("sandbox_mode = \"read-only\"\n")).len(),
            1
        );
    }

    /// A machine that has never run codex has no home to mirror. That is not an
    /// error: the launch still gets its rules.
    #[test]
    fn a_missing_codex_home_still_yields_rules() {
        let state = tempfile::tempdir().unwrap();
        let dir = build_private_home(
            Path::new("/nonexistent/.codex"),
            state.path(),
            "codex-sol-1",
        )
        .unwrap();
        assert!(dir.join("rules").is_dir());
    }
}
