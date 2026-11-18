//! The executor of the `push` job (ticket 38).
//!
//! The subject of a run is the feature, and its input key names the tip,
//! the lease and the remote (`application::remote::push_key`). The job
//! process reads the ledger only (RR-5): [`run`] pushes and gives the facts
//! of the run in its result file, and [`record`] in fleetd writes them in
//! the transaction of the end of the run, at the fleet clock of the ledger:
//!
//! 1. The fleetd git rule (Q26 of MAP.md of fleetd-p4): this job runs git
//!    with the rights of fleetd and with no sandbox, so git starts no hook
//!    and no program of the project. Each git call has the keys of
//!    [`SAFE_CONFIG`] and the environment of [`SAFE_ENV`]. The run first
//!    reads `git config --list --show-origin`: a key of the repository or
//!    worktree config that starts a program ends the run `failed`, with the
//!    key in its reason, before the push.
//! 2. `git push --force-with-lease=<branch>:<seen_remote_tip> <remote>
//!    <tip>:<branch>` (I9), bound by 10 min (F11). `core.sshCommand` and
//!    `credential.helper` are given on the command line with the values of
//!    the global config of the user, so no value of the project is used.
//!    The push never retries: a failed run backs off, and the next run is a
//!    new run of the job (FM-43).
//! 3. The pull request follows the state of the feature (ticket 39, Q22):
//!    after a passed push, the job reads `pr` of the project file at the
//!    tip of the feature branch. With `pr = gh`, a `ready` feature gets its
//!    pull request: the open pull request of the branch, else a new one,
//!    marked ready for review, with 1 section for each landed task between
//!    its markers (F17). An `open` feature with a pull request marks it as a
//!    draft. Each `gh` call runs through the launcher, bound by F11. With
//!    `pr = manual`, the compare URL goes on the needs-you list, and on an
//!    `open` feature the item says "do not merge yet". A failed pull request
//!    step records a failed push run, so the job backs off.
//! 4. The base of the pull request (ticket 38 of fleetd-p6): the branch of
//!    the lower feature while the feature is on a lower feature in flight,
//!    else the primary branch. An open pull request with another base gets
//!    `gh pr edit --base`, and the compare URL of `pr = manual` names the
//!    base. So when a stack edge ends, the push after the rebase moves the
//!    base to the primary branch, or changes the needs-you item.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use horch_marketplace::git::{GitOutput, REPO_ENV};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::fleetd::fleet_runtime::adapters::jobs_store::JobRow;
use crate::fleetd::fleet_runtime::application::jobs::{JobCtx, NewItem, Ran};
use crate::fleetd::fleet_runtime::application::ledger::Cause;
use crate::fleetd::fleet_runtime::domain::jobs::Outcome;
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::integration::adapters::exec_fetch::Tools;
use crate::fleetd::integration::adapters::executors::{check_text, clip, facts_of, FACT_TEXT_MAX};
use crate::fleetd::integration::adapters::project_file;
use crate::fleetd::integration::adapters::sqlite::{self as rows, FeatureFact, FeatureRow};
use crate::fleetd::integration::application::remote::{self, PushInput};
use crate::fleetd::integration::domain::project::{Pr, ProjectFileReader, FILE_NAME};
use crate::fleetd::worker_profiles::adapters::launcher::Launcher;
use crate::fleetd::worker_profiles::application::ports::{LaunchSpec, ProcessHost};
use crate::fleetd::worktree_custody::adapters::git::project_program;
use crate::fleetd::worktree_custody::application::create::fleet_clock;
use crate::runtime::context::ProcessEnv;
use crate::runtime::process;
use crate::taskledger::finish::pr::with_new_sections;
use crate::taskledger::model::Actor;
use crate::taskledger::tasks;
use crate::vcs::network;

/// The `-c` keys of each git call of the job, before its subcommand. The
/// command line is the last scope that git reads, so each key wins over
/// the config of the project.
const SAFE_CONFIG: [&str; 5] = [
    // Hooks, among them `pre-push`.
    "core.hooksPath=/dev/null",
    // The fsmonitor program.
    "core.fsmonitor=false",
    // The gpg program.
    "commit.gpgSign=false",
    "tag.gpgSign=false",
    // The `ext::` transport runs a command.
    "protocol.ext.allow=never",
];

