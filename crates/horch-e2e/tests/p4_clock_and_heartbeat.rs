#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::{Duration, Instant};

use horch_core::fleetd::fleet_runtime::adapters::clock_store;
use horch_core::fleetd::fleet_runtime::adapters::effects::read_heartbeat;
use horch_core::fleetd::fleet_runtime::adapters::{config_file, jobs_store};
use horch_core::fleetd::fleet_runtime::application::jobs::{JobCtx, Queued};
use horch_core::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use horch_core::fleetd::fleet_runtime::domain::ledger::Entity;
use horch_core::fleetd::ideas::application::needs_you::{open_keys, NeedsYouList};
use horch_core::fleetd::integration::adapters::exec_gate::{self, GateRequest};
use horch_core::fleetd::integration::adapters::sqlite::{
    self as integration, BundleFact, NewBranch, NewBundle, NewFeature,
};
use horch_core::fleetd::integration::application::bundles;
use horch_core::fleetd::integration::application::landing::{self, overdue_key, OVERDUE_KIND};
use horch_core::fleetd::integration::domain::bundle::{self as bundle_rules, BundleState};
use horch_core::fleetd::integration::domain::task_branch::LandingState;
use horch_core::fleetd::worktree_custody::adapters::sqlite::WorktreeState;
use horch_core::fleetd::worktree_custody::application::gate_tree::{
    self, GateTree, GateTreeRequest,
};
use horch_core::procid;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_e2e::fleetd::Fleetds;
use horch_e2e::harness::{wait_for_or, Harness};
use horch_platform::sandbox::sandboxed;
use rusqlite::{Connection, TransactionBehavior};
use serde_json::Value;

const SIGTERM: i32 = 15;

const READY_LIMIT_S: i64 = 1800;

const WOKE_AT: &str = "2030-01-01T02:00:00Z";

const GATE: &str = "/bin/sleep 300";

const WORKER: &str = "opus-hardening";

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

fn open(db: &Path) -> Connection {
    let conn = Connection::open(db).unwrap();
    conn.busy_timeout(Duration::from_secs(30)).unwrap();
    conn
}

fn link_bwrap(h: &Harness) {
    let found = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join("bwrap"))
            .find(|p| p.is_file())
    });
    if let Some(bwrap) = found {
        std::os::unix::fs::symlink(bwrap, h.bin.join("bwrap")).unwrap();
    }
}

struct World {
    fleetds: Fleetds,
    h: Harness,
}

impl Drop for World {
    fn drop(&mut self) {
        if let Ok(rows) = jobs_store::running(&open(&self.db())) {
            for row in rows {
                if let (Some(pid), Some(started)) = (row.pid, row.pid_start) {
                    procid::signal_same(pid, started, SIGTERM);
                }
            }
        }
    }
}

impl World {
    fn new(name: &str) -> World {
        let mut h = Harness::new(name).with_git();
        let fleet = h.root.join("fleet");
        let data = h.root.join("data");
        for dir in [&fleet, &data] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(
            fleet.join("fleet.toml"),
            "pass_period_s = 1\nheartbeat_max_age_s = 300\n",
        )
        .unwrap();
        h.set(
            "HORCH_LEDGER_DB",
            h.root.join("ledger.sqlite3").to_string_lossy(),
        );
        h.set("HORCH_DATA_DIR", data.to_string_lossy());
        h.set("FLEET_ROOT", fleet.to_string_lossy());
        h.set("HORCH_NO_WAKE", "1");
        h.set("HORCH_NO_WATCHDOG", "1");
        link_bwrap(&h);
        let mut w = World {
            fleetds: Fleetds::new(&h),
            h,
        };
        w.start_fleetd("fleetd");
        w
    }

    fn db(&self) -> PathBuf {
        self.h.root.join("ledger.sqlite3")
    }

    fn conn(&self) -> Connection {
        open(&self.db())
    }

    fn fleet_root(&self) -> PathBuf {
        self.h.root.join("fleet")
    }

    fn evidence(&self) -> String {
        let mut out = String::new();
        for name in ["fleetd", "fleetd-woke"] {
            out.push_str(&format!(
                "--- {name}.stderr\n{}\n",
                self.fleetds.output(name, "stderr")
            ));
        }
        let conn = self.conn();
        let mut stmt = conn
            .prepare(
                "SELECT id || ' ' || kind || ' ' || subject || ' ' || state || ' '
                 || COALESCE(substr(result_json, 1, 400), '') FROM jobs ORDER BY id",
            )
            .unwrap();
        let jobs: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        out.push_str(&format!("--- jobs\n{}\n", jobs.join("\n")));
        out
    }

