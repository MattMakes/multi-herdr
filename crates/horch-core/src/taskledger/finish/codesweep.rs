//! The code-sweep step (#39): the `code-sweep` skill's review, run through
//! the ledger. A worker never starts subagents, so each reviewer the skill's
//! triage picks becomes a review task that a fleet worker claims, with the
//! reviewer's prompt and the diff in its ticket file. When every reviewer is
//! done, horch merges the findings with the skill's verdict rules: any
//! Critical finding fails; Important findings fail until each is fixed (a
//! new commit, which is reviewed again) or answered with a reason; Minor
//! findings never block. Only the orchestrator waives a Critical or an
//! Important finding (`horch ledger waive`).
//!
//! The round state belongs to the task, not the worker: a replacement with
//! the same tree reuses the round. A new round cancels the open reviews of
//! the old one as superseded; a review cancelled in the current round is
//! queued again (#158, #163).

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use super::{Ctx, Outcome};
use crate::taskledger::model::{Actor, EventKind, TaskKind, WorktreeMode};
use crate::taskledger::store::Store;
use crate::taskledger::tasks::{self, NewTask, Task};
use crate::taskledger::util::{self, About};

/// The reviewers of the skill: code, prompt file, focus.
pub const REVIEWERS: &[(&str, &str, &str)] = &[
    ("b", "bug-hunter", "correctness"),
    ("v", "vulnerability-scanner", "security"),
    ("s", "structure-inspector", "architecture"),
    ("e", "efficiency-auditor", "performance"),
    ("c", "clarity-checker", "maintainability"),
    ("t", "coverage-analyst", "testing"),
    ("g", "contract-guardian", "contract safety"),
    ("w", "waste-detector", "simplification"),
    ("r", "reliability-auditor", "reliability"),
    ("d", "dependency-examiner", "dependencies"),
    ("j", "power-of-ten-reviewer", "JPL Power of Ten"),
    ("u", "interface-auditor", "UI"),
];

/// The skill's auto-triage: `b` and `c` always, the others by diff signals.
pub fn triage(diff: &str, files: &[String]) -> Vec<&'static str> {
    let d = diff.to_lowercase();
    let added: String = diff
        .lines()
        .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase();
    let any = |words: &[&str]| words.iter().any(|w| added.contains(w));
    let ext = |exts: &[&str]| {
        files.iter().any(|f| {
            Path::new(f)
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| exts.contains(&e))
        })
    };
    let mut out = vec!["b", "c"];
    if any(&[
        "auth",
        "password",
        "token",
        "secret",
        "crypt",
        "sanitiz",
        "escape",
        "sql",
        "exec(",
        "unsafe",
        "credential",
    ]) {
        out.push("v");
    }
    if d.contains("new file mode")
        || d.contains("rename from")
        || any(&["pub trait", "interface ", "pub struct", "impl "])
    {
        out.push("s");
    }
    if any(&[
        "for ", "while ", "loop", "async", "await", "cache", "query", ".iter()", "select ",
    ]) {
        out.push("e");
    }
    let tests_changed = files.iter().any(|f| f.contains("test"));
    let code_changed = files
        .iter()
        .any(|f| !f.contains("test") && !f.ends_with(".md"));
    if tests_changed || code_changed {
        out.push("t");
    }
    if any(&[
        "pub fn",
        "pub mod",
        "export ",
        "create table",
        "alter table",
        "schema",
        "#[derive(serialize",
    ]) {
        out.push("g");
    }
    let deleted = diff
        .lines()
        .filter(|l| l.starts_with('-') && !l.starts_with("---"))
        .count();
    if diff.lines().count() > 400 || deleted > 100 {
        out.push("w");
    }
    if any(&[
        "lock", "mutex", "thread", "spawn", "file", "socket", "drop(", "close", "kill", "signal",
    ]) {
        out.push("r");
    }
    if files.iter().any(|f| {
        f.ends_with("Cargo.toml")
            || f.ends_with("package.json")
            || f.ends_with("requirements.txt")
            || f.ends_with("go.mod")
            || f.ends_with("Cargo.lock")
    }) {
        out.push("d");
    }
    if any(&[
        " as ",
        "unwrap()",
        "#[allow",
        "eslint-disable",
        "type: ignore",
        "any)",
    ]) || diff.lines().count() > 200
    {
        out.push("j");
    }
    if ext(&["html", "css", "scss", "tsx", "jsx", "vue", "svelte"]) {
        out.push("u");
    }
    out
}

