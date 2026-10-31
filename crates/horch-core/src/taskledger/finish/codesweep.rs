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

/// What the triage reads of 1 change.
struct Signals<'a> {
    /// The whole diff, lower case.
    diff: String,
    /// The added lines, lower case.
    added: String,
    files: &'a [String],
    lines: usize,
    deleted: usize,
}

impl<'a> Signals<'a> {
    fn new(diff: &str, files: &'a [String]) -> Signals<'a> {
        let added: Vec<&str> = diff
            .lines()
            .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
            .collect();
        Signals {
            diff: diff.to_lowercase(),
            added: added.join("\n").to_lowercase(),
            files,
            lines: diff.lines().count(),
            deleted: diff
                .lines()
                .filter(|l| l.starts_with('-') && !l.starts_with("---"))
                .count(),
        }
    }

    /// Whether an added line holds 1 of `words`.
    fn adds(&self, words: &[&str]) -> bool {
        words.iter().any(|w| self.added.contains(w))
    }

    /// Whether a changed file's name ends with 1 of `ends`.
    fn file_ends_with(&self, ends: &[&str]) -> bool {
        self.files
            .iter()
            .any(|f| ends.iter().any(|e| f.ends_with(e)))
    }
}

fn security(s: &Signals<'_>) -> bool {
    s.adds(&[
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
    ])
}

fn new_files(s: &Signals<'_>) -> bool {
    s.diff.contains("new file mode")
        || s.diff.contains("rename from")
        || s.adds(&["pub trait", "interface ", "pub struct", "impl "])
}

fn hot_paths(s: &Signals<'_>) -> bool {
    s.adds(&[
        "for ", "while ", "loop", "async", "await", "cache", "query", ".iter()", "select ",
    ])
}

/// Tests or code changed: any file but a non-test Markdown file.
fn tests_or_code_changed(s: &Signals<'_>) -> bool {
    s.files
        .iter()
        .any(|f| f.contains("test") || !f.ends_with(".md"))
}

fn contracts(s: &Signals<'_>) -> bool {
    s.adds(&[
        "pub fn",
        "pub mod",
        "export ",
        "create table",
        "alter table",
        "schema",
        "#[derive(serialize",
    ])
}

fn large_change(s: &Signals<'_>) -> bool {
    s.lines > 400 || s.deleted > 100
}

fn resources(s: &Signals<'_>) -> bool {
    s.adds(&[
        "lock", "mutex", "thread", "spawn", "file", "socket", "drop(", "close", "kill", "signal",
    ])
}

fn touches_dependencies(s: &Signals<'_>) -> bool {
    s.file_ends_with(&[
        "Cargo.toml",
        "package.json",
        "requirements.txt",
        "go.mod",
        "Cargo.lock",
    ])
}

fn loose_rules(s: &Signals<'_>) -> bool {
    s.adds(&[
        " as ",
        "unwrap()",
        "#[allow",
        "eslint-disable",
        "type: ignore",
        "any)",
    ]) || s.lines > 200
}

fn touches_ui(s: &Signals<'_>) -> bool {
    const UI: &[&str] = &["html", "css", "scss", "tsx", "jsx", "vue", "svelte"];
    s.files.iter().any(|f| {
        Path::new(f)
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| UI.contains(&e))
    })
}

/// The reviewers the triage adds after `b` and `c`, in queue order, each
/// with the signal that picks it.
type Rule = fn(&Signals<'_>) -> bool;
const RULES: &[(&str, Rule)] = &[
    ("v", security),
    ("s", new_files),
    ("e", hot_paths),
    ("t", tests_or_code_changed),
    ("g", contracts),
    ("w", large_change),
    ("r", resources),
    ("d", touches_dependencies),
    ("j", loose_rules),
    ("u", touches_ui),
];

/// The skill's auto-triage: `b` and `c` always, the others by diff signals.
pub fn triage(diff: &str, files: &[String]) -> Vec<&'static str> {
    let signals = Signals::new(diff, files);
    let mut out = vec!["b", "c"];
    out.extend(
        RULES
            .iter()
            .filter(|(_, picks)| picks(&signals))
            .map(|(code, _)| *code),
    );
    out
}

