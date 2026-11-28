use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::Result;
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger::finish::complexity::{self, Judged, Scope};
use horch_core::taskledger::finish::Tools;
use horch_core::taskledger::workers;
use horch_core::taskledger::Store;

use super::taskledger::{json, tools, worker_id_opt};
use crate::output;

const NO_TASK_BASE: &str = "main";

pub fn complexity_cmd(ctx: &RuntimeContext, store: &Store, as_json: bool) -> Result<ExitCode> {
    let tools = tools(ctx);
    let (dir, base) = match claimed_worker(ctx, store)? {
        Some(wid) => complexity::claimed_scope(store.conn(), &tools, &wid)?,
        None => {
            let dir = toplevel(&tools)?;
            eprintln!(
                "horch ledger: no claimed task; judging {} against {NO_TASK_BASE}",
                dir.display()
            );
            (dir, NO_TASK_BASE.to_string())
        }
    };
    let evidence = tempfile::tempdir()?;
    let scope = Scope {
        tools: &tools,
        dir: &dir,
        base: &base,
        evidence_dir: evidence.path(),
        seq: 0,
    };
    let judged = complexity::judge(&scope)?;
    report(&judged, as_json)
}

fn claimed_worker(ctx: &RuntimeContext, store: &Store) -> Result<Option<String>> {
    let Some(wid) = worker_id_opt(ctx) else {
        return Ok(None);
    };
    let w = workers::get(store.conn(), &wid)?;
    Ok(w.task_id.map(|_| wid))
}

fn toplevel(tools: &Tools) -> Result<PathBuf> {
    let out = tools
        .git
        .run(Path::new("."), &["rev-parse", "--show-toplevel"])?;
    Ok(PathBuf::from(out.trim()))
}

fn report(judged: &Judged, as_json: bool) -> Result<ExitCode> {
    let (status, note, found): (&str, String, &[complexity::Violation]) = match judged {
        Judged::NotApplicable(note) => ("not_applicable", note.to_string(), &[]),
        Judged::Failed(o) => ("failed", o.note.clone(), &[]),
        Judged::Found { found, files, .. } => (
            "analysed",
            format!("{files} files analysed"),
            found.as_slice(),
        ),
    };
    if as_json {
        json(&serde_json::json!({ "status": status, "note": note, "findings": found }))?;
    } else {
        for v in found {
            output::println(&complexity::finding_line(v));
        }
        output::println(&match judged {
            Judged::Failed(_) => note,
            _ => format!("{} findings. {note}", found.len()),
        });
    }
    Ok(ExitCode::from(match (judged, found.is_empty()) {
        (Judged::NotApplicable(_), _) => 2,
        (Judged::Failed(_), _) | (_, false) => 1,
        _ => 0,
    }))
}