/// The state of the step between `finish` runs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct State {
    round: u32,
    /// The tree the round reviews.
    tree: String,
    dir: String,
    /// (code, review task id, findings file)
    reviews: Vec<(String, String, String)>,
    /// How many times each reviewer code of this round was queued again
    /// after a cancel.
    #[serde(default)]
    requeues: std::collections::BTreeMap<String, u32>,
}

/// How many times 1 cancelled review of 1 round is queued again. After
/// that it stays cancelled, and the step fails with the ways out.
const MAX_REQUEUES: u32 = 2;

fn state_path(evidence_root: &Path, task_id: &str) -> PathBuf {
    super::task_dir(evidence_root, task_id).join("code-sweep-state.json")
}

fn load_state(evidence_root: &Path, task_id: &str) -> Option<State> {
    std::fs::read(state_path(evidence_root, task_id))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
}

/// The task's state; a state file at the per-worker path of an older horch
/// is moved to the task path first, so its round goes on.
fn load(cx: &Ctx<'_>) -> Option<State> {
    if let Some(state) = load_state(&cx.tools.evidence_root, &cx.task.task_id) {
        return Some(state);
    }
    let legacy = cx.evidence_dir.join("code-sweep-state.json");
    let state: State = serde_json::from_slice(&std::fs::read(&legacy).ok()?).ok()?;
    save(cx, &state).ok()?;
    let _ = std::fs::remove_file(legacy);
    Some(state)
}

fn save(cx: &Ctx<'_>, state: &State) -> Result<()> {
    let path = state_path(&cx.tools.evidence_root, &cx.task.task_id);
    std::fs::create_dir_all(path.parent().expect("a task folder"))?;
    std::fs::write(path, serde_json::to_vec_pretty(state)?)?;
    Ok(())
}

/// Make the next code-sweep of the task queue a new round, also for the
/// same tree (`finish --rereview`). Only when a review of the current round
/// was cancelled or ended without its findings file: a round whose every
/// reviewer reported stands, so its findings cannot be rolled again.
pub fn forget_tree(conn: &rusqlite::Connection, evidence_root: &Path, task_id: &str) -> Result<()> {
    let Some(mut state) = load_state(evidence_root, task_id) else {
        bail!("the task has no code-sweep round yet: run horch ledger finish");
    };
    let mut lost = false;
    for (_, id, file) in &state.reviews {
        let t = tasks::get(conn, id)?;
        let empty = std::fs::read_to_string(file)
            .map(|s| s.trim().is_empty())
            .unwrap_or(true);
        lost |= t.status == "cancelled" || (t.status == "done" && empty);
    }
    if !lost {
        bail!(
            "every review of round {} is in or still runs, so the round stands. Fix its findings and commit, or ask the orchestrator to waive one",
            state.round
        );
    }
    state.tree.clear();
    let path = state_path(evidence_root, task_id);
    std::fs::write(path, serde_json::to_vec_pretty(&state)?)?;
    Ok(())
}

/// The reviewer persona: the other vendor reviews (a Codex reviewer for
/// Claude-built work, a Claude reviewer for Codex-built work).
pub fn reviewer_persona(author_harness: Option<&str>) -> &'static str {
    match author_harness {
        Some("codex") => "architect-reviewer",
        _ => "codex-reviewer",
    }
}

/// The other reviewer persona: the fallback of every review task.
pub fn other_reviewer(persona: &str) -> &'static str {
    if persona == "architect-reviewer" {
        "codex-reviewer"
    } else {
        "architect-reviewer"
    }
}

