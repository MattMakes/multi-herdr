use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::git::{dir_bytes, Git};
use super::model::SourceKind;
use super::model::{
    FindingKind, FindingStatus, RunStatus, TaskStatus, WorkerStatus, WorktreeStatus,
};
use super::runs;
use super::sql_words::{self, word};
use super::store::{optional, Store};
use super::util;
use crate::fleetd::worktree_custody::adapters::sqlite::{self as trees, WorktreeState};
use crate::runtime::bins::{git_bin, BinOverrides};
use crate::runtime::context::ProcessEnv;

pub const HEAVY_BYTES: u64 = 1 << 30;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ImportReport {
    pub dry_run: bool,
    pub files: Vec<String>,
    pub raw_added: i64,
    pub raw_skipped: i64,
    pub sessions: i64,
    pub workers: i64,
    pub tasks: i64,
    pub worktrees: i64,
    pub map_errors: i64,
    pub findings: BTreeMap<String, i64>,
}

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

fn record_key(kind: SourceKind, path: &Path, r: &Value) -> Option<String> {
    match kind {
        SourceKind::SessionLedger => str_of(r, "record_id").map(str::to_string),
        SourceKind::WorktreeClaims => str_of(r, "id").map(str::to_string),
        SourceKind::PendingMail => str_of(r, "id")
            .map(str::to_string)
            .or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned())),
    }
}

fn read_records(path: &Path, sha256: &str) -> Result<Vec<Value>> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if sha256_hex(&bytes) != sha256 {
        bail!(
            "{} changed after the import facts were read",
            path.display()
        );
    }
    parse_records(path, &bytes)
}

fn parse_records(path: &Path, bytes: &[u8]) -> Result<Vec<Value>> {
    let v: Value =
        serde_json::from_slice(bytes).with_context(|| format!("parsing {}", path.display()))?;
    Ok(match v {
        Value::Array(a) => a,
        other => vec![other],
    })
}

fn stage1(
    conn: &Connection,
    kind: SourceKind,
    path: &Path,
    records: &[Value],
    report: &mut ImportReport,
) -> Result<()> {
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
        if kind == SourceKind::SessionLedger {
            report.sessions += copy_session(conn, path, &key, r)?;
        }
    }
    Ok(())
}

fn copy_session(conn: &Connection, path: &Path, record_id: &str, r: &Value) -> Result<i64> {
    let slug = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let project = str_of(r, "project").unwrap_or(&slug).to_string();
    let updated = str_of(r, "updated_at")
        .map(str::to_string)
        .unwrap_or_else(util::now);
    let n = conn.execute(
        "INSERT OR IGNORE INTO session_records
             (record_id, worker_id, project_root, slug, record_json, updated_at)
         VALUES (?1, (SELECT worker_id FROM workers
                      WHERE worker_id = ?1 OR legacy_record_id = ?1
                      ORDER BY rowid DESC LIMIT 1),
                 ?2, ?3, ?4, ?5)",
        params![record_id, project, slug, r.to_string(), updated],
    )?;
    Ok(n as i64)
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
        &format!(
            "INSERT INTO import_findings (project_id, run_id, kind, task_id, worker_id, worktree_id,
                detail, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, {open}, ?8)",
            open = word(FindingStatus::Open)
        ),
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
        &format!(
            "INSERT INTO runs (run_id, project_id, slug, workspace_id, finish_steps, status, origin,
                started_at, ended_at)
             VALUES (?1, ?2, ?3, ?4, '[]', {done}, 'import', ?5, ?5)",
            done = word(RunStatus::Done)
        ),
        params![id, project_id, slug, workspace, util::now()],
    )?;
    Ok((project_id, id))
}

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

fn last_event_text(history: &[Value], ev: &str) -> Option<String> {
    history
        .iter()
        .rev()
        .find(|e| e.get("event").and_then(|x| x.as_str()) == Some(ev))
        .and_then(|e| e.get("text").and_then(|x| x.as_str()))
        .map(str::to_string)
}

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

