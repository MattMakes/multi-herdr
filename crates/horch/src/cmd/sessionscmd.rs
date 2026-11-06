//! `horch sessions` and `horch ledger legacy` - the port of `bin/horch-ledger` and
//! `bin/horch-sessions`.
//!
//! The `ledger` subcommands mirror the bash CLI one for one; they are the
//! plumbing `spawn`, `note`, `done` and `assign` are built on, kept exposed for
//! inspection and debugging.

use std::process::ExitCode;

use anyhow::Result;
use clap::Subcommand;
use horch_core::execution::records::{NewRecord, SessionLedger};
use horch_core::execution::store::ExecutionStore;
use horch_core::runtime::RuntimeContext;

use crate::output;

#[derive(Subcommand)]
pub enum LegacyCommand {
    /// Record a freshly spawned session.
    Add {
        #[arg(long)]
        record_id: String,
        #[arg(long)]
        agent: String,
        #[arg(long)]
        tier: String,
        #[arg(long)]
        model: String,
        #[arg(long)]
        role: String,
        #[arg(long)]
        session_id: Option<String>,
        #[arg(long)]
        task: Option<String>,
    },
    /// Attach a session id discovered after launch.
    SetSession {
        key: String,
        session_id: String,
    },
    /// Exit 0 when any record already claims this session id.
    HasSession {
        session_id: String,
    },
    /// Re-open a finished session under a role.
    Resume {
        key: String,
        role: String,
        task: Option<String>,
    },
    /// Point the live worker for a role at a new task.
    Assign {
        #[arg(long)]
        role: String,
        task: String,
    },
    Note {
        key: String,
        text: String,
    },
    Done {
        key: String,
        summary: String,
    },
    /// Print one record as JSON.
    Get {
        key: String,
    },
    List {
        #[arg(long)]
        json: bool,
    },
    /// Print the ledger file path.
    Path,
}

/// `horch sessions` - the orchestrator-facing ledger view.
///
/// Read this before spawning: a `[done]` session whose task or history overlaps
/// the new task is a resume candidate
/// (`horch spawn --resume <session-or-record-id> "task"`).
#[derive(Debug, Clone, Copy, Default)]
pub struct SessionFilter<'a> {
    pub all_workspaces: bool,
    pub live: bool,
    pub role: Option<&'a str>,
}

impl SessionFilter<'_> {
    pub fn matches(
        &self,
        workspace: Option<&str>,
        r: &horch_core::execution::legacy::Record,
    ) -> bool {
        (self.all_workspaces || workspace.is_none_or(|w| r.workspace_id.as_deref() == Some(w)))
            && (!self.live || r.execution_status().is_live())
            && self.role.is_none_or(|role| r.role == role)
    }
}

pub(crate) fn caller_workspace(ctx: &RuntimeContext) -> Option<String> {
    if let Some(workspace) = &ctx.herdr.workspace {
        return Some(workspace.to_string());
    }
    let pane = ctx.herdr.pane.as_ref()?;
    horch_core::workspace::herdr::Herdr::with_bin(&ctx.bins.harness.herdr)
        .pane_get(pane.as_str())
        .ok()
        .and_then(|p| p.workspace_id)
}

#[allow(dead_code)]
pub fn sessions(ctx: &RuntimeContext, json: bool, all: bool) -> Result<()> {
    sessions_filtered(ctx, json, all, false, false, None)
}

pub fn sessions_filtered(
    ctx: &RuntimeContext,
    json: bool,
    all: bool,
    all_workspaces: bool,
    live: bool,
    role: Option<&str>,
) -> Result<()> {
    let filter = SessionFilter {
        all_workspaces,
        live,
        role,
    };
    let caller = caller_workspace(ctx);
    let records = session_records(ctx, all, &filter, caller.as_deref())?;
    let herdr = horch_core::workspace::herdr::Herdr::with_bin(&ctx.bins.harness.herdr);
    let mut panes = std::collections::BTreeMap::new();
    for workspace in records.iter().filter_map(|r| r.workspace_id.as_deref()) {
        panes
            .entry(workspace)
            .or_insert_with(|| herdr.pane_list(workspace).ok());
    }
    let pane_gone = |r: &horch_core::execution::legacy::Record| -> bool {
        r.execution_status().is_live()
            && r.workspace_id
                .as_deref()
                .zip(r.pane_id.as_deref())
                .and_then(|(ws, pane)| {
                    panes
                        .get(ws)?
                        .as_ref()
                        .map(|ps| !ps.iter().any(|p| p.pane_id == pane))
                })
                .unwrap_or(false)
    };
    if json {
        #[derive(serde::Serialize)]
        struct SessionRow<'a> {
            #[serde(flatten)]
            record: &'a horch_core::execution::legacy::Record,
            #[serde(skip_serializing_if = "Option::is_none")]
            pane_gone: Option<bool>,
        }
        let rows: Vec<_> = records
            .iter()
            .map(|r| SessionRow {
                record: r,
                pane_gone: r.pane_id.as_ref().map(|_| pane_gone(r)),
            })
            .collect();
        output::println(&serde_json::to_string_pretty(&rows)?);
    } else {
        let mut text = ExecutionStore::render_text(&records);
        for record in records.iter().filter(|r| pane_gone(r)) {
            let key = format!("record={}\n", record.record_id);
            text = text.replace(&key, &format!("record={} (pane gone)\n", record.record_id));
        }
        output::print(&text);
        if let Some(workspace) = caller_workspace(ctx) {
            if let Ok(store) = horch_core::taskledger::open(ctx) {
                if super::taskledger::project_root(ctx)
                    .ok()
                    .and_then(|root| {
                        horch_core::taskledger::runs::of_project(store.conn(), &root).ok()
                    })
                    .is_some_and(|runs| {
                        runs.iter().any(|r| {
                            r.status == "active" && r.workspace_id.as_deref() == Some(&workspace)
                        })
                    })
                {
                    output::println("Active task-ledger run: horch ledger status");
                }
            }
        }
    }
    Ok(())
}