/// The (primary, fallback) persona of this round's reviews, and why the
/// usual reviewer was swapped, when it cannot run here.
fn reviewer_personas(cx: &Ctx<'_>) -> (&'static str, &'static str, Option<String>) {
    let usual = reviewer_persona(cx.worker.harness.as_deref());
    let other = other_reviewer(usual);
    let down = |p: &str| {
        cx.tools
            .reviewers_down
            .iter()
            .find(|(d, _)| d == p)
            .map(|(_, why)| why.clone())
    };
    match (down(usual), down(other)) {
        (Some(why), None) => (
            other,
            usual,
            Some(format!("{usual} cannot run here ({why})")),
        ),
        _ => (usual, other, None),
    }
}

/// 1 waiver: the orchestrator's ruling on 1 finding of 1 round.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Waiver {
    pub round: u32,
    pub id: String,
    pub reason: String,
}

fn waivers_path(evidence_root: &Path, task_id: &str) -> PathBuf {
    super::task_dir(evidence_root, task_id).join("code-sweep-waivers.json")
}

fn all_waivers(evidence_root: &Path, task_id: &str) -> Vec<Waiver> {
    std::fs::read(waivers_path(evidence_root, task_id))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// The waivers of the task's current round.
pub fn waivers(evidence_root: &Path, task_id: &str) -> Vec<Waiver> {
    let Some(state) = load_state(evidence_root, task_id) else {
        return Vec::new();
    };
    all_waivers(evidence_root, task_id)
        .into_iter()
        .filter(|w| w.round == state.round)
        .collect()
}

/// The run's latest worker in herdr pane `pane`, if any: who calls
/// `horch ledger waive` when HORCH_WORKER_ID is unset.
pub fn worker_in_pane(
    conn: &rusqlite::Connection,
    run_id: &str,
    pane: Option<&str>,
) -> Option<String> {
    conn.query_row(
        "SELECT worker_id FROM workers WHERE run_id = ?1 AND pane_id = ?2
         ORDER BY spawned_at DESC LIMIT 1",
        rusqlite::params![run_id, pane?],
        |row| row.get(0),
    )
    .ok()
}

/// `horch ledger waive`: the orchestrator rules that finding `id` of the
/// task's current round does not block. It must name a finding of the
/// round's report.
pub fn waive(
    store: &Store,
    evidence_root: &Path,
    task: &Task,
    id: &str,
    reason: &str,
) -> Result<Waiver> {
    let id = id.trim().to_uppercase();
    if reason.trim().is_empty() {
        bail!(
            "give a reason: horch ledger waive {} {id} \"<reason>\"",
            task.short()
        );
    }
    let state = load_state(evidence_root, &task.task_id)
        .with_context(|| format!("{} has no code-sweep round", task.short()))?;
    let report_path = Path::new(&state.dir).join("report.md");
    let report = std::fs::read_to_string(&report_path).unwrap_or_default();
    if !report.contains(&format!("- {id} [")) {
        bail!(
            "round {} of {} has no finding {id}; read {}",
            state.round,
            task.short(),
            report_path.display()
        );
    }
    let mut all = all_waivers(evidence_root, &task.task_id);
    let w = Waiver {
        round: state.round,
        id: id.clone(),
        reason: reason.trim().to_string(),
    };
    all.retain(|x| !(x.round == w.round && x.id == w.id));
    all.push(w.clone());
    std::fs::write(
        waivers_path(evidence_root, &task.task_id),
        serde_json::to_vec_pretty(&all)?,
    )?;
    util::event(
        store.conn(),
        &About {
            run: Some(&task.run_id),
            task: Some(&task.task_id),
            worker: None,
        },
        Actor::Orchestrator,
        EventKind::Verdict,
        &format!(
            "{} code-sweep round {}: {id} waived: {}",
            task.short(),
            w.round,
            w.reason
        ),
    )?;
    Ok(w)
}

/// The review folder inside the project: reviewers write there (a Codex
/// sandbox may write its working tree, not the ledger's folder). It ignores
/// itself in git.
fn review_root(root: &Path) -> Result<PathBuf> {
    let dir = root.join(".horch").join("reviews");
    std::fs::create_dir_all(&dir)?;
    let ignore = root.join(".horch").join(".gitignore");
    if !ignore.exists() {
        std::fs::write(&ignore, "*\n")?;
    }
    Ok(dir)
}

/// 1 finding of the merged report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub id: String,
    pub severity: String,
    pub reviewer: String,
    pub text: String,
}

