//! Worker worktree creation, landing, and inspection.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{anyhow, Context, Result};
use horch_core::digest::sha256_bytes;
use horch_core::fleetd::fleet_runtime::domain::requests::Kind;
use horch_core::fleetd::worktree_custody::adapters::sqlite as trees;
use horch_core::fleetd::worktree_custody::application::queries::{
    self, DiskTotal, RescueSource, WorktreeView,
};
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger;
use horch_core::taskledger::git::Git;
use horch_core::taskledger::model::TriageDecision;
use horch_core::taskledger::triage::{self, human_bytes, Measure};
use horch_core::vcs::landing::{ClaimState, Landing, Skipped, Unregistered};
use serde_json::{json, Value};

use super::messaging;
use super::operator_request;
use super::taskledger::{resolve_run, RunArg};

fn landing(ctx: &RuntimeContext) -> Landing {
    Landing::new(ctx.bins.harness.git.clone(), ctx.paths.state_root.clone())
        .with_recorded_worktrees(recorded_worktrees(ctx))
}

/// The paths the task ledger records as worktrees. A machine with no ledger
/// file records none, and is not given one by a scan.
pub fn recorded_worktrees(ctx: &RuntimeContext) -> Vec<PathBuf> {
    if !ctx.paths.ledger_db.exists() {
        return Vec::new();
    }
    let paths = horch_core::taskledger::open_existing_read_only(ctx).and_then(|store| {
        store.map_or(Ok(Vec::new()), |store| {
            horch_core::taskledger::status::worktree_paths(store.conn())
        })
    });
    match paths {
        Ok(paths) => paths,
        Err(error) => {
            eprintln!("horch: could not read the task ledger's worktrees ({error:#})");
            Vec::new()
        }
    }
}

/// Say so when the scan had to leave a worktree out. A path it cannot read
/// is the operator's to fix, and `done` must not go quiet about it.
pub fn note_skipped(command: &str, skipped: &[Skipped]) {
    for entry in skipped {
        eprintln!(
            "horch {command}: skipped worktree {} ({})",
            entry.path.display(),
            entry.reason
        );
    }
}

fn role(ctx: &RuntimeContext) -> &str {
    ctx.worker
        .as_ref()
        .and_then(|worker| worker.role.as_deref())
        .unwrap_or("orchestrator")
}

pub fn new(ctx: &RuntimeContext, name: &str) -> Result<()> {
    let dir = ctx.paths.current_dir()?;
    let record_id = ctx
        .worker
        .as_ref()
        .and_then(|worker| worker.record_id.as_deref());
    let claim = landing(ctx)
        .create(&dir, role(ctx), record_id, name)
        .map_err(|error| {
            let text = format!("{error:#}");
            if text.contains("Operation not permitted") {
                anyhow!("{text}\nThe sandbox cannot write .git here. A Codex pane writes .git only in its \
                         own folder, under permission_mode auto or acceptEdits (the fleet_write profile).")
            } else {
                error
            }
        })?;
    println!("path: {}", claim.path.display());
    println!("branch: {}", claim.branch);
    println!("base: {}", claim.base);
    println!("next: cd {}", claim.path.display());
    Ok(())
}

pub fn land(ctx: &RuntimeContext, name: Option<&str>, check: Option<&str>) -> Result<()> {
    let dir = ctx.paths.current_dir()?;
    let result = landing(ctx).land(&dir, role(ctx), name, check)?;
    let short: String = result.commit.chars().take(7).collect();
    let line = format!(
        "LANDED {} -> {} {} ({} commits)",
        result.branch, result.base, short, result.commits
    );
    println!("{line}");
    if let Some(note) = &result.cleanup_note {
        println!("horch: NOTE: {note}");
    }
    if let Err(error) = messaging::tell(ctx, "orchestrator", &format!("NOTE: {line}")) {
        eprintln!("horch worktree land: could not report landing ({error:#})");
    }
    Ok(())
}

pub fn list(ctx: &RuntimeContext) -> Result<()> {
    let dir = ctx.paths.current_dir()?;
    let landing = landing(ctx);
    for claim in landing.reconcile(&dir)? {
        let status = landing.status(&claim)?;
        println!(
            "{} {} {} {} {} ahead={} dirty={} contained={}",
            claim.name,
            claim.role,
            state_label(claim.state),
            claim.branch,
            claim.base,
            status.ahead,
            status.dirty,
            status.contained
        );
    }
    let scan = landing.unregistered(&dir)?;
    note_skipped("worktree list", &scan.skipped);
    print_unregistered(&scan.entries);
    Ok(())
}

