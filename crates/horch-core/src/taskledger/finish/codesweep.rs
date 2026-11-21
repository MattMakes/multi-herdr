use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use super::{Ctx, Outcome};
use crate::taskledger::model::{Actor, EventKind, OutboxEvent, ReviewMode, TaskKind, WorktreeMode};
use crate::taskledger::store::Store;
use crate::taskledger::tasks::{self, NewTask, Task};
use crate::taskledger::util::{self, About};

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

struct Signals<'a> {
    diff: String,
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

    fn adds(&self, words: &[&str]) -> bool {
        words.iter().any(|w| self.added.contains(w))
    }

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

fn reviewer_name(code: &str) -> &'static str {
    REVIEWERS
        .iter()
        .find(|(c, _, _)| *c == code)
        .map(|(_, n, _)| *n)
        .unwrap_or("reviewer")
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct State {
    round: u32,
    tree: String,
    dir: String,
    reviews: Vec<(String, String, String)>,
    #[serde(default)]
    requeues: BTreeMap<String, u32>,
    #[serde(default)]
    since_tree: Option<String>,
    #[serde(default)]
    ids: BTreeMap<String, String>,
    #[serde(default)]
    last_id: u32,
    #[serde(default)]
    redo: bool,
}

fn review_mode(task: &Task) -> ReviewMode {
    task.review_mode
        .as_deref()
        .and_then(|m| ReviewMode::parse(m).ok())
        .unwrap_or(ReviewMode::Full)
}

pub fn max_rounds(mode: ReviewMode) -> u32 {
    match mode {
        ReviewMode::NoReview => 0,
        ReviewMode::Light => 1,
        ReviewMode::Full => 3,
    }
}

pub fn max_reviewers(mode: ReviewMode) -> usize {
    match mode {
        ReviewMode::Light => 2,
        ReviewMode::NoReview | ReviewMode::Full => 3,
    }
}

const MAX_REQUEUES: u32 = 2;

fn state_path(evidence_root: &Path, task_id: &str) -> PathBuf {
    super::task_dir(evidence_root, task_id).join("code-sweep-state.json")
}

fn load_state(evidence_root: &Path, task_id: &str) -> Option<State> {
    std::fs::read(state_path(evidence_root, task_id))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
}

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
    state.redo = true;
    let path = state_path(evidence_root, task_id);
    std::fs::write(path, serde_json::to_vec_pretty(&state)?)?;
    Ok(())
}

pub fn reviewer_persona(author_harness: Option<&str>) -> &'static str {
    match author_harness {
        Some("codex") => "architect-reviewer",
        _ => "codex-reviewer",
    }
}

pub fn other_reviewer(persona: &str) -> &'static str {
    match persona {
        "architect-reviewer" => "codex-reviewer",
        "architect-reviewer-light" => "codex-reviewer-light",
        "codex-reviewer-light" => "architect-reviewer-light",
        _ => "architect-reviewer",
    }
}

pub fn cheaper_reviewer(persona: &str) -> &'static str {
    if persona == "architect-reviewer" {
        "architect-reviewer-light"
    } else {
        "codex-reviewer-light"
    }
}

fn reviewer_personas(cx: &Ctx<'_>, round: u32) -> (&'static str, &'static str, Option<String>) {
    let usual = reviewer_persona(cx.worker.harness.as_deref());
    let other = other_reviewer(usual);
    let down = |p: &str| {
        cx.tools
            .reviewers_down
            .iter()
            .find(|(d, _)| d == p)
            .map(|(_, why)| why.clone())
    };
    let (primary, fallback, swapped) = match (down(usual), down(other)) {
        (Some(why), None) => (
            other,
            usual,
            Some(format!("{usual} cannot run here ({why})")),
        ),
        _ => (usual, other, None),
    };
    if round > 1 {
        let light = cheaper_reviewer(primary);
        (light, other_reviewer(light), swapped)
    } else {
        (primary, fallback, swapped)
    }
}

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