/// The findings of 1 reviewer's file: the `- ` items under `## Critical`,
/// `## Important` and `## Minor`. `- none` is no finding.
pub fn parse_findings(reviewer: &str, text: &str) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let mut sev: Option<&str> = None;
    for line in text.lines() {
        let l = line.trim();
        if let Some(h) = l.strip_prefix("## ") {
            let h = h.trim().to_lowercase();
            sev = if h.starts_with("critical") {
                Some("Critical")
            } else if h.starts_with("important") {
                Some("Important")
            } else if h.starts_with("minor") {
                Some("Minor")
            } else {
                None
            };
            continue;
        }
        if let (Some(s), Some(item)) = (sev, l.strip_prefix("- ")) {
            let item = item.trim();
            if item.is_empty() || item.eq_ignore_ascii_case("none") {
                continue;
            }
            out.push((s.to_string(), reviewer.to_string(), item.to_string()));
        }
    }
    out
}

/// `F<n>: <reason>` lines of the answers file.
pub fn parse_answers(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let (id, why) = l.trim().split_once(':')?;
            let id = id.trim();
            (id.starts_with('F')
                && id[1..].chars().all(|c| c.is_ascii_digit())
                && !why.trim().is_empty())
            .then(|| (id.to_string(), why.trim().to_string()))
        })
        .collect()
}

/// 1 reviewer task of a round, before it is queued.
struct Planned {
    code: String,
    task: NewTask,
    findings: String,
}

/// The review task of reviewer `code`, for `persona` with `fallback`.
fn review_task(
    cx: &Ctx<'_>,
    name: &str,
    ticket: &Path,
    findings: &Path,
    persona: &str,
    fallback: &str,
) -> NewTask {
    let mut t = NewTask::work(
        &format!("Review {}: {name}", cx.task.short()),
        &ticket.to_string_lossy(),
        persona,
    );
    t.fallback = Some(fallback.to_string());
    t.kind = TaskKind::Review;
    t.parent_task_id = Some(cx.task.task_id.clone());
    t.worktree_mode = WorktreeMode::Shared;
    // Reviews go before ordinary work: the author waits for them.
    t.priority = -500;
    t.spec_ref = Some(findings.to_string_lossy().into_owned());
    t
}

/// Write the ticket of every reviewer the triage picks. Nothing is queued
/// yet: a prompt that does not read fails the round before any task exists.
#[allow(clippy::too_many_arguments)]
fn plan_round(
    cx: &Ctx<'_>,
    sweep: &Path,
    dir: &Path,
    diff: &str,
    diff_path: &Path,
    files: &[String],
    persona: &str,
    fallback: &str,
) -> Result<Vec<Planned>> {
    let mut out = Vec::new();
    for code in triage(diff, files) {
        let (_, name, focus) = REVIEWERS
            .iter()
            .find(|(c, _, _)| *c == code)
            .expect("triage returns known codes");
        let prompt_file = sweep.join("reviewers").join(format!("{name}.md"));
        let prompt = std::fs::read_to_string(&prompt_file).with_context(|| {
            format!(
                "reading the {name} reviewer prompt {}",
                prompt_file.display()
            )
        })?;
        let findings = dir.join(format!("{name}-findings.md"));
        let ticket = dir.join(format!("{name}.md"));
        std::fs::write(
            &ticket,
            format!(
                "# Review {short}: {name} ({focus})\n\n\
                 You are the {name} reviewer of a code-sweep review. Review only for your concern.\n\n\
                 - The diff: {diff}\n\
                 - The changed files: {files}\n\
                 - The author's worktree, for the surrounding code (read it, never edit it): {wt}\n\n\
                 Read the surrounding code of every changed file. Do not edit any file except your findings file.\n\n\
                 Write your findings to {out} in exactly this form, 1 finding per `- ` line, \
                 each with file:line, the issue and the fix. Write `- none` under a heading with no findings.\n\n\
                 ```\n## Critical\n- <file>:<line> <issue>. Fix: <fix>\n## Important\n- none\n## Minor\n- none\n```\n\n\
                 Then run: horch ledger done \"<1 line: how many findings of each severity>\"\n\n\
                 ## Your reviewer prompt\n\n{prompt}\n",
                short = cx.task.short(),
                diff = diff_path.display(),
                files = files.join(", "),
                wt = cx.dir.display(),
                out = findings.display(),
            ),
        )?;
        out.push(Planned {
            code: code.to_string(),
            task: review_task(cx, name, &ticket, &findings, persona, fallback),
            findings: findings.to_string_lossy().into_owned(),
        });
    }
    Ok(out)
}

