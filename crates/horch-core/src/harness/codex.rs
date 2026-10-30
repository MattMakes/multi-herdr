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

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{bail, Context, Result};

use super::launch::model_for;
use super::{
    generic_validate, CommandSpec, Harness, HarnessKind, LaunchEnv, PrepareRequest, Prepared,
    Session, WindowInputs, Workdir,
};
use crate::compaction::window::OperatorWindow;
use crate::fsx;
use crate::prompts;
use crate::roster::{ExecDecision, ExecRule, HarnessDefault, PermissionMode, Teammate};
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

/// Write a copy of Codex's live model catalog that makes every model use the
/// v1 multi-agent protocol. Codex otherwise exposes v2 collaboration tools
/// even when `features.multi_agent=false`.
pub(crate) fn write_v1_catalog(codex_bin: &Path, state_root: &Path) -> Result<PathBuf> {
    let mut cmd = Command::new(codex_bin);
    cmd.args(["debug", "models"]);
    let output = run_with_deadline(cmd, Duration::from_secs(5))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "{} debug models exited with {}: {}",
            codex_bin.display(),
            output.status,
            stderr.trim()
        );
    }

    let mut catalog: serde_json::Value = serde_json::from_slice(&output.stdout)
        .with_context(|| format!("parsing {} debug models output", codex_bin.display()))?;
    let models = match &mut catalog {
        serde_json::Value::Object(root) => root
            .get_mut("models")
            .and_then(serde_json::Value::as_array_mut)
            .context("model catalog object has no models array")?,
        serde_json::Value::Array(models) => models,
        _ => bail!("model catalog is neither an object nor an array"),
    };
    pin_multi_agent_v1(models);

    write_catalog(&catalog, state_root)
}

/// Pin every model that carries a multi-agent version to `v1`.
fn pin_multi_agent_v1(models: &mut [serde_json::Value]) {
    for model in models
        .iter_mut()
        .filter_map(serde_json::Value::as_object_mut)
    {
        if model.contains_key("multi_agent_version") {
            model.insert("multi_agent_version".into(), serde_json::json!("v1"));
        }
    }
}

/// Write the catalog under `<state_root>/codex/` and return its path.
fn write_catalog(catalog: &serde_json::Value, state_root: &Path) -> Result<PathBuf> {
    let dir = state_root.join("codex");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = dir.join("model-catalog-v1.json");
    let bytes = serde_json::to_vec_pretty(catalog).context("encoding v1 model catalog")?;
    fsx::write_atomic(&path, &bytes, fsx::PRIVATE_FILE)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

/// Run one short command, kill it after `timeout`, and collect both pipes.
fn run_with_deadline(mut cmd: Command, timeout: Duration) -> Result<std::process::Output> {
    crate::runtime::process::scrub_child_env(&mut cmd);
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("starting codex catalog command")?;
    let stdout = child.stdout.take().context("capturing codex stdout")?;
    let stderr = child.stderr.take().context("capturing codex stderr")?;
    let read = |mut pipe: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            bytes
        })
    };
    let out = read(Box::new(stdout));
    let err = read(Box::new(stderr));
    let status = wait_until(&mut child, Instant::now() + timeout, timeout)?;
    Ok(std::process::Output {
        status,
        stdout: out.join().unwrap_or_default(),
        stderr: err.join().unwrap_or_default(),
    })
}

/// Poll `child` until it exits or `deadline` passes; kill it on a timeout.
fn wait_until(child: &mut Child, deadline: Instant, timeout: Duration) -> Result<ExitStatus> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                bail!("codex debug models timed out after {}s", timeout.as_secs());
            }
            Err(e) => return Err(e).context("waiting for codex catalog command"),
        }
    }
}