/// The records `horch sessions` shows: the JSON ledger with the task
/// ledger's worker rows laid over it (TRF-44), filtered.
fn session_records(
    ctx: &RuntimeContext,
    all: bool,
    filter: &SessionFilter<'_>,
    caller: Option<&str>,
) -> Result<Vec<horch_core::execution::legacy::Record>> {
    Ok(ExecutionStore::open_in(ctx)?
        .read()?
        .into_iter()
        .filter(|r| (all || r.round_id.is_none()) && filter.matches(caller, r))
        .collect())
}

// Legacy plumbing retains its project-wide output.
fn print_list(ctx: &RuntimeContext, json: bool, all: bool) -> Result<()> {
    let records: Vec<_> = ExecutionStore::open_in(ctx)?
        .read()?
        .into_iter()
        .filter(|r| all || r.round_id.is_none())
        .collect();
    if json {
        output::println(&ExecutionStore::render_json(&records)?);
    } else {
        output::print(&ExecutionStore::render_text(&records));
    }
    Ok(())
}

pub fn run(ctx: &RuntimeContext, command: LegacyCommand) -> Result<ExitCode> {
    let ledger = SessionLedger::open_in(ctx)?;
    if let LegacyCommand::HasSession { session_id } = &command {
        return has_session(&ledger, session_id);
    }
    match command {
        LegacyCommand::Get { .. } | LegacyCommand::List { .. } | LegacyCommand::Path => {
            read(ctx, &ledger, command)
        }
        other => write(&ledger, other),
    }?;
    Ok(ExitCode::SUCCESS)
}