    fn start_fleetd(&mut self, name: &str) {
        let pid = self.fleetds.start(&self.h, name);
        let root = self.fleet_root();
        wait_for_or(
            "the heartbeat of the new fleetd",
            || {
                read_heartbeat(&root)
                    .ok()
                    .flatten()
                    .filter(|hb| hb.pid == pid)
                    .map(|_| ())
            },
            || self.fleetds.output(name, "stderr"),
        );
    }

    fn wait_passes(&self, passes: usize) {
        let path = self.fleet_root().join("run").join("heartbeat.json");
        let at = || -> Option<u64> {
            let v: Value = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
            v["awake"]["awake_ms"].as_u64()
        };
        let mut seen = vec![at()];
        wait_for_or(
            "fleetd passes",
            || {
                let now = at();
                if now.is_some() && seen.last() != Some(&now) {
                    seen.push(now);
                }
                (seen.len() > passes).then_some(())
            },
            || self.evidence(),
        );
    }

    fn pass(&self, run: &Run) {
        let o = self.h.run(&[
            "ledger", "watch", "--once", "--force", "--json", "--run", &run.slug,
        ]);
        assert!(o.status.success(), "horch ledger watch: {}", text(&o));
    }

    fn fleet_clock(&self) -> i64 {
        clock_store::read(&self.conn()).unwrap().0
    }

    fn overdue_items(&self) -> Vec<String> {
        open_keys(&self.conn(), OVERDUE_KIND).unwrap()
    }

    fn branch(&self, branch: &str) -> (Option<String>, i64) {
        let row = integration::branch(&self.conn(), branch)
            .unwrap()
            .unwrap_or_else(|| panic!("no branch row {branch}"));
        (row.landing_state, row.due_at)
    }