pub fn waivers(evidence_root: &Path, task_id: &str) -> Vec<Waiver> {
    all_waivers(evidence_root, task_id)
}

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

pub fn waive(
    conn: &rusqlite::Connection,
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
        conn,
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

pub(super) fn review_root(root: &Path) -> Result<PathBuf> {
    let dir = root.join(".horch").join("reviews");
    std::fs::create_dir_all(&dir)?;
    let ignore = root.join(".horch").join(".gitignore");
    if !ignore.exists() {
        std::fs::write(&ignore, "*\n")?;
    }
    Ok(dir)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub id: String,
    pub severity: String,
    pub reviewer: String,
    pub text: String,
}

fn heading_severity(heading: &str) -> Option<&'static str> {
    let h = heading.trim().to_lowercase();
    ["Critical", "Important", "Minor"]
        .into_iter()
        .find(|s| h.starts_with(&s.to_lowercase()))
}

fn finding_item(line: &str) -> Option<&str> {
    line.strip_prefix("- ")
        .map(str::trim)
        .filter(|item| !item.is_empty() && !item.eq_ignore_ascii_case("none"))
}

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

struct Planned {
    code: String,
    task: NewTask,
    findings: String,
}

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
    t.priority = -500;
    t.spec_ref = Some(findings.to_string_lossy().into_owned());
    t
}

struct Earlier {
    round: u32,
    report: PathBuf,
    answers: PathBuf,
}

struct RoundPlan<'a> {
    sweep: &'a Path,
    diff: &'a RoundDiff,
    persona: &'a str,
    fallback: &'a str,
    limit: usize,
    earlier: Option<Earlier>,
}

fn earlier_text(e: &Earlier) -> String {
    format!(
        "## Earlier rounds\n\n\
         This round reviews only the change since round {r}. The report of round {r}: {report}. \
         The author's answers: {answers}.\n\n\
         - A finding of an earlier round that is still not fixed: write it again with its id first, \
         as `- F<n> <file>:<line> <issue>. Fix: <fix>`.\n\
         - Rule on each answer in {answers}: add 1 line under `## Answers`, \
         `- F<n> accepted` or `- F<n> rejected: <why>`. \
         Write a finding with a rejected answer again under its severity, with its id first.\n\n",
        r = e.round,
        report = e.report.display(),
        answers = e.answers.display(),
    )
}

fn plan_round(cx: &Ctx<'_>, plan: &RoundPlan<'_>) -> Result<Vec<Planned>> {
    let d = plan.diff;
    let earlier = plan.earlier.as_ref().map(earlier_text).unwrap_or_default();
    let answers_form = if plan.earlier.is_some() {
        "## Answers\n- none\n"
    } else {
        ""
    };
    let mut out = Vec::new();
    for code in triage(&d.diff, &d.files).into_iter().take(plan.limit) {
        let (_, name, focus) = REVIEWERS
            .iter()
            .find(|(c, _, _)| *c == code)
            .expect("triage returns known codes");
        let prompt_file = plan.sweep.join("reviewers").join(format!("{name}.md"));
        let prompt = std::fs::read_to_string(&prompt_file).with_context(|| {
            format!(
                "reading the {name} reviewer prompt {}",
                prompt_file.display()
            )
        })?;
        let findings = d.dir.join(format!("{name}-findings.md"));
        let ticket = d.dir.join(format!("{name}.md"));
        std::fs::write(
            &ticket,
            format!(
                "# Review {short}: {name} ({focus})\n\n\
                 You are the {name} reviewer of a code-sweep review. Review only for your concern.\n\n\
                 - The diff: {diff}\n\
                 - The changed files: {files}\n\
                 - The author's worktree, for the surrounding code (read it, never edit it): {wt}\n\n\
                 Read the surrounding code of every changed file. Do not edit any file except your findings file.\n\n\
                 {earlier}\
                 Write your findings to {out} in exactly this form, 1 finding per `- ` line, \
                 each with file:line, the issue and the fix. Write `- none` under a heading with no findings.\n\n\
                 ```\n## Critical\n- <file>:<line> <issue>. Fix: <fix>\n## Important\n- none\n## Minor\n- none\n{answers_form}```\n\n\
                 Then run: horch ledger done \"<1 line: how many findings of each severity>\"\n\n\
                 ## Your reviewer prompt\n\n{prompt}\n",
                short = cx.task.short(),
                diff = d.diff_path.display(),
                files = d.files.join(", "),
                wt = cx.dir.display(),
                out = findings.display(),
            ),
        )?;
        out.push(Planned {
            code: code.to_string(),
            task: review_task(cx, name, &ticket, &findings, plan.persona, plan.fallback),
            findings: findings.to_string_lossy().into_owned(),
        });
    }
    Ok(out)
}

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