/// Cancel every open review of `old` as superseded by `round`. Returns the
/// workers that held one: the caller stops them.
fn supersede(tx: &rusqlite::Connection, old: Option<&State>, round: u32) -> Result<Vec<String>> {
    let mut held = Vec::new();
    for (_, id, _) in old.map(|o| o.reviews.as_slice()).unwrap_or_default() {
        let t = tasks::get(tx, id)?;
        if matches!(t.status.as_str(), "done" | "cancelled") {
            continue;
        }
        let t = tasks::cancel(tx, &t, &format!("superseded by round {round}"))?;
        held.extend(t.current_worker_id);
    }
    Ok(held)
}

fn wait_for_reviews(store: &Store, cx: &Ctx<'_>, state: &State) -> Result<()> {
    super::super::workers::set_wait(
        store.conn(),
        &cx.worker.worker_id,
        "reviewers",
        "waiting for reviewer tasks",
        &state
            .reviews
            .iter()
            .map(|(_, id, _)| id.clone())
            .collect::<Vec<_>>(),
        7 * 24 * 3600,
    )
}

fn queue_round(
    store: &mut Store,
    cx: &Ctx<'_>,
    sweep: &Path,
    base: &str,
    tree: &str,
    old: Option<&State>,
) -> Result<Outcome> {
    let round = old.map(|o| o.round + 1).unwrap_or(1);
    let git = &cx.tools.git;
    let diff = git.run(&cx.dir, &["diff", &format!("{base}...HEAD")])?;
    let names = git.run(&cx.dir, &["diff", "--name-only", &format!("{base}...HEAD")])?;
    let files: Vec<String> = names.lines().map(|s| s.to_string()).collect();
    let dir = review_root(&cx.root)?
        .join(format!("T{}-{}", cx.task.seq, &cx.task.task_id[..8]))
        .join(format!("round-{round}"));
    std::fs::create_dir_all(&dir)?;
    let diff_path = dir.join("diff.patch");
    std::fs::write(&diff_path, &diff)?;
    let (persona, fallback, swapped) = reviewer_personas(cx);
    let planned = plan_round(
        cx, sweep, &dir, &diff, &diff_path, &files, persona, fallback,
    )?;
    // 1 transaction: the old round's cancels and every new task. The state
    // file is written after it commits, so it never names a task that a
    // failed commit dropped. A failure queues nothing.
    let (state, shorts, stop) = store.immediate(|tx| {
        let stop = supersede(tx, old, round)?;
        let mut reviews = Vec::new();
        let mut shorts = Vec::new();
        for p in &planned {
            let added = tasks::add(tx, cx.run, &p.task, Actor::Worker)?;
            shorts.push(added.short());
            reviews.push((p.code.clone(), added.task_id.clone(), p.findings.clone()));
        }
        let state = State {
            round,
            tree: tree.to_string(),
            dir: dir.to_string_lossy().into_owned(),
            reviews,
            requeues: Default::default(),
        };
        Ok((state, shorts, stop))
    })?;
    save(cx, &state)?;
    wait_for_reviews(store, cx, &state)?;
    let mut note = format!(
        "round {round}: {} reviewer tasks queued ({}) for {persona}. horch wakes the orchestrator to spawn them, and tells you when the reviews are in. End your turn now",
        shorts.len(),
        shorts.join(", ")
    );
    if let Some(why) = swapped {
        note = format!("{why}, so the reviews go to {persona}. {note}");
    }
    let mut out = Outcome::waiting(
        &note,
        state.reviews.iter().map(|(_, id, _)| id.clone()).collect(),
    )
    .judged(Some(tree.to_string()));
    out.stop_workers = stop;
    Ok(out)
}