/// The word `list` prints for the state of a claim.
fn state_label(state: ClaimState) -> &'static str {
    match state {
        ClaimState::Open => "open",
        ClaimState::Landed => "landed",
        ClaimState::Unlanded => "unlanded",
    }
}

/// The `unregistered:` section of `list`: `none` on 1 line, else a header and
/// a line for each worktree that has no claim.
fn print_unregistered(entries: &[Unregistered]) {
    if entries.is_empty() {
        println!("unregistered: none");
    } else {
        println!("unregistered:");
        for entry in entries {
            println!(
                "{} branch={} ahead={} dirty={}",
                entry.path.display(),
                entry.branch.as_deref().unwrap_or("detached"),
                entry.ahead,
                entry.dirty
            );
        }
    }
}

/// The main checkout of the project the command runs in.
fn triage_root(ctx: &RuntimeContext, git: &Git) -> Result<PathBuf> {
    let dir = ctx.paths.project()?;
    Ok(git
        .main_root(&dir)
        .unwrap_or_else(|| dir.canonicalize().unwrap_or(dir)))
}

/// The parked worktrees of the project, measured as `measure` says.
pub fn triage_rows(ctx: &RuntimeContext, measure: Measure) -> Result<Vec<triage::Row>> {
    let git = Git::new(&ctx.bins.harness.git);
    let root = triage_root(ctx, &git)?;
    // The listing reads the ledger read-only and skips the bytes cache
    // (T-5): only fleetd writes the ledger (RR-5, T-885).
    match taskledger::open_existing_read_only(ctx)? {
        Some(store) => triage::collect(store.conn(), &git, &root, measure),
        None => triage::collect_without_ledger(&git, &root, measure),
    }
}

/// `horch worktree triage [--json]`: the facts of every parked worktree.
pub fn triage(ctx: &RuntimeContext, json: bool) -> Result<()> {
    let rows = triage_rows(ctx, Measure::Full)?;
    let git = Git::new(&ctx.bins.harness.git);
    let remote = triage::apply::pr_remote(&git, &triage_root(ctx, &git)?, &gh_bin());
    if json {
        println!("{}", triage::to_json(&rows, remote.is_some()));
        return Ok(());
    }
    print!("{}", triage::render(&rows));
    if !rows.is_empty() {
        match remote {
            Some(r) => println!("PR is offered: gh and the remote {r} exist."),
            None => println!("PR is not offered: gh or a git remote is missing."),
        }
    }
    Ok(())
}

/// The reply text: the file, or stdin for `-`.
fn read_reply(file: &str) -> Result<String> {
    if file == "-" {
        let mut text = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut text)?;
        return Ok(text);
    }
    std::fs::read_to_string(file).map_err(|e| anyhow!("reading the reply {file}: {e}"))
}

/// `gh`: `HORCH_GH_BIN`, else `gh` on PATH.
fn gh_bin() -> PathBuf {
    std::env::var_os("HORCH_GH_BIN")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("gh"))
}

