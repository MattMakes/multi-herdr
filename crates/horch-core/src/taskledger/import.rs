//! Importing the old JSON ledgers (section 11, #43), and the list of work
//! that never landed (`horch ledger unfinished`).
//!
//! Stage 1 copies every old record unchanged into `import_raw`. Stage 2
//! maps each unmapped raw row to typed rows with `origin = 'import'`, where
//! required fields are not enforced. A row that cannot be mapped keeps its
//! raw copy with `map_error`. The source files are never changed, and a
//! second import skips what it already has.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{params, Connection, Row};
use serde::Serialize;
use serde_json::Value;

use super::git::{dir_bytes, Git};
use super::model::{FindingKind, SourceKind};
use super::runs;
use super::store::{optional, Store};
use super::util;

/// A worktree over this many bytes is a `disk_heavy` finding.
pub const HEAVY_BYTES: u64 = 1 << 30;

/// What 1 import did (or would do, with `dry_run`).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ImportReport {
    pub dry_run: bool,
    pub files: Vec<String>,
    pub raw_added: i64,
    pub raw_skipped: i64,
    pub workers: i64,
    pub tasks: i64,
    pub worktrees: i64,
    pub map_errors: i64,
    pub findings: BTreeMap<String, i64>,
}

/// The old files under the state root: session ledgers, worktree claim
/// registries, pending mail.
pub fn sources(state_root: &Path) -> Vec<(SourceKind, PathBuf)> {
    let mut out = Vec::new();
    let json_in = |dir: &Path| -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "json"))
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    };
    for p in json_in(state_root) {
        // A session ledger is an array of records with a record_id.
        let is_ledger = std::fs::read(&p)
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .is_some_and(|v| {
                v.as_array()
                    .is_some_and(|a| a.first().is_some_and(|r| r.get("record_id").is_some()))
            });
        if is_ledger {
            out.push((SourceKind::SessionLedger, p));
        }
    }
    for p in json_in(&state_root.join("worktrees")) {
        out.push((SourceKind::WorktreeClaims, p));
    }
    if let Ok(rd) = std::fs::read_dir(state_root.join("mail")) {
        let mut ws: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        ws.sort();
        for w in ws {
            for p in json_in(&w.join("pending")) {
                out.push((SourceKind::PendingMail, p));
            }
        }
    }
    out
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key)
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
}

/// The key a record is stored under, when it has one.
fn record_key(kind: SourceKind, path: &Path, r: &Value) -> Option<String> {
    match kind {
        SourceKind::SessionLedger => str_of(r, "record_id").map(str::to_string),
        SourceKind::WorktreeClaims => str_of(r, "id").map(str::to_string),
        SourceKind::PendingMail => str_of(r, "id")
            .map(str::to_string)
            .or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned())),
    }
}

/// The records of an old file: an array, or 1 record.
fn read_records(path: &Path) -> Result<Vec<Value>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let v: Value =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    Ok(match v {
        Value::Array(a) => a,
        other => vec![other],
    })
}

/// Stage 1: copy each record of `path` into `import_raw`.
fn stage1(
    conn: &Connection,
    kind: SourceKind,
    path: &Path,
    report: &mut ImportReport,
) -> Result<()> {
    let records = read_records(path)?;
    for (key, r) in records
        .iter()
        .filter_map(|r| record_key(kind, path, r).map(|k| (k, r)))
    {
        let n = conn.execute(
            "INSERT OR IGNORE INTO import_raw (source_kind, source_path, source_key, raw_json, imported_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![kind.as_str(), path.to_string_lossy(), key, r.to_string(), util::now()],
        )?;
        if n > 0 {
            report.raw_added += 1;
        } else {
            report.raw_skipped += 1;
        }
    }
    Ok(())
}

fn finding(
    conn: &Connection,
    report: &mut ImportReport,
    kind: FindingKind,
    project: Option<&str>,
    run: Option<&str>,
    ids: (Option<&str>, Option<&str>, Option<&str>),
    detail: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO import_findings (project_id, run_id, kind, task_id, worker_id, worktree_id,
            detail, status, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'open', ?8)",
        params![
            project,
            run,
            kind.as_str(),
            ids.0,
            ids.1,
            ids.2,
            detail,
            util::now()
        ],
    )?;
    *report
        .findings
        .entry(kind.as_str().to_string())
        .or_default() += 1;
    Ok(())
}