/// Queue a review again for each review of the current round that the
/// orchestrator cancelled: same reviewer, same tree, same findings file.
/// Each reviewer at most [`MAX_REQUEUES`] times.
fn requeue_cancelled(store: &mut Store, cx: &Ctx<'_>, state: &mut State) -> Result<Vec<String>> {
    let (again, next) = store.immediate(|tx| {
        let mut again = Vec::new();
        let mut next = state.clone();
        for review in next.reviews.iter_mut() {
            let old = tasks::get(tx, &review.1)?;
            let count = next.requeues.entry(review.0.clone()).or_default();
            if old.status != "cancelled" || *count >= MAX_REQUEUES {
                continue;
            }
            *count += 1;
            // The persona that last could claim it: after a fallback, that one.
            let primary = old
                .claimable_by
                .clone()
                .or(old.primary_persona.clone())
                .unwrap_or_else(|| reviewer_persona(None).to_string());
            let fallback = other_reviewer(&primary);
            let name = REVIEWERS
                .iter()
                .find(|(c, _, _)| *c == review.0)
                .map(|(_, n, _)| *n)
                .unwrap_or("reviewer");
            let ticket = old.task_file.clone().unwrap_or_default();
            let _ = std::fs::remove_file(&review.2);
            let t = review_task(
                cx,
                name,
                Path::new(&ticket),
                Path::new(&review.2),
                &primary,
                fallback,
            );
            let added = tasks::add(tx, cx.run, &t, Actor::Worker)?;
            again.push(added.short());
            review.1 = added.task_id;
        }
        Ok((again, next))
    })?;
    if !again.is_empty() {
        *state = next;
        save(cx, state)?;
    }
    Ok(again)
}