/// The environment of each git call of the job. The system config is not
/// read, as for each git network call of horch.
const SAFE_ENV: [(&str, &str); 5] = [
    ("GIT_TERMINAL_PROMPT", "0"),
    ("GIT_EDITOR", "true"),
    ("GIT_SEQUENCE_EDITOR", "true"),
    ("LC_ALL", "C"),
    ("GIT_CONFIG_NOSYSTEM", "1"),
];

/// The http stall limits of a push (F11): a transfer that stays below 1000
/// bytes/s for 60 s ends.
const HTTP_STALL: [&str; 2] = ["http.lowSpeedLimit=1000", "http.lowSpeedTime=60"];

/// Runs the job `row` with the programs of this process and the launcher.
pub fn run(conn: &Connection, row: &JobRow) -> Result<Ran> {
    run_with(conn, row, &Tools::from_env(&ProcessEnv), &Launcher)
}

/// [`run`] with the programs `tools` and the launcher `host` of each `gh`
/// call. It writes no row: the facts of the push and of the pull request
/// step are in its result.
pub fn run_with(
    conn: &Connection,
    row: &JobRow,
    tools: &Tools,
    host: &dyn ProcessHost,
) -> Result<Ran> {
    let pushed = push(conn, row, &tools.git);
    let followed = match &pushed {
        Ok(input) => Some(follow_pr(conn, row, tools, host, &input.remote)),
        Err(_) => None,
    };
    let facts = match (&pushed, &followed) {
        (Ok(_), Some(Err(why))) => PushFacts {
            pushed: true,
            failure: Some(clip(format!("the pull request step failed: {why:#}"))),
            ..PushFacts::default()
        },
        (Ok(_), Some(Ok(pr))) => PushFacts {
            pushed: true,
            pr: pr.pr.clone(),
            manual: pr.manual,
            ..PushFacts::default()
        },
        (Ok(_), None) => PushFacts {
            pushed: true,
            ..PushFacts::default()
        },
        (Err(why), _) => PushFacts {
            failure: Some(clip(format!("{why:#}"))),
            ..PushFacts::default()
        },
    };
    let outcome = match (pushed, followed) {
        (Ok(input), Some(Err(why))) => Outcome {
            passed: false,
            verdict: true,
            text: format!(
                "pushed {} to {}, but the pull request step failed: {why:#}",
                input.tip, input.remote
            ),
        },
        (Ok(input), _) => Outcome {
            passed: true,
            verdict: true,
            text: format!("pushed {} to {}", input.tip, input.remote),
        },
        (Err(why), _) => Outcome {
            passed: false,
            verdict: true,
            text: format!("the push failed: {why:#}"),
        },
    };
    Ok(Ran {
        outcome,
        data: Some(serde_json::to_value(&facts)?),
    })
}

/// The longest pull request or compare URL of a result file, in bytes.
const PR_MAX: usize = 2048;

/// The facts of a push run in its result file.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PushFacts {
    /// Whether `git push` passed.
    pushed: bool,
    /// The cause of the failure of the push, or of the pull request step
    /// after a passed push.
    failure: Option<String>,
    /// The pull request or the compare URL of the feature.
    pr: Option<String>,
    /// `Some(ready)`: `pr` is a compare URL for the needs-you list, of a
    /// feature that was `ready`.
    manual: Option<bool>,
}

impl PushFacts {
    /// Refuse facts that no run gives: the form of each fact, and the
    /// facts that go together.
    fn check(&self) -> Result<()> {
        if !self.pushed && (self.failure.is_none() || self.pr.is_some() || self.manual.is_some()) {
            bail!("a failed push has its cause and no pull request");
        }
        if self.manual.is_some() && self.pr.is_none() {
            bail!("a compare URL fact has no URL");
        }
        if let Some(why) = &self.failure {
            check_text("failure", why, FACT_TEXT_MAX)?;
        }
        if let Some(pr) = &self.pr {
            check_text("pr", pr, PR_MAX)?;
            if pr.trim().is_empty() || pr.contains(['\n', '\t']) {
                bail!("the fact pr is not 1 line");
            }
        }
        Ok(())
    }
}