struct RoundDiff {
    dir: PathBuf,
    diff: String,
    diff_path: PathBuf,
    files: Vec<String>,
}

pub fn diff_range(base: &str, since_tree: Option<&str>) -> Vec<String> {
    match since_tree {
        Some(tree) => vec![tree.to_string(), "HEAD".to_string()],
        None => vec![format!("{base}...HEAD")],
    }
}

fn diff_of(cx: &Ctx<'_>, range: &[String]) -> Result<(String, String)> {
    let git = &cx.tools.git;
    let mut args = vec!["diff"];
    args.extend(range.iter().map(String::as_str));
    let diff = git.run(&cx.dir, &args)?;
    args.insert(1, "--name-only");
    let names = git.run(&cx.dir, &args)?;
    Ok((diff, names))
}

fn round_diff(cx: &Ctx<'_>, base: &str, since_tree: Option<&str>, round: u32) -> Result<RoundDiff> {
    let (diff, names) = match diff_of(cx, &diff_range(base, since_tree)) {
        Ok(d) => d,
        Err(_) if since_tree.is_some() => diff_of(cx, &diff_range(base, None))?,
        Err(e) => return Err(e),
    };
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

fn next_round(old: Option<&State>) -> (u32, Option<String>) {
    match old {
        None => (1, None),
        Some(o) if o.redo => (o.round, o.since_tree.clone()),
        Some(o) => (o.round + 1, Some(o.tree.clone()).filter(|t| !t.is_empty())),
    }
}

fn carried_answers(dir: &Path, round: u32) -> Vec<(String, String)> {
    let mut merged: BTreeMap<u32, (String, String)> = BTreeMap::new();
    for r in 1..=round {
        let file = round_dir(dir, r).join("answers.md");
        for (id, why) in parse_answers(&std::fs::read_to_string(file).unwrap_or_default()) {
            let n = id[1..].parse().unwrap_or(0);
            merged.insert(n, (id, why));
        }
    }
    merged.into_values().collect()
}

fn round_dir(dir: &Path, round: u32) -> PathBuf {
    dir.parent().unwrap_or(dir).join(format!("round-{round}"))
}

fn earlier(old: Option<&State>, d: &RoundDiff, round: u32) -> Result<Option<Earlier>> {
    if old.is_none() || round < 2 {
        return Ok(None);
    }
    let before = round - 1;
    let answers = d.dir.join("answers.md");
    let carried = carried_answers(&d.dir, before);
    if !answers.exists() {
        let text: String = carried
            .iter()
            .map(|(id, why)| format!("{id}: {why}\n"))
            .collect();
        std::fs::write(&answers, text)?;
    }
    Ok(Some(Earlier {
        round: before,
        report: round_dir(&d.dir, before).join("report.md"),
        answers,
    }))
}

fn clear_findings(old: Option<&State>) {
    for (_, _, file) in old
        .filter(|o| o.redo)
        .map(|o| o.reviews.as_slice())
        .unwrap_or_default()
    {
        let _ = std::fs::remove_file(file);
    }
}

fn queue_round(
    store: &mut Store,
    cx: &Ctx<'_>,
    sweep: &Path,
    base: &str,
    tree: &str,
    old: Option<&State>,
) -> Result<Outcome> {
    let (round, since_tree) = next_round(old);
    let d = round_diff(cx, base, since_tree.as_deref(), round)?;
    clear_findings(old);
    let (persona, fallback, swapped) = reviewer_personas(cx, round);
    let planned = plan_round(
        cx,
        &RoundPlan {
            sweep,
            diff: &d,
            persona,
            fallback,
            limit: max_reviewers(review_mode(cx.task)),
            earlier: earlier(old, &d, round)?,
        },
    )?;
    let empty = State {
        round,
        tree: tree.to_string(),
        dir: d.dir.to_string_lossy().into_owned(),
        reviews: Vec::new(),
        requeues: Default::default(),
        since_tree,
        ids: old.map(|o| o.ids.clone()).unwrap_or_default(),
        last_id: old.map_or(0, |o| o.last_id),
        redo: false,
    };
    let (state, shorts, stop) = add_round(store, cx, old, empty, &planned)?;
    save(cx, &state)?;
    wait_for_reviews(store, cx, &state)?;
    let note = queued_note(round, &shorts, persona, swapped);
    let mut out = Outcome::waiting(&note, review_ids(&state)).judged(Some(tree.to_string()));
    out.stop_workers = stop;
    Ok(out)
}

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
    let cap = max_rounds(review_mode(cx.task));
    match load(cx) {
        Some(s) if s.tree == tree => Ok(Ok((s, tree))),
        Some(s) if !s.redo && s.round >= cap => Ok(Ok((s, tree))),
        old => queue_round(store, cx, &sweep, base, &tree, old.as_ref()).map(Err),
    }
}

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