/// The synthetic, finished run that holds a project's imported records of
/// 1 workspace.
fn import_run(
    conn: &Connection,
    project: &Path,
    workspace: Option<&str>,
) -> Result<(String, String)> {
    let project_id = runs::ensure_project(conn, project)?;
    let slug = format!("imported-{}", workspace.unwrap_or("no-workspace"));
    if let Some(id) = optional(conn.query_row(
        "SELECT run_id FROM runs WHERE project_id = ?1 AND slug = ?2 AND origin = 'import'",
        params![project_id, slug],
        |r| r.get::<_, String>(0),
    ))? {
        return Ok((project_id, id));
    }
    let id = util::new_id();
    conn.execute(
        "INSERT INTO runs (run_id, project_id, slug, workspace_id, finish_steps, status, origin,
            started_at, ended_at)
         VALUES (?1, ?2, ?3, ?4, '[]', 'done', 'import', ?5, ?5)",
        params![id, project_id, slug, workspace, util::now()],
    )?;
    Ok((project_id, id))
}

/// What a session-ledger record says, read once.
struct Session<'a> {
    done_text: Option<String>,
    w_status: &'static str,
    t_status: &'static str,
    ended_without_done: bool,
    summary: Option<String>,
    tier: Option<&'a str>,
    title: String,
    task_file: Option<String>,
    created: String,
    updated: String,
    finished: Option<String>,
}

/// The text of the last history event named `ev`.
fn last_event_text(history: &[Value], ev: &str) -> Option<String> {
    history
        .iter()
        .rev()
        .find(|e| e.get("event").and_then(|x| x.as_str()) == Some(ev))
        .and_then(|e| e.get("text").and_then(|x| x.as_str()))
        .map(str::to_string)
}

/// (worker status, task status, ended without `horch done`) of a record.
fn session_statuses(
    typed: Option<&str>,
    legacy: &str,
    done: &Option<String>,
) -> (&'static str, &'static str, bool) {
    match (typed, legacy, done) {
        (Some("launch_failed"), _, _) => ("launch_failed", "cancelled", false),
        (_, "done", Some(_)) => ("done", "done", false),
        _ => ("incomplete", "cancelled", true),
    }
}

impl<'a> Session<'a> {
    fn read(r: &'a Value, project: &str) -> Session<'a> {
        let history: Vec<Value> = r
            .get("history")
            .and_then(|h| h.as_array())
            .cloned()
            .unwrap_or_default();
        let done_text = last_event_text(&history, "done");
        let typed = r
            .get("state")
            .and_then(|s| s.get("state"))
            .and_then(|s| s.as_str());
        let legacy = str_of(r, "status").unwrap_or("working");
        let (w_status, t_status, ended_without_done) = session_statuses(typed, legacy, &done_text);
        let summary = done_text.clone().or_else(|| {
            ended_without_done.then(|| "imported: ended without horch done".to_string())
        });
        let task_text = str_of(r, "task").unwrap_or("(no task)");
        let task_file = str_of(r, "plan").and_then(|p| {
            let rel = format!("ai_docs/plans/{p}.md");
            Path::new(project).join(&rel).is_file().then_some(rel)
        });
        let created = str_of(r, "created_at")
            .map(str::to_string)
            .unwrap_or_else(util::now);
        let updated = str_of(r, "updated_at")
            .map(str::to_string)
            .unwrap_or_else(|| created.clone());
        Session {
            done_text,
            w_status,
            t_status,
            ended_without_done,
            summary,
            tier: str_of(r, "tier"),
            title: task_text.chars().take(200).collect(),
            task_file,
            created,
            updated,
            finished: str_of(r, "finished_at").map(str::to_string),
        }
    }
}

/// Map 1 session-ledger record.
fn map_session(
    conn: &Connection,
    raw_id: i64,
    r: &Value,
    report: &mut ImportReport,
) -> Result<String> {
    if str_of(r, "kind") == Some("orchestrator") {
        return Ok("skipped:orchestrator".into());
    }
    let project = str_of(r, "project").context("the record names no project")?;
    let (project_id, run_id) = import_run(conn, Path::new(project), str_of(r, "workspace_id"))?;
    let session = Session::read(r, project);
    let task_id = insert_task(conn, r, &run_id, raw_id, &session)?;
    report.tasks += 1;
    let worker_id = insert_worker(conn, r, (&run_id, &task_id), raw_id, &session)?;
    report.workers += 1;
    let ids = Mapped {
        project: &project_id,
        run: &run_id,
        task: &task_id,
        worker: &worker_id,
    };
    session_findings(conn, report, r, &ids, &session)?;
    Ok(format!("workers:{worker_id}"))
}

/// The rows a session record became.
struct Mapped<'a> {
    project: &'a str,
    run: &'a str,
    task: &'a str,
    worker: &'a str,
}