/// Record the result `ran` of the push run `row`, in `tx` of fleetd: a
/// passed push and its pull request facts, or a failed run. The feature
/// and the tip come from the row, so a file changes only the feature of
/// its own job.
pub fn record(ctx: &JobCtx<'_>, tx: &Connection, row: &JobRow, ran: &Ran) -> Result<()> {
    let facts: PushFacts =
        facts_of(ran.data.as_ref())?.context("the result file of a push has no facts")?;
    facts.check()?;
    let feature = row.subject.as_str();
    rows::feature(tx, feature)?.with_context(|| format!("no feature {feature} in the ledger"))?;
    let failed = |why: &str| -> Result<()> {
        let clock = fleet_clock(tx)?;
        remote::record_push_failed(
            tx,
            ctx.guard,
            ctx.needs_you,
            ctx.config,
            feature,
            clock,
            why,
        )
    };
    if !facts.pushed {
        return failed(facts.failure.as_deref().unwrap_or_default());
    }
    let input = PushInput::parse(&row.input_key)
        .with_context(|| format!("the input key {:?} is not a push key", row.input_key))?;
    remote::record_push_passed(tx, ctx.guard, ctx.needs_you, feature, &input.tip)?;
    match facts.failure.as_deref() {
        Some(why) => failed(why),
        None => record_pr(ctx, tx, feature, &facts),
    }
}

/// The push of the run `row`: its input, or the cause of its failure.
fn push(conn: &Connection, row: &JobRow, git: &Path) -> Result<PushInput> {
    let input = PushInput::parse(&row.input_key)
        .ok_or_else(|| anyhow!("the input key {:?} is not a push key", row.input_key))?;
    if input.remote.starts_with('-') {
        return Err(anyhow!("the remote {:?} starts with -", input.remote));
    }
    let feature = rows::feature(conn, &row.subject)?
        .ok_or_else(|| anyhow!("no feature {} in the ledger", row.subject))?;
    let branch = feature
        .branch
        .ok_or_else(|| anyhow!("feature {} has no branch", row.subject))?;
    let repo = PathBuf::from(
        row.repo
            .as_deref()
            .ok_or_else(|| anyhow!("the push job {} has no repository", row.id))?,
    );
    refuse_project_programs(git, &repo)?;
    let target = format!("refs/heads/{branch}");
    let lease = format!(
        "--force-with-lease={target}:{}",
        input.seen.as_deref().unwrap_or("")
    );
    let refspec = format!("{}:{target}", input.tip);
    let args = [
        "push",
        "--porcelain",
        "--no-verify",
        "--recurse-submodules=no",
        "--receive-pack=git-receive-pack",
        &lease,
        &input.remote,
        &refspec,
    ];
    let out = run_git(git, &repo, &user_programs(git, &repo)?, &args)?;
    if out.status != 0 {
        return Err(anyhow!("{}", failure_text(&out)));
    }
    Ok(input)
}

/// An error that names the first key of the repository or worktree config
/// of `repo` that starts a program.
fn refuse_project_programs(git: &Path, repo: &Path) -> Result<()> {
    let args = ["config", "--list", "--show-origin", "--show-scope", "-z"];
    let out = run_git(git, repo, &[], &args)?;
    if out.status != 0 {
        return Err(anyhow!("git config --list failed: {}", failure_text(&out)));
    }
    if let Some((key, origin)) = project_program(&String::from_utf8_lossy(&out.stdout)) {
        return Err(anyhow!(
            "the project config sets {key} ({origin}), which starts a program; \
             fleetd runs no program of the project, so the job changes nothing"
        ));
    }
    Ok(())
}

/// The `-c` keys of the 2 programs that a push may start, with the values
/// of the global config of the user: `core.sshCommand` (else the ssh
/// command of horch with its stall limits), and each `credential.helper`
/// after an empty value that empties the list.
fn user_programs(git: &Path, repo: &Path) -> Result<Vec<String>> {
    let global = |args: &[&str]| -> Result<Vec<String>> {
        let out = run_git(git, repo, &[], args)?;
        // Exit 1: the key is not set.
        match out.status {
            0 => Ok(out.stdout_text().lines().map(str::to_string).collect()),
            1 => Ok(Vec::new()),
            _ => Err(anyhow!(
                "git {} failed: {}",
                args.join(" "),
                failure_text(&out)
            )),
        }
    };
    let ssh = global(&["config", "--global", "--get", "core.sshCommand"])?;
    let ssh = ssh.first().map_or(network::SSH_COMMAND, String::as_str);
    let mut config = vec![
        format!("core.sshCommand={ssh}"),
        "credential.helper=".to_string(),
    ];
    for helper in global(&["config", "--global", "--get-all", "credential.helper"])? {
        config.push(format!("credential.helper={helper}"));
    }
    config.extend(HTTP_STALL.iter().map(|c| c.to_string()));
    Ok(config)
}