struct Graded {
    findings: Vec<Finding>,
    table: Vec<String>,
    missing: bool,
    verdicts: Vec<AnswerVerdict>,
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnswerVerdict {
    pub id: String,
    pub reviewer: String,
    pub rejected: Option<String>,
}

pub fn parse_verdicts(reviewer: &str, text: &str) -> Vec<AnswerVerdict> {
    let mut out = Vec::new();
    let mut in_answers = false;
    for l in text.lines().map(str::trim) {
        if let Some(h) = l.strip_prefix("## ") {
            in_answers = h.trim().eq_ignore_ascii_case("answers");
            continue;
        }
        let Some(item) = finding_item(l).filter(|_| in_answers) else {
            continue;
        };
        let Some((id, rest)) = split_id(item) else {
            continue;
        };
        let rest = rest.trim_start();
        let lower = rest.to_lowercase();
        let rejected = if lower.starts_with("accepted") {
            None
        } else if lower.starts_with("rejected") {
            let why = rest["rejected".len()..]
                .trim_start_matches([':', ' '])
                .trim();
            Some(why.to_string())
        } else {
            continue;
        };
        out.push(AnswerVerdict {
            id,
            reviewer: reviewer.to_string(),
            rejected,
        });
    }
    out
}

fn split_id(item: &str) -> Option<(String, &str)> {
    let digits = item.strip_prefix('F')?;
    let n = digits.chars().take_while(char::is_ascii_digit).count();
    if n == 0 {
        return None;
    }
    let rest = &digits[n..];
    if !(rest.is_empty() || rest.starts_with([' ', ':'])) {
        return None;
    }
    Some((format!("F{}", &digits[..n]), rest.trim_start_matches(':')))
}

pub fn finding_key(reviewer: &str, text: &str) -> String {
    let mut words = text.split_whitespace();
    let first = words.next().unwrap_or_default();
    let (file, rest): (&str, Vec<&str>) = match first.split_once(':') {
        Some((file, _)) if !file.is_empty() => (file, words.collect()),
        _ => ("", text.split_whitespace().collect()),
    };
    let norm = rest.join(" ").to_lowercase();
    let norm = norm.trim_end_matches(['.', ' ']);
    crate::taskledger::tasks::ticket_sha(&format!("{file}\n{reviewer}\n{norm}"))
}

impl State {
    fn stable_id(&mut self, reviewer: &str, text: &str) -> (String, String) {
        if let Some((id, rest)) = split_id(text) {
            if self.ids.values().any(|v| *v == id) {
                return (id, rest.trim().to_string());
            }
        }
        let key = finding_key(reviewer, text);
        if let Some(id) = self.ids.get(&key) {
            return (id.clone(), text.to_string());
        }
        self.last_id += 1;
        let id = format!("F{}", self.last_id);
        self.ids.insert(key, id.clone());
        (id, text.to_string())
    }
}

fn grade_reviews(state: &mut State) -> Graded {
    let mut found_all = Vec::new();
    let mut table = Vec::new();
    let mut verdicts = Vec::new();
    for (code, _, file) in &state.reviews {
        let name = reviewer_name(code);
        let text = std::fs::read_to_string(file).unwrap_or_default();
        let found = parse_findings(name, &text);
        table.push(table_row(name, &text, &found));
        verdicts.extend(parse_verdicts(name, &text));
        found_all.extend(found);
    }
    let findings = found_all
        .into_iter()
        .map(|(severity, reviewer, text)| {
            let (id, text) = state.stable_id(&reviewer, &text);
            Finding {
                id,
                severity,
                reviewer,
                text,
            }
        })
        .collect();
    let missing = table.iter().any(|r| r.contains("| MISSING |"));
    Graded {
        findings,
        table,
        missing,
        verdicts,
    }
}

struct Rulings {
    answers_path: PathBuf,
    answers: Vec<(String, String)>,
    rejected: Vec<(String, String, String)>,
    waived: Vec<Waiver>,
}

impl Rulings {
    fn read(cx: &Ctx<'_>, state: &State, verdicts: &[AnswerVerdict]) -> Rulings {
        let dir = Path::new(&state.dir);
        let answers_path = dir.join("answers.md");
        let before = carried_answers(dir, state.round.saturating_sub(1));
        let mut answers = carried_answers(dir, state.round);
        let mut rejected = Vec::new();
        for v in verdicts {
            let Some(why) = &v.rejected else { continue };
            let unchanged = |a: &(String, String)| a.0 == v.id && before.iter().any(|b| b == a);
            if answers.iter().any(unchanged) {
                answers.retain(|a| !unchanged(a));
                rejected.push((v.id.clone(), v.reviewer.clone(), why.clone()));
            }
        }
        Rulings {
            answers_path,
            answers,
            rejected,
            waived: all_waivers(&cx.tools.evidence_root, &cx.task.task_id),
        }
    }