/// The step.
pub fn step(store: &mut Store, cx: &Ctx<'_>) -> Result<Outcome> {
    let git = &cx.tools.git;
    let base = cx
        .run
        .run_branch
        .clone()
        .or(cx.run.base_branch.clone())
        .context("the run has no run branch")?;
    let names = git.run(&cx.dir, &["diff", "--name-only", &format!("{base}...HEAD")])?;
    let tree = git.tree(&cx.dir)?;
    if names.trim().is_empty() {
        return Ok(Outcome::not_applicable("the branch changes no file").judged(Some(tree)));
    }
    let Some(sweep) = cx
        .tools
        .code_sweep_dir
        .clone()
        .filter(|d| d.join("reviewers").is_dir())
    else {
        return Ok(Outcome::fail(
            "no skill",
            None,
            "the code-sweep skill was not found. Install the dev plugin's code-sweep skill, or set HORCH_CODE_SWEEP_DIR to its folder (the one that holds reviewers/)",
        ));
    };
    let mut state = match load(cx) {
        Some(s) if s.tree == tree => s,
        old => return queue_round(store, cx, &sweep, &base, &tree, old.as_ref()),
    };
    // The round of this tree: queue cancelled reviews again, then wait.
    let again = requeue_cancelled(store, cx, &mut state)?;
    let mut pending = Vec::new();
    for (_, id, _) in &state.reviews {
        let t = tasks::get(store.conn(), id)?;
        if !matches!(t.status.as_str(), "done" | "cancelled") {
            pending.push(t.short());
        }
    }
    if !pending.is_empty() {
        // Keep the original deadline; repeated finish calls do not extend it.
        if !again.is_empty()
            || !super::super::workers::waiting(store.conn(), &cx.worker.worker_id)?
                .iter()
                .any(|w| w.kind == "reviewers")
        {
            wait_for_reviews(store, cx, &state)?;
        }
        let requeued = if again.is_empty() {
            String::new()
        } else {
            format!(
                "{} cancelled reviews are queued again ({}). ",
                again.len(),
                again.join(", ")
            )
        };
        return Ok(Outcome::waiting(
            &format!(
                "round {}: {requeued}waiting for reviewer tasks {}. End your turn; horch tells you when they are in",
                state.round,
                pending.join(", ")
            ),
            state.reviews.iter().map(|(_, id, _)| id.clone()).collect(),
        )
        .judged(Some(tree)));
    }
    super::super::workers::clear_wait(store.conn(), &cx.worker.worker_id, "reviewers")?;
    // Merge the findings.
    let mut findings = Vec::new();
    let mut table = Vec::new();
    for (code, _, file) in &state.reviews {
        let name = REVIEWERS
            .iter()
            .find(|(c, _, _)| c == code)
            .map(|(_, n, _)| *n)
            .unwrap_or("reviewer");
        let text = std::fs::read_to_string(file).unwrap_or_default();
        let found = parse_findings(name, &text);
        let count = |s: &str| found.iter().filter(|f| f.0 == s).count();
        let (c, i, m) = (count("Critical"), count("Important"), count("Minor"));
        let verdict = if text.trim().is_empty() {
            "MISSING"
        } else if c > 0 {
            "FAIL"
        } else if i > 0 {
            "WARN"
        } else {
            "PASS"
        };
        table.push(format!("| {name} | {verdict} | {c} | {i} | {m} |"));
        findings.extend(found);
    }
    let findings: Vec<Finding> = findings
        .into_iter()
        .enumerate()
        .map(|(n, (severity, reviewer, text))| Finding {
            id: format!("F{}", n + 1),
            severity,
            reviewer,
            text,
        })
        .collect();
    let answers_path = Path::new(&state.dir).join("answers.md");
    let answers = parse_answers(&std::fs::read_to_string(&answers_path).unwrap_or_default());
    let answered = |id: &str| answers.iter().any(|(a, _)| a == id);
    let waived = waivers(&cx.tools.evidence_root, &cx.task.task_id);
    let waiver = |id: &str| waived.iter().find(|w| w.id == id);
    let missing = table.iter().any(|r| r.contains("| MISSING |"));
    let critical: Vec<&Finding> = findings
        .iter()
        .filter(|f| f.severity == "Critical" && waiver(&f.id).is_none())
        .collect();
    let open_important: Vec<&Finding> = findings
        .iter()
        .filter(|f| f.severity == "Important" && !answered(&f.id) && waiver(&f.id).is_none())
        .collect();
    let overall = if !critical.is_empty() || missing {
        "FAIL"
    } else if findings
        .iter()
        .any(|f| f.severity == "Important" || f.severity == "Critical")
    {
        "WARN"
    } else {
        "PASS"
    };
    let mut report = format!(
        "# Code sweep of {} (round {})\n\nOverall verdict: {overall}\n\n| Reviewer | Verdict | Critical | Important | Minor |\n|---|---|---|---|---|\n{}\n",
        cx.task.short(),
        state.round,
        table.join("\n")
    );
    for sev in ["Critical", "Important", "Minor"] {
        report.push_str(&format!("\n## {sev}\n"));
        for f in findings.iter().filter(|f| f.severity == sev) {
            let ans = answers
                .iter()
                .find(|(a, _)| *a == f.id)
                .map(|(_, w)| format!(" — answered: {w}"))
                .unwrap_or_default();
            let ans = match waiver(&f.id) {
                Some(w) => format!("{ans} — waived by the orchestrator: {}", w.reason),
                None => ans,
            };
            report.push_str(&format!("- {} [{}] {}{ans}\n", f.id, f.reviewer, f.text));
        }
    }
    let evidence = cx
        .evidence_dir
        .join(format!("{}-code-sweep-report.md", cx.seq));
    std::fs::write(&evidence, &report)?;
    std::fs::write(Path::new(&state.dir).join("report.md"), &report)?;
    if missing {
        return Ok(Outcome::fail(
            "FAIL",
            Some(evidence.clone()),
            &format!(
                "a reviewer finished without a findings file; read {}. To review this tree again, run horch ledger finish --rereview",
                evidence.display()
            ),
        )
        .judged(Some(tree)));
    }
    if !critical.is_empty() {
        return Ok(Outcome::fail(
            "FAIL",
            Some(evidence.clone()),
            &format!(
                "{} Critical findings ({}). Fix them, commit, and run finish again: the change is reviewed again. Report: {}",
                critical.len(),
                critical.iter().map(|f| f.id.as_str()).collect::<Vec<_>>().join(", "),
                evidence.display()
            ),
        )
        .judged(Some(tree)));
    }
    if !open_important.is_empty() {
        return Ok(Outcome::fail(
            "WARN",
            Some(evidence.clone()),
            &format!(
                "{} Important findings without an answer ({}). Fix each one and commit (the change is reviewed again), or answer it in {} as `F<n>: <reason>`. Then run finish again. Report: {}",
                open_important.len(),
                open_important.iter().map(|f| f.id.as_str()).collect::<Vec<_>>().join(", "),
                answers_path.display(),
                evidence.display()
            ),
        )
        .judged(Some(tree)));
    }
    Ok(Outcome::pass(
        overall,
        Some(evidence),
        &format!("round {}: {overall}", state.round),
    )
    .judged(Some(tree)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triage_always_picks_bugs_and_clarity_and_adds_by_signal() {
        let diff = "+++ b/src/auth.rs\n+fn check(password: &str) {}\n";
        let t = triage(diff, &["src/auth.rs".into()]);
        assert!(t.starts_with(&["b", "c"]));
        assert!(t.contains(&"v"), "{t:?}");
        assert!(!t.contains(&"u"));
        let t = triage("+<div/>\n", &["web/a.tsx".into()]);
        assert!(t.contains(&"u"));
    }

    #[test]
    fn findings_and_answers_parse() {
        let f = parse_findings(
            "bug-hunter",
            "## Critical\n- none\n## Important\n- a.rs:3 off by one. Fix: <=\n## Minor\n- name\n",
        );
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].0, "Important");
        let a = parse_answers("F1: the caller clamps it\nnot an answer\nF2:\n");
        assert_eq!(
            a,
            vec![("F1".to_string(), "the caller clamps it".to_string())]
        );
    }

    #[test]
    fn triage_keeps_the_reviewer_order() {
        let mut diff = String::from(
            "diff --git a/x b/x\nnew file mode 100644\n+let password = 1;\n+pub struct A;\n\
             +for x in y {}\n+pub fn f() {}\n+let l = lock();\n+let n = m as u8;\n",
        );
        diff.push_str(&"+x\n".repeat(400));
        let files: Vec<String> = ["src/a_test.rs", "Cargo.toml", "web/x.tsx"]
            .map(String::from)
            .to_vec();
        assert_eq!(
            triage(&diff, &files),
            ["b", "c", "v", "s", "e", "t", "g", "w", "r", "d", "j", "u"]
        );
        assert_eq!(triage("+x\n", &["README.md".into()]), ["b", "c"]);
        assert_eq!(triage("+x\n", &["a_test.md".into()]), ["b", "c", "t"]);
        let deleted = "-x\n".repeat(101);
        assert_eq!(triage(&deleted, &["a.md".into()]), ["b", "c", "w"]);
    }

    #[test]
    fn findings_outside_a_severity_heading_do_not_count() {
        let f = parse_findings(
            "r",
            "- before any heading\n## Critical\n-  \n- None\n  - a.rs:1 boom  \n## Notes\n- not a finding\n## minor things\n- small\n",
        );
        assert_eq!(
            f,
            vec![
                (
                    "Critical".to_string(),
                    "r".to_string(),
                    "a.rs:1 boom".to_string()
                ),
                ("Minor".to_string(), "r".to_string(), "small".to_string()),
            ]
        );
    }
}