    fn jobs_tx<T>(&self, f: impl FnOnce(&JobCtx<'_>, &Connection) -> anyhow::Result<T>) -> T {
        let config = config_file::load(&self.fleet_root()).unwrap();
        let ctx = JobCtx {
            guard: None,
            config: &config,
            needs_you: &NeedsYouList,
        };
        let mut conn = self.conn();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let out = f(&ctx, &tx).unwrap();
        tx.commit().unwrap();
        out
    }

    fn feature(&self) -> (String, Run) {
        let project = self.h.project.canonicalize().unwrap();
        self.jobs_tx(|_, tx| {
            let id = runs::create_feature_run(tx, &project, "main")?;
            let branch = format!("fleet/{id}");
            let new = NewFeature {
                id: &id,
                name: Some(&id),
                branch: Some(&branch),
                primary_branch: "main",
            };
            integration::insert_feature(tx, None, &new, cause("the test adds the feature"))?;
            tx.execute(
                "INSERT INTO ideas (id, feature_id, state, created_at, updated_at)
                 VALUES ('I-' || ?1, ?1, 'in_progress',
                         '2026-11-18T12:00:00Z', '2026-11-18T12:00:00Z')",
                [&id],
            )?;
            let run = runs::feature_run(tx, &id)?.expect("the run of the feature");
            Ok((id, run))
        })
    }

    fn done_task_branch(&self, tx: &Connection, run: &Run, feature: &str, branch: &str) {
        let new = NewTask::work("the work", "t.md", WORKER);
        let task = tasks::add(tx, run, &new, Actor::Orchestrator)
            .unwrap()
            .task_id;
        tx.execute(
            "UPDATE tasks SET status = 'done' WHERE task_id = ?1",
            [&task],
        )
        .unwrap();
        let row = NewBranch {
            name: branch,
            feature_id: feature,
            task_id: Some(&task),
            owner: "worker",
            due_at: 0,
        };
        integration::insert_branch(tx, None, &row, cause("the test binds the task branch"))
            .unwrap();
    }

    fn git(&self, args: &[&str]) -> String {
        let o = self.h.git_cmd(args).output().unwrap();
        assert!(o.status.success(), "git {args:?}: {}", text(&o));
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }

    fn gate_tree(&self, run: &Run, feature: &str, tip: &str) {
        let repo = self.h.project.canonicalize().unwrap();
        std::fs::create_dir_all(repo.join(".worktrees")).unwrap();
        let branch = format!("fleet/{feature}");
        let req = GateTreeRequest {
            feature_id: feature,
            run_id: &run.run_id,
            repo: &repo,
            base: &branch,
            tip,
        };
        let clock = FleetSeconds(self.fleet_clock());
        let reserved = self.jobs_tx(|ctx, tx| gate_tree::reserve(ctx, tx, &req, clock));
        assert!(
            matches!(reserved, GateTree::Reserved { .. }),
            "{reserved:?}"
        );
        wait_for_or(
            "fleetd to make the gate tree",
            || {
                gate_tree::of_feature(&self.conn(), &repo, feature)
                    .unwrap()
                    .filter(|t| t.state == WorktreeState::Created)
                    .map(|_| ())
            },
            || self.evidence(),
        );
    }
}

#[test]
fn pl4_sleep_past_a_branch_deadline_fires_nothing_on_wake() {
    let mut w = World::new("p4-pl4-sleep");
    let (feature, run) = w.feature();
    let branch = format!("fleet/{feature}/work");
    w.jobs_tx(|_, tx| {
        w.done_task_branch(tx, &run, &feature, &branch);
        Ok(())
    });
    let ready = Some(LandingState::Ready.as_str().to_string());
    let due = wait_for_or(
        "fleetd to move the branch to ready",
        || {
            let (state, due) = w.branch(&branch);
            (state == ready).then_some(due)
        },
        || w.evidence(),
    );
    let left = due - w.fleet_clock();
    assert!(
        left > 0 && left <= READY_LIMIT_S,
        "the ready branch is due {left} s after the fleet clock, not within its \
         limit of {READY_LIMIT_S} s\n{}",
        w.evidence()
    );

    w.fleetds.stop_current();
    let before = w.fleet_clock();
    w.h.set("HORCH_NOW", WOKE_AT);
    let awake = Instant::now();
    w.start_fleetd("fleetd-woke");
    w.wait_passes(3);
    w.fleetds.stop_current();
    let awake_s = i64::try_from(awake.elapsed().as_secs()).unwrap() + 1;
    let grew = w.fleet_clock() - before;
    assert!(
        grew <= awake_s,
        "the fleet clock grew {grew} s in {awake_s} s of awake time: it follows \
         the calendar clock\n{}",
        w.evidence()
    );
    assert_eq!(
        w.branch(&branch),
        (ready.clone(), due),
        "the wake moved the ready branch\n{}",
        w.evidence()
    );
    assert!(
        w.overdue_items().is_empty(),
        "the wake fired the deadline of the branch\n{}",
        w.evidence()
    );

    let left = due - w.fleet_clock();
    assert!(left > 0, "the sleep used up the fleet time of the branch");
    w.h.advance_fleet_clock(left - 1);
    w.pass(&run);
    assert!(
        w.overdue_items().is_empty(),
        "the deadline fired 1 s before the rest of its fleet time\n{}",
        w.evidence()
    );
    assert_eq!(w.branch(&branch).0, ready);
    w.h.advance_fleet_clock(1);
    w.pass(&run);
    assert_eq!(
        w.overdue_items(),
        [overdue_key(&branch)],
        "the deadline fires after the rest of its fleet time\n{}",
        w.evidence()
    );
}

#[test]
fn pl5_fast_gate_and_full_gate_of_5_min_keep_fleetd_healthy() {
    let w = World::new("p4-pl5-gates");
    if let Some(gap) = exec_gate::sandbox_gap(sandboxed) {
        panic!("this host has no gate sandbox, so no gate runs: {gap}");
    }
    std::fs::write(
        w.h.project.join("fleet.toml"),
        format!("remote = \"none\"\n\n[gates]\nfast = \"{GATE}\"\nfull = \"{GATE}\"\n"),
    )
    .unwrap();
    w.git(&["add", "fleet.toml"]);
    w.git(&["commit", "-q", "--no-gpg-sign", "-m", "gates"]);
    let main = w.git(&["rev-parse", "HEAD"]);
    w.git(&["checkout", "-q", "-b", "candidate", "main"]);
    std::fs::write(w.h.project.join("work.txt"), "work\n").unwrap();
    w.git(&["add", "work.txt"]);
    w.git(&["commit", "-q", "--no-gpg-sign", "-m", "work"]);
    let candidate = w.git(&["rev-parse", "HEAD"]);
    w.git(&["checkout", "-q", "main"]);

    let (fast_feature, fast_run) = w.feature();
    let (full_feature, full_run) = w.feature();
    for feature in [&fast_feature, &full_feature] {
        w.git(&["branch", &format!("fleet/{feature}"), "main"]);
    }
    w.gate_tree(&fast_run, &fast_feature, &main);
    w.gate_tree(&full_run, &full_feature, &main);

    let repo = w.h.project.canonicalize().unwrap();
    let member = format!("fleet/{fast_feature}/work");
    let clock = FleetSeconds(w.fleet_clock());
    let (fast, full) = w.jobs_tx(|ctx, tx| {
        w.done_task_branch(tx, &fast_run, &fast_feature, &member);
        for to in [LandingState::Verifying, LandingState::Ready] {
            landing::enter(ctx, tx, &member, to, clock, "the test readies the branch")?;
        }
        let new = NewBundle {
            feature_id: &fast_feature,
            base_tip: &main,
            parent_id: None,
            members: &[member.as_str()],
            due_at: clock.0,
        };
        let id = integration::insert_bundle(tx, None, &new, cause("the test bundles"))?;
        landing::enter(
            ctx,
            tx,
            &member,
            LandingState::InBundle,
            clock,
            "the test bundles",
        )?;
        let subject = id.to_string();
        transition(
            tx,
            None,
            Entity::Bundle,
            &subject,
            BundleState::Gating.as_str(),
            cause("the test built the candidate"),
            &NeedsYouList,
        )?;
        let due = bundle_rules::due_at(BundleState::Gating, clock.0, &bundles::limits(ctx.config))
            .expect("gating has a limit");
        for fact in [BundleFact::CandidateTip(&candidate), BundleFact::DueAt(due)] {
            integration::set_bundle_fact(
                tx,
                None,
                id,
                fact,
                cause("the test built the candidate"),
            )?;
        }
        let gate = |kind, subject: &str, feature_id: &str, tip: &str| -> anyhow::Result<i64> {
            let req = GateRequest {
                kind,
                subject,
                feature_id,
                tip,
                base_tip: &main,
                repo: &repo,
            };
            match exec_gate::queue(ctx, tx, &req, clock)? {
                Some(Queued::New(job)) => Ok(job),
                other => anyhow::bail!("the {kind} of {feature_id} is not queued: {other:?}"),
            }
        };
        let fast = gate(Kind::FastGate, &subject, &fast_feature, &candidate)?;
        integration::set_bundle_fact(tx, None, id, BundleFact::JobId(fast), cause("the gate"))?;
        let full = gate(Kind::FullGate, &full_feature, &full_feature, &main)?;
        Ok((fast, full))
    });

    let job = |id: i64| jobs_store::get(&w.conn(), id).unwrap().unwrap();
    wait_for_or(
        "fleetd to run both gates",
        || (job(fast).state == State::Running && job(full).state == State::Running).then_some(()),
        || w.evidence(),
    );
    let fleetd = w.fleetds.row().expect("the fleetd row");
    let started = Instant::now();
    let first_beat = read_heartbeat(&w.fleet_root())
        .unwrap()
        .expect("the heartbeat of fleetd")
        .awake;
    let mut beat_during_gates = false;
    let (fast_end, full_end) = loop {
        assert_eq!(
            w.fleetds.row(),
            Some(fleetd),
            "the PID or the start time of the fleetd row changed after {:?} of the gates\n{}",
            started.elapsed(),
            w.evidence()
        );
        assert!(
            procid::alive(fleetd.0, Some(fleetd.1)),
            "fleetd (PID {}) ended after {:?} of the gates\n{}",
            fleetd.0,
            started.elapsed(),
            w.evidence()
        );
        let hb = read_heartbeat(&w.fleet_root())
            .unwrap()
            .expect("the heartbeat of fleetd");
        let (a, b) = (job(fast), job(full));
        if a.state == State::Running && b.state == State::Running && hb.awake != first_beat {
            beat_during_gates = true;
        }
        if a.state != State::Running && b.state != State::Running {
            break (a, b);
        }
        assert!(
            started.elapsed() < Duration::from_secs(420),
            "a gate of `{GATE}` still runs after {:?}\n{}",
            started.elapsed(),
            w.evidence()
        );
        std::thread::sleep(Duration::from_secs(1));
    };
    assert!(
        beat_during_gates,
        "fleetd wrote no heartbeat while both gates ran\n{}",
        w.evidence()
    );
    for end in [&fast_end, &full_end] {
        assert_eq!(
            end.state,
            State::Passed,
            "the {} of `{GATE}` ended {:?}: {:?}",
            end.kind,
            end.state,
            end.result_json
        );
    }
    assert!(
        started.elapsed() >= Duration::from_secs(290),
        "the gates of `{GATE}` ended after only {:?}",
        started.elapsed()
    );
    for (feature, gate, tip) in [
        (&fast_feature, "fast", &candidate),
        (&full_feature, "full", &main),
    ] {
        wait_for_or(
            "fleetd to record the verdict of the gate",
            || {
                integration::gate_run_of(&w.conn(), feature, gate, tip, &main)
                    .unwrap()
                    .and_then(|r| r.result)
                    .filter(|r| r == "pass")
            },
            || w.evidence(),
        );
    }
}
