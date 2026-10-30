//! The pr step (#37): the run's 1 pull request, from the run branch into
//! the main branch, holds a section for every landed task.
//!
//! Each task owns the text between `<!-- horch:task T-<n> -->` and
//! `<!-- /horch:task T-<n> -->`; text outside every marker is never changed.
//! Updates run under the landing lock, so 2 workers never overwrite each
//! other's section.

use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use super::{land, worktree_of, Ctx, Outcome};
use crate::taskledger::model::{Actor, EventKind, StepStatus};
use crate::taskledger::runs;
use crate::taskledger::store::Store;
use crate::taskledger::util::{self, About};

fn gh(bin: &Path, dir: &Path, args: &[&str]) -> Result<String> {
    let mut cmd = Command::new(bin);
    cmd.args(args).current_dir(dir);
    crate::runtime::process::scrub_child_env(&mut cmd);
    let out = cmd
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                anyhow::anyhow!(
                    "gh is not installed ({}): install GitHub CLI and run gh auth login, then run horch ledger finish again",
                    bin.display()
                )
            } else {
                anyhow::Error::new(e).context("running gh")
            }
        })?;
    if !out.status.success() {
        bail!(
            "gh {} failed: {}. Check gh auth status, then run horch ledger finish again",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The markers of 1 task's section.
fn markers(short: &str) -> (String, String) {
    (
        format!("<!-- horch:task {short} -->"),
        format!("<!-- /horch:task {short} -->"),
    )
}

/// `body` with the section of `short` set to `section`: replaced in place
/// when it exists, else appended. Nothing outside the markers changes.
pub fn with_section(body: &str, short: &str, section: &str) -> String {
    let (open, close) = markers(short);
    let block = format!("{open}\n{}\n{close}", section.trim_end());
    if let (Some(a), Some(b)) = (body.find(&open), body.find(&close)) {
        if a < b {
            let end = b + close.len();
            return format!("{}{}{}", &body[..a], block, &body[end..]);
        }
    }
    if body.trim().is_empty() {
        block
    } else {
        format!("{}\n\n{}", body.trim_end(), block)
    }
}

#[derive(Deserialize)]
struct PrView {
    body: String,
    #[serde(rename = "headRefOid")]
    head: String,
}

/// The pr step.
pub fn step(store: &mut Store, cx: &Ctx<'_>) -> Result<Outcome> {
    let Some(remote) = cx.run.remote.clone() else {
        return Ok(Outcome::not_applicable(
            "the run has no remote, so it has no pull request",
        ));
    };
    let run_branch = cx
        .run
        .run_branch
        .clone()
        .context("the run has no run branch")?;
    let base = cx
        .run
        .base_branch
        .clone()
        .context("the run has no main branch")?;
    let landed = match worktree_of(store.conn(), &cx.task.task_id)? {
        Some(w) => w.landed_commit.context("the task has not landed")?,
        None => cx.tools.git.rev(&cx.root, &run_branch)?,
    };
    let _ = remote;
    let section = section_text(store, cx, &landed)?;
    let short = cx.task.short();
    // 1 writer at a time: the landing lock of this repository.
    let _lock = land::land_lock(cx)?;
    let run = runs::get(store.conn(), &cx.run.run_id)?;
    let number = match run.pr_number {
        Some(n) => n,
        None => {
            let body = with_section(
                &format!(
                    "Run `{}`: tasks land on `{run_branch}`; this PR brings them to `{base}`.",
                    run.slug
                ),
                &short,
                &section,
            );
            let url = gh(
                &cx.tools.gh,
                &cx.root,
                &[
                    "pr",
                    "create",
                    "--base",
                    &base,
                    "--head",
                    &run_branch,
                    "--title",
                    &format!("{}: fleet run", run.slug),
                    "--body",
                    &body,
                ],
            )?;
            let url = url.lines().last().unwrap_or_default().trim().to_string();
            let n: i64 = url
                .rsplit('/')
                .next()
                .and_then(|n| n.parse().ok())
                .with_context(|| format!("gh pr create printed no PR URL: {url}"))?;
            runs::set_pr(store.conn(), &run.run_id, n, &url)?;
            n
        }
    };
    let n = number.to_string();
    let view: PrView = serde_json::from_str(&gh(
        &cx.tools.gh,
        &cx.root,
        &["pr", "view", &n, "--json", "body,headRefOid"],
    )?)
    .context("reading gh pr view --json body,headRefOid")?;
    let body = with_section(&view.body, &short, &section);
    if body != view.body {
        let file = cx.evidence_dir.join(format!("{}-pr-body.md", cx.seq));
        std::fs::write(&file, &body)?;
        gh(
            &cx.tools.gh,
            &cx.root,
            &["pr", "edit", &n, "--body-file", &file.to_string_lossy()],
        )?;
    }
    util::event(
        store.conn(),
        &About {
            run: Some(&cx.run.run_id),
            task: Some(&cx.task.task_id),
            worker: Some(&cx.worker.worker_id),
        },
        Actor::Worker,
        EventKind::PrUpdated,
        &format!("PR #{n} has the section of {short}."),
    )?;
    let has_it = view.head == landed || cx.tools.git.is_ancestor(&cx.root, &landed, &view.head);
    if !has_it {
        return Ok(Outcome::fail(
            "PR head behind",
            None,
            &format!(
                "PR #{n} head {} does not contain {landed}: the run branch is not pushed; run horch ledger finish to retry the push",
                view.head
            ),
        ));
    }
    Ok(Outcome::pass(
        &format!("PR #{n}"),
        None,
        &format!("PR #{n} holds {short}"),
    ))
}

fn section_text(store: &Store, cx: &Ctx<'_>, landed: &str) -> Result<String> {
    let steps = super::steps_of(store.conn(), &cx.task.task_id, &cx.worker.worker_id)?;
    let verdict = |name: &str| {
        steps
            .iter()
            .find(|s| s.name == name)
            .map(|s| {
                let st = StepStatus::parse(&s.status)
                    .map(|s| s.as_str())
                    .unwrap_or("?");
                s.verdict.clone().unwrap_or_else(|| st.to_string())
            })
            .unwrap_or_else(|| "-".into())
    };
    let mut s = format!("### {} {}\n\n", cx.task.short(), cx.task.title);
    let summary = cx
        .worker
        .summary
        .clone()
        .or(cx.task.summary.clone())
        .unwrap_or_else(|| format!("Landed by {}.", cx.worker.label()));
    s.push_str(&summary);
    s.push_str("\n\n");
    s.push_str(&format!(
        "- Commit: `{}`\n- code-sweep: {}\n- complexity-sweep: {}\n- check: {}\n",
        &landed[..landed.len().min(12)],
        verdict("code-sweep"),
        verdict("complexity-sweep"),
        verdict("recheck")
    ));
    for w in super::codesweep::waivers(&cx.tools.evidence_root, &cx.task.task_id) {
        s.push_str(&format!(
            "- code-sweep {} waived by the orchestrator: {}\n",
            w.id, w.reason
        ));
    }
    if let Some(url) = &cx.task.issue_url {
        if let Some(n) = url
            .rsplit('/')
            .next()
            .filter(|n| n.chars().all(|c| c.is_ascii_digit()))
        {
            s.push_str(&format!("\nCloses #{n}\n"));
        }
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_section_is_replaced_in_place_and_others_are_kept() {
        let body =
            "Intro.\n\n<!-- horch:task T-1 -->\nold\n<!-- /horch:task T-1 -->\n\nOperator note.";
        let b = with_section(body, "T-1", "new");
        assert_eq!(
            b,
            "Intro.\n\n<!-- horch:task T-1 -->\nnew\n<!-- /horch:task T-1 -->\n\nOperator note."
        );
        let b2 = with_section(&b, "T-2", "two");
        assert!(b2.contains("<!-- horch:task T-1 -->\nnew"));
        assert!(b2.ends_with("<!-- horch:task T-2 -->\ntwo\n<!-- /horch:task T-2 -->"));
        assert!(b2.contains("Operator note."));
        assert_eq!(with_section(&b2, "T-2", "two"), b2, "idempotent");
    }
}
