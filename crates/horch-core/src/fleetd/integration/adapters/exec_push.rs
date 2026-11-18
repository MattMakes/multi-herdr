//! The executor of the `push` job (ticket 38).
//!
//! The subject of a run is the feature, and its input key names the tip,
//! the lease and the remote (`application::remote::push_key`). The run
//! pushes outside each transaction, then records the result in 1 short
//! transaction at the fleet clock of the ledger:
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

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use horch_marketplace::git::{GitOutput, REPO_ENV};
use rusqlite::{Connection, Transaction, TransactionBehavior};

use crate::fleetd::fleet_runtime::adapters::jobs_store::JobRow;
use crate::fleetd::fleet_runtime::domain::config::FleetConfig;
use crate::fleetd::fleet_runtime::domain::jobs::Outcome;
use crate::fleetd::ideas::application::needs_you::NeedsYouList;
use crate::fleetd::integration::adapters::sqlite as rows;
use crate::fleetd::integration::application::remote::{self, PushInput};
use crate::fleetd::worktree_custody::adapters::git::project_program;
use crate::fleetd::worktree_custody::application::create::{fleet_clock, fleet_config};
use crate::runtime::bins::{git_bin, BinOverrides};
use crate::runtime::context::ProcessEnv;
use crate::runtime::process;
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

/// Runs the job `row` with the git of this process, the `fleet.toml` of
/// the fleet of the ledger, and the fleet clock of the ledger.
pub fn run(conn: &Connection, row: &JobRow) -> Result<Outcome> {
    let config = match conn.path().filter(|p| !p.is_empty()) {
        Some(ledger) => fleet_config(Path::new(ledger))?,
        None => FleetConfig::default(),
    };
    run_with(
        conn,
        row,
        &git_bin(&BinOverrides::from_env(&ProcessEnv)),
        &config,
    )
}

/// [`run`] with the git binary `git` and the fleet config `config`.
pub fn run_with(
    conn: &Connection,
    row: &JobRow,
    git: &Path,
    config: &FleetConfig,
) -> Result<Outcome> {
    let feature = row.subject.as_str();
    let pushed = push(conn, row, git);
    in_transaction(conn, |tx| match &pushed {
        Ok(input) => remote::record_push_passed(tx, None, &NeedsYouList, feature, &input.tip),
        Err(why) => {
            let clock = fleet_clock(tx)?;
            let why = format!("{why:#}");
            remote::record_push_failed(tx, None, &NeedsYouList, config, feature, clock, &why)
        }
    })?;
    Ok(match pushed {
        Ok(input) => Outcome {
            passed: true,
            verdict: true,
            text: format!("pushed {} to {}", input.tip, input.remote),
        },
        Err(why) => Outcome {
            passed: false,
            verdict: true,
            text: format!("the push failed: {why:#}"),
        },
    })
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

/// Run `f` in 1 immediate transaction of `conn`.
fn in_transaction(conn: &Connection, f: impl FnOnce(&Transaction<'_>) -> Result<()>) -> Result<()> {
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    f(&tx)?;
    tx.commit()?;
    Ok(())
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