/// Insert the task of a session record; give back its id.
fn insert_task(
    conn: &Connection,
    r: &Value,
    run_id: &str,
    raw_id: i64,
    s: &Session<'_>,
) -> Result<String> {
    let seq: i64 = conn.query_row(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM tasks WHERE run_id = ?1",
        [run_id],
        |x| x.get(0),
    )?;
    let task_id = util::new_id();
    conn.execute(
        "INSERT INTO tasks (task_id, run_id, seq, title, task_file, primary_persona, claimable_by,
            phase, status, attempts, summary, origin, import_raw_id, created_at, updated_at, finished_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?7, ?8, 1, ?9, 'import', ?10, ?11, ?12, ?13)",
        params![
            task_id,
            run_id,
            seq,
            s.title,
            s.task_file,
            s.tier,
            str_of(r, "phase"),
            s.t_status,
            s.summary,
            raw_id,
            s.created,
            s.updated,
            s.finished.clone().or(Some(s.updated.clone()))
        ],
    )?;
    Ok(task_id)
}

/// Insert the worker of a session record; give back its id.
fn insert_worker(
    conn: &Connection,
    r: &Value,
    (run_id, task_id): (&str, &str),
    raw_id: i64,
    s: &Session<'_>,
) -> Result<String> {
    let worker_id = util::new_id();
    conn.execute(
        "INSERT INTO workers (worker_id, run_id, kind, persona, role, harness, model, effort,
            workspace_id, pane_id, session_id, legacy_record_id, task_id, attempt, status, exit_code,
            summary, end_reason, origin, import_raw_id, spawned_at, claimed_at, ended_at)
         VALUES (?1, ?2, 'worker', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 1, ?13, ?14, ?15, ?16,
            'import', ?17, ?18, ?18, ?19)",
        params![
            worker_id,
            run_id,
            s.tier,
            str_of(r, "role"),
            str_of(r, "agent"),
            str_of(r, "model"),
            str_of(r, "effort"),
            str_of(r, "workspace_id"),
            str_of(r, "pane_id"),
            str_of(r, "session_id"),
            str_of(r, "record_id"),
            task_id,
            s.w_status,
            r.get("exit_code").and_then(|x| x.as_i64()),
            s.summary,
            if s.ended_without_done { Some("ended without horch done") } else { None },
            raw_id,
            s.created,
            s.finished.clone().or(Some(s.updated.clone()))
        ],
    )?;
    Ok(worker_id)
}

/// The findings of a session record: it ended without `horch done`, or it
/// reported done with work that did not land.
fn session_findings(
    conn: &Connection,
    report: &mut ImportReport,
    r: &Value,
    ids: &Mapped<'_>,
    s: &Session<'_>,
) -> Result<()> {
    let about = (Some(ids.task), Some(ids.worker), None);
    if s.ended_without_done {
        finding(
            conn,
            report,
            FindingKind::EndedWithoutDone,
            Some(ids.project),
            Some(ids.run),
            about,
            &format!(
                "{} ({}) ended without horch done. Task: {}. Session: {}.",
                str_of(r, "role").unwrap_or("?"),
                s.tier.unwrap_or("?"),
                s.title,
                str_of(r, "session_id").unwrap_or("(unknown)")
            ),
        )?;
    }
    let unlanded = s.done_text.as_deref().filter(|t| t.contains("UNLANDED:"));
    if let Some(text) = unlanded {
        finding(
            conn,
            report,
            FindingKind::DoneNotLanded,
            Some(ids.project),
            Some(ids.run),
            about,
            &format!(
                "{} reported done, but its work did not land: {text}",
                str_of(r, "role").unwrap_or("?"),
            ),
        )?;
    }
    Ok(())
}