struct Mapped<'a> {
    project: &'a str,
    run: &'a str,
    task: &'a str,
    worker: &'a str,
}

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

type MappedRecord = (String, String, String, Option<String>);

fn mapped_record(conn: &Connection, record_id: &str) -> Result<Option<MappedRecord>> {
    optional(conn.query_row(
        "SELECT w.worker_id, w.task_id, w.run_id, w.status FROM workers w WHERE w.legacy_record_id = ?1",
        [record_id],
        |r| Ok((r.get(0)?, r.get::<_, String>(1)?, r.get(2)?, r.get(3)?)),
    ))
}

fn claim_repo(git: &Git, dir: &Path) -> Option<PathBuf> {
    git.main_root(dir).or_else(|| {
        dir.ancestors()
            .find(|a| a.file_name().is_some_and(|n| n == ".worktrees"))
            .and_then(|w| w.parent())
            .map(Path::to_path_buf)
    })
}

fn claim_status(state: &str, exists: bool) -> &'static str {
    match (state, exists) {
        ("landed", false) => "cleaned",
        ("landed", true) => "landed",
        ("unlanded", _) => "unlanded",
        (_, false) => "abandoned",
        _ => "open",
    }
}

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

struct ClaimFacts<'a> {
    status: &'static str,
    ahead: usize,
    dirty: usize,
    bytes: Option<u64>,
    project_id: String,
    run_id: Option<String>,
    ids: (Option<&'a str>, Option<&'a str>, Option<&'a str>),
    record_done: bool,
    text: String,
}

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

fn insert_worktree(
    conn: &Connection,
    c: &Value,
    (id, raw_id): (&str, i64),
    record: Option<&MappedRecord>,
    (repo, path): (&Path, &str),
    (status, bytes): (&str, Option<u64>),
) -> Result<()> {
    let state = if status == WorktreeStatus::Cleaned.as_str() {
        WorktreeState::Removed
    } else {
        WorktreeState::Orphaned
    };
    conn.execute(
        "INSERT INTO worktrees (worktree_id, task_id, worker_id, repo_root, path, branch, base,
            base_commit, status, state, landed_commit, bytes_on_disk, origin, import_raw_id,
            created_at, reason)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 'import', ?13, ?14, ?15)
         ON CONFLICT(path) WHERE state <> 'removed' DO NOTHING",
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
            state.as_str(),
            str_of(c, "landed_commit"),
            bytes.map(|b| b as i64),
            raw_id,
            str_of(c, "created_at")
                .map(str::to_string)
                .unwrap_or_else(util::now),
            str_of(c, "reason")
        ],
    )?;
    Ok(())
}

fn map_claim(
    conn: &Connection,
    facts: &ImportFacts,
    raw_id: i64,
    c: &Value,
    report: &mut ImportReport,
) -> Result<String> {
    let path = str_of(c, "path").context("the claim has no path")?;
    let branch = str_of(c, "branch").unwrap_or("");
    let base = str_of(c, "base").unwrap_or("");
    let dir = Path::new(path);
    let disk = facts
        .claim(c)
        .with_context(|| format!("the import facts have no entry for the claim {path}"))?;
    let repo = disk
        .repo
        .as_deref()
        .map(PathBuf::from)
        .context("cannot tell which repository the claim belongs to")?;
    let record = str_of(c, "record_id")
        .map(|r| mapped_record(conn, r))
        .transpose()?
        .flatten();
    let bytes = disk.bytes;
    let status = claim_status(str_of(c, "state").unwrap_or("open"), bytes.is_some());
    let id = util::new_id();
    insert_worktree(
        conn,
        c,
        (&id, raw_id),
        record.as_ref(),
        (&repo, path),
        (status, bytes),
    )?;
    report.worktrees += 1;
    let found = claim_facts(
        conn,
        (disk.ahead, disk.dirty),
        (&repo, dir),
        record.as_ref(),
        &id,
        ((branch, base), (status, bytes)),
    )?;
    claim_findings(conn, report, &found)?;
    Ok(format!("worktrees:{id}"))
}