/// The prompt file name of reviewer `code`.
fn reviewer_name(code: &str) -> &'static str {
    REVIEWERS
        .iter()
        .find(|(c, _, _)| *c == code)
        .map(|(_, n, _)| *n)
        .unwrap_or("reviewer")
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

/// The severity a `## ` heading opens, if any.
fn heading_severity(heading: &str) -> Option<&'static str> {
    let h = heading.trim().to_lowercase();
    ["Critical", "Important", "Minor"]
        .into_iter()
        .find(|s| h.starts_with(&s.to_lowercase()))
}

/// The text of a `- ` item that is a finding: not empty, not `none`.
fn finding_item(line: &str) -> Option<&str> {
    line.strip_prefix("- ")
        .map(str::trim)
        .filter(|item| !item.is_empty() && !item.eq_ignore_ascii_case("none"))
}

/// The findings of 1 reviewer's file: the `- ` items under `## Critical`,
/// `## Important` and `## Minor`. `- none` is no finding.
pub fn parse_findings(reviewer: &str, text: &str) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let mut sev: Option<&str> = None;
    for l in text.lines().map(str::trim) {
        if let Some(h) = l.strip_prefix("## ") {
            sev = heading_severity(h);
        } else if let (Some(s), Some(item)) = (sev, finding_item(l)) {
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
#[allow(clippy::too_many_arguments)] // the round inputs are all distinct paths and strings of one step
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

/// The review task ids of the round.
fn review_ids(state: &State) -> Vec<String> {
    state.reviews.iter().map(|(_, id, _)| id.clone()).collect()
}

fn wait_for_reviews(store: &Store, cx: &Ctx<'_>, state: &State) -> Result<()> {
    super::super::workers::set_wait(
        store.conn(),
        &cx.worker.worker_id,
        "reviewers",
        "waiting for reviewer tasks",
        &review_ids(state),
        7 * 24 * 3600,
    )
}

/// The change a round reviews, written into the round's folder.
struct RoundDiff {
    dir: PathBuf,
    diff: String,
    diff_path: PathBuf,
    files: Vec<String>,
}

/// The folder of round `round` of the task's reviews, with the branch's
/// diff in it.
fn round_diff(cx: &Ctx<'_>, base: &str, round: u32) -> Result<RoundDiff> {
    let git = &cx.tools.git;
    let diff = git.run(&cx.dir, &["diff", &format!("{base}...HEAD")])?;
    let names = git.run(&cx.dir, &["diff", "--name-only", &format!("{base}...HEAD")])?;
    let dir = review_root(&cx.root)?
        .join(format!("T{}-{}", cx.task.seq, &cx.task.task_id[..8]))
        .join(format!("round-{round}"));
    std::fs::create_dir_all(&dir)?;
    let diff_path = dir.join("diff.patch");
    std::fs::write(&diff_path, &diff)?;
    Ok(RoundDiff {
        dir,
        diff,
        diff_path,
        files: names.lines().map(String::from).collect(),
    })
}

/// What `finish` says after it queued a round.
fn queued_note(round: u32, shorts: &[String], persona: &str, swapped: Option<String>) -> String {
    let note = format!(
        "round {round}: {} reviewer tasks queued ({}) for {persona}. horch wakes the orchestrator to spawn them, and tells you when the reviews are in. End your turn now",
        shorts.len(),
        shorts.join(", ")
    );
    match swapped {
        Some(why) => format!("{why}, so the reviews go to {persona}. {note}"),
        None => note,
    }
}

/// The new round, the short names of its tasks, and the workers that held
/// a review of the old round. 1 transaction: the old round's cancels and
/// every new task. The state file is written after it commits, so it never
/// names a task that a failed commit dropped. A failure queues nothing.
fn add_round(
    store: &mut Store,
    cx: &Ctx<'_>,
    old: Option<&State>,
    round: State,
    planned: &[Planned],
) -> Result<(State, Vec<String>, Vec<String>)> {
    store.immediate(|tx| {
        let stop = supersede(tx, old, round.round)?;
        let mut state = round;
        let mut shorts = Vec::new();
        for p in planned {
            let added = tasks::add(tx, cx.run, &p.task, Actor::Worker)?;
            shorts.push(added.short());
            state
                .reviews
                .push((p.code.clone(), added.task_id.clone(), p.findings.clone()));
        }
        Ok((state, shorts, stop))
    })
}

fn queue_round(
    store: &mut Store,
    cx: &Ctx<'_>,
    sweep: &Path,
    base: &str,
    tree: &str,
    old: Option<&State>,
) -> Result<Outcome> {
    let round = old.map_or(1, |o| o.round + 1);
    let d = round_diff(cx, base, round)?;
    let (persona, fallback, swapped) = reviewer_personas(cx);
    let planned = plan_round(
        cx,
        sweep,
        &d.dir,
        &d.diff,
        &d.diff_path,
        &d.files,
        persona,
        fallback,
    )?;
    let empty = State {
        round,
        tree: tree.to_string(),
        dir: d.dir.to_string_lossy().into_owned(),
        reviews: Vec::new(),
        requeues: Default::default(),
    };
    let (state, shorts, stop) = add_round(store, cx, old, empty, &planned)?;
    save(cx, &state)?;
    wait_for_reviews(store, cx, &state)?;
    let note = queued_note(round, &shorts, persona, swapped);
    let mut out = Outcome::waiting(&note, review_ids(&state)).judged(Some(tree.to_string()));
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
            let name = reviewer_name(&review.0);
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

/// The round of the branch's tree, with the tree: the saved round when it
/// reviews this tree. Else a new round is queued, and its outcome, like a
/// step with nothing to review, is the `Err`.
fn current_round(
    store: &mut Store,
    cx: &Ctx<'_>,
    base: &str,
) -> Result<std::result::Result<(State, String), Outcome>> {
    let git = &cx.tools.git;
    let names = git.run(&cx.dir, &["diff", "--name-only", &format!("{base}...HEAD")])?;
    let tree = git.tree(&cx.dir)?;
    if names.trim().is_empty() {
        return Ok(Err(
            Outcome::not_applicable("the branch changes no file").judged(Some(tree))
        ));
    }
    let Some(sweep) = cx
        .tools
        .code_sweep_dir
        .clone()
        .filter(|d| d.join("reviewers").is_dir())
    else {
        return Ok(Err(Outcome::fail(
            "no skill",
            None,
            "the code-sweep skill was not found. Install the dev plugin's code-sweep skill, or set HORCH_CODE_SWEEP_DIR to its folder (the one that holds reviewers/)",
        )));
    };
    match load(cx) {
        Some(s) if s.tree == tree => Ok(Ok((s, tree))),
        old => queue_round(store, cx, &sweep, base, &tree, old.as_ref()).map(Err),
    }
}

/// The short names of the round's reviews that are not in yet.
fn pending_reviews(store: &Store, state: &State) -> Result<Vec<String>> {
    let mut pending = Vec::new();
    for (_, id, _) in &state.reviews {
        let t = tasks::get(store.conn(), id)?;
        if !matches!(t.status.as_str(), "done" | "cancelled") {
            pending.push(t.short());
        }
    }
    Ok(pending)
}

/// Record the wait for the round's reviewers. Keep the original deadline;
/// repeated finish calls do not extend it. Reviews queued again start a
/// new wait.
fn keep_waiting(store: &Store, cx: &Ctx<'_>, state: &State, requeued: bool) -> Result<()> {
    if requeued
        || !super::super::workers::waiting(store.conn(), &cx.worker.worker_id)?
            .iter()
            .any(|w| w.kind == "reviewers")
    {
        wait_for_reviews(store, cx, state)?;
    }
    Ok(())
}

fn requeued_note(again: &[String]) -> String {
    if again.is_empty() {
        String::new()
    } else {
        format!(
            "{} cancelled reviews are queued again ({}). ",
            again.len(),
            again.join(", ")
        )
    }
}

/// Queue cancelled reviews of the round again, then wait for the reviews
/// that are not in. `None` when every review is in.
fn wait_for_round(
    store: &mut Store,
    cx: &Ctx<'_>,
    state: &mut State,
    tree: &str,
) -> Result<Option<Outcome>> {
    let again = requeue_cancelled(store, cx, state)?;
    let pending = pending_reviews(store, state)?;
    if pending.is_empty() {
        super::super::workers::clear_wait(store.conn(), &cx.worker.worker_id, "reviewers")?;
        return Ok(None);
    }
    keep_waiting(store, cx, state, !again.is_empty())?;
    let requeued = requeued_note(&again);
    Ok(Some(
        Outcome::waiting(
            &format!(
                "round {}: {requeued}waiting for reviewer tasks {}. End your turn; horch tells you when they are in",
                state.round,
                pending.join(", ")
            ),
            review_ids(state),
        )
        .judged(Some(tree.to_string())),
    ))
}

/// The merged findings of a round's reviews.
struct Graded {
    findings: Vec<Finding>,
    /// 1 report row per reviewer.
    table: Vec<String>,
    /// A reviewer finished without a findings file.
    missing: bool,
}

/// The report row of 1 reviewer.
fn table_row(name: &str, text: &str, found: &[(String, String, String)]) -> String {
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
    format!("| {name} | {verdict} | {c} | {i} | {m} |")
}

/// Read every findings file of the round and number the findings.
fn grade_reviews(state: &State) -> Graded {
    let mut found_all = Vec::new();
    let mut table = Vec::new();
    for (code, _, file) in &state.reviews {
        let name = reviewer_name(code);
        let text = std::fs::read_to_string(file).unwrap_or_default();
        let found = parse_findings(name, &text);
        table.push(table_row(name, &text, &found));
        found_all.extend(found);
    }
    let findings = found_all
        .into_iter()
        .enumerate()
        .map(|(n, (severity, reviewer, text))| Finding {
            id: format!("F{}", n + 1),
            severity,
            reviewer,
            text,
        })
        .collect();
    let missing = table.iter().any(|r| r.contains("| MISSING |"));
    Graded {
        findings,
        table,
        missing,
    }
}

/// The author's answers and the orchestrator's waivers of a round.
struct Rulings {
    answers_path: PathBuf,
    answers: Vec<(String, String)>,
    waived: Vec<Waiver>,
}

impl Rulings {
    fn read(cx: &Ctx<'_>, state: &State) -> Rulings {
        let answers_path = Path::new(&state.dir).join("answers.md");
        let answers = parse_answers(&std::fs::read_to_string(&answers_path).unwrap_or_default());
        Rulings {
            answers_path,
            answers,
            waived: waivers(&cx.tools.evidence_root, &cx.task.task_id),
        }
    }

    fn answer(&self, id: &str) -> Option<&str> {
        self.answers
            .iter()
            .find(|(a, _)| a == id)
            .map(|(_, why)| why.as_str())
    }

    fn waiver(&self, id: &str) -> Option<&Waiver> {
        self.waived.iter().find(|w| w.id == id)
    }

    /// Whether `f` blocks the step: a Critical finding until it is waived,
    /// an Important one until it is answered or waived.
    fn blocks(&self, f: &Finding) -> bool {
        self.waiver(&f.id).is_none() && (f.severity == "Critical" || self.answer(&f.id).is_none())
    }
}

/// The verdict of a round.
struct Judgement<'a> {
    overall: &'static str,
    missing: bool,
    critical: Vec<&'a Finding>,
    open_important: Vec<&'a Finding>,
}

fn judge<'a>(g: &'a Graded, r: &Rulings) -> Judgement<'a> {
    let blocking = |severity: &str| -> Vec<&'a Finding> {
        g.findings
            .iter()
            .filter(|f| f.severity == severity && r.blocks(f))
            .collect()
    };
    let critical = blocking("Critical");
    let overall = if !critical.is_empty() || g.missing {
        "FAIL"
    } else if g
        .findings
        .iter()
        .any(|f| f.severity == "Important" || f.severity == "Critical")
    {
        "WARN"
    } else {
        "PASS"
    };
    Judgement {
        overall,
        missing: g.missing,
        open_important: blocking("Important"),
        critical,
    }
}