/// (worker id, task id, run id, worker status) of an imported record.
type MappedRecord = (String, String, String, Option<String>);

/// The task and worker an imported record became, by its legacy record id.
fn mapped_record(conn: &Connection, record_id: &str) -> Result<Option<MappedRecord>> {
    optional(conn.query_row(
        "SELECT w.worker_id, w.task_id, w.run_id, w.status FROM workers w WHERE w.legacy_record_id = ?1",
        [record_id],
        |r| Ok((r.get(0)?, r.get::<_, String>(1)?, r.get(2)?, r.get(3)?)),
    ))
}

/// The repository a claim's worktree belongs to.
fn claim_repo(git: &Git, dir: &Path) -> Option<PathBuf> {
    git.main_root(dir).or_else(|| {
        // The worktree is gone: its repo is the folder above .worktrees.
        dir.ancestors()
            .find(|a| a.file_name().is_some_and(|n| n == ".worktrees"))
            .and_then(|w| w.parent())
            .map(Path::to_path_buf)
    })
}

/// The status a claim's worktree has in the ledger.
fn claim_status(state: &str, exists: bool) -> &'static str {
    match (state, exists) {
        ("landed", false) => "cleaned",
        ("landed", true) => "landed",
        ("unlanded", _) => "unlanded",
        (_, false) => "abandoned",
        _ => "open",
    }
}

/// (commits not on the base, uncommitted files) of a claim's worktree.
fn claim_measures(
    git: &Git,
    repo: &Path,
    dir: &Path,
    (branch, base): (&str, &str),
    exists: bool,
) -> (usize, usize) {
    let ahead = if !branch.is_empty() && !base.is_empty() && git.branch_exists(repo, branch) {
        git.commits_missing_from(repo, branch, base)
            .map(|v| v.len())
            .unwrap_or(0)
    } else {
        0
    };
    let dirty = if exists {
        git.dirty(dir).map(|d| d.len()).unwrap_or(0)
    } else {
        0
    };
    (ahead, dirty)
}

/// What a claim's worktree and its disk say, for the findings.
struct ClaimFacts<'a> {
    status: &'static str,
    ahead: usize,
    dirty: usize,
    bytes: Option<u64>,
    project_id: String,
    run_id: Option<String>,
    ids: (Option<&'a str>, Option<&'a str>, Option<&'a str>),
    /// The record's worker reported done.
    record_done: bool,
    /// The one-line description every finding of the claim carries.
    text: String,
}

/// The kinds of finding a claim raises, in order.
fn claim_kinds(f: &ClaimFacts<'_>) -> Vec<FindingKind> {
    let mut kinds = Vec::new();
    if matches!(f.status, "open" | "unlanded" | "abandoned") && f.ahead > 0 {
        kinds.push(FindingKind::WorktreeUnlanded);
        if f.record_done {
            kinds.push(FindingKind::DoneNotLanded);
        }
    }
    if f.dirty > 0 {
        kinds.push(FindingKind::DirtyWorktree);
    }
    if f.bytes.is_some_and(|b| b > HEAVY_BYTES) && f.status != "cleaned" {
        kinds.push(FindingKind::DiskHeavy);
    }
    kinds
}

/// The findings of a claim: unlanded commits, dirty files, a heavy folder.
fn claim_findings(conn: &Connection, report: &mut ImportReport, f: &ClaimFacts<'_>) -> Result<()> {
    for kind in claim_kinds(f) {
        finding(
            conn,
            report,
            kind,
            Some(&f.project_id),
            f.run_id.as_deref(),
            f.ids,
            &f.text,
        )?;
    }
    Ok(())
}