fn claim_facts<'a>(
    conn: &Connection,
    (ahead, dirty): (usize, usize),
    (repo, dir): (&Path, &Path),
    record: Option<&'a MappedRecord>,
    id: &'a str,
    ((branch, base), (status, bytes)): ((&str, &str), (&'static str, Option<u64>)),
) -> Result<ClaimFacts<'a>> {
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

fn orphan_reported(conn: &Connection, p: &str) -> Result<bool> {
    let reported: i64 = conn.query_row(
        "SELECT count(*) FROM import_findings WHERE kind = 'worktree_orphan' AND detail LIKE ?1",
        [format!("{p} %")],
        |r| r.get(0),
    )?;
    Ok(reported > 0)
}

fn project_roots(conn: &Connection) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare("SELECT project_id, root FROM projects")?;
    let roots = stmt
        .query_map([], |r| r.try_into())?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(roots)
}

fn known_worktrees(conn: &Connection) -> Result<HashSet<PathBuf>> {
    let mut stmt = conn.prepare("SELECT path FROM worktrees WHERE state <> 'removed'")?;
    let paths = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(paths.iter().map(|p| canonical(p)).collect())
}

fn canonical(p: &str) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| PathBuf::from(p))
}

fn orphans(conn: &Connection, facts: &ImportFacts, report: &mut ImportReport) -> Result<()> {
    let known = known_worktrees(conn)?;
    for (pid, root) in project_roots(conn)? {
        let listed = facts
            .worktrees
            .get(&root)
            .map(Vec::as_slice)
            .unwrap_or_default();
        for ListedTree { path: p, bytes } in listed {
            if known.contains(&canonical(p)) || orphan_reported(conn, p)? {
                continue;
            }
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

fn map_staged(
    tx: &Connection,
    facts: &ImportFacts,
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
        map_one(tx, facts, kind, (raw_id, &json), report)?;
    }
    Ok(())
}

fn map_one(
    tx: &Connection,
    facts: &ImportFacts,
    kind: SourceKind,
    (raw_id, json): (i64, &str),
    report: &mut ImportReport,
) -> Result<()> {
    let v: Value = serde_json::from_str(json)?;
    let mapped = match kind {
        SourceKind::SessionLedger => map_session(tx, raw_id, &v, report),
        _ => map_claim(tx, facts, raw_id, &v, report),
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

fn stage1_all(
    tx: &Connection,
    state_root: &Path,
    facts: &ImportFacts,
    report: &mut ImportReport,
) -> Result<()> {
    let found = sources(state_root);
    let paths: BTreeSet<String> = found
        .iter()
        .map(|(_, p)| p.to_string_lossy().into_owned())
        .collect();
    if paths != facts.files.keys().cloned().collect() {
        bail!(
            "the old files under {} changed after the import facts were read",
            state_root.display()
        );
    }
    for (kind, path) in found {
        let name = path.to_string_lossy().into_owned();
        let records = read_records(&path, &facts.files[&name])?;
        report.files.push(name);
        stage1(tx, kind, &path, &records, report)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ImportFacts {
    pub files: BTreeMap<String, String>,
    pub claims: Vec<ClaimDisk>,
    pub worktrees: BTreeMap<String, Vec<ListedTree>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ClaimDisk {
    pub path: String,
    pub branch: String,
    pub base: String,
    pub repo: Option<String>,
    pub bytes: Option<u64>,
    pub ahead: usize,
    pub dirty: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ListedTree {
    pub path: String,
    pub bytes: u64,
}

impl ImportFacts {
    fn claim(&self, c: &Value) -> Option<&ClaimDisk> {
        let (path, branch, base) = claim_names(c);
        self.claims
            .iter()
            .find(|d| d.path == path && d.branch == branch && d.base == base)
    }
}

fn claim_names(c: &Value) -> (&str, &str, &str) {
    (
        str_of(c, "path").unwrap_or(""),
        str_of(c, "branch").unwrap_or(""),
        str_of(c, "base").unwrap_or(""),
    )
}

pub fn gather(conn: &Connection, git: &Git, state_root: &Path) -> Result<ImportFacts> {
    let mut facts = ImportFacts::default();
    let mut claims = Vec::new();
    for (kind, path) in sources(state_root) {
        let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        let name = path.to_string_lossy().into_owned();
        facts.files.insert(name, sha256_hex(&bytes));
        if kind == SourceKind::WorktreeClaims {
            claims.extend(parse_records(&path, &bytes)?);
        }
    }
    claims.extend(staged_claims(conn)?);
    for c in &claims {
        let Some(disk) = claim_disk(git, c) else {
            continue;
        };
        if facts.claim(c).is_none() {
            facts.claims.push(disk);
        }
    }
    let mut roots: BTreeSet<String> = project_roots(conn)?.into_iter().map(|(_, r)| r).collect();
    roots.extend(facts.claims.iter().filter_map(|d| d.repo.clone()));
    for root in roots {
        let listed = fleet_worktrees(git, Path::new(&root))
            .into_iter()
            .map(|path| ListedTree {
                bytes: dir_bytes(Path::new(&path)),
                path,
            })
            .collect();
        facts.worktrees.insert(root, listed);
    }
    Ok(facts)
}

fn staged_claims(conn: &Connection) -> Result<Vec<Value>> {
    let mut stmt = conn.prepare(
        "SELECT raw_json FROM import_raw
         WHERE source_kind = ?1 AND mapped_to IS NULL AND map_error IS NULL",
    )?;
    let rows = stmt
        .query_map([SourceKind::WorktreeClaims.as_str()], |r| {
            r.get::<_, String>(0)
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.iter()
        .map(|j| serde_json::from_str(j).context("parsing a staged claim"))
        .collect()
}

fn claim_disk(git: &Git, c: &Value) -> Option<ClaimDisk> {
    let (path, branch, base) = claim_names(c);
    if path.is_empty() {
        return None;
    }
    let dir = Path::new(path);
    let repo = claim_repo(git, dir);
    let exists = dir.exists();
    let (ahead, dirty) = match &repo {
        Some(repo) => claim_measures(git, repo, dir, (branch, base), exists),
        None => (0, 0),
    };
    Some(ClaimDisk {
        path: path.to_string(),
        branch: branch.to_string(),
        base: base.to_string(),
        repo: repo.map(|r| r.to_string_lossy().into_owned()),
        bytes: exists.then(|| dir_bytes(dir)),
        ahead,
        dirty,
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub fn import_from(
    conn: &Connection,
    state_root: &Path,
    facts: &ImportFacts,
    dry_run: bool,
) -> Result<ImportReport> {
    let mut report = ImportReport {
        dry_run,
        ..Default::default()
    };
    stage1_all(conn, state_root, facts, &mut report)?;
    for kind in [SourceKind::SessionLedger, SourceKind::WorktreeClaims] {
        map_staged(conn, facts, kind, &mut report)?;
    }
    orphans(conn, facts, &mut report)?;
    Ok(report)
}

pub fn import(
    store: &mut Store,
    git: &Git,
    state_root: &Path,
    dry_run: bool,
) -> Result<ImportReport> {
    let facts = gather(store.conn(), git, state_root)?;
    let tx = store
        .conn()
        .unchecked_transaction()
        .context("starting the import")?;
    let report = import_from(&tx, state_root, &facts, dry_run)?;
    if dry_run {
        tx.rollback()?;
    } else {
        tx.commit()?;
    }
    Ok(report)
}

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

pub fn sync_native(conn: &Connection) -> Result<()> {
    sync_worktrees(conn)?;
    sync_custody_trees(conn)?;
    sync_tasks(conn)
}

fn sync_worktrees(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare(&cancelled_open_trees_sql())?;
    let rows = stmt
        .query_map([], |r| r.try_into())?
        .collect::<rusqlite::Result<Vec<(String, String, String, String, String, i64)>>>()?;
    for (task, run, project, wt, path, seq) in rows {
        conn.execute(
            &format!(
                "INSERT INTO import_findings (project_id, run_id, kind, task_id, worktree_id, detail, status, created_at)
                 VALUES (?1, ?2, 'task_cancelled_open_worktree', ?3, ?4, ?5, {open}, ?6)",
                open = word(FindingStatus::Open)
            ),
            params![project, run, task, wt, format!("T-{seq} was cancelled with its worktree {path} still open"), util::now()],
        )?;
    }
    Ok(())
}

fn cancelled_open_trees_sql() -> String {
    format!(
        "SELECT t.task_id, t.run_id, r.project_id, w.worktree_id, w.path, t.seq
         FROM tasks t JOIN runs r ON r.run_id = t.run_id JOIN worktrees w ON w.task_id = t.task_id
         WHERE t.origin = 'native' AND t.status = {cancelled}
           AND (w.status IN ({unfinished})
                -- The cancel keeps the folder of the worktree it abandons (#136).
                OR (w.status = {abandoned} AND w.cleaned_at IS NULL))
           AND w.worktree_id NOT IN (SELECT f.worktree_id FROM import_findings f
                                     WHERE f.kind = 'task_cancelled_open_worktree'
                                       AND f.worktree_id IS NOT NULL)",
        cancelled = word(TaskStatus::Cancelled),
        unfinished = sql_words::list(&[
            WorktreeStatus::Open,
            WorktreeStatus::Unlanded,
            WorktreeStatus::CleanupFailed
        ]),
        abandoned = word(WorktreeStatus::Abandoned)
    )
}

fn sync_custody_trees(conn: &Connection) -> Result<()> {
    conn.execute(&custody_trees_sql(), [util::now()])?;
    Ok(())
}

fn custody_trees_sql() -> String {
    format!(
            "INSERT INTO import_findings (project_id, run_id, kind, task_id, worktree_id, detail, status, created_at)
             SELECT r.project_id, t.run_id, {kind}, t.task_id, w.worktree_id,
                 'T-' || t.seq || ': the worktree ' || w.path || ' (' || w.state
                     || ' when found) can hold work that never landed',
                 {open}, ?1
             FROM worktrees w JOIN tasks t ON t.task_id = w.task_id JOIN runs r ON r.run_id = t.run_id
             WHERE t.origin = 'native'
               AND (w.state IN ('orphaned', 'rescued') OR w.rescue_ref IS NOT NULL)
               AND w.worktree_id NOT IN (SELECT f.worktree_id FROM import_findings f
                                         WHERE f.worktree_id IS NOT NULL)",
            kind = word(FindingKind::WorktreeUnlanded),
            open = word(FindingStatus::Open)
    )
}

fn sync_tasks(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare(&reaped_tasks_sql())?;
    let rows = stmt
        .query_map([], |r| r.try_into())?
        .collect::<rusqlite::Result<Vec<(String, String, String, i64, String)>>>()?;
    for (task, run, project, seq, status) in rows {
        conn.execute(
            &format!(
                "INSERT INTO import_findings (project_id, run_id, kind, task_id, detail, status, created_at)
                 VALUES (?1, ?2, 'reaped_never_finished', ?3, ?4, {open}, ?5)",
                open = word(FindingStatus::Open)
            ),
            params![project, run, task, format!("T-{seq} ({}): a worker died on it and the run ended before another finished it", TaskStatus::shown_word(&status)), util::now()],
        )?;
    }
    Ok(())
}

fn reaped_tasks_sql() -> String {
    format!(
        "SELECT t.task_id, t.run_id, r.project_id, t.seq, t.status
         FROM tasks t JOIN runs r ON r.run_id = t.run_id
         WHERE t.origin = 'native' AND r.status <> {active} AND t.status NOT IN ({ended})
           AND EXISTS (SELECT 1 FROM workers w WHERE w.task_id = t.task_id AND w.status = {incomplete})
           AND t.task_id NOT IN (SELECT f.task_id FROM import_findings f
                                 WHERE f.kind = 'reaped_never_finished'
                                   AND f.task_id IS NOT NULL)",
        active = word(RunStatus::Active),
        ended = sql_words::list(sql_words::TASKS_ENDED),
        incomplete = word(WorkerStatus::Incomplete)
    )
}

pub fn unfinished(conn: &Connection, root: Option<&Path>) -> Result<Vec<Unfinished>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT f.finding_id, f.kind, r.slug, p.root, f.task_id, f.worktree_id,
            f.detail || COALESCE('. Rescue ref: ' || w.rescue_ref, ''), f.created_at
         FROM import_findings f
         LEFT JOIN runs r ON r.run_id = f.run_id
         LEFT JOIN projects p ON p.project_id = f.project_id
         LEFT JOIN worktrees w ON w.worktree_id = f.worktree_id
         WHERE f.status = {open} AND (?1 IS NULL OR p.root = ?1)
         ORDER BY r.started_at DESC, f.finding_id DESC",
        open = word(FindingStatus::Open)
    ))?;
    let rows = stmt
        .query_map([root.map(runs::root_key)], Unfinished::from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let git = Git::new(git_bin(&BinOverrides::from_env(&ProcessEnv)));
    let mut open = Vec::with_capacity(rows.len());
    for f in rows {
        if !work_landed(conn, &git, &f)? {
            open.push(f);
        }
    }
    Ok(open)
}

fn work_landed(conn: &Connection, git: &Git, f: &Unfinished) -> Result<bool> {
    let (Some(task), Some(wt)) = (f.task_id.as_deref(), f.worktree_id.as_deref()) else {
        return Ok(false);
    };
    let Some(landed) = landed_commit_of(conn, task)? else {
        return Ok(false);
    };
    let Some(tree) = trees::get(conn, wt)? else {
        return Ok(false);
    };
    let repo = Path::new(&tree.repo_root);
    if let Some(rescue) = &tree.rescue_ref {
        if !rescue_landed(git, repo, rescue, &landed) {
            return Ok(false);
        }
    }
    let folder = Path::new(&tree.path);
    if tree.state != WorktreeState::Removed && folder.is_dir() {
        return Ok(folder_landed(git, folder, &landed));
    }
    if tree.rescue_ref.is_some() {
        return Ok(true);
    }
    Ok(tree_landed_commit(conn, wt)?.is_some_and(|c| git.is_ancestor(repo, &c, &landed)))
}

fn landed_commit_of(conn: &Connection, task: &str) -> Result<Option<String>> {
    optional(conn.query_row(
        &format!(
            "SELECT w.landed_commit FROM tasks t JOIN worktrees w ON w.task_id = t.task_id
             WHERE t.task_id = ?1 AND t.origin = 'native' AND t.status = {done}
               AND w.landed_commit IS NOT NULL
             ORDER BY w.landed_at DESC, w.rowid DESC LIMIT 1",
            done = word(TaskStatus::Done)
        ),
        [task],
        |r| r.get(0),
    ))
}

fn tree_landed_commit(conn: &Connection, wt: &str) -> Result<Option<String>> {
    Ok(optional(conn.query_row(
        "SELECT landed_commit FROM worktrees WHERE worktree_id = ?1",
        [wt],
        |r| r.get::<_, Option<String>>(0),
    ))?
    .flatten())
}

fn rescue_landed(git: &Git, repo: &Path, rescue: &str, landed: &str) -> bool {
    git.is_ancestor(repo, rescue, landed)
        || (saved_nothing(git, repo, rescue)
            && git.is_ancestor(repo, &format!("{rescue}^1"), landed))
}

fn folder_landed(git: &Git, folder: &Path, landed: &str) -> bool {
    let clean = git.dirty(folder).is_ok_and(|d| d.is_empty());
    clean
        && git
            .rev(folder, "HEAD")
            .is_ok_and(|h| git.is_ancestor(folder, &h, landed))
}

fn saved_nothing(git: &Git, repo: &Path, rescue: &str) -> bool {
    let tree_of =
        |rev: String| git.run(repo, &["rev-parse", "--verify", &format!("{rev}^{{tree}}")]);
    match (tree_of(rescue.to_string()), tree_of(format!("{rescue}^1"))) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

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

pub fn decide(
    conn: &Connection,
    id: i64,
    status: FindingStatus,
    by: &str,
    note: &str,
) -> Result<()> {
    let n = conn.execute(
        &format!(
            "UPDATE import_findings SET status = ?2, decided_by = ?3, decided_at = ?4, decision_note = ?5
             WHERE finding_id = ?1 AND status = {open}",
            open = word(FindingStatus::Open)
        ),
        params![id, status.as_str(), by, util::now(), note],
    )?;
    if n == 0 {
        anyhow::bail!("finding {id} is not open");
    }
    Ok(())
}

pub fn reopen(
    conn: &Connection,
    run: &runs::Run,
    new: &super::tasks::NewTask,
    finding: &Unfinished,
) -> Result<super::tasks::Task> {
    let task = super::tasks::add(conn, run, new, super::model::Actor::Orchestrator)?;
    decide(
        conn,
        finding.finding_id,
        FindingStatus::Requeued,
        "orchestrator",
        &task.task_id,
    )?;
    if let Some(wt) = &finding.worktree_id {
        let w = super::finish::worktree(conn, wt)?;
        if Path::new(&w.path).exists() && w.status != "cleaned" {
            conn.execute(
                &format!(
                    "UPDATE worktrees SET task_id = ?2, worker_id = NULL, status = {open} WHERE worktree_id = ?1",
                    open = word(WorktreeStatus::Open)
                ),
                [wt, &task.task_id],
            )?;
        }
    }
    Ok(task)
}

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

pub fn reopened_from(conn: &Connection, task_id: &str) -> Result<Option<String>> {
    let f: Option<(String, Option<String>)> = optional(conn.query_row(
        &format!(
            "SELECT detail, task_id FROM import_findings WHERE status = {requeued} AND decision_note = ?1",
            requeued = word(FindingStatus::Requeued)
        ),
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

#[cfg(test)]
mod trf_45_tests {
    use super::*;
    use crate::execution::store::ExecutionStore;

    #[test]
    fn trf_45_ledger_import_copies_old_json_once() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state");
        std::fs::create_dir_all(&state).unwrap();
        let file = state.join("-repo.json");
        let text = serde_json::json!([
            {"record_id": "a", "session_id": null, "agent": "claude", "tier": "opus",
             "model": "opus", "role": "opus-1", "status": "done", "task": "t1",
             "history": [], "created_at": "2026-01-01T00:00:00Z",
             "updated_at": "2026-01-01T00:00:00Z", "project": "/repo"},
            {"record_id": "b", "session_id": "s-b", "agent": "codex", "tier": "codex-sol",
             "model": "gpt", "role": "codex-sol-1", "status": "working", "task": "t2",
             "history": [], "created_at": "2026-01-02T00:00:00Z",
             "updated_at": "2026-01-02T00:00:00Z"}
        ])
        .to_string();
        std::fs::write(&file, &text).unwrap();
        let db = tmp.path().join("ledger.sqlite3");
        let mut store = Store::open(&db).unwrap();
        let git = Git::new("git");

        let dry = import(&mut store, &git, &state, true).unwrap();
        assert_eq!(dry.sessions, 2);
        let first = import(&mut store, &git, &state, false).unwrap();
        assert_eq!(first.sessions, 2);
        let again = import(&mut store, &git, &state, false).unwrap();
        assert_eq!(again.sessions, 0);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), text);

        let rows: Vec<(String, String, String)> = store
            .conn()
            .prepare("SELECT record_id, project_root, slug FROM session_records ORDER BY record_id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(
            rows,
            [
                ("a".into(), "/repo".into(), "-repo".into()),
                ("b".into(), "-repo".into(), "-repo".into())
            ]
        );
        std::fs::remove_file(&file).unwrap();
        let records = ExecutionStore::in_db(&state, &db, "/repo").read().unwrap();
        let ids: Vec<&str> = records.iter().map(|r| r.record_id.as_str()).collect();
        assert_eq!(ids, ["a", "b"]);
    }

    #[test]
    fn trf_45_import_keeps_a_non_empty_history() {
        use crate::execution::legacy::HistoryEntry;
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state");
        std::fs::create_dir_all(&state).unwrap();
        let file = state.join("-repo.json");
        let entry = |at: &str, event: &str, text: &str| serde_json::json!({"at": at, "event": event, "text": text});
        let text = serde_json::json!([
            {"record_id": "a", "session_id": null, "agent": "claude", "tier": "opus",
             "model": "opus", "role": "opus-1", "status": "working", "task": "t1",
             "history": [
                 entry("2026-01-01T00:00:00Z", "spawn", "spawned"),
                 entry("2026-01-01T00:01:00Z", "note", "first note"),
                 entry("2026-01-01T00:02:00Z", "note", "second note")
             ],
             "created_at": "2026-01-01T00:00:00Z",
             "updated_at": "2026-01-01T00:02:00Z", "project": "/repo"}
        ])
        .to_string();
        std::fs::write(&file, &text).unwrap();
        let db = tmp.path().join("ledger.sqlite3");
        let mut store = Store::open(&db).unwrap();
        let report = import(&mut store, &Git::new("git"), &state, false).unwrap();
        assert_eq!(report.sessions, 1);
        std::fs::remove_file(&file).unwrap();

        let records = ExecutionStore::in_db(&state, &db, "/repo").with_writer(std::sync::Arc::new(
            crate::taskledger::store::LedgerWriter::fleetd(),
        ));
        let events = |records: &ExecutionStore| -> Vec<(String, String)> {
            records
                .get("a")
                .unwrap()
                .history
                .into_iter()
                .map(|h| (h.event, h.text))
                .collect()
        };
        assert_eq!(events(&records).len(), 3, "{:?}", events(&records));
        records
            .update(|rs| {
                let r = rs.iter_mut().find(|r| r.record_id == "a").unwrap();
                r.history.push(HistoryEntry {
                    at: "2026-01-01T00:03:00Z".into(),
                    event: "note".into(),
                    text: "third note".into(),
                });
                Ok(())
            })
            .unwrap();
        assert_eq!(
            events(&records),
            [
                ("spawn".to_string(), "spawned".to_string()),
                ("note".into(), "first note".into()),
                ("note".into(), "second note".into()),
                ("note".into(), "third note".into()),
            ]
        );
    }
}

#[cfg(test)]
mod sync_native_tests {
    use super::*;

    fn plan(conn: &Connection, sql: &str) -> Vec<String> {
        let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        let args = vec!["now"; stmt.parameter_count()];
        let rows = stmt
            .query_map(rusqlite::params_from_iter(args), |r| r.get::<_, String>(3))
            .unwrap();
        rows.collect::<rusqlite::Result<_>>().unwrap()
    }

    #[test]
    fn each_sync_statement_reads_the_findings_once() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(&tmp.path().join("ledger.sqlite3")).unwrap();
        for sql in [
            cancelled_open_trees_sql(),
            custody_trees_sql(),
            reaped_tasks_sql(),
        ] {
            let plan = plan(store.conn(), &sql);
            let per_row = plan
                .windows(2)
                .any(|w| w[0].starts_with("CORRELATED") && w[1] == "SCAN f");
            assert!(
                !per_row,
                "a correlated read of import_findings in {sql}: {plan:#?}"
            );
        }
    }
}