/// `git -C <dir> <SAFE_CONFIG> <config> <args>` with the environment of
/// [`SAFE_ENV`], in its own process group, killed at the bound of the git
/// network calls (F11). A non-zero exit is an `Ok` output.
fn run_git(git: &Path, dir: &Path, config: &[String], args: &[&str]) -> Result<GitOutput> {
    let mut cmd = horch_platform::start::command(git);
    cmd.current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // The `-C` folder alone selects the repository and its config.
    for key in REPO_ENV {
        cmd.env_remove(key);
    }
    for (key, value) in SAFE_ENV {
        cmd.env(key, value);
    }
    horch_platform::group::new_group(&mut cmd);
    cmd.arg("-C").arg(std::path::absolute(dir)?);
    for c in SAFE_CONFIG
        .iter()
        .copied()
        .chain(config.iter().map(String::as_str))
    {
        cmd.arg("-c").arg(c);
    }
    cmd.args(args);
    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow!("cannot run {}: {e}", git.display()))?;
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut buf);
            }
            buf
        })
    };
    let stdout = drain(child.stdout.take().map(|p| Box::new(p) as _));
    let stderr = drain(child.stderr.take().map(|p| Box::new(p) as _));
    let bound = network::bound(args);
    let deadline = Instant::now() + bound;
    let Some(status) = process::wait_until(&mut child, deadline, Duration::from_millis(50))? else {
        let pid = child.id();
        process::kill_group(&mut child);
        let _ = child.wait();
        return Err(network::Timeout {
            command: format!("git {}", args.join(" ")),
            bound,
            pid,
        }
        .into());
    };
    Ok(GitOutput {
        status: status.code().unwrap_or(-1),
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

/// The message of a failed git call: its stderr, and the porcelain lines
/// of a push, which name a rejected ref and its cause.
fn failure_text(out: &GitOutput) -> String {
    let text = format!(
        "{} {}",
        String::from_utf8_lossy(&out.stderr).trim(),
        String::from_utf8_lossy(&out.stdout).trim()
    );
    text.trim().to_string()
}

// ─── the pull request ───────────────────────────────────────────────────

/// The kind of the needs-you item of the compare URL of a feature with
/// `pr = manual`.
pub const PR_MANUAL_KIND: &str = "pr_manual";

/// The key of the needs-you item of the compare URL of `feature`.
pub fn pr_manual_key(feature: &str) -> String {
    format!("{PR_MANUAL_KIND}:{feature}")
}

/// What the pull request step found: the pull request or compare URL of
/// the feature, and for a compare URL whether the feature was `ready`.
#[derive(Debug, Default)]
struct PrFacts {
    pr: Option<String>,
    manual: Option<bool>,
}

/// The pull request step after a passed push of the feature of `row` to
/// `remote` (Q22). It changes nothing in the ledger.
fn follow_pr(
    conn: &Connection,
    row: &JobRow,
    tools: &Tools,
    host: &dyn ProcessHost,
    remote: &str,
) -> Result<PrFacts> {
    let f = rows::feature(conn, &row.subject)?
        .ok_or_else(|| anyhow!("no feature {} in the ledger", row.subject))?;
    let (Some(branch), Some(repo)) = (f.branch.as_deref(), row.repo.as_deref()) else {
        return Ok(PrFacts::default());
    };
    let repo = Path::new(repo);
    let reader = BlobReader {
        git: &tools.git,
        repo,
    };
    let ready = f.state == "ready";
    let draft = f.state == "open" && f.pr.is_some();
    if !ready && !draft {
        return Ok(PrFacts::default());
    }
    let base = pr_base(conn, &f)?;
    let settings = project_file::load(&reader, &format!("refs/heads/{branch}"))?;
    match settings.pr {
        Pr::Manual => manual(&tools.git, repo, remote, branch, &base, ready),
        Pr::Gh => {
            let gh = Gh {
                bin: tools
                    .gh
                    .as_deref()
                    .ok_or_else(|| anyhow!("gh is not on PATH"))?,
                host,
                repo,
                temp: &tools.temp,
                tag: format!("{}-{}", row.id, f.id),
            };
            if ready {
                gh_ready(conn, &gh, &f, branch, &base)
            } else {
                gh_draft(&gh, &f, &base)
            }
        }
    }
}

/// The base branch of the pull request of `f`: the branch of its lower
/// feature while that feature is in flight, else the primary branch.
fn pr_base(conn: &Connection, f: &FeatureRow) -> Result<String> {
    if let Some(lower) = f.stacked_on.as_deref() {
        if let Some(l) = rows::feature(conn, lower)? {
            if let Some(branch) = l
                .branch
                .filter(|_| matches!(l.state.as_str(), "open" | "ready"))
            {
                return Ok(branch);
            }
        }
    }
    Ok(f.primary_branch.clone())
}

/// The pull request as `gh pr view --json url,state,isDraft,body,baseRefName`
/// reads it.
#[derive(serde::Deserialize)]
struct PrView {
    url: String,
    state: String,
    #[serde(rename = "isDraft")]
    draft: bool,
    body: String,
    /// `None` when `gh` did not give it: the base is not known, so it does
    /// not move.
    #[serde(rename = "baseRefName", default)]
    base: Option<String>,
}

/// Set the base of the open pull request `v` to `base` when `gh` gives
/// another base.
fn move_base(gh: &Gh<'_>, v: &PrView, base: &str) -> Result<()> {
    if v.base.as_deref().is_some_and(|now| now != base) {
        gh.call(&["pr", "edit", &v.url, "--base", base])?;
    }
    Ok(())
}

/// A `ready` feature with `pr = gh`: its recorded pull request, else the
/// open pull request of its branch (a claim attempt opened it), else a new
/// one. The body gets the section of each landed task that it lacks, and
/// a draft becomes ready for review. A recorded pull request that is not
/// open is left alone: the fetch job puts it on the needs-you list.
fn gh_ready(
    conn: &Connection,
    gh: &Gh<'_>,
    f: &FeatureRow,
    branch: &str,
    base: &str,
) -> Result<PrFacts> {
    let found = match f.pr.as_deref() {
        Some(pr) => Some(gh.view(pr)?),
        // gh exits non-zero when the branch has no pull request.
        None => gh.view(branch).ok(),
    };
    let sections = task_sections(conn, &f.id, branch)?;
    let url = match found {
        Some(v) if v.state.eq_ignore_ascii_case("OPEN") => {
            move_base(gh, &v, base)?;
            let body = with_new_sections(&v.body, &sections);
            if body != v.body {
                let file = gh.body_file(&body)?;
                gh.call(&["pr", "edit", &v.url, "--body-file", &file])?;
            }
            if v.draft {
                gh.call(&["pr", "ready", &v.url])?;
            }
            v.url
        }
        Some(_) if f.pr.is_some() => return Ok(PrFacts::default()),
        _ => {
            let intro = format!(
                "Feature `{}`: its tasks land on `{branch}`; this pull request brings them to `{base}`.",
                f.id
            );
            let file = gh.body_file(&with_new_sections(&intro, &sections))?;
            let title = format!("{}: feature {}", f.name.as_deref().unwrap_or(branch), f.id);
            let out = gh.call(&[
                "pr",
                "create",
                "--base",
                base,
                "--head",
                branch,
                "--title",
                &title,
                "--body-file",
                &file,
            ])?;
            out.lines()
                .map(str::trim)
                .rfind(|l| l.starts_with("https://") || l.starts_with("http://"))
                .map(str::to_string)
                .ok_or_else(|| anyhow!("gh pr create printed no URL: {}", out.trim()))?
        }
    };
    Ok(PrFacts {
        pr: Some(url),
        manual: None,
    })
}

/// An `open` feature with a pull request and `pr = gh`: an open pull
/// request that is not a draft becomes a draft.
fn gh_draft(gh: &Gh<'_>, f: &FeatureRow, base: &str) -> Result<PrFacts> {
    let Some(pr) = f.pr.as_deref() else {
        return Ok(PrFacts::default());
    };
    let v = gh.view(pr)?;
    if v.state.eq_ignore_ascii_case("OPEN") {
        move_base(gh, &v, base)?;
        if !v.draft {
            gh.call(&["pr", "ready", &v.url, "--undo"])?;
        }
    }
    Ok(PrFacts::default())
}

/// `pr = manual`: the compare URL of the feature branch over `base`, and
/// whether the feature is `ready`, for the text of its needs-you item. A
/// remote with no web address gets the branch names.
fn manual(
    git: &Path,
    repo: &Path,
    remote: &str,
    branch: &str,
    base: &str,
    ready: bool,
) -> Result<PrFacts> {
    let key = format!("remote.{remote}.url");
    let out = run_git(git, repo, &[], &["config", "--get", &key])?;
    let link = web_base(&out.stdout_text())
        .map(|web| format!("{web}/compare/{base}...{branch}"))
        .unwrap_or_else(|| format!("{base}...{branch} of the remote {remote}"));
    Ok(PrFacts {
        pr: Some(link),
        manual: Some(ready),
    })
}

/// The text of the needs-you item of the compare URL `link` of `feature`.
fn manual_text(feature: &str, link: &str, ready: bool) -> String {
    if ready {
        format!("Feature {feature} is ready. Open its pull request, then merge it: {link}")
    } else {
        format!(
            "Feature {feature} is open again: do not merge {link} yet. fleetd tells you when it is ready."
        )
    }
}

/// The web address of the remote URL `url`: `https://<host>/<path>` with
/// no user, port or `.git`. `None` for a local path.
fn web_base(url: &str) -> Option<String> {
    let url = url.trim().trim_end_matches('/');
    let url = url.strip_suffix(".git").unwrap_or(url);
    let schemed = ["https://", "http://", "ssh://", "git://"]
        .iter()
        .find_map(|s| url.strip_prefix(s));
    let (host, path) = match schemed {
        Some(rest) => rest.split_once('/')?,
        None => {
            // The scp form `user@host:path`. A drive letter is a local path.
            let (host, path) = url.split_once(':')?;
            if host.contains('/') || host.len() < 2 {
                return None;
            }
            (host, path)
        }
    };
    let host = host.rsplit('@').next()?.split(':').next()?;
    let path = path.trim_start_matches('/');
    (!host.is_empty() && !path.is_empty()).then(|| format!("https://{host}/{path}"))
}

/// The section of each landed task of `feature` (F17), oldest first, as
/// `(short id, text)`.
fn task_sections(conn: &Connection, feature: &str, branch: &str) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT task_id FROM branches
         WHERE feature_id = ?1 AND task_id IS NOT NULL AND landing_state = 'landed'
         ORDER BY rowid",
    )?;
    let ids = stmt
        .query_map([feature], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ids.iter()
        .map(|id| {
            let t = tasks::get(conn, id)?;
            let short = t.short();
            let text = format!("### {short} {}\n\nLanded on `{branch}`.", t.title);
            Ok((short, text))
        })
        .collect()
}

/// Record the facts of the pull request step of `feature`, in `tx`: `pr`,
/// `pr_state` `open`, and the needs-you item of a compare URL.
fn record_pr(ctx: &JobCtx<'_>, tx: &Connection, feature: &str, facts: &PushFacts) -> Result<()> {
    let Some(pr) = facts.pr.as_deref() else {
        return Ok(());
    };
    let f =
        rows::feature(tx, feature)?.ok_or_else(|| anyhow!("no feature {feature} in the ledger"))?;
    let cause = Cause {
        actor: Actor::Horch,
        text: "the push job followed the pull request",
    };
    if f.pr.as_deref() != Some(pr) {
        rows::set_feature_fact(tx, ctx.guard, feature, FeatureFact::Pr(pr), cause)?;
    }
    if f.pr_state != "open" {
        rows::set_feature_fact(tx, ctx.guard, feature, FeatureFact::PrState("open"), cause)?;
    }
    if let Some(ready) = facts.manual {
        let key = pr_manual_key(feature);
        let text = manual_text(feature, pr, ready);
        let item = NewItem {
            key: &key,
            kind: PR_MANUAL_KIND,
            entity: Entity::Feature,
            entity_id: feature,
            text: &text,
        };
        ctx.needs_you.open_item(tx, &item)?;
    }
    Ok(())
}

/// The `gh` of 1 run: each call runs through the launcher in the
/// repository, bound by the bound of a git network call (F11).
struct Gh<'a> {
    bin: &'a Path,
    host: &'a dyn ProcessHost,
    repo: &'a Path,
    temp: &'a Path,
    /// The run and the feature, in the names of the files of the calls.
    tag: String,
}