/// Insert the worktree row of a claim; the path is unique, so a claim seen
/// twice keeps its first row.
fn insert_worktree(
    conn: &Connection,
    c: &Value,
    (id, raw_id): (&str, i64),
    record: Option<&MappedRecord>,
    (repo, path): (&Path, &str),
    (status, bytes): (&str, Option<u64>),
) -> Result<()> {
    conn.execute(
        "INSERT INTO worktrees (worktree_id, task_id, worker_id, repo_root, path, branch, base,
            base_commit, status, landed_commit, bytes_on_disk, origin, import_raw_id, created_at, reason)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'import', ?12, ?13, ?14)
         ON CONFLICT(path) DO NOTHING",
        params![
            id,
            record.map(|(_, t, _, _)| t),
            record.map(|(w, _, _, _)| w),
            repo.to_string_lossy(),
            path,
            str_of(c, "branch").unwrap_or(""),
            str_of(c, "base").unwrap_or(""),
            str_of(c, "base_commit"),
            status,
            str_of(c, "landed_commit"),
            bytes.map(|b| b as i64),
            raw_id,
            str_of(c, "created_at").map(str::to_string).unwrap_or_else(util::now),
            str_of(c, "reason")
        ],
    )?;
    Ok(())
}

/// Map 1 worktree claim, and record what the disk says about it.
fn map_claim(
    conn: &Connection,
    git: &Git,
    raw_id: i64,
    c: &Value,
    report: &mut ImportReport,
) -> Result<String> {
    let path = str_of(c, "path").context("the claim has no path")?;
    let branch = str_of(c, "branch").unwrap_or("");
    let base = str_of(c, "base").unwrap_or("");
    let dir = Path::new(path);
    let repo = claim_repo(git, dir).context("cannot tell which repository the claim belongs to")?;
    let record = str_of(c, "record_id")
        .map(|r| mapped_record(conn, r))
        .transpose()?
        .flatten();
    let exists = dir.exists();
    let status = claim_status(str_of(c, "state").unwrap_or("open"), exists);
    let id = util::new_id();
    let bytes = exists.then(|| dir_bytes(dir));
    insert_worktree(
        conn,
        c,
        (&id, raw_id),
        record.as_ref(),
        (&repo, path),
        (status, bytes),
    )?;
    report.worktrees += 1;
    let facts = claim_facts(
        conn,
        git,
        (&repo, dir),
        record.as_ref(),
        &id,
        ((branch, base), (status, bytes)),
    )?;
    claim_findings(conn, report, &facts)?;
    Ok(format!("worktrees:{id}"))
}

/// Measure the claim's worktree and gather what its findings need. A worktree
/// is on disk exactly when `bytes` is known.
fn claim_facts<'a>(
    conn: &Connection,
    git: &Git,
    (repo, dir): (&Path, &Path),
    record: Option<&'a MappedRecord>,
    id: &'a str,
    ((branch, base), (status, bytes)): ((&str, &str), (&'static str, Option<u64>)),
) -> Result<ClaimFacts<'a>> {
    let (ahead, dirty) = claim_measures(git, repo, dir, (branch, base), bytes.is_some());
    let text = format!(
        "{} (branch {branch}, base {base}): {}, {ahead} commits not on {base}, {dirty} uncommitted files, {} bytes",
        dir.display(),
        if bytes.is_some() { "on disk" } else { "gone from disk" },
        bytes.unwrap_or(0)
    );
    Ok(ClaimFacts {
        status,
        ahead,
        dirty,
        bytes,
        project_id: runs::ensure_project(conn, repo)?,
        run_id: record.map(|(_, _, r, _)| r.clone()),
        ids: (
            record.map(|(_, t, _, _)| t.as_str()),
            record.map(|(w, _, _, _)| w.as_str()),
            Some(id),
        ),
        record_done: record.is_some_and(|(_, _, _, s)| s.as_deref() == Some("done")),
        text,
    })
}

