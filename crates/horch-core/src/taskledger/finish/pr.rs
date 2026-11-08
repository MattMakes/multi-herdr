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

/// The commit the task landed: its worktree's, or the run branch's for a
/// shared-checkout task.
fn landed_commit(store: &Store, cx: &Ctx<'_>, run_branch: &str) -> Result<String> {
    match worktree_of(store.conn(), &cx.task.task_id)? {
        Some(w) => w.landed_commit.context("the task has not landed"),
        None => cx.tools.git.rev(&cx.root, run_branch),
    }
}

/// Open the run's PR with the task's section, and record its number.
fn create_pr(
    store: &Store,
    cx: &Ctx<'_>,
    run: &runs::Run,
    run_branch: &str,
    base: &str,
    short: &str,
    section: &str,
) -> Result<i64> {
    let body = with_section(
        &format!(
            "Run `{}`: tasks land on `{run_branch}`; this PR brings them to `{base}`.",
            run.slug
        ),
        short,
        section,
    );
    let url = gh(
        &cx.tools.gh,
        &cx.root,
        &[
            "pr",
            "create",
            "--base",
            base,
            "--head",
            run_branch,
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
        .unwrap_or_default()
        .parse()
        .ok()
        .with_context(|| format!("gh pr create printed no PR URL: {url}"))?;
    runs::set_pr(store.conn(), &run.run_id, n, &url)?;
    Ok(n)
}

/// The number of the run's PR; the first task to get here opens it.
fn pr_number(
    store: &Store,
    cx: &Ctx<'_>,
    run_branch: &str,
    base: &str,
    short: &str,
    section: &str,
) -> Result<i64> {
    let run = runs::get(store.conn(), &cx.run.run_id)?;
    match run.pr_number {
        Some(n) => Ok(n),
        None => create_pr(store, cx, &run, run_branch, base, short, section),
    }
}

/// Write the task's section into the body of the run's PR when it differs,
/// and record it. The PR's number, and the PR as it was read.
fn sync_pr_body(
    store: &Store,
    cx: &Ctx<'_>,
    run_branch: &str,
    base: &str,
    short: &str,
    section: &str,
) -> Result<(String, PrView)> {
    let n = pr_number(store, cx, run_branch, base, short, section)?.to_string();
    let n = n.as_str();
    let view: PrView = serde_json::from_str(&gh(
        &cx.tools.gh,
        &cx.root,
        &["pr", "view", n, "--json", "body,headRefOid"],
    )?)
    .context("reading gh pr view --json body,headRefOid")?;
    let body = with_section(&view.body, short, section);
    if body != view.body {
        let file = cx.evidence_dir.join(format!("{}-pr-body.md", cx.seq));
        std::fs::write(&file, &body)?;
        gh(
            &cx.tools.gh,
            &cx.root,
            &["pr", "edit", n, "--body-file", &file.to_string_lossy()],
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
    Ok((n.to_string(), view))
}

/// How the pr step waits for GitHub's PR head to catch up with a push
/// (TRF-50): `tries` re-reads, `pause` apart, slept with `sleep`.
#[derive(Clone, Copy)]
struct HeadWait {
    tries: u32,
    pause: std::time::Duration,
    sleep: fn(std::time::Duration),
}

impl HeadWait {
    /// GitHub updates a PR's head some seconds after a push: 6 reads, 5 s apart.
    const GITHUB: HeadWait = HeadWait {
        tries: 6,
        pause: std::time::Duration::from_secs(5),
        sleep: std::thread::sleep,
    };
}

/// What the pr step compares: the landed commit and where it must be.
struct Heads<'a> {
    git: &'a crate::taskledger::git::Git,
    gh: &'a Path,
    root: &'a Path,
    remote: &'a str,
    run_branch: &'a str,
    landed: &'a str,
}

impl Heads<'_> {
    /// Whether `head` is the landed commit or holds it.
    fn holds(&self, head: &str) -> bool {
        head == self.landed || self.git.is_ancestor(self.root, self.landed, head)
    }

    /// The tip of the run branch on the remote, or "" when it has none.
    fn remote_tip(&self) -> Result<String> {
        let rf = format!("refs/heads/{}", self.run_branch);
        let out = self
            .git
            .try_network(self.root, &["ls-remote", self.remote, &rf])?;
        if !out.ok {
            bail!("git ls-remote {} {rf} failed: {}", self.remote, out.stderr);
        }
        Ok(out
            .stdout
            .lines()
            .find(|l| l.ends_with(&rf))
            .and_then(|l| l.split_whitespace().next())
            .unwrap_or_default()
            .to_string())
    }

    /// Whether the remote tip `tip` holds the landed commit; fetches the run
    /// branch first when the tip object is not here.
    fn tip_holds(&self, tip: &str) -> bool {
        if tip.is_empty() {
            return false;
        }
        let obj = format!("{tip}^{{commit}}");
        let here = |g: &Self| {
            g.git
                .try_run(g.root, &["cat-file", "-e", &obj])
                .map(|o| o.ok)
                .unwrap_or(false)
        };
        if !here(self) {
            let _ = self
                .git
                .try_network(self.root, &["fetch", self.remote, self.run_branch]);
        }
        self.holds(tip)
    }

    /// The head of PR `n`, read again.
    fn pr_head(&self, n: &str) -> Result<String> {
        #[derive(Deserialize)]
        struct Head {
            #[serde(rename = "headRefOid")]
            head: String,
        }
        let out = gh(
            self.gh,
            self.root,
            &["pr", "view", n, "--json", "headRefOid"],
        )?;
        Ok(serde_json::from_str::<Head>(&out)
            .context("reading gh pr view --json headRefOid")?
            .head)
    }
}

/// Pass when the head of PR `n` holds the landed commit. When the head read
/// lacks it but the remote run branch holds it, GitHub lags the push: read
/// the head again as `wait` says, and pass with a note when it still lags.
/// Fail only when the remote run branch lacks the commit (TRF-50).
fn verdict(h: &Heads<'_>, head: &str, n: &str, short: &str, wait: &HeadWait) -> Result<Outcome> {
    let pass = |extra: &str| {
        Outcome::pass(
            &format!("PR #{n}"),
            None,
            &format!("PR #{n} holds {short}{extra}"),
        )
    };
    if h.holds(head) {
        return Ok(pass(""));
    }
    let tip = h.remote_tip()?;
    if !h.tip_holds(&tip) {
        let tip = if tip.is_empty() {
            "(none)"
        } else {
            tip.as_str()
        };
        return Ok(Outcome::fail(
            "PR head behind",
            None,
            &format!(
                "the remote run branch {}/{} is at {tip}, which does not hold {}; PR #{n} head is {head}; run horch ledger finish to retry the push",
                h.remote, h.run_branch, h.landed
            ),
        ));
    }
    let mut last = head.to_string();
    for _ in 0..wait.tries {
        (wait.sleep)(wait.pause);
        last = h.pr_head(n)?;
        if h.holds(&last) {
            return Ok(pass(""));
        }
    }
    Ok(pass(&format!(
        ". NOTE: GitHub still shows PR #{n} head {last}, but the remote run branch {}/{} is at {tip}, which holds {}; GitHub catches up later",
        h.remote, h.run_branch, h.landed
    )))
}

/// The pr step.
pub fn step(store: &mut Store, cx: &Ctx<'_>) -> Result<Outcome> {
    if let Some(wt) = cx.worktree.as_ref() {
        if land::no_commits_since_claim(&cx.tools.git, wt)? {
            return Ok(Outcome::not_applicable("no commits")
                .judged(cx.tools.git.tree(Path::new(&wt.path)).ok()));
        }
    }
    let Some(remote) = &cx.run.remote else {
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
    let landed = landed_commit(store, cx, &run_branch)?;
    // `gh pr create` refuses a head with no commit ahead of the base.
    if cx.tools.git.is_ancestor(&cx.root, &landed, &base) {
        return Ok(Outcome::not_applicable(&format!(
            "the task landed no commit that {base} lacks, so it needs no pull request"
        )));
    }
    let work = own_work(store, cx, &base, &landed)?;
    if work.as_ref().is_some_and(|w| w.commits.is_empty()) {
        return Ok(Outcome::not_applicable(
            "the task landed no commit of its own, so the PR has no section for it",
        ));
    }
    let section = section_text(store, cx, &landed, work.as_ref())?;
    let short = cx.task.short();
    // 1 writer at a time: the landing lock of this repository.
    let _lock = land::land_lock(cx)?;
    let (n, view) = sync_pr_body(store, cx, &run_branch, &base, &short, &section)?;
    let heads = Heads {
        git: &cx.tools.git,
        gh: &cx.tools.gh,
        root: &cx.root,
        remote,
        run_branch: &run_branch,
        landed: &landed,
    };
    verdict(&heads, &view.head, &n, &short, &HeadWait::GITHUB)
}

/// The state of `run`'s PR for `run close` (C5, TRF-20), asked with gh.
/// Not applicable with no remote, no gh, or no commit on the run branch
/// that the main branch lacks. A failing gh is an error, never a pass.
pub fn pr_check(
    gh_bin: &Path,
    git: &crate::taskledger::git::Git,
    run: &runs::Run,
) -> Result<runs::PrCheck> {
    use runs::PrCheck;
    if run.remote.is_none() {
        return Ok(PrCheck::NotApplicable("the run has no remote".into()));
    }
    let root = Path::new(&run.project_root);
    let (Some(branch), Some(base)) = (&run.run_branch, &run.base_branch) else {
        return Ok(PrCheck::NotApplicable(
            "the run has no run branch or main branch".into(),
        ));
    };
    let Some(n) = run.pr_number else {
        if git.is_ancestor(root, branch, base) {
            return Ok(PrCheck::NotApplicable(format!(
                "{branch} has no commit that {base} lacks"
            )));
        }
        return Ok(PrCheck::Missing);
    };
    let n_text = n.to_string();
    let args = ["pr", "view", n_text.as_str(), "--json", "state"];
    let mut cmd = Command::new(gh_bin);
    cmd.args(args).current_dir(root);
    crate::runtime::process::scrub_child_env(&mut cmd);
    let out = match cmd.output() {
        Ok(out) => out,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PrCheck::NotApplicable(format!(
                "gh is not installed ({})",
                gh_bin.display()
            )))
        }
        Err(e) => return Err(anyhow::Error::new(e).context("running gh")),
    };
    if !out.status.success() {
        bail!(
            "gh {} failed: {}. Check gh auth status, then run horch ledger run close again",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    #[derive(Deserialize)]
    struct State {
        state: String,
    }
    let st: State =
        serde_json::from_slice(&out.stdout).context("reading gh pr view --json state")?;
    Ok(if st.state.eq_ignore_ascii_case("MERGED") {
        PrCheck::Merged(n)
    } else {
        PrCheck::Unmerged {
            number: n,
            state: st.state.to_lowercase(),
        }
    })
}

/// What 1 task put on the run branch: the subjects of its own commits,
/// oldest first, each with its short sha, and the files they touch.
struct Work {
    commits: Vec<String>,
    files: Vec<String>,
}

/// The most commit subjects and files a section lists before it says how
/// many more there are.
const LIST_MAX: usize = 20;

/// The landed commits of the run's other tasks that landed before this one:
/// at another commit, or at the same commit and first (by time, then row).
fn earlier_lands(store: &Store, cx: &Ctx<'_>) -> Result<Vec<String>> {
    let Some(mine) = worktree_of(store.conn(), &cx.task.task_id)? else {
        return Ok(Vec::new());
    };
    let mut stmt = store.conn().prepare(
        "SELECT DISTINCT w.landed_commit FROM worktrees w
         JOIN tasks t ON t.task_id = w.task_id
         JOIN worktrees m ON m.worktree_id = ?2
         WHERE t.run_id = ?1 AND w.task_id <> m.task_id AND w.landed_commit IS NOT NULL
           AND (w.landed_commit <> m.landed_commit
                OR (w.landed_at, w.rowid) < (m.landed_at, m.rowid))",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![cx.run.run_id, mine.worktree_id], |r| {
            r.get::<_, String>(0)
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// The commits the task added to the run branch: those of `landed` that
/// neither `base` nor an earlier land of the run holds. A land fast-forwards
/// the run branch, so an earlier land is an ancestor of `landed`. `None` for a
/// shared-checkout task with no recorded start, whose commits cannot be told
/// from the others on the run branch.
fn own_work(store: &Store, cx: &Ctx<'_>, base: &str, landed: &str) -> Result<Option<Work>> {
    let git = &cx.tools.git;
    let mut not = vec![format!("^{base}")];
    if cx.task.uses_worktree() {
        not.extend(
            earlier_lands(store, cx)?
                .into_iter()
                .filter(|c| git.is_ancestor(&cx.root, c, landed))
                .map(|c| format!("^{c}")),
        );
    } else {
        match super::shared_start(&cx.tools.evidence_root, &cx.task.task_id) {
            Some(start) => not.push(format!("^{start}")),
            None => return Ok(None),
        }
    }
    let log = |format: &str, extra: &[&str]| -> Result<String> {
        let mut args = vec!["log", format];
        args.extend_from_slice(extra);
        args.push(landed);
        args.extend(not.iter().map(String::as_str));
        git.run(&cx.root, &args)
    };
    let commits = log("--format=%s (`%h`)", &["--reverse"])?
        .lines()
        .map(str::to_string)
        .collect();
    let mut files: Vec<String> = log("--format=", &["--name-only"])?
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(str::to_string)
        .collect();
    files.sort();
    files.dedup();
    Ok(Some(Work { commits, files }))
}

/// `items` as a markdown list of at most [`LIST_MAX`] lines, `code` ones in
/// backticks.
fn list(items: &[String], code: bool, noun: &str) -> String {
    let mut s = String::new();
    for i in items.iter().take(LIST_MAX) {
        if code {
            s.push_str(&format!("- `{i}`\n"));
        } else {
            s.push_str(&format!("- {i}\n"));
        }
    }
    if items.len() > LIST_MAX {
        s.push_str(&format!("- and {} more {noun}\n", items.len() - LIST_MAX));
    }
    s
}

/// The task's section: what its commits did, the files they touch, who
/// landed it where, and the sweeps that ran. A step that did not run, or
/// did not apply, is left out.
fn section_text(store: &Store, cx: &Ctx<'_>, landed: &str, work: Option<&Work>) -> Result<String> {
    let steps = super::steps_of(store.conn(), &cx.task.task_id, &cx.worker.worker_id)?;
    let ran = |name: &str| {
        steps.iter().find(|s| s.name == name).and_then(|s| {
            let st = StepStatus::parse(&s.status).ok()?;
            if st == StepStatus::NotApplicable {
                return None;
            }
            Some(format!(
                "{name}: {}",
                s.verdict.clone().unwrap_or_else(|| st.as_str().to_string())
            ))
        })
    };
    let mut s = format!("### {} {}\n\n", cx.task.short(), cx.task.title);
    if let Some(work) = work {
        s.push_str(&list(&work.commits, false, "commits"));
        if !work.files.is_empty() {
            let n = work.files.len();
            s.push_str(&format!(
                "\n<details><summary>{n} file{} changed</summary>\n\n{}\n</details>\n",
                if n == 1 { "" } else { "s" },
                list(&work.files, true, "files")
            ));
        }
        s.push('\n');
    }
    s.push_str(&format!(
        "Landed by {} at `{}`.",
        cx.worker.label(),
        &landed[..landed.len().min(12)]
    ));
    let sweeps: Vec<String> = ["code-sweep", "complexity-sweep", "recheck"]
        .into_iter()
        .filter_map(ran)
        .collect();
    if !sweeps.is_empty() {
        s.push_str(&format!(" {}.", sweeps.join(", ")));
    }
    s.push('\n');
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

    const FAKE_GH: &str = r#"#!/bin/sh
[ "$1" = warm ] && exit 0
d="$(dirname "$0")"
n=$(cat "$d/n" 2>/dev/null || echo 0)
n=$((n + 1))
echo "$n" > "$d/n"
h=$(sed -n "${n}p" "$d/heads")
[ -z "$h" ] && h=$(tail -n 1 "$d/heads")
printf '{"headRefOid":"%s"}' "$h"
"#;

    fn no_sleep(_: std::time::Duration) {}

    const NO_WAIT: HeadWait = HeadWait {
        tries: 6,
        pause: std::time::Duration::from_secs(5),
        sleep: no_sleep,
    };

    /// A repo whose remote `origin` has `run` at commit `old`, a local
    /// commit `new` on top, and a fake gh in its own directory.
    struct World {
        _tmp: tempfile::TempDir,
        root: std::path::PathBuf,
        gh: std::path::PathBuf,
        git: crate::taskledger::git::Git,
        old: String,
        new: String,
    }

    impl World {
        fn new(push_new: bool) -> World {
            let tmp = tempfile::tempdir().unwrap();
            let top = tmp.path().canonicalize().unwrap();
            let root = top.join("repo");
            std::fs::create_dir_all(&root).unwrap();
            let git = crate::taskledger::git::Git::new("git");
            let g = |args: &[&str]| git.run(&root, args).unwrap();
            g(&["init", "--quiet", "-b", "run"]);
            g(&["config", "user.name", "t"]);
            g(&["config", "user.email", "t@example.com"]);
            g(&["config", "commit.gpgsign", "false"]);
            g(&["commit", "--quiet", "--allow-empty", "-m", "old"]);
            let old = g(&["rev-parse", "HEAD"]);
            let bare = top.join("remote.git");
            g(&["init", "--quiet", "--bare", &bare.to_string_lossy()]);
            g(&["remote", "add", "origin", &bare.to_string_lossy()]);
            g(&["push", "--quiet", "origin", "run:run"]);
            g(&["commit", "--quiet", "--allow-empty", "-m", "new"]);
            let new = g(&["rev-parse", "HEAD"]);
            if push_new {
                g(&["push", "--quiet", "origin", "run:run"]);
            }
            let ghdir = top.join("gh");
            std::fs::create_dir_all(&ghdir).unwrap();
            let gh = ghdir.join("gh");
            let warm = crate::test_support::write_warmed(&gh, FAKE_GH, &["warm"]);
            assert!(warm.status.success());
            World {
                _tmp: tmp,
                root,
                gh,
                git,
                old,
                new,
            }
        }

        /// The heads the fake gh prints, 1 per re-read; the last repeats.
        fn gh_heads(&self, heads: &[&str]) {
            let dir = self.gh.parent().unwrap();
            std::fs::write(dir.join("heads"), heads.join("\n") + "\n").unwrap();
        }

        fn gh_reads(&self) -> u32 {
            std::fs::read_to_string(self.gh.parent().unwrap().join("n"))
                .map(|s| s.trim().parse().unwrap())
                .unwrap_or(0)
        }

        fn verdict(&self) -> Outcome {
            let h = Heads {
                git: &self.git,
                gh: &self.gh,
                root: &self.root,
                remote: "origin",
                run_branch: "run",
                landed: &self.new,
            };
            verdict(&h, &self.old, "7", "T-1", &NO_WAIT).unwrap()
        }
    }

    #[test]
    fn trf_50_lagging_pr_head_passes_when_the_remote_branch_holds_the_commit() {
        let w = World::new(true);
        w.gh_heads(&[&w.old]);
        let o = w.verdict();
        assert_eq!(o.status, StepStatus::Passed, "{o:?}");
        let note = &o.note;
        assert!(
            note.contains("NOTE: GitHub still shows PR #7 head"),
            "{note}"
        );
        assert!(note.contains(&w.old) && note.contains(&w.new), "{note}");
        assert_eq!(o.verdict.as_deref(), Some("PR #7"));
        assert_eq!(w.gh_reads(), 6, "6 re-reads, then pass");
    }

    #[test]
    fn trf_50_pr_head_catches_up_on_a_later_read() {
        let w = World::new(true);
        w.gh_heads(&[&w.old, &w.new]);
        let o = w.verdict();
        assert_eq!(o.status, StepStatus::Passed, "{o:?}");
        assert_eq!(o.note, "PR #7 holds T-1");
        assert_eq!(w.gh_reads(), 2, "pass at the read that holds it");
    }

    #[test]
    fn trf_50_unpushed_commit_still_fails_and_names_the_remote_tip() {
        let w = World::new(false);
        w.gh_heads(&[&w.new]);
        let o = w.verdict();
        assert_eq!(o.status, StepStatus::Failed, "{o:?}");
        assert_eq!(o.verdict.as_deref(), Some("PR head behind"));
        let text = &o.note;
        assert!(
            text.contains(&format!(
                "the remote run branch origin/run is at {}, which does not hold {}",
                w.old, w.new
            )),
            "{text}"
        );
        assert_eq!(w.gh_reads(), 0, "no wait when the push is missing");
    }
}