/// `horch worktree triage apply <file|->`: parse the operator's reply and
/// send its decisions to fleetd as 1 `worktree_triage` request. A bad reply
/// runs nothing. fleetd applies `KEEP` and `FINISH` in the request, and
/// queues 1 `triage_apply` job for `ARCHIVE`, `DELETE`, `LAND` and `PR`.
pub fn triage_apply(ctx: &RuntimeContext, file: &str, primary: &str) -> Result<ExitCode> {
    let text = read_reply(file)?;
    let git = Git::new(&ctx.bins.harness.git);
    let root = triage_root(ctx, &git)?;
    let store = read_only(ctx)?;
    let rows = triage::collect(store.conn(), &git, &root, Measure::Ids)?;
    let gh = gh_bin();
    let remote = triage::apply::pr_remote(&git, &root, &gh);
    let decided = triage::reply::parse(&text, &rows, remote.is_some())?;
    let mut decisions = Vec::new();
    for d in &decided {
        let row = rows
            .iter()
            .find(|r| r.id == d.id)
            .with_context(|| format!("{} is not a row of this triage", d.id))?;
        let ticket = match d.decision {
            TriageDecision::Finish => {
                let direction = d.direction.as_deref().unwrap_or("");
                match triage::apply::finish_brief(store.conn(), &root, row, direction)? {
                    Some(path) => Some(json!({
                        "sha256": sha256_bytes(&std::fs::read(root.join(&path))?).to_string(),
                        "path": path,
                    })),
                    None => None,
                }
            }
            _ => None,
        };
        decisions.push(json!({
            "row": row,
            "decision": d.decision.as_str(),
            "direction": d.direction,
            "line": d.line,
            "ticket": ticket,
        }));
    }
    let payload = json!({ "root": root, "primary": primary, "decisions": decisions });
    operator_request::run(ctx, Kind::WorktreeTriage, payload, |r| {
        for a in r["applied"].as_array().into_iter().flatten() {
            println!("{}", a["line"].as_str().unwrap_or_default());
        }
        for line in r["queued"].as_array().into_iter().flatten() {
            println!("{}", line.as_str().unwrap_or_default());
        }
        let failed = r["failed"].as_u64().unwrap_or(0);
        if failed > 0 {
            return Err(anyhow!(
                "{failed} of {} decisions failed; nothing of theirs was deleted. Run horch worktree triage again",
                decided.len()
            ));
        }
        Ok(())
    })
}

/// `horch worktree restore <name>`: a task (`T<seq>` of the active run) or
/// a worktree id whose row has a rescue ref gives its newest rescue ref
/// back as a branch and a tree (MAP.md of fleetd-p3, Q14). Each other name
/// gets the folder again from its archive or trash ref.
pub fn restore(ctx: &RuntimeContext, name: &str) -> Result<ExitCode> {
    let git = Git::new(&ctx.bins.harness.git);
    let root = triage_root(ctx, &git)?;
    let store = read_only(ctx)?;
    // A project with no single active run has no `T<seq>` to look up; a
    // worktree id needs no run.
    let run = resolve_run(ctx, &store, &RunArg::default())
        .ok()
        .map(|r| r.run_id);
    let source = queries::rescue_source(store.conn(), run.as_deref(), name)?;
    let facts = match &source {
        Some(source) => rescue_facts(&git, source)?,
        None => archive_facts(&store, &git, &root, name)?,
    };
    let payload = json!({
        "name": name,
        "run_id": run,
        "root": root,
        "facts": facts,
    });
    operator_request::run(ctx, Kind::WorktreeRestore, payload, |r| {
        println!("{}", r["line"].as_str().unwrap_or_default());
        Ok(())
    })
}

/// The git facts of the restore of `source` as the branch
/// `rescue/T<seq>-<n>` in its tree `<project>/.worktrees/rescue-T<seq>-<n>`.
/// fleetd refuses the restore when the branch or the folder exists.
fn rescue_facts(git: &Git, source: &RescueSource) -> Result<Value> {
    let repo = PathBuf::from(&source.row.repo_root);
    let slug = format!("T{}-{}", source.seq, source.n);
    let branch = format!("rescue/{slug}");
    let path = repo.join(".worktrees").join(format!("rescue-{slug}"));
    Ok(json!({
        "path_exists": path.exists(),
        "branch_exists": git.branch_exists(&repo, &branch),
        "branch_commit": null,
        "rescue_commit": git.rev(&repo, &format!("{}^{{commit}}", source.rescue_ref))?,
        "archive": null,
    }))
}

/// The git facts of the restore of the folder `<root>/.worktrees/<name>`
/// from its archive ref, else its newest trash ref, on the branch of the
/// archived row, else `restored/<name>`.
fn archive_facts(store: &taskledger::Store, git: &Git, root: &Path, name: &str) -> Result<Value> {
    let archive = match triage::refs::restorable(git, root, name)? {
        Some(r) => Some(json!({ "commit": git.rev(root, &r)?, "name": r })),
        None => None,
    };
    let old = match &archive {
        Some(at) => match triage::apply::archived_row(
            store.conn(),
            root,
            at["name"].as_str().unwrap_or_default(),
        )? {
            Some(id) => trees::get(store.conn(), &id)?,
            None => None,
        },
        None => None,
    };
    let branch = old
        .map(|o| o.branch)
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| format!("restored/{name}"));
    let branch_exists = git.branch_exists(root, &branch);
    let branch_commit = if branch_exists {
        Some(git.rev(root, &branch)?)
    } else {
        None
    };
    Ok(json!({
        "path_exists": root.join(".worktrees").join(name).exists(),
        "branch_exists": branch_exists,
        "branch_commit": branch_commit,
        "rescue_commit": null,
        "archive": archive,
    }))
}

