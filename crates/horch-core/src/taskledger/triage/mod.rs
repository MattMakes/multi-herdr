//! Worktree triage (#308): every worktree that no step will take to an
//! outcome, with the facts an operator needs to decide about it.
//!
//! [`collect`] lists the parked rows: ledger worktrees that no live worker
//! holds and that are not `cleaned`, `archived` or `trashed`, and folders
//! under `.worktrees/` that git knows and the ledger does not. Each row gets
//! an ID (`W1`, `W2`, ...) in name order. The decisions are applied in
//! [`apply`]; this module only reads.

pub mod apply;
mod facts;
pub mod refs;
pub mod reply;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::Result;
use rusqlite::Connection;
use serde::Serialize;

use super::git::Git;
use super::model::{TaskStatus, WorkerStatus};
use super::store::optional;

pub use facts::{Age, Diff, Disk, Uncommitted};

/// How much [`collect`] measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Measure {
    /// Every fact: git history, the diff, the disk.
    Full,
    /// Only which rows exist, their tips and branches: for a count, or for
    /// `apply`.
    Ids,
}

/// 1 parked worktree.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Row {
    /// `W1`, `W2`, ... in name order.
    pub id: String,
    /// The folder's path under `.worktrees/`, or its last component.
    pub name: String,
    /// `ledger` (a ledger row) or `folder` (git knows it, the ledger does not).
    pub origin: String,
    pub worktree_id: Option<String>,
    /// The ledger status.
    pub status: Option<String>,
    /// The decision recorded at an earlier triage (`KEEP` rows come back).
    pub decision: Option<String>,
    /// `T-<seq> <title>`.
    pub task: Option<String>,
    pub task_id: Option<String>,
    pub run_id: Option<String>,
    /// The open `import_findings` rows about this worktree.
    pub findings: Vec<i64>,
    /// `None` for a detached HEAD.
    pub branch: Option<String>,
    pub path: String,
    pub base: String,
    /// The commit the work is at; `None` when the branch and folder are gone.
    pub tip: Option<String>,
    pub on_disk: bool,
    pub age: Age,
    pub disk: Option<Disk>,
    pub ahead: usize,
    pub behind: usize,
    pub diff: Diff,
    pub uncommitted: Uncommitted,
    /// Every commit has a patch-equivalent commit in base.
    pub superseded: bool,
    /// Conflicting files of a merge into base; `None` when git cannot tell.
    pub conflicts: Option<usize>,
    /// The newest finish step and its result.
    pub last_check: Option<String>,
    pub why_parked: Option<String>,
}

/// What a ledger row says, before the git facts.
struct LedgerRow {
    worktree_id: String,
    repo_root: String,
    path: String,
    branch: String,
    base: String,
    status: String,
    decision: Option<String>,
    created_at: String,
    cleaned_at: Option<String>,
    reason: Option<String>,
    worker_id: Option<String>,
    task_id: Option<String>,
    task: Option<(i64, String, String, Option<String>)>,
    run_id: Option<String>,
}

/// The statuses of a worktree that is done with, one way or another.
pub const SETTLED: [&str; 3] = ["cleaned", "archived", "trashed"];