/// The paths of the fleet worktrees `git worktree list` reports in `root`;
/// none when the git call fails.
fn fleet_worktrees(git: &Git, root: &Path) -> Vec<String> {
    git.try_run(root, &["worktree", "list", "--porcelain"])
        .ok()
        .filter(|out| out.ok)
        .map(|out| {
            out.stdout
                .lines()
                .filter_map(|l| l.strip_prefix("worktree "))
                .filter(|p| p.contains("/.worktrees/"))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Whether an orphan finding already names the worktree `p`.
fn orphan_reported(conn: &Connection, p: &str) -> Result<bool> {
    let reported: i64 = conn.query_row(
        "SELECT count(*) FROM import_findings WHERE kind = 'worktree_orphan' AND detail LIKE ?1",
        [format!("{p} %")],
        |r| r.get(0),
    )?;
    Ok(reported > 0)
}

/// Every project: (project id, root).
fn project_roots(conn: &Connection) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare("SELECT project_id, root FROM projects")?;
    let roots = stmt
        .query_map([], |r| r.try_into())?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(roots)
}

/// The canonical paths of every worktree the ledger names. Git can report a
/// worktree through a different spelling than its claim (macOS `/tmp` is
/// `/private/tmp`).
fn known_worktrees(conn: &Connection) -> Result<HashSet<PathBuf>> {
    let mut stmt = conn.prepare("SELECT path FROM worktrees")?;
    let paths = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(paths.iter().map(|p| canonical(p)).collect())
}

fn canonical(p: &str) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| PathBuf::from(p))
}

/// Folders git knows under `<repo>/.worktrees/` that no row names: reported,
/// never removed.
fn orphans(conn: &Connection, git: &Git, report: &mut ImportReport) -> Result<()> {
    let known = known_worktrees(conn)?;
    for (pid, root) in project_roots(conn)? {
        for p in fleet_worktrees(git, Path::new(&root)) {
            if known.contains(&canonical(&p)) || orphan_reported(conn, &p)? {
                continue;
            }
            let bytes = dir_bytes(Path::new(&p));
            finding(
                conn,
                report,
                FindingKind::WorktreeOrphan,
                Some(&pid),
                None,
                (None, None, None),
                &format!("{p} is a git worktree no claim names ({bytes} bytes). It is reported, never removed by the import."),
            )?;
        }
    }
    Ok(())
}

/// Map the unmapped raw rows of `kind` to typed rows. A row that cannot be
/// mapped keeps its raw copy with the error.
fn map_staged(
    tx: &Connection,
    git: &Git,
    kind: SourceKind,
    report: &mut ImportReport,
) -> Result<()> {
    let mut stmt = tx.prepare(
        "SELECT raw_id, raw_json FROM import_raw
         WHERE source_kind = ?1 AND mapped_to IS NULL AND map_error IS NULL ORDER BY raw_id",
    )?;
    let rows = stmt
        .query_map([kind.as_str()], |r| r.try_into())?
        .collect::<rusqlite::Result<Vec<(i64, String)>>>()?;
    for (raw_id, json) in rows {
        map_one(tx, git, kind, (raw_id, &json), report)?;
    }
    Ok(())
}

/// Map 1 raw row and record where it went, or why it did not.
fn map_one(
    tx: &Connection,
    git: &Git,
    kind: SourceKind,
    (raw_id, json): (i64, &str),
    report: &mut ImportReport,
) -> Result<()> {
    let v: Value = serde_json::from_str(json)?;
    let mapped = match kind {
        SourceKind::SessionLedger => map_session(tx, raw_id, &v, report),
        _ => map_claim(tx, git, raw_id, &v, report),
    };
    match mapped {
        Ok(to) => tx.execute(
            "UPDATE import_raw SET mapped_to = ?2 WHERE raw_id = ?1",
            params![raw_id, to],
        )?,
        Err(e) => {
            report.map_errors += 1;
            tx.execute(
                "UPDATE import_raw SET map_error = ?2 WHERE raw_id = ?1",
                params![raw_id, format!("{e:#}")],
            )?
        }
    };
    Ok(())
}