/// The merged report of the round.
fn render_report(cx: &Ctx<'_>, state: &State, g: &Graded, r: &Rulings, overall: &str) -> String {
    let mut report = format!(
        "# Code sweep of {} (round {})\n\nOverall verdict: {overall}\n\n| Reviewer | Verdict | Critical | Important | Minor |\n|---|---|---|---|---|\n{}\n",
        cx.task.short(),
        state.round,
        g.table.join("\n")
    );
    for sev in ["Critical", "Important", "Minor"] {
        report.push_str(&format!("\n## {sev}\n"));
        for f in g.findings.iter().filter(|f| f.severity == sev) {
            let ans = r
                .answer(&f.id)
                .map(|w| format!(" — answered: {w}"))
                .unwrap_or_default();
            let ans = match r.waiver(&f.id) {
                Some(w) => format!("{ans} — waived by the orchestrator: {}", w.reason),
                None => ans,
            };
            report.push_str(&format!("- {} [{}] {}{ans}\n", f.id, f.reviewer, f.text));
        }
    }
    report
}

fn ids(findings: &[&Finding]) -> String {
    findings
        .iter()
        .map(|f| f.id.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

impl Judgement<'_> {
    /// The step's outcome: its verdict is the overall verdict.
    fn outcome(&self, state: &State, r: &Rulings, evidence: PathBuf) -> Outcome {
        let report = evidence.display();
        let note = if self.missing {
            format!(
                "a reviewer finished without a findings file; read {report}. To review this tree again, run horch ledger finish --rereview"
            )
        } else if !self.critical.is_empty() {
            format!(
                "{} Critical findings ({}). Fix them, commit, and run finish again: the change is reviewed again. Report: {report}",
                self.critical.len(),
                ids(&self.critical)
            )
        } else if !self.open_important.is_empty() {
            format!(
                "{} Important findings without an answer ({}). Fix each one and commit (the change is reviewed again), or answer it in {} as `F<n>: <reason>`. Then run finish again. Report: {report}",
                self.open_important.len(),
                ids(&self.open_important),
                r.answers_path.display()
            )
        } else {
            return Outcome::pass(
                self.overall,
                Some(evidence.clone()),
                &format!("round {}: {}", state.round, self.overall),
            );
        };
        Outcome::fail(self.overall, Some(evidence.clone()), &note)
    }
}

/// The step.
pub fn step(store: &mut Store, cx: &Ctx<'_>) -> Result<Outcome> {
    let base = cx
        .run
        .run_branch
        .clone()
        .or(cx.run.base_branch.clone())
        .context("the run has no run branch")?;
    let (mut state, tree) = match current_round(store, cx, &base)? {
        Ok(round) => round,
        Err(outcome) => return Ok(outcome),
    };
    if let Some(waiting) = wait_for_round(store, cx, &mut state, &tree)? {
        return Ok(waiting);
    }
    let graded = grade_reviews(&state);
    let rulings = Rulings::read(cx, &state);
    let judgement = judge(&graded, &rulings);
    let report = render_report(cx, &state, &graded, &rulings, judgement.overall);
    let evidence = cx
        .evidence_dir
        .join(format!("{}-code-sweep-report.md", cx.seq));
    std::fs::write(&evidence, &report)?;
    std::fs::write(Path::new(&state.dir).join("report.md"), &report)?;
    Ok(judgement
        .outcome(&state, &rulings, evidence)
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
