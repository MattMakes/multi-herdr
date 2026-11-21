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

const SAFE_CONFIG: [&str; 5] = [
    "core.hooksPath=/dev/null",
    "core.fsmonitor=false",
    "commit.gpgSign=false",
    "tag.gpgSign=false",
    "protocol.ext.allow=never",
];

const SAFE_ENV: [(&str, &str); 5] = [
    ("GIT_TERMINAL_PROMPT", "0"),
    ("GIT_EDITOR", "true"),
    ("GIT_SEQUENCE_EDITOR", "true"),
    ("LC_ALL", "C"),
    ("GIT_CONFIG_NOSYSTEM", "1"),
];

const HTTP_STALL: [&str; 2] = ["http.lowSpeedLimit=1000", "http.lowSpeedTime=60"];

pub fn run(conn: &Connection, row: &JobRow) -> Result<Ran> {
    run_with(conn, row, &Tools::from_env(&ProcessEnv), &Launcher)
}

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

const PR_MAX: usize = 2048;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PushFacts {
    pushed: bool,
    failure: Option<String>,
    pr: Option<String>,
    manual: Option<bool>,
}

impl PushFacts {
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

fn user_programs(git: &Path, repo: &Path) -> Result<Vec<String>> {
    let global = |args: &[&str]| -> Result<Vec<String>> {
        let out = run_git(git, repo, &[], args)?;
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

fn run_git(git: &Path, dir: &Path, config: &[String], args: &[&str]) -> Result<GitOutput> {
    let mut cmd = horch_platform::start::command(git);
    cmd.current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
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

fn failure_text(out: &GitOutput) -> String {
    let text = format!(
        "{} {}",
        String::from_utf8_lossy(&out.stderr).trim(),
        String::from_utf8_lossy(&out.stdout).trim()
    );
    text.trim().to_string()
}

pub const PR_MANUAL_KIND: &str = "pr_manual";

pub fn pr_manual_key(feature: &str) -> String {
    format!("{PR_MANUAL_KIND}:{feature}")
}

#[derive(Debug, Default)]
struct PrFacts {
    pr: Option<String>,
    manual: Option<bool>,
}

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

#[derive(serde::Deserialize)]
struct PrView {
    url: String,
    state: String,
    #[serde(rename = "isDraft")]
    draft: bool,
    body: String,
    #[serde(rename = "baseRefName", default)]
    base: Option<String>,
}

fn move_base(gh: &Gh<'_>, v: &PrView, base: &str) -> Result<()> {
    if v.base.as_deref().is_some_and(|now| now != base) {
        gh.call(&["pr", "edit", &v.url, "--base", base])?;
    }
    Ok(())
}

fn gh_ready(
    conn: &Connection,
    gh: &Gh<'_>,
    f: &FeatureRow,
    branch: &str,
    base: &str,
) -> Result<PrFacts> {
    let found = match f.pr.as_deref() {
        Some(pr) => Some(gh.view(pr)?),
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

fn manual_text(feature: &str, link: &str, ready: bool) -> String {
    if ready {
        format!("Feature {feature} is ready. Open its pull request, then merge it: {link}")
    } else {
        format!(
            "Feature {feature} is open again: do not merge {link} yet. fleetd tells you when it is ready."
        )
    }
}

fn web_base(url: &str) -> Option<String> {
    let url = url.trim().trim_end_matches('/');
    let url = url.strip_suffix(".git").unwrap_or(url);
    let schemed = ["https://", "http://", "ssh://", "git://"]
        .iter()
        .find_map(|s| url.strip_prefix(s));
    let (host, path) = match schemed {
        Some(rest) => rest.split_once('/')?,
        None => {
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

struct Gh<'a> {
    bin: &'a Path,
    host: &'a dyn ProcessHost,
    repo: &'a Path,
    temp: &'a Path,
    tag: String,
}

impl Gh<'_> {
    fn view(&self, pr: &str) -> Result<PrView> {
        let fields = "url,state,isDraft,body,baseRefName";
        let text = self.call(&["pr", "view", pr, "--json", fields])?;
        text.lines()
            .find_map(|l| serde_json::from_str::<PrView>(l).ok())
            .ok_or_else(|| anyhow!("gh gave no pull request: {}", text.trim()))
    }

    fn body_file(&self, body: &str) -> Result<String> {
        let file = self
            .temp
            .join(format!("horch-push-gh-{}.body.md", self.tag));
        std::fs::write(&file, body)?;
        Ok(file.to_string_lossy().into_owned())
    }

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

struct BlobReader<'a> {
    git: &'a Path,
    repo: &'a Path,
}

impl ProjectFileReader for BlobReader<'_> {
    fn read(&self, tip: &str) -> std::io::Result<Option<String>> {
        let spec = format!("{tip}:{FILE_NAME}");
        let out = run_git(self.git, self.repo, &[], &["cat-file", "blob", &spec])
            .map_err(std::io::Error::other)?;
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