/// Remove an existing skills symlink; refuse anything that is not one.
#[cfg(not(windows))]
fn clear_skills_link(target: &Path) -> Result<()> {
    match std::fs::symlink_metadata(target) {
        Ok(meta) if meta.file_type().is_symlink() => std::fs::remove_file(target)?,
        Ok(_) => anyhow::bail!("refusing to replace non-symlink {}", target.display()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    Ok(())
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
    /// one mirrors, and `root` is where the private one is built. `root` must
    /// be outside every writable root of the sandbox, or a worker can rewrite
    /// the rules of a pane before codex reads them.
    pub fn install(
        home: &Path,
        codex_home: &Path,
        root: &Path,
        role: &str,
        rules: &[ExecRule],
    ) -> Result<Rules> {
        install_rules(home, codex_home, root, role, rules)
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
    #[cfg(not(windows))]
    pub(crate) fn attach_skills(&self, skills_dir: &Path) -> Result<()> {
        let Rules::Private { dir } = self else {
            anyhow::bail!("Codex phase skills require a private CODEX_HOME; use WSL on Windows");
        };
        let target = dir.join("skills");
        clear_skills_link(&target)?;
        std::os::unix::fs::symlink(skills_dir, &target)?;
        Ok(())
    }

    #[cfg(windows)]
    pub(crate) fn attach_skills(&self, _skills_dir: &Path) -> Result<()> {
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
    root: &Path,
    role: &str,
    rules: &[ExecRule],
) -> Result<Rules> {
    let dir = build_private_home(codex_home, root, role)?;
    std::fs::write(dir.join("rules").join("horch.rules"), render(rules))
        .with_context(|| format!("writing this launch's rules under {}", dir.display()))?;
    Ok(Rules::Private { dir })
}

/// Assemble a private codex home under `root`, mirroring `source`.
///
/// Rebuilt on every launch rather than cached, so an entry added to the real
/// codex home since the last fleet is linked rather than missing.
///
/// A pane killed outright never reaches [`Rules::finish`] and leaves its
/// directory behind. That is why `role` is in the name: the leftovers are
/// symlinks and one rules file, harmless to delete, and legible about which
/// pane they came from.
#[cfg(not(windows))]
fn build_private_home(source: &Path, root: &Path, role: &str) -> Result<PathBuf> {
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
    let dir = root
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
    _root: &Path,
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
///
/// Only allow rules go here. Every codex session reads this file, so a
/// forbidden worker rule would also refuse the command to the codex
/// orchestrator and to the operator's own sessions. The 1 broad ledger rule
/// that v0.4.0 appended is removed, so the narrowed worker rules apply (#181).
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn ensure_rules(home: &Path, rules: &[ExecRule]) -> Result<()> {
    let path = rules_path(home);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let read = read_rules_text(&path)?;
    let existing = read.replace(&prompts::codex_rule_block(&broad_ledger_rule()), "");

    // Match on the rendered pattern list, exactly as the bash `grep -qF` did.
    let appended: String = rules
        .iter()
        .filter(|r| r.decision == ExecDecision::Allow)
        .filter(|r| !existing.contains(&format!("[{}]", r.pattern)))
        .map(prompts::codex_rule_block)
        .collect();
    if appended.is_empty() && existing == read {
        Ok(())
    } else {
        write_rules(&path, existing, &appended)
    }
}

/// Only a missing file is empty. A file that is not UTF-8 text is an
/// error: rewriting it from "" would drop the operator's own rules.
fn read_rules_text(path: &Path) -> Result<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// Write `existing` plus `appended`, starting `appended` on its own line.
fn write_rules(path: &Path, existing: String, appended: &str) -> Result<()> {
    let mut next = existing;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(appended);
    std::fs::write(path, next).with_context(|| format!("writing {}", path.display()))
}

/// The broad worker rule that v0.4.0 appended to the shared rules file. It
/// let a worker run every `horch ledger` subcommand outside its sandbox.
fn broad_ledger_rule() -> ExecRule {
    ExecRule {
        pattern: r#""horch", "ledger""#.into(),
        justification: "claim, report and finish tasks through the task ledger".into(),
        decision: ExecDecision::Allow,
    }
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

/// Stable unique marker shared by launch-time and done-time discovery.
pub fn launch_token(record_id: &str) -> String {
    format!("HORCH-LAUNCH:{record_id}:END")
}

/// Candidates must carry this launch's token in their first user message.
pub fn find_launch_rollouts(
    sessions: &Path,
    project: &str,
    since: SystemTime,
    record_id: &str,
) -> Vec<RolloutCandidate> {
    let token = launch_token(record_id);
    find_rollouts(sessions, project, since)
        .into_iter()
        .filter(|candidate| {
            let Ok(file) = std::fs::File::open(&candidate.path) else {
                return false;
            };
            use std::io::BufRead;
            let mut first_response_user = None;
            for line in std::io::BufReader::new(file.take(1024 * 1024))
                .lines()
                .map_while(Result::ok)
            {
                let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
                    continue;
                };
                let payload = &value["payload"];
                if first_response_user.is_none()
                    && value["type"] == "response_item"
                    && payload["role"] == "user"
                {
                    first_response_user =
                        Some(payload["content"].as_array().is_some_and(|parts| {
                            parts.iter().any(|part| {
                                part["text"]
                                    .as_str()
                                    .is_some_and(|text| text.contains(&token))
                            })
                        }));
                }
                // The event records the actual submitted prompt; response items can
                // include preceding AGENTS/environment context represented as user text.
                if value["type"] == "event_msg" && payload["type"] == "user_message" {
                    return payload["message"]
                        .as_str()
                        .is_some_and(|text| text.contains(&token));
                }
            }
            first_response_user.unwrap_or(false)
        })
        .collect()
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
        if entry.metadata().is_ok_and(|meta| meta.is_dir()) {
            collect_rollouts(&entry.path(), since, workdir, out);
        } else {
            out.extend(rollout_candidate(&entry, since, workdir));
        }
    }
}

/// The rollout file `entry` names, if it is a recent file of this workdir.
fn rollout_candidate(
    entry: &std::fs::DirEntry,
    since: SystemTime,
    workdir: &Workdir,
) -> Option<RolloutCandidate> {
    let meta = entry.metadata().ok()?;
    meta.is_file().then_some(())?;
    let modified = meta.modified().ok()?;
    // An equal mtime is this launch's: mtimes come from a coarse clock,
    // so a rollout written in the marker's tick has the marker's mtime.
    (modified >= since).then_some(())?;
    let path = entry.path();
    let session_id = session_id_from_rollout(path.file_name()?.to_str()?)?.to_string();
    head_names_cwd(&path, workdir).then_some(())?;
    Some(RolloutCandidate {
        session_id,
        path,
        modified,
    })
}

/// Does the first 16KB of `path` record `workdir` as a `"cwd"`?
fn head_names_cwd(path: &Path, workdir: &Workdir) -> bool {
    read_head(path).is_some_and(|head| {
        head.match_indices("\"cwd\":").any(|(i, key)| {
            let rest = head[i + key.len()..].trim_start();
            serde_json::Deserializer::from_str(rest)
                .into_iter::<String>()
                .next()
                .and_then(|v| v.ok())
                .is_some_and(|cwd| workdir.matches(Path::new(&cwd)))
        })
    })
}

/// The first 16KB of `path`, lossily decoded.
fn read_head(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; 16 * 1024];
    let read = file.read(&mut buf).ok()?;
    buf.truncate(read);
    Some(String::from_utf8_lossy(&buf).into_owned())
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
    /// The state root is the pane's one extra writable root: `horch note`
    /// and `horch done` lock and replace the ledger directly in it, and
    /// `horch tell` spools long messages there. An execpolicy `allow` lifts
    /// the sandbox only for a command that matches its prefix, so
    /// `horch note x 2>&1; echo $?` ran sandboxed and failed on the lock
    /// (live check B2). A write or plan pane gets the root inside
    /// [`FLEET_WRITE_PROFILE`]. Otherwise only a `workspace-write` launch
    /// (or codex's default) gets it, as `--add-dir`:
    /// a `read-only` one exits at launch on `--add-dir`, so it gets the root
    /// as a `sandbox_workspace_write.writable_roots` pair for a later
    /// switch; full access needs none.
    fn prepare(&self, ctx: &RuntimeContext, req: &PrepareRequest<'_>) -> Result<Prepared> {
        let home = codex_home(&ctx.paths.home, ctx.inherited.codex_home.as_deref());
        // The data root, not the state root: every pane may write the
        // state root (review F13 of T-7).
        let rules = Rules::install(
            &ctx.paths.home,
            &home,
            &ctx.paths.data_root,
            req.role,
            req.exec_rules,
        )?;
        if let Some(skills) = req.skills {
            rules.attach_skills(&skills.skills_dir())?;
        }
        let sandbox = launch_sandbox(req.teammate, || {
            ConfigLayers::read(&home, args_profile(&req.teammate.args))
        });
        let mut prepared = Prepared::default();
        // Codex 0.160.0 exits 1 when `--add-dir` meets a read-only sandbox
        // ("the effective permissions do not allow additional writable
        // roots"), and full access needs no extra root.
        let state_root = ctx.paths.state_root.to_string_lossy().into_owned();
        let profile_form = write_profile_form(req.teammate) || plan_profile_form(req.teammate);
        match sandbox.as_deref() {
            // A write pane runs the fleet write profile; a plan pane defines
            // it for `horch mode <role> write` (X6 repro 5). The profile holds
            // the state root and the pane's own TMPDIR.
            _ if profile_form => {
                let tmpdir = pane_tmpdir(&ctx.paths.state_root, req.role);
                std::fs::create_dir_all(&tmpdir)
                    .with_context(|| format!("creating {}", tmpdir.display()))?;
                prepared.extra_args = fleet_write_profile_args(&state_root);
                let value =
                    serde_json::to_string(&tmpdir.to_string_lossy()).context("encoding TMPDIR")?;
                prepared.extra_args.push("-c".into());
                prepared
                    .extra_args
                    .push(format!("shell_environment_policy.set.TMPDIR={value}"));
                prepared.on_finish(move || {
                    let _ = std::fs::remove_dir_all(&tmpdir);
                });
            }
            None | Some(WORKSPACE_WRITE) => {
                prepared.extra_args = vec!["--add-dir".into(), state_root];
            }
            // Another read-only launch names it as a workspace-write root:
            // codex reads it only after a switch to workspace-write, and
            // starts normally with it (X6 repro 2).
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
        // A JSON string is a valid TOML basic string, newlines escaped too.
        for (key, value) in req.shell_env {
            if key.is_empty() || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                continue;
            }
            let value = serde_json::to_string(value).context("encoding a shell variable")?;
            prepared.extra_args.push("-c".into());
            prepared
                .extra_args
                .push(format!("shell_environment_policy.set.{key}={value}"));
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
        let profile = args_profile(&inputs.teammate.args);
        let layers = ConfigLayers::read(inputs.codex_home, profile);
        codex_operator_limit(layers.as_ref(), inputs.codex_home, profile)
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
/// `config.toml`, at its top level and in its tables. The legacy
/// `[profiles.<p>]` table and the top-level `profile` key are not read: Codex
/// 0.163.0 refuses them; `ConfigLayers` reads `<name>.config.toml`. A
/// string loses its quotes and a number its `_` separators. A line scanner
/// like `trust::codex_trust`, not a TOML parser: no TOML crate.
pub fn codex_config_value(text: &str, key: &str) -> Option<String> {
    toml_entries(text)
        .into_iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| toml_scalar(&v))
}

/// The operator's Codex config as one launch reads it: `config.toml`, and for
/// `-p <name>` the `<name>.config.toml` layered over it. Codex 0.163.0 reads
/// that file from the codex home and refuses `-p <name>` while `config.toml`
/// holds `[profiles.<name>]` or a top-level `profile`, so horch reads neither.
pub(crate) struct ConfigLayers {
    base: Option<String>,
    profile_file: Option<String>,
}

impl ConfigLayers {
    /// `dir/config.toml` and, for `profile`, `dir/<profile>.config.toml`.
    /// `None` when neither file reads.
    pub(crate) fn read(dir: &Path, profile: Option<&str>) -> Option<Self> {
        let base = read_config(&dir.join("config.toml"));
        let profile_file = profile_config_path(dir, profile).and_then(|p| read_config(&p));
        (base.is_some() || profile_file.is_some()).then_some(Self { base, profile_file })
    }

    /// `config` as `config.toml`, with no profile file.
    #[cfg(test)]
    fn base(config: &str) -> Self {
        Self {
            base: Some(config.to_string()),
            profile_file: None,
        }
    }

    /// The value of `key`, and whether the profile file gave it. The profile
    /// file wins; `config.toml` is read at its top level only. A `-p` with no
    /// profile file reads `config.toml` alone.
    fn lookup(&self, key: &str) -> Option<(String, bool)> {
        if let Some(v) = self
            .profile_file
            .as_deref()
            .and_then(|text| codex_config_value(text, key))
        {
            return Some((v, true));
        }
        let base = self.base.as_deref()?;
        codex_config_value(base, key).map(|v| (v, false))
    }

    fn value(&self, key: &str) -> Option<String> {
        self.lookup(key).map(|(v, _)| v)
    }
}

/// `<dir>/<profile>.config.toml`, the file `-p <profile>` layers. `None`
/// without a profile, or when the name is not one file name.
fn profile_config_path(dir: &Path, profile: Option<&str>) -> Option<PathBuf> {
    let name = profile?;
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
        return None;
    }
    Some(dir.join(format!("{name}.config.toml")))
}

/// Codex's auto-compact limit key in `config.toml` and in a `-c` pair.
const LIMIT_KEY: &str = "model_auto_compact_token_limit";

/// The operator's Codex limit over the config in `dir`, for the launch's
/// `profile` (design §6.4). A value that is not a number gives
/// `tokens: None`. The detail names the file that gave the value.
pub(crate) fn codex_operator_limit(
    layers: Option<&ConfigLayers>,
    dir: &Path,
    profile: Option<&str>,
) -> Option<OperatorWindow> {
    let (value, from_profile) = layers?.lookup(LIMIT_KEY)?;
    let path = match profile_config_path(dir, profile).filter(|_| from_profile) {
        Some(p) => p,
        None => dir.join("config.toml"),
    }
    .to_string_lossy()
    .into_owned();
    Some(match value.parse() {
        Ok(n) => OperatorWindow {
            tokens: Some(n),
            detail: path,
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
fn launch_sandbox(
    teammate: &Teammate,
    config: impl FnOnce() -> Option<ConfigLayers>,
) -> Option<String> {
    let mode_args = permission_args(teammate).unwrap_or_default();
    let mut flag: Option<String> = None;
    let mut pair: Option<String> = None;
    let mut it = mode_args.iter().chain(&teammate.args);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-s" | "--sandbox" => flag = it.next().cloned(),
            "--full-auto" | "--approve-for-me" | "--not-so-yolo" => {
                flag = Some(WORKSPACE_WRITE.into())
            }
            "--dangerously-bypass-approvals-and-sandbox" | "--yolo" => {
                flag = Some("danger-full-access".into())
            }
            "-c" | "--config" => {
                let Some((key, value)) = it.next().and_then(|p| p.split_once('=')) else {
                    continue;
                };
                match key.trim() {
                    "sandbox_mode" => pair = Some(toml_scalar(value)),
                    "default_permissions" => pair = Some(profile_sandbox(&toml_scalar(value))),
                    _ => {}
                }
            }
            other => {
                if let Some(s) = other.strip_prefix("--sandbox=") {
                    flag = Some(s.to_string());
                }
            }
        }
    }
    flag.or(pair).or_else(|| {
        let layers = config()?;
        layers.value("sandbox_mode").or_else(|| {
            layers
                .value("default_permissions")
                .map(|p| profile_sandbox(&p))
        })
    })
}

/// Whether a codex launch of `teammate` can have the network, judged from
/// the teammate file and the harness defaults. The operator's `config.toml`
/// is not read, and it can turn the network on, so the answer fails closed:
/// only a read-only sandbox, or a workspace-write sandbox whose args or
/// forced harness defaults turn `sandbox_workspace_write.network_access`
/// off, has none. A default without `force` yields to `config.toml`.
pub(crate) fn launch_has_network(teammate: &Teammate) -> bool {
    const KEY: &str = "sandbox_workspace_write.network_access";
    match launch_sandbox(teammate, || None).as_deref() {
        Some(READ_ONLY) => false,
        Some(WORKSPACE_WRITE) => {
            let forced: Vec<String> =
                HarnessDefault::for_harness(&teammate.harness_defaults, HarnessKind::Codex)
                    .filter(|d| d.force.is_some())
                    .flat_map(|d| d.args.iter().cloned())
                    .collect();
            config_pair_value(&teammate.args, KEY).or_else(|| config_pair_value(&forced, KEY))
                != Some("false")
        }
        _ => true,
    }
}

/// The sandbox of a `default_permissions` profile: the built-in ones by
/// name; a named profile as itself (no `--add-dir`: its roots are its own).
fn profile_sandbox(profile: &str) -> String {
    match profile {
        ":read-only" => READ_ONLY.into(),
        ":workspace" => WORKSPACE_WRITE.into(),
        ":danger-full-access" => "danger-full-access".into(),
        other => other.into(),
    }
}

/// The fleet's write profile. A write pane launches in it; a plan pane
/// defines it so `horch mode <role> write` can select it in `/permissions`
/// (X6 repro 5). See `fleet_write_profile_args`.
pub const FLEET_WRITE_PROFILE: &str = "fleet_write";

/// The `-c` pairs that define [`FLEET_WRITE_PROFILE`]:
///
/// - The pane's folder (codex's project roots) is writable throughout,
///   `.git`, `.codex`, `.agents` and `.aws` too, which `:workspace` keeps
///   read-only. So a Codex worker can commit and make worktrees.
/// - Everything else stays read-only: `/tmp` and the system temp dir as
///   well. `state_root` is the one extra writable root: `horch note` and
///   `horch done` write the ledger in it, and the pane's `TMPDIR` lives
///   under it ([`pane_tmpdir`]).
/// - The network is on.
///
/// The sandbox itself keeps processes apart: a sandboxed command can signal
/// only processes in its own sandbox (Seatbelt `signal (target
/// same-sandbox)`; a PID namespace on Linux).
fn fleet_write_profile_args(state_root: &str) -> Vec<String> {
    let p = FLEET_WRITE_PROFILE;
    [
        format!("permissions.{p}.extends=\":workspace\""),
        format!(
            "permissions.{p}.filesystem={{\":project_roots\"={{\".\"=\"write\",\".git\"=\"write\",\
             \".codex\"=\"write\",\".agents\"=\"write\",\".aws\"=\"write\"}},\
             \":slash_tmp\"=\"read\",\":tmpdir\"=\"read\"}}"
        ),
        format!(
            "permissions.{p}.workspace_roots={{{}=true}}",
            toml_string(state_root)
        ),
        format!("permissions.{p}.network.enabled=true"),
    ]
    .into_iter()
    .flat_map(|pair| ["-c".to_string(), pair])
    .collect()
}

/// The args after `codex` that run a command in the sandbox of a Codex
/// write pane (#181): [`FLEET_WRITE_PROFILE`] with `dir` as its project
/// root and `state_root` as its 1 extra writable root. The command follows
/// the last arg, `--`. `codex sandbox` keeps the command in its own process
/// group, so killing the group of `codex` kills the command too.
pub fn sandbox_args(state_root: &Path, dir: &Path) -> Vec<String> {
    let mut args = vec!["sandbox".to_string()];
    args.extend(fleet_write_profile_args(&state_root.to_string_lossy()));
    args.extend([
        "-P".into(),
        FLEET_WRITE_PROFILE.into(),
        "-C".into(),
        dir.to_string_lossy().into_owned(),
        "--".into(),
    ]);
    args
}

/// The pane's private temp dir, `<state root>/tmp/<role>`: `/tmp` and the
/// system temp dir are read-only in [`FLEET_WRITE_PROFILE`].
pub(crate) fn pane_tmpdir(state_root: &Path, role: &str) -> PathBuf {
    let safe: String = role
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    state_root.join("tmp").join(safe)
}

/// Whether a write teammate launches in [`FLEET_WRITE_PROFILE`]: its own
/// args set no sandbox and no profile (Codex refuses `sandbox_mode` and
/// `default_permissions` together).
fn write_profile_form(teammate: &Teammate) -> bool {
    matches!(
        teammate.permission_mode,
        Some(PermissionMode::Auto | PermissionMode::AcceptEdits)
    ) && !sets_own_sandbox(teammate)
}

/// Whether a plan teammate launches in the profile form: its own args set
/// no sandbox and no profile (Codex refuses `sandbox_mode` and
/// `default_permissions` together).
fn plan_profile_form(teammate: &Teammate) -> bool {
    teammate.permission_mode == Some(PermissionMode::Plan) && !sets_own_sandbox(teammate)
}

/// Whether the teammate's own args choose a sandbox or a permissions
/// profile.
fn sets_own_sandbox(teammate: &Teammate) -> bool {
    let keys = HarnessDefault::config_keys(&teammate.args);
    let flags = [
        "-s",
        "--sandbox",
        "--full-auto",
        "--approve-for-me",
        "--not-so-yolo",
        "--dangerously-bypass-approvals-and-sandbox",
        "--yolo",
    ];
    keys.iter()
        .any(|k| matches!(*k, "sandbox_mode" | "default_permissions"))
        || teammate
            .args
            .iter()
            .any(|a| flags.contains(&a.as_str()) || a.starts_with("--sandbox="))
}

/// The sandbox and approval args of `permission_mode`. A plan pane in the
/// profile form gets `default_permissions=":read-only"`, not `-s
/// read-only`: only then does `/permissions` list Read Only, so `horch mode
/// <role> plan` can switch back (X6 repro 3). `None`: no mode, or no codex
/// equivalent.
///
/// A write pane selects [`FLEET_WRITE_PROFILE`] (defined in `prepare`, which
/// knows the state root) and never asks. A teammate whose own args choose a
/// sandbox gets the plain flags, and its own args, later in the argv, win.
/// Every mode gets `--search`,
/// codex's live web search tool.
fn permission_args(teammate: &Teammate) -> Option<Vec<String>> {
    let mut args: Vec<String> = if plan_profile_form(teammate) {
        [
            "-c",
            "default_permissions=\":read-only\"",
            "-a",
            "on-request",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    } else if write_profile_form(teammate) {
        vec![
            "-c".into(),
            format!("default_permissions=\"{FLEET_WRITE_PROFILE}\""),
            "-a".into(),
            "never".into(),
        ]
    } else {
        teammate.permission_mode?.codex_args()?
    };
    args.push("--search".into());
    Some(args)
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
/// set the key, or when `config` (the inherited config layers) sets it and
/// the entry has no `force`. Any other token is left out when the teammate's
/// args hold it. `config` is read only when a pair needs it.
pub(crate) fn default_args(
    teammate: &Teammate,
    config: impl FnOnce() -> Option<ConfigLayers>,
) -> Vec<String> {
    let teammate_keys = HarnessDefault::config_keys(&teammate.args);
    let mut config = Some(config);
    let mut layers: Option<ConfigLayers> = None;
    let mut operator_sets = |key: &str| {
        if let Some(read) = config.take() {
            layers = read();
        }
        layers.as_ref().is_some_and(|l| l.value(key).is_some())
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

fn config_pair_value<'a>(args: &'a [String], key: &str) -> Option<&'a str> {
    let mut found = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg != "-c" && arg != "--config" {
            continue;
        }
        let Some((pair_key, value)) = args.next().and_then(|pair| pair.split_once('=')) else {
            continue;
        };
        if pair_key.trim() == key {
            found = Some(value.trim());
        }
    }
    found
}

/// Whether the final Codex `-c` pairs turn native multi-agent support off.
pub(crate) fn multi_agent_off(env: &LaunchEnv, teammate: &Teammate) -> bool {
    let defaults = default_args(teammate, || {
        let home = env.home()?;
        let dir = codex_home(home, env.codex_home.as_deref());
        ConfigLayers::read(&dir, args_profile(&teammate.args))
    });
    config_pair_value(&teammate.args, "features.multi_agent")
        .or_else(|| config_pair_value(&defaults, "features.multi_agent"))
        == Some("false")
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
    if teammate.worker_channel {
        const CHANNEL_ENV_VARS: [&str; 14] = [
            "HORCH_WORKSPACE_ID",
            "HORCH_RECORD_ID",
            "HORCH_STATE_DIR",
            "HORCH_DATA_DIR",
            "HORCH_TEAMMATES_DIR",
            "HORCH_CODEX_BIN",
            "HERDR_PANE_ID",
            "HERDR_SOCKET_PATH",
            "HERDR_BIN_PATH",
            "HORCH_HERDR_BIN",
            "XDG_STATE_HOME",
            "XDG_DATA_HOME",
            "HOME",
            "PATH",
        ];
        let horch = env.horch_exe.to_string_lossy();
        let workdir = env
            .workdir
            .as_deref()
            .context("the worker channel needs a launch workdir")?
            .to_string_lossy();
        let channel_args = ["channel", "serve", "--agent", "codex"]
            .map(toml_string)
            .join(",");
        let env_vars = CHANNEL_ENV_VARS.map(toml_string).join(",");
        for pair in [
            format!("mcp_servers.horch.command={}", toml_string(&horch)),
            format!("mcp_servers.horch.args=[{channel_args}]"),
            format!("mcp_servers.horch.cwd={}", toml_string(&workdir)),
            format!("mcp_servers.horch.env_vars=[{env_vars}]"),
        ] {
            cmd.arg("-c").arg(pair);
        }
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
        match permission_args(teammate) {
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
    let defaults = default_args(teammate, || {
        let home = env.home()?;
        let dir = codex_home(home, env.codex_home.as_deref());
        ConfigLayers::read(&dir, args_profile(&teammate.args))
    });
    let multi_agent_off = config_pair_value(&teammate.args, "features.multi_agent")
        .or_else(|| config_pair_value(&defaults, "features.multi_agent"))
        == Some("false");
    cmd.args(defaults);
    // The fleet window, with the defaults: only when the decision applies
    // it, and not over the teammate's own pair.
    let window = env.compact_window.as_ref().filter(|d| d.applied);
    if let Some(n) = window.and_then(|d| d.tokens) {
        if !HarnessDefault::config_keys(&teammate.args).contains(&LIMIT_KEY) {
            cmd.arg("-c").arg(format!("{LIMIT_KEY}={n}"));
        }
    }
    cmd.args(&teammate.args);
    if multi_agent_off {
        if let Some(path) = env
            .state_root
            .as_ref()
            .map(|root| root.join("codex/model-catalog-v1.json"))
            .filter(|path| path.is_file())
        {
            cmd.arg("-c").arg(format!(
                "model_catalog_json={}",
                toml_string(&path.to_string_lossy())
            ));
        }
    }
    if let Session::Resume(id) = session {
        cmd.arg(id);
    }
    // An empty prompt is no prompt: a resumed orchestrator starts idle.
    if !prompt.is_empty() {
        cmd.arg(prompt);
    }
    Ok(cmd)
}

// ─── run-time permission switch (`horch mode`, X6) ─────────────────────────

/// The sandbox mode `horch mode` puts a `permission_mode: plan` pane in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneMode {
    /// Read-only, as the teammate launches.
    Plan,
    /// Workspace-write with the state root, for a handoff and the ledger.
    Write,
}

impl PaneMode {
    /// `plan` or `write`: the CLI argument and the `mode-changed` text.
    pub fn as_str(self) -> &'static str {
        match self {
            PaneMode::Plan => "plan",
            PaneMode::Write => "write",
        }
    }

    pub fn parse(s: &str) -> Option<PaneMode> {
        match s {
            "plan" => Some(PaneMode::Plan),
            "write" => Some(PaneMode::Write),
            _ => None,
        }
    }

    /// The `/permissions` item that selects this mode (Codex 0.160.0).
    pub fn menu_label(self) -> &'static str {
        match self {
            PaneMode::Plan => "Read Only",
            PaneMode::Write => FLEET_WRITE_PROFILE,
        }
    }
}

/// The title line of Codex's `/permissions` menu.
pub const PERMISSIONS_MENU_TITLE: &str = "Update Model Permissions";

/// The `/permissions` menu as a screen shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionsMenu {
    /// The item labels in order, without a ` (current)` mark.
    pub items: Vec<String>,
    /// The index of the item under the `›` cursor.
    pub cursor: usize,
}

impl PermissionsMenu {
    pub fn cursor_label(&self) -> Option<&str> {
        self.items.get(self.cursor).map(String::as_str)
    }

    /// The `down` presses from the cursor to `label`. The menu wraps.
    pub fn downs_to(&self, label: &str) -> Option<usize> {
        let target = self.items.iter().position(|i| i == label)?;
        let n = self.items.len();
        Some((target + n - self.cursor) % n)
    }
}

/// The newest `/permissions` menu on `screen`: the item lines after the
/// last title. `None` without a title, items or a cursor.
pub fn parse_permissions_menu(screen: &str) -> Option<PermissionsMenu> {
    let start = screen.rfind(PERMISSIONS_MENU_TITLE)?;
    let mut items = Vec::new();
    let mut cursor = None;
    for line in screen[start..].lines().skip(1) {
        let mut rest = line.trim_start();
        let here = rest.starts_with('›');
        if here {
            rest = rest.trim_start_matches('›').trim_start();
        }
        let Some((num, label)) = rest.split_once(". ") else {
            continue;
        };
        if num.is_empty() || !num.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let label = label.split("  ").next().unwrap_or("").trim();
        let label = label.strip_suffix("(current)").unwrap_or(label).trim();
        if here {
            cursor = Some(items.len());
        }
        items.push(label.to_string());
    }
    Some(PermissionsMenu {
        cursor: cursor?,
        items,
    })
    .filter(|m| !m.items.is_empty())
}

/// The value of the newest `Permissions:` line of a `/status` block, with
/// its wrapped parts joined (Codex wraps it at narrow widths, mid-path).
pub fn status_permissions(screen: &str) -> Option<String> {
    let lines: Vec<&str> = screen.lines().collect();
    let at = lines.iter().rposition(|l| l.contains("Permissions:"))?;
    let first = lines[at].split_once("Permissions:")?.1.trim();
    let mut value = first.to_string();
    // A wrapped part is indented under the value; the next key is not.
    for line in &lines[at + 1..] {
        let indent = line.len() - line.trim_start().len();
        if indent <= 6 || line.trim().is_empty() {
            break;
        }
        value.push_str(line.trim());
    }
    Some(value)
}

/// Whether a `/status` Permissions value shows `mode`: `Read Only ...` for
/// `Plan`, `Profile fleet_write` for `Write` (X6 repro 5; `/status` does not
/// list a profile's roots). Whitespace is ignored, as a wrap can split any
/// word.
pub fn status_shows(mode: PaneMode, permissions: &str) -> bool {
    let value: String = permissions.split_whitespace().collect();
    match mode {
        PaneMode::Plan => value.starts_with("ReadOnly"),
        PaneMode::Write => value == format!("Profile{FLEET_WRITE_PROFILE}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roster::Roster;
    use serde_json::{json, Value};
    use std::time::Duration;

    fn command_args(cmd: &Command) -> Vec<String> {
        cmd.get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[cfg(unix)]
    fn fake_codex(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        // Warm the new script without a deadline. macOS can spend tens of
        // seconds on its first exec while syspolicyd inspects it.
        let _ = Command::new(&path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        path
    }

    #[cfg(unix)]
    #[test]
    fn nw_01_catalog_forces_v1() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let object = fake_codex(
            tmp.path(),
            "codex-object",
            r#"printf '%s\n' '{"models":[{"slug":"a","multi_agent_version":"v2"},{"slug":"b","multi_agent_version":"v1"},{"slug":"c"}]}'"#,
        );
        let path = write_v1_catalog(&object, &tmp.path().join("state-object")).unwrap();
        let value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["models"][0]["multi_agent_version"], "v1");
        assert_eq!(value["models"][1]["multi_agent_version"], "v1");
        assert!(value["models"][2].get("multi_agent_version").is_none());
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let array = fake_codex(
            tmp.path(),
            "codex-array",
            r#"printf '%s\n' '[{"slug":"a","multi_agent_version":"v2"}]'"#,
        );
        let path = write_v1_catalog(&array, &tmp.path().join("state-array")).unwrap();
        let value: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(value[0]["multi_agent_version"], "v1");
    }

    #[cfg(unix)]
    #[test]
    fn w3_attach_skills_replaces_a_symlink_and_refuses_a_real_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let (home, old, new) = (
            tmp.path().join("home"),
            tmp.path().join("old"),
            tmp.path().join("new"),
        );
        for d in [&home, &old, &new] {
            std::fs::create_dir_all(d).unwrap();
        }
        let rules = Rules::Private { dir: home.clone() };
        // No link yet: the link is created.
        rules.attach_skills(&old).unwrap();
        assert_eq!(std::fs::read_link(home.join("skills")).unwrap(), old);
        // An old link is replaced, never followed.
        rules.attach_skills(&new).unwrap();
        assert_eq!(std::fs::read_link(home.join("skills")).unwrap(), new);
        // A real directory is the operator's: refuse and leave it alone.
        std::fs::remove_file(home.join("skills")).unwrap();
        std::fs::create_dir_all(home.join("skills")).unwrap();
        let err = rules.attach_skills(&new).unwrap_err().to_string();
        assert_eq!(
            err,
            format!(
                "refusing to replace non-symlink {}",
                home.join("skills").display()
            )
        );
        assert!(home.join("skills").is_dir());
        // Without a private home there is nowhere to attach.
        let err = Rules::Shared.attach_skills(&new).unwrap_err().to_string();
        assert_eq!(
            err,
            "Codex phase skills require a private CODEX_HOME; use WSL on Windows"
        );
    }

    #[cfg(unix)]
    #[test]
    fn w3_a_command_past_its_deadline_is_killed_and_named() {
        let tmp = tempfile::tempdir().unwrap();
        let slow = fake_codex(tmp.path(), "slow", "sleep 30");
        let started = Instant::now();
        let err = run_with_deadline(Command::new(&slow), Duration::from_millis(300)).unwrap_err();
        assert_eq!(err.to_string(), "codex debug models timed out after 0s");
        assert!(started.elapsed() < Duration::from_secs(20));
    }

    #[cfg(unix)]
    #[test]
    fn nw_01_catalog_failure_is_an_err() {
        let tmp = tempfile::tempdir().unwrap();
        let codex = fake_codex(tmp.path(), "codex-fail", "echo broken >&2\nexit 1");
        let err = write_v1_catalog(&codex, &tmp.path().join("state")).unwrap_err();
        assert!(err.to_string().contains("exited with"), "{err:#}");
        assert!(err.to_string().contains("broken"), "{err:#}");
    }

    fn catalog_args(env: &LaunchEnv, feature: &str) -> Vec<String> {
        let teammate = Teammate {
            model: Some("gpt-6-sol".into()),
            args: vec!["-c".into(), format!("features.multi_agent={feature}")],
            ..Default::default()
        };
        command_args(&codex_command(env, &teammate, Session::Unmanaged, "p", None).unwrap())
    }

    #[test]
    fn nw_02_multi_agent_off_points_at_the_v1_catalog() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("codex/model-catalog-v1.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{}").unwrap();
        let mut env = LaunchEnv::for_test();
        env.state_root = Some(tmp.path().to_path_buf());
        let args = catalog_args(&env, "false");
        let pair = format!(
            "model_catalog_json={}",
            toml_string(&path.to_string_lossy())
        );
        let own = args
            .iter()
            .position(|arg| arg == "features.multi_agent=false")
            .unwrap();
        let added = args.iter().position(|arg| arg == &pair).unwrap();
        assert!(added > own, "{args:?}");
    }

    #[test]
    fn nw_02_multi_agent_on_keeps_the_live_catalog() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("codex/model-catalog-v1.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "{}").unwrap();
        let mut env = LaunchEnv::for_test();
        env.state_root = Some(tmp.path().to_path_buf());
        let args = catalog_args(&env, "true");
        assert!(!args
            .iter()
            .any(|arg| arg.starts_with("model_catalog_json=")));
    }

    #[test]
    fn nw_02_missing_catalog_adds_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let mut env = LaunchEnv::for_test();
        env.state_root = Some(tmp.path().to_path_buf());
        let args = catalog_args(&env, "false");
        assert!(!args
            .iter()
            .any(|arg| arg.starts_with("model_catalog_json=")));
    }

    /// A resumed fleet orchestrator gets no prompt: the thread id ends the
    /// command line, with no empty argument after it.
    #[test]
    fn an_empty_prompt_is_left_off_the_command_line() {
        let teammate = Teammate {
            model: Some("gpt-5.6-sol".into()),
            ..Default::default()
        };
        let args = command_args(
            &codex_command(
                &LaunchEnv::for_test(),
                &teammate,
                Session::Resume("thread-1"),
                "",
                None,
            )
            .unwrap(),
        );
        assert_eq!(
            args.last().map(String::as_str),
            Some("thread-1"),
            "{args:?}"
        );
    }

    #[test]
    fn arc_30_codex_orchestrator_gets_the_horch_mcp_server() {
        let mut env = LaunchEnv::for_test();
        env.horch_exe = "/opt/horch test".into();
        env.workdir = Some("/work tree".into());
        let teammate = Teammate {
            model: Some("gpt-5.6-sol".into()),
            worker_channel: true,
            ..Default::default()
        };
        let args = command_args(
            &codex_command(&env, &teammate, Session::Resume("thread-1"), "p", None).unwrap(),
        );
        assert_eq!(args[0], "resume");
        let pairs: Vec<&str> = args[1..9]
            .chunks_exact(2)
            .map(|pair| {
                assert_eq!(pair[0], "-c");
                pair[1].as_str()
            })
            .collect();
        let parsed: Vec<(&str, Value)> = pairs
            .iter()
            .map(|pair| {
                let (key, value) = pair.split_once('=').unwrap();
                (key, serde_json::from_str(value).unwrap())
            })
            .collect();
        assert_eq!(
            parsed[0],
            ("mcp_servers.horch.command", json!("/opt/horch test"))
        );
        assert_eq!(
            parsed[1],
            (
                "mcp_servers.horch.args",
                json!(["channel", "serve", "--agent", "codex"])
            )
        );
        assert_eq!(parsed[2], ("mcp_servers.horch.cwd", json!("/work tree")));
        assert_eq!(
            parsed[3],
            (
                "mcp_servers.horch.env_vars",
                json!([
                    "HORCH_WORKSPACE_ID",
                    "HORCH_RECORD_ID",
                    "HORCH_STATE_DIR",
                    "HORCH_DATA_DIR",
                    "HORCH_TEAMMATES_DIR",
                    "HORCH_CODEX_BIN",
                    "HERDR_PANE_ID",
                    "HERDR_SOCKET_PATH",
                    "HERDR_BIN_PATH",
                    "HORCH_HERDR_BIN",
                    "XDG_STATE_HOME",
                    "XDG_DATA_HOME",
                    "HOME",
                    "PATH"
                ])
            )
        );
    }

    #[test]
    fn arc_30_codex_without_worker_channel_adds_nothing() {
        let teammate = Teammate {
            model: Some("gpt-5.6-sol".into()),
            ..Default::default()
        };
        let args = command_args(
            &codex_command(
                &LaunchEnv::for_test(),
                &teammate,
                Session::Unmanaged,
                "p",
                None,
            )
            .unwrap(),
        );
        assert!(!args.iter().any(|arg| arg.starts_with("mcp_servers.horch.")));
    }

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

    /// The decision codex takes for `argv` under the `prefix_rule` blocks in
    /// `rules`: the strictest decision of every rule whose pattern is a
    /// prefix of `argv`, or `None` (the command runs in the sandbox).
    /// A block that does not parse, or an unknown decision, fails the test.
    fn rule_decision(rules: &str, argv: &[&str]) -> Option<String> {
        let rank = |d: &str| {
            ["allow", "prompt", "forbidden"]
                .iter()
                .position(|x| *x == d)
                .unwrap_or_else(|| panic!("unknown decision {d:?}"))
        };
        rules
            .split("prefix_rule(")
            .skip(1)
            .filter_map(|block| {
                let field = |name: &str| {
                    let line = block
                        .lines()
                        .find(|l| l.trim().starts_with(name))
                        .unwrap_or_else(|| panic!("no {name} in {block}"));
                    let value = line.split_once('=').unwrap().1.trim();
                    serde_json::from_str::<serde_json::Value>(value.trim_end_matches(','))
                        .unwrap_or_else(|e| panic!("{name} in {block}: {e}"))
                };
                let pattern: Vec<String> = serde_json::from_value(field("pattern")).unwrap();
                let decision: String = serde_json::from_value(field("decision")).unwrap();
                let matches =
                    pattern.len() <= argv.len() && pattern.iter().zip(argv).all(|(p, a)| p == a);
                matches.then_some(decision)
            })
            .max_by_key(|d| rank(d))
    }

    /// #181: a codex worker reaches the task ledger outside its sandbox only
    /// through the subcommands a worker runs. `finish` lifts the sandbox;
    /// `add` and `reopen`, which take any `--check` command, are refused; a
    /// subcommand no rule names runs in the sandbox.
    #[cfg(unix)]
    #[test]
    fn a_worker_launch_allows_ledger_finish_and_forbids_ledger_add() {
        let tmp = tempfile::tempdir().unwrap();
        let roster = Roster::builtin().unwrap();
        let teammate = roster.require("codex-sol").unwrap();
        let (_, prepared) = prepare_with_rules(tmp.path(), teammate, roster.exec_rules());
        let home = &prepared
            .env
            .iter()
            .find(|(k, _)| k == "CODEX_HOME")
            .expect("a private codex home")
            .1;
        let rules = std::fs::read_to_string(Path::new(home).join("rules/horch.rules")).unwrap();
        let allow = Some("allow".to_string());
        let forbidden = Some("forbidden".to_string());
        for (argv, want) in [
            (&["horch", "ledger", "finish"][..], &allow),
            (&["horch", "ledger", "claim"], &allow),
            (&["horch", "ledger", "progress", "step 1 done"], &allow),
            (&["horch", "ledger", "worktree", "new", "x"], &allow),
            (&["horch", "ledger", "done", "summary"], &allow),
            (&["horch", "ledger", "status"], &allow),
            (&["horch", "ledger", "show", "T-1"], &allow),
            (&["horch", "ledger", "steps"], &allow),
            (
                &["horch", "ledger", "add", "t.md", "--check", "sh x"],
                &forbidden,
            ),
            (&["horch", "ledger", "add-map", "MAP.md"], &forbidden),
            (
                &["horch", "ledger", "reopen", "1", "--check", "sh x"],
                &forbidden,
            ),
            (&["horch", "ledger", "cancel", "T-1"], &None),
            (&["horch", "ledger"], &None),
            (&["horch", "done", "summary"], &allow),
        ] {
            assert_eq!(&rule_decision(&rules, argv), want, "{argv:?} in\n{rules}");
        }
    }

    /// Whether the codex argv of a launch of `teammate` turns the network on:
    /// the `-c` pairs of the built command and of `prepare`, read for the
    /// permissions profile the launch selects.
    #[cfg(unix)]
    fn argv_has_network(teammate: &Teammate) -> bool {
        use crate::harness::launch::{command_in, LaunchEnv, Session};
        let tmp = tempfile::tempdir().unwrap();
        let (_, prepared) = prepare_in(tmp.path(), teammate);
        let cmd = command_in(
            &LaunchEnv::for_test(),
            teammate,
            Session::Unmanaged,
            "p",
            None,
        )
        .unwrap();
        let mut argv = command_args(&cmd);
        argv.extend(prepared.extra_args.iter().cloned());
        match config_pair_value(&argv, "default_permissions") {
            Some("\"fleet_write\"") => {
                config_pair_value(&argv, "permissions.fleet_write.network.enabled") == Some("true")
            }
            Some("\":read-only\"") => false,
            other => panic!(
                "{}: unexpected profile {other:?} in {argv:?}",
                teammate.name
            ),
        }
    }

    /// #151: the orchestrator routes work by `brief_description`, so each
    /// codex teammate's description names the network setting its launch
    /// really has. Every bundled codex teammate launches in the fleet write
    /// profile, which has the network; a plan pane has none.
    #[cfg(unix)]
    #[test]
    fn each_codex_teammate_states_the_network_its_launch_has() {
        let roster = Roster::builtin().unwrap();
        // Every bundled codex teammate launches with the network.
        let expected = [
            "astra-creative",
            "codex-luna",
            "codex-network",
            "codex-reviewer",
            "codex-sol",
            "codex-swift-reviewer",
            "codex-terra",
            "orchestrator-codex",
            "visual-prototyper",
        ];
        let codex: Vec<&str> = roster
            .names()
            .into_iter()
            .filter(|n| roster.require(n).unwrap().agent == HarnessKind::Codex)
            .collect();
        assert_eq!(codex, expected, "a new codex teammate needs a row");
        for name in expected {
            let t = roster.require(name).unwrap();
            assert!(argv_has_network(t), "{name}: argv");
            assert!(launch_has_network(t), "{name}: launch_has_network");
            assert!(
                t.brief_description.contains("+ network"),
                "{name}: {:?} does not say \"+ network\"",
                t.brief_description
            );
        }
        let plan = Teammate {
            permission_mode: Some(PermissionMode::Plan),
            ..roster.require("codex-sol").unwrap().clone()
        };
        assert!(!argv_has_network(&plan));
        assert!(!launch_has_network(&plan));
    }

    /// A teammate whose own args choose the sandbox: workspace-write has the
    /// network unless the args or a forced harness default set
    /// `network_access=false`; full access always has it.
    /// A sandbox the teammate file does not decide (codex's default, the
    /// operator's config, a named profile) counts as having the network, so
    /// the network-safety check fails closed.
    #[test]
    fn a_teammates_own_sandbox_decides_its_network() {
        let with = |args: &[&str]| Teammate {
            permission_mode: Some(PermissionMode::Auto),
            args: args.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        };
        // workspace-write counts as having the network unless the args turn
        // it off: codex's config.toml can turn it on, and is not read here.
        assert!(launch_has_network(&with(&["-s", "workspace-write"])));
        assert!(!launch_has_network(&with(&[
            "-s",
            "workspace-write",
            "-c",
            "sandbox_workspace_write.network_access=false"
        ])));
        assert!(!launch_has_network(&with(&[
            "-s",
            "workspace-write",
            "--config",
            "sandbox_workspace_write.network_access=false"
        ])));
        assert!(launch_has_network(&with(&[
            "-s",
            "workspace-write",
            "-c",
            "sandbox_workspace_write.network_access=true"
        ])));
        assert!(launch_has_network(&with(&["-s", "danger-full-access"])));
        assert!(!launch_has_network(&with(&["-s", "read-only"])));
        assert!(launch_has_network(&with(&[
            "-s",
            "workspace-write",
            "--config",
            "sandbox_workspace_write.network_access=true"
        ])));
        let profile = Teammate {
            args: vec!["-c".into(), "default_permissions=\"ops\"".into()],
            ..Default::default()
        };
        assert!(launch_has_network(&profile));
        assert!(launch_has_network(&Teammate::default()));

        // A harness default decides only when the teammate's args leave the
        // key unset, and it turns the network off only with `force`: a
        // default without it yields to the operator's config.toml.
        let with_default = |args: &[&str], force: Option<&str>| Teammate {
            harness_defaults: vec![HarnessDefault {
                harness: HarnessKind::Codex,
                base: None,
                env: Default::default(),
                args: vec![
                    "-c".into(),
                    "sandbox_workspace_write.network_access=false".into(),
                ],
                settings: None,
                config: None,
                agent_settings: None,
                force: force.map(str::to_string),
            }],
            ..with(args)
        };
        assert!(!launch_has_network(&with_default(
            &["-s", "workspace-write"],
            Some("test")
        )));
        assert!(launch_has_network(&with_default(
            &[
                "-s",
                "workspace-write",
                "-c",
                "sandbox_workspace_write.network_access=true"
            ],
            Some("test")
        )));
        assert!(launch_has_network(&with_default(
            &["-s", "workspace-write"],
            None
        )));
    }

    /// Review F13 of T-7: the private codex homes lived under the state
    /// root, which every pane may write, so a sandboxed worker could rewrite
    /// the rules (and drop the `forbidden` blocks) of a pane horch launches
    /// later. A private home is now built outside every writable root.
    #[cfg(unix)]
    #[test]
    fn the_private_codex_home_is_outside_every_writable_root() {
        let roster = Roster::builtin().unwrap();
        for teammate in [roster.require("codex-sol").unwrap(), &Teammate::default()] {
            let tmp = tempfile::tempdir().unwrap();
            let (ctx, prepared) = prepare_in(tmp.path(), teammate);
            let home = Path::new(
                &prepared
                    .env
                    .iter()
                    .find(|(k, _)| k == "CODEX_HOME")
                    .expect("a private codex home")
                    .1,
            )
            .to_path_buf();
            assert!(home.join("rules/horch.rules").is_file(), "{home:?}");
            assert!(home.starts_with(&ctx.paths.data_root), "{home:?}");
            assert!(!home.starts_with(&ctx.paths.state_root), "{home:?}");
            let added = prepared
                .extra_args
                .windows(2)
                .filter(|w| w[0] == "--add-dir")
                .map(|w| Path::new(&w[1]));
            for root in added {
                assert!(!home.starts_with(root), "{home:?} is under {root:?}");
            }
        }
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
        prepare_with_rules(tmp, teammate, &[])
    }

    /// [`prepare_in`], with `exec_rules` installed in the pane's rules file.
    #[cfg(unix)]
    fn prepare_with_rules(
        tmp: &Path,
        teammate: &Teammate,
        exec_rules: &[ExecRule],
    ) -> (RuntimeContext, Prepared) {
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
                    exec_rules,
                    skills: None,
                    compact_window: None,
                    teammate,
                    model: "gpt-5.6-sol",
                    workdir: tmp,
                    shell_env: &[],
                },
            )
            .unwrap();
        (ctx, prepared)
    }

    /// W2 C2: the operator's `shell_environment_policy.inherit = "core"`
    /// stripped `HORCH_*` and `HERDR_*` from the agent's shell commands, so
    /// `horch tell`, `note` and `done` failed with `HORCH_RECORD_ID is
    /// unset`. Each fleet variable is set through the policy instead, which
    /// `inherit` does not filter. A name that is not a plain variable name
    /// is skipped.
    #[cfg(unix)]
    #[test]
    fn w2_a_launch_sets_the_fleet_variables_in_the_shell_policy() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        let env = crate::runtime::MapEnv::new(tmp.path())
            .with("HOME", &home.to_string_lossy())
            .with(
                "HORCH_STATE_DIR",
                &tmp.path().join("state").to_string_lossy(),
            );
        let ctx = RuntimeContext::from_env(&env).unwrap();
        let shell_env = [
            ("HORCH_RECORD_ID".to_string(), "r-1".to_string()),
            ("HERDR_PANE_ID".to_string(), "w1:p2".to_string()),
            ("HORCH_TASK".to_string(), "say \"hi\"\nnow".to_string()),
            ("BAD KEY".to_string(), "x".to_string()),
        ];
        let prepared = Codex
            .prepare(
                &ctx,
                &PrepareRequest {
                    role: "codex-1",
                    exec_rules: &[],
                    skills: None,
                    compact_window: None,
                    teammate: &Teammate::default(),
                    model: "gpt-5.6-sol",
                    workdir: tmp.path(),
                    shell_env: &shell_env,
                },
            )
            .unwrap();
        let pairs: Vec<&str> = prepared
            .extra_args
            .windows(2)
            .filter(|w| w[0] == "-c")
            .map(|w| w[1].as_str())
            .collect();
        assert_eq!(
            pairs,
            [
                r#"shell_environment_policy.set.HORCH_RECORD_ID="r-1""#,
                r#"shell_environment_policy.set.HERDR_PANE_ID="w1:p2""#,
                r#"shell_environment_policy.set.HORCH_TASK="say \"hi\"\nnow""#,
            ]
        );
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
        let files: Vec<_> = config
            .map(|text| ("config.toml", text))
            .into_iter()
            .collect();
        fleet_dirs_with(teammate, &files)
    }

    /// `fleet_dirs` over a codex home holding `files` (name, text).
    fn fleet_dirs_with(teammate: &Teammate, files: &[(&str, &str)]) -> Vec<String> {
        let tmp = tempfile::tempdir().unwrap();
        if !files.is_empty() {
            let dir = tmp.path().join("home").join(".codex");
            std::fs::create_dir_all(&dir).unwrap();
            for (name, text) in files {
                std::fs::write(dir.join(name), text).unwrap();
            }
        }
        let (_, prepared) = prepare_in(tmp.path(), teammate);
        let roots = format!("permissions.{FLEET_WRITE_PROFILE}.workspace_roots=");
        prepared
            .extra_args
            .windows(2)
            .filter_map(|w| match w[0].as_str() {
                "--add-dir" => Some(w[1].clone()),
                // The profile's root counts only when the pane runs in it.
                "-c" if write_profile_form(teammate) => w[1].strip_prefix(&roots).map(String::from),
                _ => None,
            })
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

    /// The `-c` pairs a prepared launch for `teammate` passes, and its state
    /// root.
    #[cfg(unix)]
    fn config_pairs(teammate: &Teammate) -> (String, Vec<String>) {
        let tmp = tempfile::tempdir().unwrap();
        let (ctx, prepared) = prepare_in(tmp.path(), teammate);
        let pairs = prepared
            .extra_args
            .windows(2)
            .filter(|w| w[0] == "-c")
            .map(|w| w[1].clone())
            .collect();
        (ctx.paths.state_root.to_string_lossy().into_owned(), pairs)
    }

    /// Write and plan panes define the fleet write profile. A plan pane
    /// cannot take `--add-dir`, and `horch mode <role> write` selects the
    /// profile; writes worked under it and failed again under Read Only (X6
    /// repro 5c, 5d). A write pane runs in it. The folder is writable with
    /// its metadata dirs, `/tmp` and the system temp dir are read-only, the
    /// state root is the extra root, the network is on, and `TMPDIR` is the
    /// pane's own dir under the state root.
    #[cfg(unix)]
    #[test]
    fn write_and_plan_launches_define_the_fleet_write_profile() {
        use crate::roster::PermissionMode::*;
        for mode in [Plan, AcceptEdits, Auto] {
            let teammate = Teammate {
                permission_mode: Some(mode),
                ..Default::default()
            };
            let (state, got) = config_pairs(&teammate);
            assert_eq!(
                got,
                vec![
                    "permissions.fleet_write.extends=\":workspace\"".to_string(),
                    "permissions.fleet_write.filesystem={\":project_roots\"={\".\"=\"write\",\
                     \".git\"=\"write\",\".codex\"=\"write\",\".agents\"=\"write\",\".aws\"=\"write\"},\
                     \":slash_tmp\"=\"read\",\":tmpdir\"=\"read\"}"
                        .to_string(),
                    format!("permissions.fleet_write.workspace_roots={{\"{state}\"=true}}"),
                    "permissions.fleet_write.network.enabled=true".to_string(),
                    format!("shell_environment_policy.set.TMPDIR=\"{state}/tmp/codex-1\""),
                ],
                "permission_mode {mode:?}"
            );
        }
        for mode in [Some(BypassPermissions), None] {
            let teammate = Teammate {
                permission_mode: mode,
                ..Default::default()
            };
            assert!(
                config_pairs(&teammate).1.is_empty(),
                "permission_mode {mode:?}"
            );
        }
    }

    /// A pane's TMPDIR exists before codex starts, and a role name cannot
    /// leave `<state root>/tmp`.
    #[test]
    fn the_pane_tmpdir_is_made_and_stays_under_the_state_root() {
        let tmp = tempfile::tempdir().unwrap();
        let (ctx, _prepared) = prepare_in(
            tmp.path(),
            &Teammate {
                permission_mode: Some(crate::roster::PermissionMode::Auto),
                ..Default::default()
            },
        );
        assert!(ctx.paths.state_root.join("tmp/codex-1").is_dir());
        assert_eq!(
            pane_tmpdir(Path::new("/s"), "../a b/c"),
            PathBuf::from("/s/tmp/___a_b_c")
        );
    }

    /// A read-only launch outside the profile form (the teammate sets its
    /// own sandbox) names the state root as a workspace-write root instead:
    /// codex starts with it and reads it after a switch (X6 repro 2). A
    /// teammate's own pair is kept.
    #[cfg(unix)]
    #[test]
    fn a_legacy_read_only_launch_names_the_state_root_for_a_later_switch() {
        let legacy = Teammate {
            args: vec!["-s".into(), "read-only".into()],
            ..Default::default()
        };
        let (state, got) = config_pairs(&legacy);
        assert_eq!(
            got,
            vec![format!(
                "sandbox_workspace_write.writable_roots=[\"{state}\"]"
            )]
        );
        let own = Teammate {
            args: vec![
                "-s".into(),
                "read-only".into(),
                "-c".into(),
                "sandbox_workspace_write.writable_roots=[\"/own\"]".into(),
            ],
            ..Default::default()
        };
        assert!(config_pairs(&own).1.is_empty());
    }

    /// The plan argv: `default_permissions=":read-only"` and no `-s`, so
    /// `/permissions` lists Read Only (X6 repro 3). With the teammate's own
    /// sandbox, the legacy pair stays (codex refuses both together).
    #[test]
    fn a_plan_pane_launches_with_the_read_only_profile() {
        use crate::roster::PermissionMode::Plan;
        let argv = |args: Vec<&str>| -> Vec<String> {
            let teammate = Teammate {
                model: Some("gpt-5.6-sol".into()),
                permission_mode: Some(Plan),
                args: args.into_iter().map(String::from).collect(),
                ..Default::default()
            };
            codex_command(
                &super::super::LaunchEnv::for_test(),
                &teammate,
                Session::Unmanaged,
                "P",
                None,
            )
            .unwrap()
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
        };
        let plain = argv(vec![]);
        let at = plain
            .iter()
            .position(|a| a == "default_permissions=\":read-only\"");
        let at = at.expect("the read-only profile");
        assert_eq!(plain[at - 1], "-c");
        assert_eq!(plain[at + 1..at + 3], ["-a", "on-request"]);
        assert!(!plain.iter().any(|a| a == "-s" || a == "read-only"));
        let own = argv(vec!["--sandbox", "workspace-write"]);
        assert!(own.windows(2).any(|w| w == ["-s", "read-only"]));
        assert!(!own.iter().any(|a| a.starts_with("default_permissions")));
    }

    /// `launch_sandbox` reads a `default_permissions` profile, in the argv
    /// and in `config.toml`.
    #[test]
    fn a_default_permissions_profile_gives_its_sandbox() {
        let with = |args: Vec<&str>, config: Option<&str>| {
            let t = Teammate {
                args: args.into_iter().map(String::from).collect(),
                ..Default::default()
            };
            launch_sandbox(&t, || config.map(ConfigLayers::base))
        };
        assert_eq!(
            with(vec!["-c", "default_permissions=\":read-only\""], None).as_deref(),
            Some("read-only")
        );
        assert_eq!(
            with(vec![], Some("default_permissions = \":workspace\"\n")).as_deref(),
            Some("workspace-write")
        );
        assert_eq!(
            with(vec![], Some("default_permissions = \"mine\"\n")).as_deref(),
            Some("mine")
        );
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
            // `--approve-for-me` means workspace-write.
            (Some(Auto), vec![], true),
            (Some(AcceptEdits), vec![], true),
            (None, vec!["--approve-for-me"], true),
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

    /// With no sandbox in the argv, the inherited config decides: the profile
    /// file, then the top level of `config.toml`.
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
        let files = [
            ("config.toml", "sandbox_mode = \"workspace-write\"\n"),
            ("ro.config.toml", "sandbox_mode = \"read-only\"\n"),
        ];
        assert!(fleet_dirs_with(&profiled, &files).is_empty());
        let plan_over_config = Teammate {
            permission_mode: Some(crate::roster::PermissionMode::Auto),
            ..Default::default()
        };
        assert_eq!(
            fleet_dirs(&plan_over_config, Some("sandbox_mode = \"read-only\"\n")).len(),
            1
        );
    }

    /// A codex home holding `files` (name, text), and the layers a launch with
    /// `args` reads from it.
    fn layers_of(files: &[(&str, &str)], args: &[&str]) -> (tempfile::TempDir, Teammate) {
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in files {
            std::fs::write(dir.path().join(name), text).unwrap();
        }
        let t = Teammate {
            args: args.iter().map(|a| a.to_string()).collect(),
            ..Default::default()
        };
        (dir, t)
    }

    fn value_of(dir: &Path, t: &Teammate, key: &str) -> Option<String> {
        ConfigLayers::read(dir, args_profile(&t.args))?.value(key)
    }

    /// `-p <name>` layers `$CODEX_HOME/<name>.config.toml` over `config.toml`:
    /// the profile file wins, and `config.toml` answers what it leaves out.
    #[test]
    fn a_profile_file_is_found_and_wins_over_config_toml() {
        let (dir, t) = layers_of(
            &[
                ("config.toml", "model = \"base\"\ntui.theme = \"dark\"\n"),
                ("q.config.toml", "model = \"from-q\"\n"),
            ],
            &["-p", "q"],
        );
        assert_eq!(value_of(dir.path(), &t, "model").as_deref(), Some("from-q"));
        assert_eq!(
            value_of(dir.path(), &t, "tui.theme").as_deref(),
            Some("dark")
        );
        assert_eq!(value_of(dir.path(), &t, "absent"), None);
        let long = Teammate {
            args: vec!["--profile=q".into()],
            ..Default::default()
        };
        assert_eq!(
            value_of(dir.path(), &long, "model").as_deref(),
            Some("from-q")
        );
    }

    /// `config.toml`'s legacy `[profiles.q]` table is not read: not with a
    /// profile file present (only the top level is), and not without one.
    #[test]
    fn the_legacy_profiles_table_is_never_read() {
        let legacy = "model = \"base\"\n[profiles.q]\nmodel = \"legacy\"\nnotify = \"x\"\n";
        let (dir, t) = layers_of(
            &[
                ("config.toml", legacy),
                ("q.config.toml", "tui.theme = \"light\"\n"),
            ],
            &["-p", "q"],
        );
        assert_eq!(value_of(dir.path(), &t, "model").as_deref(), Some("base"));
        assert_eq!(value_of(dir.path(), &t, "notify"), None);
        let (dir, t) = layers_of(&[("config.toml", legacy)], &["-p", "q"]);
        assert_eq!(value_of(dir.path(), &t, "model").as_deref(), Some("base"));
        assert_eq!(value_of(dir.path(), &t, "notify"), None);
        let (dir, t) = layers_of(
            &[("config.toml", "[profiles.q]\nmodel = \"legacy\"\n")],
            &["-p", "q"],
        );
        assert_eq!(value_of(dir.path(), &t, "model"), None);
    }

    /// A top-level `profile = "q"` selects nothing (Codex refuses it), with
    /// or without `-p`; a profile file alone (no `config.toml`) still answers.
    #[test]
    fn a_top_level_profile_key_is_not_a_profile_and_a_profile_file_alone_answers() {
        let (dir, t) = layers_of(
            &[
                ("config.toml", "profile = \"q\"\nmodel = \"base\"\n"),
                ("q.config.toml", "model = \"from-q\"\n"),
            ],
            &[],
        );
        assert_eq!(value_of(dir.path(), &t, "model").as_deref(), Some("base"));
        let (dir, t) = layers_of(&[("q.config.toml", "model = \"only\"\n")], &["-p", "q"]);
        assert_eq!(value_of(dir.path(), &t, "model").as_deref(), Some("only"));
        let (dir, t) = layers_of(&[], &["-p", "q"]);
        assert!(ConfigLayers::read(dir.path(), args_profile(&t.args)).is_none());
    }

    /// No `-p`: only `config.toml` is read, whatever profile files exist.
    #[test]
    fn without_a_profile_flag_only_config_toml_is_read() {
        let (dir, t) = layers_of(
            &[
                ("config.toml", "model = \"base\"\n"),
                ("q.config.toml", "model = \"from-q\"\nnotify = \"x\"\n"),
            ],
            &[],
        );
        assert_eq!(value_of(dir.path(), &t, "model").as_deref(), Some("base"));
        assert_eq!(value_of(dir.path(), &t, "notify"), None);
    }

    /// A profile name that is not one file name reads no file.
    #[test]
    fn a_profile_name_cannot_leave_the_codex_home() {
        let dir = Path::new("/h");
        assert_eq!(
            profile_config_path(dir, Some("q")),
            Some(dir.join("q.config.toml"))
        );
        for bad in ["", ".", "..", "../x", "a/b", "a\\b"] {
            assert_eq!(profile_config_path(dir, Some(bad)), None, "{bad:?}");
        }
        assert_eq!(profile_config_path(dir, None), None);
    }

    /// The operator limit comes from the profile file and names it.
    #[test]
    fn the_operator_limit_reads_the_profile_file() {
        let (dir, t) = layers_of(
            &[
                ("config.toml", "model_auto_compact_token_limit = 180000\n"),
                (
                    "q.config.toml",
                    "model_auto_compact_token_limit = 160_000\n",
                ),
            ],
            &["-p", "q"],
        );
        let profile = args_profile(&t.args);
        let layers = ConfigLayers::read(dir.path(), profile);
        let got = codex_operator_limit(layers.as_ref(), dir.path(), profile).unwrap();
        assert_eq!(got.tokens, Some(160_000));
        assert!(got.detail.ends_with("q.config.toml"), "{}", got.detail);
        let layers = ConfigLayers::read(dir.path(), None);
        let got = codex_operator_limit(layers.as_ref(), dir.path(), None).unwrap();
        assert_eq!(got.tokens, Some(180_000));
        assert!(got.detail.ends_with("config.toml"), "{}", got.detail);
    }

    /// `-p <name>` only works in the private home if `<name>.config.toml` is
    /// reachable there: the home links every entry but `rules/`.
    #[test]
    fn a_private_home_reaches_the_profile_file() {
        let home = fake_codex_home();
        let real = home.path().join(".codex");
        std::fs::write(real.join("q.config.toml"), "model = \"from-q\"\n").unwrap();
        let state = tempfile::tempdir().unwrap();
        let dir = build_private_home(&real, state.path(), "codex-sol-1").unwrap();
        assert!(dir
            .join("q.config.toml")
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            std::fs::read_to_string(dir.join("q.config.toml")).unwrap(),
            "model = \"from-q\"\n"
        );
    }

    /// The `/permissions` menu of a legacy read-only launch, after a switch,
    /// as the research pane showed it (Codex 0.160.0, X6).
    const MENU_3: &str = "  rc=0\n\n  Update Model Permissions\n\n\n\
        › 1. Ask for approval (current)  Read and edit workspace files and run commands, with approval required for internet access\n\
        \x20                                or edits outside the workspace\n\
        \x20 2. Approve for me              Only ask for actions detected as potentially unsafe\n\
        \x20 3. Full Access                 Use with caution: Codex can edit files outside this workspace and access the internet\n\
        \x20                                without approval\n\n  enter select · esc back\n";

    /// The menu of the profile launch (X6 repro 5b): 5 items, the cursor on
    /// item 2, narrow wrapping.
    const MENU_5: &str = "  Update Model Permissions\n\n\
        \x20 1. Ask for approval  Read and edit workspace files and run\n\
        \x20                      commands\n\
        › 2. Approve for me    Only ask for actions detected as\n\
        \x20                      potentially unsafe\n\
        \x20 3. Full Access       Use with caution\n\
        \x20 4. Read Only         Read workspace files\n\
        \x20 5. fleet_write (current)\n\n  enter select · esc back\n";

    #[test]
    fn the_permissions_menu_parses_its_items_and_cursor() {
        let m = parse_permissions_menu(MENU_3).unwrap();
        assert_eq!(
            m.items,
            ["Ask for approval", "Approve for me", "Full Access"]
        );
        assert_eq!(m.cursor, 0);
        let m = parse_permissions_menu(MENU_5).unwrap();
        assert_eq!(
            m.items,
            [
                "Ask for approval",
                "Approve for me",
                "Full Access",
                "Read Only",
                "fleet_write"
            ]
        );
        assert_eq!(m.cursor_label(), Some("Approve for me"));
        assert!(parse_permissions_menu("› Ask Codex to do anything\n").is_none());
    }

    /// Down moves wrap (seen live), so the count is (target - cursor) mod n.
    #[test]
    fn the_downs_to_a_label_wrap_around_the_menu() {
        let m = parse_permissions_menu(MENU_5).unwrap();
        assert_eq!(m.downs_to(PaneMode::Plan.menu_label()), Some(2));
        assert_eq!(m.downs_to(PaneMode::Write.menu_label()), Some(3));
        assert_eq!(m.downs_to("Ask for approval"), Some(4));
        assert_eq!(m.downs_to("Approve for me"), Some(0));
        let m3 = parse_permissions_menu(MENU_3).unwrap();
        assert_eq!(m3.downs_to(PaneMode::Plan.menu_label()), None);
    }

    /// Only the menu after the last title counts: an older menu in the
    /// scrollback does not.
    #[test]
    fn only_the_newest_menu_counts() {
        let screen = format!("{MENU_5}\n• Permission selection requested: Read Only\n{MENU_3}");
        let m = parse_permissions_menu(&screen).unwrap();
        assert_eq!(m.items.len(), 3);
    }

    /// `/status` wraps the Permissions value at narrow widths (seen live:
    /// the state root broke after `.local/`). The newest block counts.
    #[test]
    fn the_status_permissions_line_joins_its_wrapped_parts() {
        let screen = "  Permissions:         Read Only\n  Agents.md:           <none>\n\
            ...\n  Directory:           ~/projects/multi-herdr\n\
            \x20 Permissions:         Workspace [/Users/mascott/.local/\n\
            \x20                      state/horch] (Ask for approval)\n\
            \x20 Agents.md:           <none>\n";
        let got = status_permissions(screen).unwrap();
        assert_eq!(
            got,
            "Workspace [/Users/mascott/.local/state/horch] (Ask for approval)"
        );
        assert!(!status_shows(PaneMode::Write, &got));
        assert!(!status_shows(PaneMode::Plan, &got));
        assert!(status_shows(PaneMode::Write, "Profile fleet_write"));
        assert!(status_shows(PaneMode::Write, "Profile fleet_\n write"));
        assert!(!status_shows(PaneMode::Write, "Profile fleet_plan"));
        assert!(status_shows(PaneMode::Plan, "Read Only (Ask for approval)"));
        assert!(!status_shows(PaneMode::Plan, "Profile fleet_write"));
        assert!(status_permissions("no status here").is_none());
    }

    #[test]
    fn a_pane_mode_parses_its_own_names_only() {
        assert_eq!(PaneMode::parse("write"), Some(PaneMode::Write));
        assert_eq!(PaneMode::parse("plan"), Some(PaneMode::Plan));
        assert_eq!(PaneMode::parse("read-only"), None);
        assert_eq!(PaneMode::Write.as_str(), "write");
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

    /// Windows shares 1 rules file between every codex session. A forbidden
    /// worker rule there would also refuse `horch ledger add` to the codex
    /// orchestrator and to the operator's own sessions, so the shared file
    /// gets only the allow rules.
    #[test]
    fn the_shared_rules_file_gets_no_forbidden_rule() {
        let home = tempfile::tempdir().unwrap();
        ensure_rules(home.path(), Roster::builtin().unwrap().exec_rules()).unwrap();
        let text = std::fs::read_to_string(rules_path(home.path())).unwrap();
        assert!(!text.contains("forbidden"), "{text}");
        assert_eq!(
            rule_decision(&text, &["horch", "ledger", "finish"]),
            Some("allow".into())
        );
        assert_eq!(
            rule_decision(&text, &["horch", "ledger", "add", "t.md"]),
            None
        );
    }

    /// A shared rules file that cannot be read as text (UTF-16 from
    /// PowerShell, 1 bad byte) is an error, never an empty file to overwrite:
    /// the operator's own rules stay.
    #[test]
    fn an_unreadable_shared_rules_file_is_left_alone() {
        let home = tempfile::tempdir().unwrap();
        let path = rules_path(home.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let utf16: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("# mine\n".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        std::fs::write(&path, &utf16).unwrap();
        let err = ensure_rules(home.path(), Roster::builtin().unwrap().exec_rules());
        assert!(err.is_err(), "an unreadable file must not be rewritten");
        assert_eq!(std::fs::read(&path).unwrap(), utf16);
    }

    /// v0.4.0 appended 1 broad `["horch", "ledger"]` allow to the shared file.
    /// An upgrade removes exactly that block, so the narrowed rules apply on
    /// Windows too, and keeps the operator's own rules.
    #[test]
    fn the_shared_rules_file_loses_the_old_broad_ledger_rule() {
        let home = tempfile::tempdir().unwrap();
        let path = rules_path(home.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let own = "prefix_rule(\n    pattern = [\"horch\", \"ledger\"],\n    \
                   decision = \"allow\",\n    justification = \"my own\",\n)\n";
        let old = "prefix_rule(\n    pattern = [\"horch\", \"ledger\"],\n    \
                   decision = \"allow\",\n    justification = \"herdr-fleet: claim, report \
                   and finish tasks through the task ledger\",\n)\n";
        std::fs::write(&path, format!("# the user's own\n{old}")).unwrap();
        ensure_rules(home.path(), Roster::builtin().unwrap().exec_rules()).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# the user's own\n"), "{text}");
        assert!(!text.contains("claim, report and finish"), "{text}");
        assert_eq!(
            rule_decision(&text, &["horch", "ledger", "cancel", "T-1"]),
            None
        );
        assert_eq!(
            rule_decision(&text, &["horch", "ledger", "claim"]),
            Some("allow".into())
        );

        // The next launch finds the narrowed rules already there and writes
        // nothing.
        ensure_rules(home.path(), Roster::builtin().unwrap().exec_rules()).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);

        // A file that already holds the narrowed rules still loses the old
        // broad block, and a second launch leaves it byte for byte.
        std::fs::write(&path, format!("{text}{old}")).unwrap();
        ensure_rules(home.path(), Roster::builtin().unwrap().exec_rules()).unwrap();
        let migrated = std::fs::read(&path).unwrap();
        assert_eq!(String::from_utf8(migrated.clone()).unwrap(), text);
        ensure_rules(home.path(), Roster::builtin().unwrap().exec_rules()).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), migrated);

        // A rule the operator wrote with the same pattern is theirs: it stays.
        std::fs::write(&path, own).unwrap();
        ensure_rules(home.path(), Roster::builtin().unwrap().exec_rules()).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().starts_with(own));
    }
}