/// Stage 1 for every old file under `state_root`.
fn stage1_all(tx: &Connection, state_root: &Path, report: &mut ImportReport) -> Result<()> {
    for (kind, path) in sources(state_root) {
        report.files.push(path.to_string_lossy().into_owned());
        stage1(tx, kind, &path, report)?;
    }
    Ok(())
}

/// Import every old file under `state_root`. With `dry_run` nothing is kept.
pub fn import(
    store: &mut Store,
    git: &Git,
    state_root: &Path,
    dry_run: bool,
) -> Result<ImportReport> {
    let mut report = ImportReport {
        dry_run,
        ..Default::default()
    };
    let tx = store
        .conn()
        .unchecked_transaction()
        .context("starting the import")?;
    stage1_all(&tx, state_root, &mut report)?;
    // Stage 2: session records first, so claims find their records.
    for kind in [SourceKind::SessionLedger, SourceKind::WorktreeClaims] {
        map_staged(&tx, git, kind, &mut report)?;
    }
    orphans(&tx, git, &mut report)?;
    if dry_run {
        tx.rollback()?;
    } else {
        tx.commit()?;
    }
    Ok(report)
}

/// 1 open finding, as `ledger unfinished` lists it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Unfinished {
    pub finding_id: i64,
    pub kind: String,
    pub run: Option<String>,
    pub project: Option<String>,
    pub task_id: Option<String>,
    pub worktree_id: Option<String>,
    pub detail: String,
    pub created_at: String,
}

impl Unfinished {
    /// The 8 columns of the `import_findings` SELECTs, in order.
    fn from_row(r: &Row<'_>) -> rusqlite::Result<Unfinished> {
        let (finding_id, kind, run, project, task_id, worktree_id, detail, created_at) =
            r.try_into()?;
        Ok(Unfinished {
            finding_id,
            kind,
            run,
            project,
            task_id,
            worktree_id,
            detail,
            created_at,
        })
    }
}

/// Native runs feed `unfinished` too: a cancelled task that still has an
/// open worktree, and a task whose last attempt died and that no worker
/// finished. Each becomes a finding once.
pub fn sync_native(conn: &Connection) -> Result<()> {
    sync_worktrees(conn)?;
    sync_tasks(conn)
}

/// A finding for each cancelled task that still has an open worktree.
fn sync_worktrees(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT t.task_id, t.run_id, r.project_id, w.worktree_id, w.path, t.seq
         FROM tasks t JOIN runs r ON r.run_id = t.run_id JOIN worktrees w ON w.task_id = t.task_id
         WHERE t.origin = 'native' AND t.status = 'cancelled'
           AND (w.status IN ('open', 'unlanded', 'cleanup_failed')
                -- The cancel keeps the folder of the worktree it abandons (#136).
                OR (w.status = 'abandoned' AND w.cleaned_at IS NULL))
           AND NOT EXISTS (SELECT 1 FROM import_findings f WHERE f.worktree_id = w.worktree_id
                           AND f.kind = 'task_cancelled_open_worktree')",
    )?;
    let rows = stmt
        .query_map([], |r| r.try_into())?
        .collect::<rusqlite::Result<Vec<(String, String, String, String, String, i64)>>>()?;
    for (task, run, project, wt, path, seq) in rows {
        conn.execute(
            "INSERT INTO import_findings (project_id, run_id, kind, task_id, worktree_id, detail, status, created_at)
             VALUES (?1, ?2, 'task_cancelled_open_worktree', ?3, ?4, ?5, 'open', ?6)",
            params![project, run, task, wt, format!("T-{seq} was cancelled with its worktree {path} still open"), util::now()],
        )?;
    }
    Ok(())
}