    fn answer(&self, id: &str) -> Option<&str> {
        self.answers
            .iter()
            .find(|(a, _)| a == id)
            .map(|(_, why)| why.as_str())
    }

    fn rejection(&self, id: &str) -> Option<&(String, String, String)> {
        self.rejected.iter().find(|r| r.0 == id)
    }

    fn waiver(&self, id: &str) -> Option<&Waiver> {
        self.waived.iter().rev().find(|w| w.id == id)
    }

    fn blocks(&self, f: &Finding) -> bool {
        self.waiver(&f.id).is_none() && (f.severity == "Critical" || self.answer(&f.id).is_none())
    }
}

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
            let ans = match r.rejection(&f.id) {
                Some((_, who, why)) => format!("{ans} — answer rejected by {who}: {why}"),
                None => ans,
            };
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

struct Bound {
    mode: ReviewMode,
    last: bool,
    moved: bool,
}

impl Judgement<'_> {
    fn blocking(&self) -> Vec<&Finding> {
        self.critical
            .iter()
            .chain(self.open_important.iter())
            .copied()
            .collect()
    }

    fn outcome(&self, state: &State, r: &Rulings, bound: &Bound, evidence: PathBuf) -> Outcome {
        let report = evidence.display();
        let last = |what: String| {
            format!(
                "{what} in round {}, the last review round (Review: {}). horch asked the orchestrator to rule on them",
                state.round,
                bound.mode.as_str()
            )
        };
        let note = if self.missing {
            format!(
                "a reviewer finished without a findings file; read {report}. To review this tree again, run horch ledger finish --rereview"
            )
        } else if !self.critical.is_empty() && bound.last {
            format!(
                "{}. A fix is not reviewed again: the orchestrator waives each one. Report: {report}",
                last(format!(
                    "{} Critical findings ({})",
                    self.critical.len(),
                    ids(&self.critical)
                ))
            )
        } else if !self.critical.is_empty() {
            format!(
                "{} Critical findings ({}). Fix them, commit, and run finish again: the change is reviewed again. Report: {report}",
                self.critical.len(),
                ids(&self.critical)
            )
        } else if !self.open_important.is_empty() && bound.last {
            format!(
                "{}. Answer each one in {} as `F<n>: <reason>` (after a fix: `F<n>: fixed in <commit>`), or wait for a waiver. Then run finish again. Report: {report}",
                last(format!(
                    "{} Important findings without an answer ({})",
                    self.open_important.len(),
                    ids(&self.open_important)
                )),
                r.answers_path.display()
            )
        } else if !self.open_important.is_empty() {
            format!(
                "{} Important findings without an answer ({}). Fix each one and commit (the change is reviewed again), or answer it in {} as `F<n>: <reason>`. Then run finish again. Report: {report}",
                self.open_important.len(),
                ids(&self.open_important),
                r.answers_path.display()
            )
        } else {
            let tail = if bound.moved {
                format!(
                    ". Round {} is the last review round (Review: {}): the change since it is not reviewed",
                    state.round,
                    bound.mode.as_str()
                )
            } else {
                String::new()
            };
            return Outcome::pass(
                self.overall,
                Some(evidence.clone()),
                &format!("round {}: {}{tail}", state.round, self.overall),
            );
        };
        Outcome::fail(self.overall, Some(evidence.clone()), &note)
    }
}

