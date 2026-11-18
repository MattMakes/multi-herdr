//! Worker worktree creation, landing, and inspection.

use std::path::PathBuf;

use anyhow::{anyhow, bail, Result};
use horch_core::fleetd::worktree_custody::adapters::sqlite::NewWorktree;
use horch_core::fleetd::worktree_custody::application::create::{self, JobProcess, Lease};
use horch_core::fleetd::worktree_custody::application::queries::{
    self, DiskTotal, RescueSource, WorktreeView,
};
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger;
use horch_core::taskledger::finish::lease_and_wait;
use horch_core::taskledger::git::Git;
use horch_core::taskledger::model::WorktreeStatus;
use horch_core::taskledger::triage::{self, human_bytes, Measure};
use horch_core::taskledger::util;
use horch_core::vcs::landing::{ClaimState, Landing, Skipped, Unregistered};

use super::messaging;
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
    // A sandboxed caller (a codex pane) cannot write the ledger. The listing
    // then reads it read-only and skips the bytes cache (T-5).
    let store = match taskledger::open_existing(ctx) {
        Ok(store) => store,
        Err(_) => taskledger::open_existing_read_only(ctx)?,
    };
    match store {
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
    let jobs = JobProcess::of_this_process()?;
    let env = triage::apply::Env {
        git: &git,
        root: &root,
        evidence_root: &evidence,
        gh: remote.is_some().then_some(gh.as_path()),
        remote: remote.as_deref(),
        primary,
        jobs: &jobs,
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

/// `horch worktree restore <name>`: a task (`T<seq>` of the active run) or
/// a worktree id whose row has a rescue ref gives its newest rescue ref
/// back as a branch and a tree (MAP.md of fleetd-p3, Q14). Each other name
/// gets the folder again from its archive or trash ref.
pub fn restore(ctx: &RuntimeContext, name: &str) -> Result<()> {
    let git = Git::new(&ctx.bins.harness.git);
    let root = triage_root(ctx, &git)?;
    let mut store = taskledger::open(ctx)?;
    let jobs = JobProcess::of_this_process()?;
    // A project with no single active run has no `T<seq>` to look up; a
    // worktree id needs no run.
    let run = resolve_run(ctx, &store, &RunArg::default()).ok();
    let source =
        queries::rescue_source(store.conn(), run.as_ref().map(|r| r.run_id.as_str()), name)?;
    let line = match source {
        Some(source) => restore_rescue(&mut store, &git, &jobs, &source)?,
        None => triage::apply::restore(&store, &git, &jobs, &root, name)?,
    };
    println!("{line}");
    Ok(())
}

/// Make the branch `rescue/T<seq>-<n>` at the rescue commit of `source` and
/// its tree `<project>/.worktrees/rescue-T<seq>-<n>`, through the
/// `worktree_add` job of custody. It refuses, and changes nothing, when the
/// branch or the folder exists.
fn restore_rescue(
    store: &mut taskledger::Store,
    git: &Git,
    jobs: &JobProcess,
    source: &RescueSource,
) -> Result<String> {
    let repo = PathBuf::from(&source.row.repo_root);
    let slug = format!("T{}-{}", source.seq, source.n);
    let branch = format!("rescue/{slug}");
    let path = repo.join(".worktrees").join(format!("rescue-{slug}"));
    if git.branch_exists(&repo, &branch) {
        bail!("the branch {branch} exists; restore changed nothing");
    }
    if path.exists() {
        bail!("{} exists; restore changed nothing", path.display());
    }
    let commit = git.rev(&repo, &format!("{}^{{commit}}", source.rescue_ref))?;
    let new = NewWorktree {
        worktree_id: util::new_id(),
        task_id: None,
        worker_id: None,
        repo_root: source.row.repo_root.clone(),
        path: path.to_string_lossy().into_owned(),
        branch: branch.clone(),
        base: source.rescue_ref.clone(),
        base_commit: Some(commit),
        status: WorktreeStatus::Open,
        mode: None,
    };
    let lease = Lease {
        task_id: None,
        // Only the attach of an existing task tree reads the worker; a
        // new row takes `new.worker_id`.
        worker_id: "",
        attempt: None,
        from: None,
        new: Some(&new),
    };
    let id = lease_and_wait(store.path(), jobs, &lease)?;
    store.immediate(|tx| create::give_to_operator(tx, &id))?;
    Ok(format!(
        "restored {} on {branch} at {}",
        source.rescue_ref,
        path.display()
    ))
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
    if let Some(job) = r.would {
        line.push_str(&format!(" would={job}"));
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