impl Gh<'_> {
    /// The pull request `pr` (a URL, a number or a branch).
    fn view(&self, pr: &str) -> Result<PrView> {
        let fields = "url,state,isDraft,body,baseRefName";
        let text = self.call(&["pr", "view", pr, "--json", fields])?;
        // The output file has stdout and stderr: take the line with the JSON.
        text.lines()
            .find_map(|l| serde_json::from_str::<PrView>(l).ok())
            .ok_or_else(|| anyhow!("gh gave no pull request: {}", text.trim()))
    }

    /// `body` in a file for `--body-file`. Its path.
    fn body_file(&self, body: &str) -> Result<String> {
        let file = self
            .temp
            .join(format!("horch-push-gh-{}.body.md", self.tag));
        std::fs::write(&file, body)?;
        Ok(file.to_string_lossy().into_owned())
    }

    /// `gh <args>`: its output, or an error with its output when it exits
    /// non-zero or runs past its bound.
    fn call(&self, args: &[&str]) -> Result<String> {
        let output = self.temp.join(format!("horch-push-gh-{}.out", self.tag));
        let _ = std::fs::remove_file(&output);
        let spec = LaunchSpec {
            program: self.bin.to_path_buf(),
            args: args.iter().map(Into::into).collect(),
            cwd: Some(self.repo.to_path_buf()),
            env_rule: Default::default(),
            detach: false,
            output: Some(output.clone()),
        };
        let mut child = self.host.launch(&spec).map_err(|e| anyhow!("{e}"))?.child;
        let until = Instant::now() + network::OTHER_BOUND;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if Instant::now() >= until {
                horch_platform::group::kill_child_group(&mut child);
                let _ = child.wait();
                let _ = std::fs::remove_file(&output);
                return Err(anyhow!(
                    "gh {} ran past its bound and horch killed it",
                    args[..2.min(args.len())].join(" ")
                ));
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let text = std::fs::read_to_string(&output).unwrap_or_default();
        let _ = std::fs::remove_file(&output);
        if !status.success() {
            return Err(anyhow!(
                "gh {} failed ({status}): {}",
                args[..2.min(args.len())].join(" "),
                text.trim()
            ));
        }
        Ok(text)
    }
}