/// A finding for each unfinished task of an ended run whose worker died on it.
fn sync_tasks(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT t.task_id, t.run_id, r.project_id, t.seq, t.status
         FROM tasks t JOIN runs r ON r.run_id = t.run_id
         WHERE t.origin = 'native' AND r.status <> 'active' AND t.status NOT IN ('done', 'cancelled')
           AND EXISTS (SELECT 1 FROM workers w WHERE w.task_id = t.task_id AND w.status = 'incomplete')
           AND NOT EXISTS (SELECT 1 FROM import_findings f WHERE f.task_id = t.task_id
                           AND f.kind = 'reaped_never_finished')",
    )?;
    let rows = stmt
        .query_map([], |r| r.try_into())?
        .collect::<rusqlite::Result<Vec<(String, String, String, i64, String)>>>()?;
    for (task, run, project, seq, status) in rows {
        conn.execute(
            "INSERT INTO import_findings (project_id, run_id, kind, task_id, detail, status, created_at)
             VALUES (?1, ?2, 'reaped_never_finished', ?3, ?4, 'open', ?5)",
            params![project, run, task, format!("T-{seq} ({status}): a worker died on it and the run ended before another finished it"), util::now()],
        )?;
    }
    Ok(())
}

/// Every open finding, newest first; of 1 project when `root` is given.
pub fn unfinished(conn: &Connection, root: Option<&Path>) -> Result<Vec<Unfinished>> {
    sync_native(conn)?;
    let mut stmt = conn.prepare(
        "SELECT f.finding_id, f.kind, r.slug, p.root, f.task_id, f.worktree_id, f.detail, f.created_at
         FROM import_findings f
         LEFT JOIN runs r ON r.run_id = f.run_id
         LEFT JOIN projects p ON p.project_id = f.project_id
         WHERE f.status = 'open' AND (?1 IS NULL OR p.root = ?1)
         ORDER BY r.started_at DESC, f.finding_id DESC",
    )?;
    let rows = stmt
        .query_map([root.map(runs::root_key)], Unfinished::from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// 1 finding by id.
pub fn get_finding(conn: &Connection, id: i64) -> Result<Unfinished> {
    conn.query_row(
        "SELECT f.finding_id, f.kind, r.slug, p.root, f.task_id, f.worktree_id, f.detail, f.created_at
         FROM import_findings f LEFT JOIN runs r ON r.run_id = f.run_id
         LEFT JOIN projects p ON p.project_id = f.project_id WHERE f.finding_id = ?1",
        [id],
        Unfinished::from_row,
    )
    .with_context(|| format!("no finding {id}"))
}

/// Close a finding: `requeued` (with the new task) or `dismissed`.
pub fn decide(conn: &Connection, id: i64, status: &str, by: &str, note: &str) -> Result<()> {
    let n = conn.execute(
        "UPDATE import_findings SET status = ?2, decided_by = ?3, decided_at = ?4, decision_note = ?5
         WHERE finding_id = ?1 AND status = 'open'",
        params![id, status, by, util::now(), note],
    )?;
    if n == 0 {
        anyhow::bail!("finding {id} is not open");
    }
    Ok(())
}

/// The earlier sessions of `task_id`, 1 indented line each.
fn earlier_sessions(conn: &Connection, task_id: &str) -> Result<String> {
    let mut stmt = conn.prepare(
        "SELECT transcript_path, session_id, summary FROM workers WHERE task_id = ?1 ORDER BY attempt",
    )?;
    let rows = stmt
        .query_map([task_id], |r| r.try_into())?
        .collect::<rusqlite::Result<Vec<(Option<String>, Option<String>, Option<String>)>>>()?;
    Ok(rows
        .iter()
        .map(|(tr, sid, summary)| {
            format!(
                "\n    Earlier session {}: transcript {}. Summary: {}",
                sid.as_deref().unwrap_or("(unknown)"),
                tr.as_deref().unwrap_or("(unknown)"),
                summary.as_deref().unwrap_or("(none)")
            )
        })
        .collect())
}

/// What a reopened task's claim shows: the finding it came from, and the
/// earlier attempt's transcript.
pub fn reopened_from(conn: &Connection, task_id: &str) -> Result<Option<String>> {
    let f: Option<(String, Option<String>)> = optional(conn.query_row(
        "SELECT detail, task_id FROM import_findings WHERE status = 'requeued' AND decision_note = ?1",
        [task_id],
        |r| r.try_into(),
    ))?;
    let Some((mut out, old_task)) = f else {
        return Ok(None);
    };
    if let Some(t) = old_task {
        out.push_str(&earlier_sessions(conn, &t)?);
    }
    Ok(Some(out))
}
