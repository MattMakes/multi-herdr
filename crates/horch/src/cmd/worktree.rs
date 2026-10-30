//! Worker worktree creation, landing, and inspection.

use std::path::PathBuf;

use anyhow::{anyhow, Result};
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger;
use horch_core::taskledger::git::Git;
use horch_core::taskledger::triage::{self, Measure};
use horch_core::vcs::landing::{ClaimState, Landing, Skipped};

use super::messaging;

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
    let paths = horch_core::taskledger::open(ctx)
        .and_then(|store| horch_core::taskledger::status::worktree_paths(store.conn()));
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
    if let Err(error) = messaging::tell(ctx, "orchestrator", &format!("NOTE: {line}")) {
        eprintln!("horch worktree land: could not report landing ({error:#})");
    }
    Ok(())
}

pub fn list(ctx: &RuntimeContext) -> Result<()> {
    let dir = ctx.paths.current_dir()?;
    let landing = landing(ctx);
    for claim in landing.claims(&dir)? {
        let status = landing.status(&claim)?;
        let state = match claim.state {
            ClaimState::Open => "open",
            ClaimState::Landed => "landed",
            ClaimState::Unlanded => "unlanded",
        };
        println!(
            "{} {} {} {} {} ahead={} dirty={} contained={}",
            claim.name,
            claim.role,
            state,
            claim.branch,
            claim.base,
            status.ahead,
            status.dirty,
            status.contained
        );
    }
    let scan = landing.unregistered(&dir)?;
    note_skipped("worktree list", &scan.skipped);
    if scan.is_empty() {
        println!("unregistered: none");
    } else {
        println!("unregistered:");
        for entry in scan.entries {
            println!(
                "{} branch={} ahead={} dirty={}",
                entry.path.display(),
                entry.branch.as_deref().unwrap_or("detached"),
                entry.ahead,
                entry.dirty
            );
        }
    }
    Ok(())
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
    match taskledger::open_existing(ctx)? {
        Some(store) => triage::collect(store.conn(), &git, &root, measure),
        None => triage::collect_without_ledger(&git, &root, measure),
    }
}

/// `horch worktree triage [--json]`: the facts of every parked worktree.
pub fn triage(ctx: &RuntimeContext, json: bool) -> Result<()> {
    let rows = triage_rows(ctx, Measure::Full)?;
    if json {
        println!("{}", triage::to_json(&rows));
    } else {
        print!("{}", triage::render(&rows));
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
/// run each decision. A bad reply runs nothing.
pub fn triage_apply(ctx: &RuntimeContext, file: &str, primary: &str) -> Result<()> {
    let text = read_reply(file)?;
    let git = Git::new(&ctx.bins.harness.git);
    let root = triage_root(ctx, &git)?;
    let mut store = taskledger::open(ctx)?;
    let rows = triage::collect(store.conn(), &git, &root, Measure::Ids)?;
    let gh = gh_bin();
    let remote = triage::apply::pr_remote(&git, &root, &gh);
    let decided = triage::reply::parse(&text, &rows, remote.is_some())?;
    let evidence = ctx
        .paths
        .ledger_db
        .parent()
        .map(|p| p.join("evidence"))
        .unwrap_or_else(|| PathBuf::from("evidence"));
    let env = triage::apply::Env {
        git: &git,
        root: &root,
        evidence_root: &evidence,
        gh: remote.is_some().then_some(gh.as_path()),
        remote: remote.as_deref(),
        primary,
    };
    let applied = triage::apply::apply(&mut store, &env, &rows, &decided)?;
    for a in &applied {
        println!("{}", a.line());
    }
    wake_for_finish(ctx, &store, &applied);
    let failed = applied.iter().filter(|a| !a.ok).count();
    if failed > 0 {
        return Err(anyhow!(
            "{failed} of {} decisions failed; nothing of theirs was deleted. Run horch worktree triage again",
            applied.len()
        ));
    }
    Ok(())
}

/// Wake each run's orchestrator for the tasks that `FINISH` queued.
fn wake_for_finish(
    ctx: &RuntimeContext,
    store: &taskledger::Store,
    applied: &[triage::apply::Applied],
) {
    for q in applied.iter().filter_map(|a| a.queued.as_ref()) {
        match taskledger::runs::get(store.conn(), &q.run_id) {
            Ok(run) => super::taskledger_wake::wake(
                ctx,
                store,
                &run,
                &format!(
                    "NOTE: worktree triage queued {} (FINISH). It inherits its worktree. Spawn a worker for it.",
                    q.task
                ),
            ),
            Err(e) => eprintln!("horch worktree triage: no run {} to wake ({e:#})", q.run_id),
        }
    }
}

/// `horch worktree restore <name>`: the folder again, from its archive or
/// trash ref.
pub fn restore(ctx: &RuntimeContext, name: &str) -> Result<()> {
    let git = Git::new(&ctx.bins.harness.git);
    let root = triage_root(ctx, &git)?;
    let store = taskledger::open(ctx)?;
    println!("{}", triage::apply::restore(&store, &git, &root, name)?);
    Ok(())
}

/// `horch ledger gc`: delete the trash refs older than 30 days.
pub fn gc(ctx: &RuntimeContext) -> Result<()> {
    let git = Git::new(&ctx.bins.harness.git);
    let root = triage_root(ctx, &git)?;
    let deleted = triage::refs::gc(&git, &root, horch_core::clock::now())?;
    if deleted.is_empty() {
        println!(
            "No trash ref is older than {} days.",
            triage::refs::TRASH_DAYS
        );
    }
    for r in deleted {
        println!("deleted {r}");
    }
    Ok(())
}