/// The project file at a commit, read with `git cat-file` under the fleetd
/// git rule, which starts no program of the project.
struct BlobReader<'a> {
    git: &'a Path,
    repo: &'a Path,
}

impl ProjectFileReader for BlobReader<'_> {
    fn read(&self, tip: &str) -> std::io::Result<Option<String>> {
        let spec = format!("{tip}:{FILE_NAME}");
        let out = run_git(self.git, self.repo, &[], &["cat-file", "blob", &spec])
            .map_err(std::io::Error::other)?;
        // The commit has no project file: git exits 128.
        Ok((out.status == 0).then(|| String::from_utf8_lossy(&out.stdout).into_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_push_has_the_push_bound_and_the_safe_keys() {
        assert_eq!(
            network::bound(&["push", "--porcelain"]),
            network::PUSH_BOUND
        );
        for key in [
            "core.hooksPath=/dev/null",
            "core.fsmonitor=false",
            "commit.gpgSign=false",
            "tag.gpgSign=false",
        ] {
            assert!(SAFE_CONFIG.contains(&key), "{key}");
        }
        for pair in [
            ("GIT_TERMINAL_PROMPT", "0"),
            ("GIT_EDITOR", "true"),
            ("GIT_SEQUENCE_EDITOR", "true"),
        ] {
            assert!(SAFE_ENV.contains(&pair), "{pair:?}");
        }
    }
}