fn has_session(ledger: &SessionLedger, session_id: &str) -> Result<ExitCode> {
    Ok(if ledger.has_session(session_id)? {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

/// The subcommands that only print.
fn read(ctx: &RuntimeContext, ledger: &SessionLedger, command: LegacyCommand) -> Result<()> {
    match command {
        LegacyCommand::Get { key } => print_record(ledger, &key),
        LegacyCommand::List { json } => print_list(ctx, json, true),
        LegacyCommand::Path => {
            output::println(&ledger.path().display().to_string());
            Ok(())
        }
        _ => unreachable!("run routes only the print subcommands here"),
    }
}

fn print_record(ledger: &SessionLedger, key: &str) -> Result<()> {
    output::println(&serde_json::to_string_pretty(&ledger.get(key)?)?);
    Ok(())
}

/// The subcommands that change the ledger.
fn write(ledger: &SessionLedger, command: LegacyCommand) -> Result<()> {
    match command {
        LegacyCommand::Add {
            record_id,
            agent,
            tier,
            model,
            role,
            session_id,
            task,
        } => ledger.add(NewRecord {
            record_id: &record_id,
            agent: &agent,
            tier: &tier,
            model: &model,
            role: &role,
            session_id: session_id.as_deref(),
            task: task.as_deref().unwrap_or_default(),
            phase: None,
        }),
        LegacyCommand::SetSession { key, session_id } => ledger.set_session(&key, &session_id),
        LegacyCommand::Resume { key, role, task } => {
            ledger.resume(&key, &role, task.as_deref().unwrap_or_default())
        }
        LegacyCommand::Assign { role, task } => ledger.assign(&role, &task),
        LegacyCommand::Note { key, text } => ledger.note(&key, &text),
        LegacyCommand::Done { key, summary } => ledger.done(&key, &summary),
        _ => unreachable!("run routes only the write subcommands here"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horch_core::taskledger::model::Actor;
    use horch_core::taskledger::runs::{self, NewRun};
    use horch_core::taskledger::tasks::{self, NewTask};
    use horch_core::taskledger::workers::{self, NewWorker};

    const LEDGER: &str = r#"[
  {
    "record_id": "r1",
    "session_id": "s1",
    "agent": "claude",
    "tier": "opus",
    "model": "opus",
    "role": "opus-1",
    "status": "working",
    "task": "Run horch ledger claim.",
    "history": [
      {"at": "2026-11-05T10:00:00Z", "event": "spawned", "text": "Run horch ledger claim."},
      {"at": "2026-11-05T10:05:00Z", "event": "note", "text": "claimed"}
    ],
    "created_at": "2026-11-05T10:00:00Z",
    "updated_at": "2026-11-05T10:05:00Z",
    "workspace_id": "w1"
  }
]"#;

    /// The `horch sessions` text before TRF-44, pinned.
    const PINNED: &str = "[working] opus (opus-1)  session=s1  record=r1
  agent=claude model=opus  updated=2026-11-05T10:05:00Z
  task: Run horch ledger claim.
  note @ 2026-11-05T10:05:00Z: claimed

";

    fn ctx(dir: &std::path::Path) -> RuntimeContext {
        let project = dir.join("repo");
        std::fs::create_dir_all(&project).unwrap();
        let ctx = RuntimeContext::from_env(
            &horch_core::runtime::MapEnv::new(project.to_str().unwrap())
                .with_exe("/opt/horch/bin/horch")
                .with("HORCH_STATE_DIR", dir.join("state").to_str().unwrap())
                .with("HORCH_PROJECT_DIR", project.to_str().unwrap())
                .with(
                    "HORCH_LEDGER_DB",
                    dir.join("ledger.sqlite3").to_str().unwrap(),
                ),
        )
        .unwrap();
        let store = ExecutionStore::open_in(&ctx).unwrap();
        std::fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        std::fs::write(store.path(), LEDGER).unwrap();
        ctx
    }

    fn text(ctx: &RuntimeContext) -> String {
        let records = session_records(ctx, false, &SessionFilter::default(), Some("w1")).unwrap();
        ExecutionStore::render_text(&records)
    }

    #[test]
    fn trf_44_sessions_text_is_pinned_without_task_ledger_rows() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        assert_eq!(text(&ctx), PINNED);
        // A task ledger with no run of this project changes nothing.
        horch_core::taskledger::open(&ctx).unwrap();
        assert_eq!(text(&ctx), PINNED);
    }

    #[test]
    fn trf_44_sessions_shows_a_ledger_worker_with_its_claimed_task() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let root = ctx.paths.project().unwrap();
        let mut store = horch_core::taskledger::open(&ctx).unwrap();
        let other = store
            .immediate(|tx| {
                let run = runs::create(
                    tx,
                    &root,
                    &NewRun {
                        slug: "r".into(),
                        ..Default::default()
                    },
                )?;
                let task = tasks::add(
                    tx,
                    &run,
                    &NewTask::work("Fix the parser", "t.md", "opus"),
                    Actor::Orchestrator,
                )?;
                let linked = workers::insert(
                    tx,
                    &NewWorker {
                        run_id: run.run_id.clone(),
                        persona: "opus".into(),
                        role: Some("opus-1".into()),
                        workspace_id: Some("w1".into()),
                        legacy_record_id: Some("r1".into()),
                        ..Default::default()
                    },
                )?;
                let other = workers::insert(
                    tx,
                    &NewWorker {
                        run_id: run.run_id.clone(),
                        persona: "codex".into(),
                        role: Some("codex-2".into()),
                        harness: Some("codex".into()),
                        workspace_id: Some("w1".into()),
                        ..Default::default()
                    },
                )?;
                tx.execute(
                    "UPDATE workers SET status = 'working', task_id = ?2 WHERE worker_id = ?1",
                    rusqlite::params![linked, task.task_id],
                )?;
                tx.execute(
                    "UPDATE workers SET status = 'claiming' WHERE worker_id = ?1",
                    [&other],
                )?;
                Ok(other)
            })
            .unwrap();
        let shown = text(&ctx);
        assert!(shown.contains("record=r1\n"), "{shown}");
        assert!(shown.contains("task: T-1: Fix the parser\n"), "{shown}");
        assert!(!shown.contains("Run horch ledger claim."), "{shown}");
        assert!(
            shown.contains(&format!(
                "codex (codex-2)  session=not-yet-known  record={other}"
            )),
            "{shown}"
        );
        assert!(shown.contains("agent=codex"), "{shown}");
    }
}
