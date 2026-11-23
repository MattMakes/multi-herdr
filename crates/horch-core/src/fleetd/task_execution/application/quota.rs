use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};

use crate::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::ideas::application::needs_you::NeedsYouList;
use crate::fleetd::task_execution::domain::quota::{exit, released, Exit, Reading, Window};
use crate::fleetd::task_execution::domain::retry::Outcome;
use crate::fleetd::task_execution::domain::task::AttemptState;
use crate::roster::Roster;
use crate::routing::quota::{model_family, pool_for, QuotaView};
use crate::taskledger::model::{Actor, TaskStatus};
use crate::taskledger::workers;
use crate::taskledger::{tasks, util};

pub const USAGE_LIMIT_FALLBACK: &str = "usage_limit_fallback";
pub const USAGE_LIMIT: &str = "usage_limit";
pub const POOL_RESET: &str = "pool_reset";

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

pub struct Pools<'a> {
    pub roster: &'a Roster,
    pub quota: &'a QuotaView,
    pub full_use: f64,
}

impl Pools<'_> {
    pub fn reading(&self, persona: &str) -> Option<Reading> {
        let t = self.roster.get(persona)?;
        let agent = t.agent.as_str();
        let model = t.model.as_deref().unwrap_or_default();
        let pool = pool_for(agent, model);
        let scope = model_family(model);
        let windows = self
            .quota
            .file
            .pools
            .get(pool)
            .map(|p| {
                p.windows
                    .iter()
                    .filter(|w| w.scope_model.is_none() || w.scope_model.as_deref() == scope)
                    .map(|w| Window {
                        used: w.used,
                        reset_at: w
                            .resets_at
                            .as_deref()
                            .and_then(crate::clock::parse)
                            .map(|t| t.timestamp()),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Some(Reading {
            pool: pool.to_string(),
            blocks: self.quota.assess(agent, model).state.blocks(),
            windows,
        })
    }

    fn now(&self) -> i64 {
        self.quota.now.timestamp()
    }

    fn fallback(&self, persona: &str, of_task: Option<&str>) -> Option<String> {
        let t = self.roster.get(persona);
        if t.is_some_and(|t| t.no_fallback_reason.is_some()) {
            return None;
        }
        of_task
            .map(str::to_string)
            .or_else(|| t.and_then(|t| t.profile().fallback.clone()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Moved {
    Restart { persona: String },
    Wait,
}

struct Attempt {
    task_id: Option<String>,
    worker_id: String,
    state: String,
    outcome: Option<String>,
}

fn attempt(conn: &Connection, id: &str) -> Result<Attempt> {
    conn.query_row(
        "SELECT task_id, worker_id, state, outcome FROM attempts WHERE id = ?1",
        [id],
        |r| {
            Ok(Attempt {
                task_id: r.get(0)?,
                worker_id: r.get(1)?,
                state: r.get(2)?,
                outcome: r.get(3)?,
            })
        },
    )
    .optional()?
    .with_context(|| format!("no attempt {id}"))
}

pub fn on_quota(
    tx: &Connection,
    guard: Option<Generation>,
    attempt_id: &str,
    pools: &Pools<'_>,
) -> Result<Option<Moved>> {
    if tx.is_autocommit() {
        bail!("the quota rule writes its rows in 1 open transaction of the ledger");
    }
    let a = attempt(tx, attempt_id)?;
    let quota = Outcome::Quota.as_str();
    if a.state != AttemptState::Ended.as_str() || a.outcome.as_deref() != Some(quota) {
        bail!("attempt {attempt_id} did not end with the outcome {quota}");
    }
    let Some(task_id) = a.task_id else {
        return Ok(None);
    };
    let task = tasks::get(tx, &task_id)?;
    if task.status != TaskStatus::Running.as_str()
        || task.current_worker_id.as_deref() != Some(a.worker_id.as_str())
    {
        return Ok(None);
    }
    let old = workers::get(tx, &a.worker_id)?;
    let (decided, own_pool) = decide(&old, &task, pools);
    tx.execute(
        "UPDATE tasks SET attempt_floor = attempt_floor + 1, current_worker_id = NULL,
            updated_at = ?2
         WHERE task_id = ?1",
        params![task_id, util::now()],
    )?;
    match decided {
        Exit::Restart { persona: fb } => {
            let why = format!("usage limit of pool {own_pool}");
            move_to_fallback(tx, guard, &task, fb, &why).map(Some)
        }
        Exit::Wait => {
            let to = TaskStatus::WaitingQuota.as_str();
            let why = cause(USAGE_LIMIT);
            transition(tx, guard, Entity::Task, &task_id, to, why, &NeedsYouList)?;
            Ok(Some(Moved::Wait))
        }
    }
}

fn decide(old: &workers::Worker, task: &tasks::Task, pools: &Pools<'_>) -> (Exit, String) {
    let persona = old.persona.clone().unwrap_or_default();
    let own = pools.reading(&persona);
    let own_pool = own.map_or_else(String::new, |r| r.pool);
    let fallback = pools.fallback(&persona, task.fallback_persona.as_deref());
    let candidate = fallback
        .as_deref()
        .and_then(|f| Some((f, pools.reading(f)?)));
    let decided = exit(
        &own_pool,
        candidate.as_ref().map(|(f, r)| (*f, r)),
        pools.full_use,
        pools.now(),
    );
    (decided, own_pool)
}

fn move_to_fallback(
    tx: &Connection,
    guard: Option<Generation>,
    task: &tasks::Task,
    fb: String,
    reason: &str,
) -> Result<Moved> {
    let task_id = &task.task_id;
    let to = TaskStatus::Ready.as_str();
    let why = cause(USAGE_LIMIT_FALLBACK);
    transition(tx, guard, Entity::Task, task_id, to, why, &NeedsYouList)?;
    tx.execute(
        "UPDATE tasks SET claimable_by = ?2, fallback_reason = ?3, not_before = NULL
         WHERE task_id = ?1",
        params![task_id, fb, reason],
    )?;
    Ok(Moved::Restart { persona: fb })
}

pub fn release_reset_pools(
    tx: &Connection,
    guard: Option<Generation>,
    pools: &Pools<'_>,
) -> Result<Vec<String>> {
    if tx.is_autocommit() {
        bail!("the quota rule writes its rows in 1 open transaction of the ledger");
    }
    let waiting: Vec<(String, Option<String>)> = {
        let mut stmt = tx.prepare(
            "SELECT task_id, claimable_by FROM tasks WHERE status = ?1 ORDER BY task_id",
        )?;
        let rows = stmt.query_map([TaskStatus::WaitingQuota.as_str()], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    let mut moved = Vec::new();
    for (task_id, persona) in waiting {
        let reading = persona.as_deref().and_then(|p| pools.reading(p));
        if reading.is_some_and(|r| !released(&r, pools.full_use, pools.now())) {
            continue;
        }
        let to = TaskStatus::Ready.as_str();
        transition(
            tx,
            guard,
            Entity::Task,
            &task_id,
            to,
            cause(POOL_RESET),
            &NeedsYouList,
        )?;
        moved.push(task_id);
    }
    Ok(moved)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::{DateTime, Utc};

    use super::*;
    use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
    use crate::fleetd::task_execution::application::{commands, lease};
    use crate::fleetd::task_execution::domain::lease::Limits;
    use crate::fleetd::task_execution::domain::task::Recorded;
    use crate::fleetd::task_execution::domain::task::{attempt_id, Reap};
    use crate::harness::HarnessKind;
    use crate::roster::Teammate;
    use crate::routing::policy::Policy;
    use crate::routing::quota::{PoolReading, QuotaFile, Window as QuotaWindow};
    use crate::taskledger::model::WorktreeMode;
    use crate::taskledger::runs;
    use crate::taskledger::runs::NewRun;
    use crate::taskledger::tasks::NewTask;
    use crate::taskledger::testing::bind_live;
    use crate::taskledger::workers::NewWorker;
    use crate::taskledger::Store;

    const FULL: f64 = 0.98;
    const AGENT: Recorded = Recorded {
        pid: 4_242,
        pid_start: Some(77),
    };

    fn now() -> DateTime<Utc> {
        crate::clock::parse("2026-11-17T12:00:00Z").unwrap()
    }

    fn teammate(name: &str, agent: HarnessKind, model: &str, fallback: Option<&str>) -> Teammate {
        Teammate {
            name: name.to_string(),
            agent,
            model: Some(model.to_string()),
            fallbacks: fallback.map(str::to_string).into_iter().collect(),
            ..Default::default()
        }
    }

    fn roster() -> Roster {
        let mut r = Roster::builtin().unwrap();
        r.insert_for_test(teammate(
            "dev",
            HarnessKind::Claude,
            "claude-opus-5-5",
            Some("alt"),
        ));
        r.insert_for_test(teammate("alt", HarnessKind::Codex, "gpt-5.5", None));
        r.insert_for_test(teammate(
            "same",
            HarnessKind::Claude,
            "claude-sonnet-5-5",
            None,
        ));
        let mut pinned = teammate("pinned", HarnessKind::Claude, "claude-opus-5-5", None);
        pinned.no_fallback_reason = Some("a top-tier seat".into());
        r.insert_for_test(pinned);
        r
    }

    fn pool(used: f64, resets_at: Option<&str>) -> PoolReading {
        PoolReading {
            windows: vec![QuotaWindow::new(
                300,
                used,
                resets_at.map(str::to_string),
                None,
            )],
            observed_at: Some("2026-11-17T11:59:00Z".into()),
            ..Default::default()
        }
    }

    fn quota(claude: PoolReading, codex: PoolReading) -> QuotaView {
        let mut pools = BTreeMap::new();
        pools.insert("claude".to_string(), claude);
        pools.insert("codex".to_string(), codex);
        let file = QuotaFile {
            schema: 1,
            written_at: None,
            pools,
            harnesses: BTreeMap::new(),
        };
        QuotaView::new(file, now(), Policy::default(), true)
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        store: Store,
        task: String,
        worker: String,
    }

    fn fixture(persona: &str, fallback: Option<&str>) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        let (task, worker) = store
            .immediate(|tx| {
                let run = runs::create(
                    tx,
                    dir.path(),
                    &NewRun {
                        slug: "r1".into(),
                        ..Default::default()
                    },
                )?;
                crate::taskledger::schema::run_idea(
                    tx,
                    &run.run_id,
                    &crate::taskledger::util::now(),
                    true,
                )?;
                let mut new = NewTask::work("a", "tickets/a.md", persona);
                new.fallback = fallback.map(str::to_string);
                new.worktree_mode = WorktreeMode::Shared;
                let task = tasks::add(tx, &run, &new, Actor::Orchestrator)?.task_id;
                let w = NewWorker {
                    run_id: run.run_id.clone(),
                    persona: persona.into(),
                    pane_id: Some("w0:p1".into()),
                    workspace_id: Some("w0".into()),
                    ..Default::default()
                };
                let id = bind_live(tx, &task, &w, Some(AGENT))?.worker_id;
                tx.execute(
                    "UPDATE workers SET supervisor_pid = 4241, supervisor_started = 76
                     WHERE worker_id = ?1",
                    [&id],
                )?;
                Ok((task, id))
            })
            .unwrap();
        let mut f = Fixture {
            _dir: dir,
            store,
            task,
            worker,
        };
        f.end_with_quota();
        f
    }

    impl Fixture {
        fn first_attempt(&self) -> String {
            attempt_id(&self.task, 1)
        }

        fn end_with_quota(&mut self) {
            let reap = Reap {
                attempt: self.first_attempt(),
                worker: self.worker.clone(),
                outcome: Outcome::Quota.as_str().to_string(),
                end_worker: true,
                task: None,
                cleanup: Default::default(),
            };
            self.store
                .immediate(|tx| commands::end_attempt(tx, &reap).map(|_| ()))
                .unwrap();
        }

        fn on_quota(&mut self, q: &QuotaView) -> Option<Moved> {
            let roster = roster();
            let pools = Pools {
                roster: &roster,
                quota: q,
                full_use: FULL,
            };
            let a = self.first_attempt();
            self.store
                .immediate(|tx| on_quota(tx, None, &a, &pools))
                .unwrap()
        }

        fn release(&mut self, q: &QuotaView) -> Vec<String> {
            let roster = roster();
            let pools = Pools {
                roster: &roster,
                quota: q,
                full_use: FULL,
            };
            self.store
                .immediate(|tx| release_reset_pools(tx, None, &pools))
                .unwrap()
        }

        fn task(&self) -> tasks::Task {
            tasks::get(self.store.conn(), &self.task).unwrap()
        }

        fn lease(&self) -> Option<i64> {
            self.store
                .conn()
                .query_row(
                    "SELECT expires_at FROM leases WHERE task_id = ?1",
                    [&self.task],
                    |r| r.get(0),
                )
                .optional()
                .unwrap()
        }

        fn causes(&self) -> Vec<(String, String, String)> {
            let mut stmt = self
                .store
                .conn()
                .prepare(
                    "SELECT from_state, to_state, cause FROM events
                     WHERE entity = 'task' AND entity_id = ?1 AND from_state IS NOT NULL
                     ORDER BY rowid",
                )
                .unwrap();
            stmt.query_map([&self.task], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        }
    }

    fn claude_full() -> QuotaView {
        quota(pool(1.0, Some("2026-11-17T14:00:00Z")), pool(0.1, None))
    }

    fn both_full() -> QuotaView {
        quota(pool(1.0, Some("2026-11-17T14:00:00Z")), pool(0.99, None))
    }

    #[test]
    fn fm5_usage_limit_burns_no_attempt_and_moves_to_fallback() {
        let mut f = fixture("dev", None);
        let before = f.task();
        let moved = f.on_quota(&claude_full());
        assert_eq!(
            moved,
            Some(Moved::Restart {
                persona: "alt".into()
            })
        );
        let t = f.task();
        assert_eq!(t.status, "ready");
        assert_eq!(t.claimable_by.as_deref(), Some("alt"));
        assert_eq!(t.current_worker_id, None);
        assert_eq!(
            t.attempts, before.attempts,
            "tasks.attempts does not change"
        );
        assert_eq!(t.attempt_floor, before.attempt_floor + 1);
        assert_eq!(
            t.attempts - t.attempt_floor,
            before.attempts - before.attempt_floor - 1
        );
        assert_eq!(f.lease(), None, "a ready task holds no lease");
        let last = f.causes().pop().unwrap();
        assert_eq!(
            last,
            (
                "running".into(),
                "ready".into(),
                USAGE_LIMIT_FALLBACK.into()
            )
        );
        assert_eq!(t.not_before, None, "a usage limit sets no backoff");
        let attempts: i64 = f
            .store
            .conn()
            .query_row("SELECT count(*) FROM attempts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(attempts, 1, "only the attempt that hit the limit");
        assert_eq!(f.on_quota(&claude_full()), None);
    }

    #[test]
    fn fallback_on_the_same_pool_or_no_fallback_waits() {
        let mut f = fixture("dev", Some("same"));
        assert_eq!(f.on_quota(&claude_full()), Some(Moved::Wait));
        assert_eq!(f.task().status, "waiting_quota");
        let mut f = fixture("dev", None);
        assert_eq!(f.on_quota(&both_full()), Some(Moved::Wait));
    }

    #[test]
    fn persona_with_a_no_fallback_reason_waits_in_waiting_quota() {
        let mut f = fixture("pinned", Some("alt"));
        assert_eq!(f.on_quota(&claude_full()), Some(Moved::Wait));
        let t = f.task();
        assert_eq!(t.status, "waiting_quota");
        assert_eq!(t.claimable_by.as_deref(), Some("pinned"));
    }

    #[test]
    fn transition_running_to_waiting_quota_pauses_the_lease() {
        let mut f = fixture("same", None);
        let before = f.task();
        let lease = f.lease().expect("the claim wrote a lease");
        assert_eq!(f.on_quota(&claude_full()), Some(Moved::Wait));
        let t = f.task();
        assert_eq!(t.status, "waiting_quota");
        assert_eq!(t.attempts, before.attempts);
        assert_eq!(t.attempt_floor, before.attempt_floor + 1);
        let last = f.causes().pop().unwrap();
        assert_eq!(
            last,
            ("running".into(), "waiting_quota".into(), USAGE_LIMIT.into())
        );
        let limits = |_: &str| Limits {
            lease_s: 120,
            stall_after_s: 300,
            heartbeat_max_age_s: 30,
        };
        let expired = f
            .store
            .immediate(|tx| {
                lease::renew_or_expire_leases(
                    tx,
                    None,
                    FleetSeconds(lease + 100_000),
                    &BTreeMap::new(),
                    &limits,
                )
            })
            .unwrap();
        assert!(expired.is_empty());
        assert_eq!(f.lease(), Some(lease));
        assert_eq!(f.task().status, "waiting_quota");
        assert!(f.release(&claude_full()).is_empty());
        let after_reset = quota(pool(1.0, Some("2026-11-17T11:00:00Z")), pool(0.1, None));
        assert_eq!(f.release(&after_reset), vec![f.task.clone()]);
        assert_eq!(f.task().status, "ready");
        assert_eq!(f.lease(), None);
        let last = f.causes().pop().unwrap();
        assert_eq!(
            last,
            ("waiting_quota".into(), "ready".into(), POOL_RESET.into())
        );
    }

    #[test]
    fn waiting_task_without_reset_at_is_released_when_the_probe_reports_ok() {
        let mut f = fixture("same", None);
        let limited = quota(pool(1.0, None), pool(0.1, None));
        assert_eq!(f.on_quota(&limited), Some(Moved::Wait));
        assert!(f.release(&limited).is_empty());
        let ok = quota(pool(0.2, None), pool(0.1, None));
        assert_eq!(f.release(&ok), vec![f.task.clone()]);
    }

    #[test]
    fn pres_06_s13_quota_probe_holds_no_credential() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("quota.json");
        let file = QuotaFile {
            schema: 1,
            written_at: None,
            pools: [(
                "claude".to_string(),
                pool(1.0, Some("2026-11-17T14:00:00Z")),
            )]
            .into_iter()
            .collect(),
            harnesses: BTreeMap::new(),
        };
        std::fs::write(&path, serde_json::to_string(&file).unwrap()).unwrap();
        let loaded = QuotaFile::load(dir.path(), Some(&path)).unwrap();
        let view = QuotaView::new(loaded, now(), Policy::default(), true);
        let roster = roster();
        let pools = Pools {
            roster: &roster,
            quota: &view,
            full_use: FULL,
        };
        let dev = pools.reading("dev").unwrap();
        assert_eq!(dev.pool, "claude");
        assert_eq!(dev.windows.len(), 1);
        assert_eq!(dev.windows[0].used, 1.0);
        let alt = pools.reading("alt").unwrap();
        assert_eq!((alt.pool.as_str(), alt.windows.len()), ("codex", 0));
        let src = include_str!("quota.rs");
        let code: Vec<&str> = src
            .split("#[cfg(test)]")
            .next()
            .unwrap()
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect();
        for api in [
            "std::env",
            "env::var",
            "std::process",
            "Command::new",
            "keychain",
            "ANTHROPIC_API_KEY",
        ] {
            let hit = code.iter().find(|l| l.contains(api));
            assert!(hit.is_none(), "quota.rs uses {api}: {hit:?}");
        }
    }
}