/// The ledger, opened read-only. fleetd creates it at its start.
fn read_only(ctx: &RuntimeContext) -> Result<taskledger::Store> {
    taskledger::open_existing_read_only(ctx)?.with_context(|| {
        format!(
            "the task ledger {} does not exist. fleetd creates it at its start: run horch fleetd check",
            ctx.paths.ledger_db.display()
        )
    })
}

/// `horch worktrees [--json]`: 1 line for each tree that is not `removed`,
/// then the total. It only reads the ledger.
pub fn worktrees(ctx: &RuntimeContext, json: bool) -> Result<()> {
    let (rows, total) = match taskledger::open_existing_read_only(ctx)? {
        Some(store) => (
            queries::worktree_rows(store.conn())?,
            queries::total_bytes(store.conn())?,
        ),
        None => (Vec::new(), DiskTotal::default()),
    };
    if json {
        let v = serde_json::json!({
            "worktrees": rows,
            "count": total.count,
            "total_bytes": total.bytes,
            "unmeasured": total.unmeasured,
        });
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    for r in &rows {
        println!("{}", worktree_line(r));
    }
    println!("{}", total_line(&total));
    Ok(())
}

/// 1 tree: a word for each fact, the path last.
fn worktree_line(r: &WorktreeView) -> String {
    let dash = |v: &Option<String>| v.clone().unwrap_or_else(|| "-".into());
    let mut line = format!(
        "state={} status={} task={} run={} worker={} owner={} age={} size={} dirty={}",
        r.state,
        r.status,
        dash(&r.task),
        dash(&r.run),
        dash(&r.worker),
        dash(&r.owner_attempt),
        r.age_s.map_or_else(|| "-".into(), age),
        r.bytes_on_disk.map_or_else(
            || "?".into(),
            |b| human_bytes(u64::try_from(b).unwrap_or(0)).replace(' ', "")
        ),
        u8::from(r.dirty),
    );
    if let Some(rescue) = &r.rescue_ref {
        line.push_str(&format!(" rescue={rescue}"));
    }
    if !r.jobs.is_empty() {
        let jobs: Vec<String> = r
            .jobs
            .iter()
            .map(|j| format!("{}#{}:{}", j.kind, j.id, j.state))
            .collect();
        line.push_str(&format!(" jobs={}", jobs.join(",")));
    }
    line.push_str(&format!(" path={}", r.path));
    line
}

/// The total size of all trees.
fn total_line(t: &DiskTotal) -> String {
    let unmeasured = match t.unmeasured {
        0 => String::new(),
        n => format!("; {n} not measured"),
    };
    format!(
        "total: {} worktree{}, {} on disk{unmeasured}",
        t.count,
        if t.count == 1 { "" } else { "s" },
        human_bytes(t.bytes)
    )
}

/// An age in its largest whole unit: `45s`, `5m`, `2h`, `3d`.
fn age(s: i64) -> String {
    match s.max(0) {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m", s / 60),
        s if s < 86_400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86_400),
    }
}

/// `horch ledger gc`: delete the trash refs older than 30 days, and return
/// the deleted refs.
pub fn gc(ctx: &RuntimeContext) -> Result<Vec<String>> {
    let git = Git::new(&ctx.bins.harness.git);
    let root = triage_root(ctx, &git)?;
    let deleted = triage::refs::gc(&git, &root, horch_core::clock::now())?;
    if deleted.is_empty() {
        println!(
            "No trash ref is older than {} days.",
            triage::refs::TRASH_DAYS
        );
    }
    for r in &deleted {
        println!("deleted {r}");
    }
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each state of a claim has its own word in the lines of `list`.
    #[test]
    fn each_claim_state_has_its_own_word() {
        assert_eq!(state_label(ClaimState::Open), "open");
        assert_eq!(state_label(ClaimState::Landed), "landed");
        assert_eq!(state_label(ClaimState::Unlanded), "unlanded");
    }
}