pub fn ask_orchestrator(
    conn: &rusqlite::Connection,
    task: &Task,
    round: u32,
    blocking: &[&Finding],
    answers: &Path,
    report: &Path,
) -> Result<()> {
    let list = ids(blocking);
    let first = blocking.first().map_or("F1", |f| f.id.as_str());
    let text = format!(
        "QUESTION: {short} code-sweep round {round} is the last review round, and {n} findings still block ({list}). \
         Waive each one that does not block: horch ledger waive {short} {first} \"<reason>\". \
         Or tell the author to answer it in {answers}. Report: {report}",
        short = task.short(),
        n = blocking.len(),
        answers = answers.display(),
        report = report.display(),
    );
    conn.execute(
        "INSERT OR IGNORE INTO outbox (id, run_id, event, key, text, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            util::new_id(),
            task.run_id,
            OutboxEvent::Question.as_str(),
            format!("{}:code-sweep:round-{round}:{list}", task.task_id),
            text,
            util::now(),
        ],
    )?;
    Ok(())
}

pub fn step(store: &mut Store, cx: &Ctx<'_>) -> Result<Outcome> {
    let mode = review_mode(cx.task);
    if mode == ReviewMode::NoReview {
        return Ok(Outcome::not_applicable("the ticket says Review: none"));
    }
    let base = cx.diff_base()?;
    let (mut state, tree) = match current_round(store, cx, &base)? {
        Ok(round) => round,
        Err(outcome) => return Ok(outcome),
    };
    if let Some(waiting) = wait_for_round(store, cx, &mut state, &tree)? {
        return Ok(waiting);
    }
    let graded = grade_reviews(&mut state);
    save(cx, &state)?;
    let rulings = Rulings::read(cx, &state, &graded.verdicts);
    let judgement = judge(&graded, &rulings);
    let report = render_report(cx, &state, &graded, &rulings, judgement.overall);
    let evidence = cx
        .evidence_dir
        .join(format!("{}-code-sweep-report.md", cx.seq));
    std::fs::write(&evidence, &report)?;
    let round_report = Path::new(&state.dir).join("report.md");
    std::fs::write(&round_report, &report)?;
    let bound = Bound {
        mode,
        last: state.round >= max_rounds(mode),
        moved: state.tree != tree,
    };
    let blocking = judgement.blocking();
    if bound.last && !judgement.missing && !blocking.is_empty() {
        ask_orchestrator(
            store.conn(),
            cx.task,
            state.round,
            &blocking,
            &rulings.answers_path,
            &round_report,
        )?;
    }
    Ok(judgement
        .outcome(&state, &rulings, &bound, evidence)
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

    #[test]
    fn trf_27_light_runs_1_round_and_full_runs_3() {
        assert_eq!(max_rounds(ReviewMode::NoReview), 0);
        assert_eq!(max_rounds(ReviewMode::Light), 1);
        assert_eq!(max_rounds(ReviewMode::Full), 3);
    }

    #[test]
    fn trf_28_rounds_2_and_3_diff_from_the_last_reviewed_tree() {
        assert_eq!(diff_range("fleet/r", None), ["fleet/r...HEAD"]);
        assert_eq!(diff_range("fleet/r", Some("abc")), ["abc", "HEAD"]);
        let old = State {
            round: 1,
            tree: "t1".into(),
            ..Default::default()
        };
        assert_eq!(next_round(None), (1, None));
        assert_eq!(next_round(Some(&old)), (2, Some("t1".into())));
        let redo = State {
            round: 2,
            since_tree: Some("t1".into()),
            redo: true,
            ..Default::default()
        };
        assert_eq!(next_round(Some(&redo)), (2, Some("t1".into())));
    }

    #[test]
    fn trf_29_a_finding_keeps_its_id_when_its_line_moves() {
        let k = finding_key("bug-hunter", "a.rs:2 Off by  one.");
        assert_eq!(k, finding_key("bug-hunter", "a.rs:40 off by one"));
        assert_ne!(k, finding_key("clarity-checker", "a.rs:2 off by one"));
        assert_ne!(k, finding_key("bug-hunter", "b.rs:2 off by one"));
        let mut st = State::default();
        assert_eq!(st.stable_id("b", "a.rs:2 x").0, "F1");
        assert_eq!(st.stable_id("b", "c.rs:1 y").0, "F2");
        assert_eq!(st.stable_id("b", "a.rs:9 x").0, "F1");
        assert_eq!(
            st.stable_id("c", "F2 c.rs:5 y again"),
            ("F2".to_string(), "c.rs:5 y again".to_string())
        );
        assert_eq!(st.stable_id("c", "F9 unknown id").0, "F3");
    }

    #[test]
    fn trf_29_reviewers_accept_or_reject_each_answer() {
        let v = parse_verdicts(
            "r",
            "## Important\n- F1 accepted\n## Answers\n- F1 accepted\n- F2 rejected: no check\n- F3 maybe\n- none\n",
        );
        assert_eq!(
            v,
            vec![
                AnswerVerdict {
                    id: "F1".into(),
                    reviewer: "r".into(),
                    rejected: None
                },
                AnswerVerdict {
                    id: "F2".into(),
                    reviewer: "r".into(),
                    rejected: Some("no check".into())
                },
            ]
        );
    }

    #[test]
    fn trf_30_rounds_have_at_most_3_reviewers_and_later_rounds_are_cheaper() {
        assert_eq!(max_reviewers(ReviewMode::Light), 2);
        assert_eq!(max_reviewers(ReviewMode::Full), 3);
        assert_eq!(cheaper_reviewer("codex-reviewer"), "codex-reviewer-light");
        assert_eq!(
            cheaper_reviewer("architect-reviewer"),
            "architect-reviewer-light"
        );
    }
}