fn ledger_rows(conn: &Connection) -> Result<Vec<LedgerRow>> {
    let mut stmt = conn.prepare(
        "SELECT w.worktree_id, w.path, w.branch, w.base, w.status, w.decision, w.created_at,
                w.cleaned_at, w.reason, w.worker_id, w.task_id, t.seq, t.title, t.status, t.summary,
                COALESCE(t.run_id, m.run_id), w.repo_root
         FROM worktrees w LEFT JOIN tasks t ON t.task_id = w.task_id
         LEFT JOIN worktree_modes m ON m.worktree_id = w.worktree_id
         WHERE w.status NOT IN ('cleaned', 'archived', 'trashed')",
    )?;
    let rows = stmt
        .query_map([], |r| {
            let seq: Option<i64> = r.get(11)?;
            Ok(LedgerRow {
                worktree_id: r.get(0)?,
                path: r.get(1)?,
                branch: r.get(2)?,
                base: r.get(3)?,
                status: r.get(4)?,
                decision: r.get(5)?,
                created_at: r.get(6)?,
                cleaned_at: r.get(7)?,
                reason: r.get(8)?,
                worker_id: r.get(9)?,
                task_id: r.get(10)?,
                task: match seq {
                    Some(seq) => Some((seq, r.get(12)?, r.get(13)?, r.get(14)?)),
                    None => None,
                },
                run_id: r.get(15)?,
                repo_root: r.get(16)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn canonical(p: &str) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| PathBuf::from(p))
}

/// Whether a live worker holds the row: its own worker, or its task's.
fn held_by_live_worker(conn: &Connection, row: &LedgerRow) -> Result<bool> {
    let current: Option<String> = match &row.task_id {
        Some(t) => optional(conn.query_row(
            "SELECT current_worker_id FROM tasks WHERE task_id = ?1",
            [t],
            |r| r.get(0),
        ))?
        .flatten(),
        None => None,
    };
    for id in row.worker_id.iter().chain(current.iter()) {
        let status: Option<String> = optional(conn.query_row(
            "SELECT status FROM workers WHERE worker_id = ?1",
            [id],
            |r| r.get(0),
        ))?;
        if status.is_some_and(|s| WorkerStatus::parse(&s).is_ok_and(WorkerStatus::is_live)) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether the row waits for a step that will take it on: a task that is
/// queued, running or landing has a worker coming.
fn task_pending(row: &LedgerRow) -> bool {
    row.task.as_ref().is_some_and(|(_, _, status, _)| {
        [TaskStatus::Queued, TaskStatus::Running, TaskStatus::Landing]
            .iter()
            .any(|s| s.as_str() == status)
    })
}

/// Whether nothing is left of the row to decide about: no folder, and no
/// branch (or a row the ledger already marked as cleaned up).
fn nothing_left(git: &Git, root: &Path, row: &LedgerRow) -> bool {
    !Path::new(&row.path).exists()
        && (row.cleaned_at.is_some()
            || row.branch.is_empty()
            || !git.branch_exists(root, &row.branch))
}

/// Whether a ledger row is parked: no step will take it to an outcome.
fn is_parked(conn: &Connection, git: &Git, root: &Path, row: &LedgerRow) -> Result<bool> {
    if nothing_left(git, root, row) {
        return Ok(false);
    }
    if matches!(
        row.status.as_str(),
        "unlanded" | "abandoned" | "cleanup_failed"
    ) {
        return Ok(true);
    }
    Ok(!task_pending(row) && !held_by_live_worker(conn, row)?)
}

/// The name of a worktree folder: its path under `<root>/.worktrees/`, else
/// its last component.
pub fn name_of(root: &Path, path: &Path) -> String {
    let under = root.join(".worktrees");
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let under = std::fs::canonicalize(&under).unwrap_or(under);
    match path.strip_prefix(&under) {
        Ok(rel) if !rel.as_os_str().is_empty() => rel.to_string_lossy().into_owned(),
        _ => path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
    }
}

/// 1 entry of `git worktree list --porcelain`.
#[derive(Debug, Clone)]
struct Listed {
    path: String,
    branch: Option<String>,
}

fn listed_worktrees(git: &Git, root: &Path) -> Vec<Listed> {
    let Ok(out) = git.try_run(root, &["worktree", "list", "--porcelain"]) else {
        return Vec::new();
    };
    let mut entries: Vec<Listed> = Vec::new();
    for line in out.stdout.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            entries.push(Listed {
                path: p.to_string(),
                branch: None,
            });
        } else if let (Some(b), Some(last)) = (line.strip_prefix("branch "), entries.last_mut()) {
            last.branch = Some(b.trim_start_matches("refs/heads/").to_string());
        }
    }
    entries
}

/// Folders under `<root>/.worktrees/` that git knows and no unsettled ledger
/// row names.
fn folder_rows(git: &Git, root: &Path, known: &HashSet<PathBuf>) -> Vec<Listed> {
    let under = canonical(&root.join(".worktrees").to_string_lossy());
    listed_worktrees(git, root)
        .into_iter()
        .filter(|l| {
            let p = canonical(&l.path);
            p.starts_with(&under) && p.exists() && !known.contains(&p)
        })
        .collect()
}

/// The base a folder row is measured against: the default branch.
pub fn default_branch(git: &Git, root: &Path) -> Option<String> {
    let remote_head = git
        .try_run(
            root,
            &[
                "symbolic-ref",
                "--quiet",
                "--short",
                "refs/remotes/origin/HEAD",
            ],
        )
        .ok()
        .filter(|o| o.ok)
        .and_then(|o| o.stdout.strip_prefix("origin/").map(str::to_string));
    remote_head
        .into_iter()
        .chain(["main".to_string(), "master".to_string()])
        .find(|b| git.branch_exists(root, b))
        .or_else(|| git.current_branch(root))
}

/// The open findings about the worktree `id` at `path`.
fn findings_of(conn: &Connection, id: Option<&str>, path: &str) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare(
        "SELECT finding_id FROM import_findings WHERE status = 'open'
           AND ((?1 IS NOT NULL AND worktree_id = ?1)
                OR (kind = 'worktree_orphan' AND detail LIKE ?2))
         ORDER BY finding_id",
    )?;
    let ids = stmt
        .query_map(rusqlite::params![id, format!("{path} %")], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<i64>>>()?;
    Ok(ids)
}

/// The newest finish step of `task_id`, as `<step> <status>`.
fn last_check(conn: &Connection, task_id: Option<&str>) -> Result<Option<String>> {
    let Some(task) = task_id else {
        return Ok(None);
    };
    optional(conn.query_row(
        "SELECT name || ' ' || status FROM task_steps WHERE task_id = ?1
         ORDER BY COALESCE(ended_at, started_at) DESC, seq DESC LIMIT 1",
        [task],
        |r| r.get(0),
    ))
}

/// Why the row is parked: its reason, its task's summary, its worker's end.
fn why_parked(conn: &Connection, row: &LedgerRow) -> Result<Option<String>> {
    let worker: Option<(Option<String>, Option<String>)> = match &row.worker_id {
        Some(w) => optional(conn.query_row(
            "SELECT end_reason, summary FROM workers WHERE worker_id = ?1",
            [w],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ))?,
        None => None,
    };
    let (end, summary) = worker.unwrap_or_default();
    let parts: Vec<String> = [
        row.reason.clone(),
        row.task.as_ref().and_then(|t| t.3.clone()),
        end.map(|e| format!("worker: {e}")),
        summary.map(|s| format!("summary: {s}")),
    ]
    .into_iter()
    .flatten()
    .filter(|s| !s.trim().is_empty())
    .collect();
    Ok((!parts.is_empty()).then(|| parts.join("; ")))
}

/// A row before its ID and git facts.
fn ledger_row_of(conn: &Connection, root: &Path, l: &LedgerRow) -> Result<Row> {
    let mut row = Row::blank(root, &l.path, &l.base);
    row.origin = "ledger".into();
    row.worktree_id = Some(l.worktree_id.clone());
    row.status = Some(l.status.clone());
    row.decision = l.decision.clone();
    row.task = l
        .task
        .as_ref()
        .map(|(seq, title, _, _)| format!("T-{seq} {title}"));
    row.task_id = l.task_id.clone();
    row.run_id = l.run_id.clone();
    row.findings = findings_of(conn, Some(&l.worktree_id), &l.path)?;
    row.branch = (!l.branch.is_empty()).then(|| l.branch.clone());
    row.age.created_at = Some(l.created_at.clone());
    row.last_check = last_check(conn, l.task_id.as_deref())?;
    row.why_parked = why_parked(conn, l)?;
    Ok(row)
}

fn folder_row_of(conn: &Connection, root: &Path, l: &Listed, base: &str) -> Result<Row> {
    let mut row = Row::blank(root, &l.path, base);
    row.origin = "folder".into();
    row.branch = l.branch.clone();
    row.findings = findings_of(conn, None, &l.path)?;
    row.age.created_at = facts::created_at(Path::new(&l.path));
    row.why_parked = Some("a folder under .worktrees/ that no ledger row names".into());
    Ok(row)
}

impl Row {
    fn blank(root: &Path, path: &str, base: &str) -> Row {
        Row {
            id: String::new(),
            name: name_of(root, Path::new(path)),
            origin: String::new(),
            worktree_id: None,
            status: None,
            decision: None,
            task: None,
            task_id: None,
            run_id: None,
            findings: Vec::new(),
            branch: None,
            path: path.to_string(),
            base: base.to_string(),
            tip: None,
            on_disk: Path::new(path).exists(),
            age: Age::default(),
            disk: None,
            ahead: 0,
            behind: 0,
            diff: Diff::default(),
            uncommitted: Uncommitted::default(),
            superseded: false,
            conflicts: None,
            last_check: None,
            why_parked: None,
        }
    }
}

/// The parked ledger rows of the repository at `root`.
fn parked_ledger_rows(
    conn: &Connection,
    git: &Git,
    root: &Path,
) -> Result<(Vec<Row>, HashSet<PathBuf>)> {
    let root_key = canonical(&root.to_string_lossy());
    let mut known = HashSet::new();
    let mut rows = Vec::new();
    for l in ledger_rows(conn)? {
        if canonical(&l.repo_root) != root_key {
            continue;
        }
        known.insert(canonical(&l.path));
        if is_parked(conn, git, root, &l)? {
            rows.push(ledger_row_of(conn, root, &l)?);
        }
    }
    Ok((rows, known))
}

/// Every parked worktree of the repository at `root` (its main checkout),
/// in name order with IDs `W1`, `W2`, ...
pub fn collect(conn: &Connection, git: &Git, root: &Path, measure: Measure) -> Result<Vec<Row>> {
    let (mut rows, known) = parked_ledger_rows(conn, git, root)?;
    let base = default_branch(git, root).unwrap_or_else(|| "HEAD".into());
    for l in folder_rows(git, root, &known) {
        rows.push(folder_row_of(conn, root, &l, &base)?);
    }
    rows.sort_by(|a, b| a.name.cmp(&b.name).then(a.path.cmp(&b.path)));
    for (i, row) in rows.iter_mut().enumerate() {
        row.id = format!("W{}", i + 1);
        facts::measure(git, root, row, measure);
    }
    Ok(rows)
}

/// [`collect`] on a machine with no ledger file: folder rows only. The scan
/// does not create the file (#199).
pub fn collect_without_ledger(git: &Git, root: &Path, measure: Measure) -> Result<Vec<Row>> {
    let mut conn = Connection::open_in_memory()?;
    super::schema::migrate(&mut conn)?;
    collect(&conn, git, root, measure)
}

/// The total bytes on disk of `rows` (measured rows only).
pub fn total_bytes(rows: &[Row]) -> u64 {
    rows.iter()
        .filter_map(|r| r.disk.as_ref())
        .map(|d| d.bytes)
        .sum()
}

/// `--json`: `{"rows": [...]}`.
pub fn to_json(rows: &[Row]) -> String {
    serde_json::to_string_pretty(&serde_json::json!({ "rows": rows })).unwrap_or_default()
}

/// A byte count for people: `1.2 GB`, `340 MB`, `12 KB`.
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [(&str, u64); 3] = [("GB", 1 << 30), ("MB", 1 << 20), ("KB", 1 << 10)];
    UNITS
        .iter()
        .find(|(_, size)| bytes >= *size)
        .map(|(unit, size)| format!("{:.1} {unit}", bytes as f64 / *size as f64))
        .unwrap_or_else(|| format!("{bytes} B"))
}

/// 1 line of facts per row, then the reply template.
pub fn render(rows: &[Row]) -> String {
    if rows.is_empty() {
        return "No parked worktrees.\n".to_string();
    }
    let mut out = String::new();
    for r in rows {
        out.push_str(&render_row(r));
    }
    out.push('\n');
    out.push_str(&reply::template(rows));
    out
}

fn render_row(r: &Row) -> String {
    let disk = r
        .disk
        .as_ref()
        .map(|d| {
            format!(
                "{} ({} ignored)",
                human_bytes(d.bytes),
                human_bytes(d.ignored_bytes)
            )
        })
        .unwrap_or_else(|| "not on disk".into());
    let u = &r.uncommitted;
    let mut out = format!(
        "{} {} [{}{}] branch {} base {}\n    {} ahead, {} behind; {} files +{} -{} ({}); uncommitted: {} modified, {} staged, {} untracked{}\n    disk {disk}; {}; conflicts {}; last commit {}\n",
        r.id,
        r.name,
        r.origin,
        r.status.as_deref().map(|s| format!(" {s}")).unwrap_or_default(),
        r.branch.as_deref().unwrap_or("(detached)"),
        r.base,
        r.ahead,
        r.behind,
        r.diff.files,
        r.diff.insertions,
        r.diff.deletions,
        r.diff.top_paths.join(", "),
        u.modified,
        u.staged,
        u.untracked,
        if u.largest_untracked.is_empty() {
            String::new()
        } else {
            format!(" ({})", u.largest_untracked.join(", "))
        },
        if r.superseded { "superseded" } else { "not in base" },
        r.conflicts.map(|c| c.to_string()).unwrap_or_else(|| "unknown".into()),
        r.age.last_commit_at.as_deref().unwrap_or("none"),
    );
    for (label, value) in [
        ("task", &r.task),
        ("last check", &r.last_check),
        ("why parked", &r.why_parked),
        ("decided before", &r.decision),
    ] {
        if let Some(v) = value {
            out.push_str(&format!("    {label}: {v}\n"));
        }
    }
    out
}
