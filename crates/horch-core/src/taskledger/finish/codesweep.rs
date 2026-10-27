//! The code-sweep step (#39): the `code-sweep` skill's review, run through
//! the ledger. A worker never starts subagents, so each reviewer the skill's
//! triage picks becomes a review task that a fleet worker claims, with the
//! reviewer's prompt and the diff in its ticket file. When every reviewer is
//! done, horch merges the findings with the skill's verdict rules: any
//! Critical finding fails; Important findings fail until each is fixed (a
//! new commit, which is reviewed again) or answered with a reason; Minor
//! findings never block.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::{Ctx, Outcome};
use crate::taskledger::model::{Actor, TaskKind, WorktreeMode};
use crate::taskledger::store::Store;
use crate::taskledger::tasks::{self, NewTask};

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
}

fn state_path(cx: &Ctx<'_>) -> PathBuf {
    cx.evidence_dir.join("code-sweep-state.json")
}

fn load(cx: &Ctx<'_>) -> Option<State> {
    std::fs::read(state_path(cx))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
}

/// The reviewer persona: the other vendor reviews (a Codex reviewer for
/// Claude-built work, a Claude reviewer for Codex-built work).
pub fn reviewer_persona(author_harness: Option<&str>) -> &'static str {
    match author_harness {
        Some("codex") => "architect-reviewer",
        _ => "codex-reviewer",
    }
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

fn queue_round(
    store: &mut Store,
    cx: &Ctx<'_>,
    sweep: &Path,
    base: &str,
    tree: &str,
    round: u32,
) -> Result<Outcome> {
    let git = &cx.tools.git;
    let diff = git.run(&cx.dir, &["diff", &format!("{base}...HEAD")])?;
    let names = git.run(&cx.dir, &["diff", "--name-only", &format!("{base}...HEAD")])?;
    let files: Vec<String> = names.lines().map(|s| s.to_string()).collect();
    let dir = review_root(&cx.root)?
        .join(format!("T{}-{}", cx.task.seq, &cx.worker.worker_id[..8]))
        .join(format!("round-{round}"));
    std::fs::create_dir_all(&dir)?;
    let diff_path = dir.join("diff.patch");
    std::fs::write(&diff_path, &diff)?;
    let persona = reviewer_persona(cx.worker.harness.as_deref());
    let mut reviews = Vec::new();
    let mut shorts = Vec::new();
    for code in triage(&diff, &files) {
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
        let mut t = NewTask::work(
            &format!("Review {}: {name}", cx.task.short()),
            &ticket.to_string_lossy(),
            persona,
        );
        t.kind = TaskKind::Review;
        t.parent_task_id = Some(cx.task.task_id.clone());
        t.worktree_mode = WorktreeMode::Shared;
        // Reviews go before ordinary work: the author waits for them.
        t.priority = -500;
        t.spec_ref = Some(findings.to_string_lossy().into_owned());
        let added = store.immediate(|tx| tasks::add(tx, cx.run, &t, Actor::Worker))?;
        shorts.push(added.short());
        reviews.push((
            code.to_string(),
            added.task_id.clone(),
            findings.to_string_lossy().into_owned(),
        ));
    }
    let state = State {
        round,
        tree: tree.to_string(),
        dir: dir.to_string_lossy().into_owned(),
        reviews: reviews.clone(),
    };
    std::fs::write(state_path(cx), serde_json::to_vec_pretty(&state)?)?;
    Ok(Outcome::waiting(
        &format!(
            "round {round}: {} reviewer tasks queued ({}) for {persona}. horch wakes the orchestrator to spawn them, and tells you when the reviews are in. End your turn now",
            shorts.len(),
            shorts.join(", ")
        ),
        reviews.into_iter().map(|(_, id, _)| id).collect(),
    )
    .judged(Some(tree.to_string())))
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
    let tree = git.rev(&cx.dir, "HEAD^{tree}")?;
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
    let state = load(cx);
    let state = match state {
        Some(s) if s.tree == tree => s,
        Some(s) => return queue_round(store, cx, &sweep, &base, &tree, s.round + 1),
        None => return queue_round(store, cx, &sweep, &base, &tree, 1),
    };
    // The round of this tree: wait for its reviewers.
    let mut pending = Vec::new();
    for (_, id, _) in &state.reviews {
        let t = tasks::get(store.conn(), id)?;
        if !matches!(t.status.as_str(), "done" | "cancelled") {
            pending.push(t.short());
        }
    }
    if !pending.is_empty() {
        return Ok(Outcome::waiting(
            &format!(
                "round {}: waiting for reviewer tasks {}. End your turn; horch tells you when they are in",
                state.round,
                pending.join(", ")
            ),
            state.reviews.iter().map(|(_, id, _)| id.clone()).collect(),
        )
        .judged(Some(tree)));
    }
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
    let missing = table.iter().any(|r| r.contains("| MISSING |"));
    let critical: Vec<&Finding> = findings
        .iter()
        .filter(|f| f.severity == "Critical")
        .collect();
    let open_important: Vec<&Finding> = findings
        .iter()
        .filter(|f| f.severity == "Important" && !answered(&f.id))
        .collect();
    let overall = if !critical.is_empty() || missing {
        "FAIL"
    } else if findings.iter().any(|f| f.severity == "Important") {
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
                "a reviewer wrote no findings file; read {}",
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
}
