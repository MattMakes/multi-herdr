//! The e2e tests of exec mode (fleetd-p7 ticket 33; FM-20, FR-6 part,
//! FR-9 part, RR-11; `docs/specs/fleetd/design.md`, "Worker profiles and
//! adapters" (Exec or interactive)).
//!
//! Each test makes its own ledger, fleet root and git project in a temp
//! folder, seeds 1 ready task, and starts a test `horch fleetd` with 1 pass
//! each second. fleetd binds the task, launches its attempt in a pane of the
//! fake herdr, and the wrapper `horch worker` runs the fake CLI of the
//! profile in exec mode. No test starts a real agent CLI, and each test
//! waits for an event (a row, a call of a fake), never for a time.
//!
//! The binary of each CLI is a small script around its fake. A conformance
//! run of fleetd (its prompt is under `conformance/`) gets the scenario
//! `conformance`, so each profile stays healthy. The exec run of each
//! attempt gets the scenario that the test gives for its attempt number.
//!
//! Unix only: the scripts are `sh`.

#![cfg(unix)]

use std::path::PathBuf;
use std::process::Output;

use horch_core::fleetd::task_execution::application::retry::CONTEXT_LIMIT_LOOP;
use horch_core::taskledger::model::{Actor, WorkerKind};
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{schema, Store};
use horch_e2e::harness::{fixtures, wait_for_or, Harness};
use serde_json::Value;

/// The `fleet.toml` of each test: 1 pass each second.
const FLEET_TOML: &str = "pass_period_s = 1\n";

/// The scenario of a run that does the task: 1 commit, then the result.
const DOES_THE_TASK: &str = "commit,result";

/// The context window of each new attempt (FR-9).
const WINDOW: &str = "256000";

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// A CLI with an exec adapter: the name of its fake in the call log, the
/// fake binary, and the variable that names its binary for horch.
#[derive(Clone, Copy)]
enum Cli {
    Claude,
    Codex,
    Agy,
}

impl Cli {
    const ALL: [Cli; 3] = [Cli::Claude, Cli::Codex, Cli::Agy];

    fn fake(self) -> &'static str {
        match self {
            Cli::Claude => "claude",
            Cli::Codex => "codex",
            Cli::Agy => "antigravity",
        }
    }

    fn binary(self) -> &'static str {
        match self {
            Cli::Claude => "fake-claude",
            Cli::Codex => "fake-codex",
            Cli::Agy => "fake-antigravity",
        }
    }

    fn bin_env(self) -> &'static str {
        match self {
            Cli::Claude => "HORCH_CLAUDE_BIN",
            Cli::Codex => "HORCH_CODEX_BIN",
            Cli::Agy => "HORCH_ANTIGRAVITY_BIN",
        }
    }
}

/// The script of the binary of 1 CLI. `$1` of a warm call is `warm`. The
/// exec run of the attempt `A<n>` gets `runs[n - 1]`, and the last entry
/// when `runs` is shorter. The script reads `n` from the path of the
/// `START.md` of the attempt: it writes no file, so the sandbox of the CLI
/// refuses nothing.
fn cli_script(fake: &std::path::Path, runs: &[&str]) -> String {
    let mut pick = String::new();
    for (i, s) in runs.iter().enumerate() {
        pick.push_str(&format!("  {}) s='{s}' ;;\n", i + 1));
    }
    let last = runs.last().copied().unwrap_or("default");
    format!(
        "#!/bin/sh\n\
         [ \"$1\" = warm ] && exit 0\n\
         case \"$*\" in\n\
         */conformance/*) s=conformance ;;\n\
         */attempts/A*/inbox/START.md*)\n\
         n=$(printf '%s\\n' \"$@\" | sed -n 's|.*/attempts/A\\([0-9]*\\)/inbox/START.md.*|\\1|p' | head -n 1)\n\
         case \"$n\" in\n{pick}  *) s='{last}' ;;\n esac ;;\n\
         *) s=default ;;\n\
         esac\n\
         HORCH_FAKE_SCENARIO=\"$s\" exec '{fake}' \"$@\"\n",
        fake = fake.display(),
    )
}

/// 1 task of a persona, its fallback, and the scenarios of each CLI, by
/// attempt number.
struct Spec<'a> {
    persona: &'a str,
    fallback: Option<&'a str>,
    runs: &'a [(Cli, &'a [&'a str])],
}

/// A fleet with 1 ready task, a test fleetd and the workspace `w1`, whose
/// panes run their commands.
struct World {
    h: Harness,
    task_id: String,
}

impl World {
    fn new(name: &str, spec: &Spec<'_>) -> World {
        let mut h = Harness::new(name).with_git();
        let fleet = h.root.join("fleet");
        std::fs::create_dir_all(&fleet).unwrap();
        std::fs::write(fleet.join("fleet.toml"), FLEET_TOML).unwrap();
        // The fake agent runs `horch result` and `horch tell` from PATH.
        std::os::unix::fs::symlink(Harness::horch_bin(), h.bin.join("horch")).unwrap();
        h.set(
            "HORCH_LEDGER_DB",
            h.root.join("ledger.sqlite3").to_string_lossy(),
        );
        h.set("FLEET_ROOT", fleet.to_string_lossy());
        h.set("HORCH_NO_WAKE", "1");
        h.set("HORCH_NO_WATCHDOG", "1");
        h.set(
            "HORCH_QUOTA_FILE",
            fixtures().join("quota/all-ok.json").to_string_lossy(),
        );
        h.set("HORCH_WORKSPACE_ID", "w1");
        // `exec`: the fake herdr runs the command of each pane.
        h.set("HORCH_FAKE_SCENARIO", "exec");
        let built = Harness::horch_bin().parent().unwrap().to_path_buf();
        for cli in Cli::ALL {
            let runs = spec
                .runs
                .iter()
                .find(|(c, _)| c.fake() == cli.fake())
                .map_or(&[DOES_THE_TASK][..], |(_, r)| *r);
            let script = cli_script(&built.join(cli.binary()), runs);
            let path = h.write_warmed(&format!("{}-p7", cli.fake()), script.as_bytes());
            h.set(cli.bin_env(), path.to_string_lossy());
        }
        let mut w = World {
            h,
            task_id: String::new(),
        };
        w.declare_gates();
        w.task_id = w.seed_task(spec);
        w.h.serve_fleetd();
        let mut create = std::process::Command::new(w.h.bin.join("herdr"));
        w.h.seal(&mut create);
        let o = create
            .args(["workspace", "create", "--label", name])
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", text(&o));
        w
    }

    fn fleet_root(&self) -> PathBuf {
        self.h.root.join("fleet")
    }

    /// The project declares its gates, so no hold of a missing fast gate
    /// stops the dispatch (Phase 4, Q-C).
    fn declare_gates(&self) {
        std::fs::write(
            self.h.project.join("fleet.toml"),
            "[gates]\nfast = \"true\"\nfull = \"true\"\n",
        )
        .unwrap();
        for args in [
            &["add", "fleet.toml"][..],
            &["commit", "--quiet", "--no-gpg-sign", "-m", "gates"],
        ] {
            let o = self.h.git_cmd(args).output().unwrap();
            assert!(o.status.success(), "git {args:?}: {}", text(&o));
        }
    }

    /// The run, its idea in flight (FR-12), the pane that fleetd splits
    /// for a launch, and 1 queued task. The test writes them before fleetd
    /// runs. Gives the task id.
    fn seed_task(&self, spec: &Spec<'_>) -> String {
        let ticket = "# T0\n";
        let dir = self.h.project.join("tickets");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("t0.md"), ticket).unwrap();
        let store = Store::open(&self.h.ledger_db()).unwrap();
        let conn = store.conn();
        let run = runs::create(
            conn,
            &self.h.project,
            &NewRun {
                slug: "r1".into(),
                base_branch: Some("main".into()),
                workspace_id: Some("w1".into()),
                ..Default::default()
            },
        )
        .unwrap();
        schema::run_idea(conn, &run.run_id, &horch_core::clock::now_stamp(), true)
            .unwrap()
            .expect("the idea of r1");
        let seat = workers::insert(
            conn,
            &NewWorker {
                run_id: run.run_id.clone(),
                kind: Some(WorkerKind::Orchestrator),
                persona: "orchestrator".into(),
                workspace_id: Some("w1".into()),
                pane_id: Some("w1:p1".into()),
                ..Default::default()
            },
        )
        .unwrap();
        runs::set_orchestrator(conn, &run.run_id, &seat).unwrap();
        let mut new = NewTask::work("t0", "tickets/t0.md", spec.persona);
        new.fallback = spec.fallback.map(str::to_string);
        new.ticket_snapshot = Some(ticket.to_string());
        new.land_check = Some("true".into());
        tasks::add(conn, &run, &new, Actor::Orchestrator)
            .unwrap()
            .task_id
    }

    /// Rows of 1 text column of the ledger.
    fn rows(&self, sql: &str) -> Vec<String> {
        let conn = rusqlite::Connection::open(self.h.ledger_db()).unwrap();
        let mut stmt = conn.prepare(sql).unwrap();
        stmt.query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }

    fn task_status(&self) -> String {
        self.rows("SELECT status FROM tasks")
            .pop()
            .unwrap_or_default()
    }

    /// The attempts that the task used: `attempts - attempt_floor`.
    fn used(&self) -> String {
        self.rows("SELECT CAST(attempts - attempt_floor AS TEXT) FROM tasks")
            .pop()
            .unwrap_or_default()
    }

    /// `<attempt> <state> <outcome>` of each attempt of the task, oldest
    /// first.
    fn attempts(&self) -> Vec<String> {
        self.rows(
            "SELECT substr(id, instr(id, '/') + 1) || ' ' || state || ' ' || coalesce(outcome, '-')
             FROM attempts WHERE task_id IS NOT NULL ORDER BY rowid",
        )
    }

    /// The exec runs of the attempts by `cli`: each call of its fake with
    /// a `START.md` that is not a conformance run, oldest first.
    fn exec_runs(&self, cli: Cli) -> Vec<Value> {
        self.h
            .calls_of(cli.fake())
            .into_iter()
            .filter(|c| {
                c.get("exec_prompt").is_some() && !c["argv"].to_string().contains("/conformance/")
            })
            .collect()
    }

    /// What a failed wait shows.
    fn diagnostics(&self) -> String {
        let mut calls: Vec<String> = self
            .h
            .calls()
            .iter()
            .filter(|c| !c["argv"].to_string().contains("/conformance/"))
            .map(|c| format!("{} {}", c["fake"], c["argv"]))
            .collect();
        calls.dedup();
        calls.truncate(60);
        for cli in Cli::ALL {
            for run in self.exec_runs(cli) {
                let mut acts = run.clone();
                if let Some(o) = acts.as_object_mut() {
                    o.retain(|k, _| {
                        ["fake", "commit", "ask_file", "result", "violations"].contains(&k.as_str())
                    });
                }
                calls.push(format!("exec run: {acts}"));
            }
        }
        let read = |p: PathBuf| std::fs::read_to_string(p).unwrap_or_default();
        format!(
            "calls:\n{}\nfleetd.stderr:\n{}\nattempts: {:?}\ntask: {}\nevents:\n{}\nfleet events:\n{}",
            calls.join("\n"),
            read(self.h.root.join("fleetd.stderr")),
            self.attempts(),
            self.task_status(),
            self.rows(
                "SELECT kind || ' ' || text FROM events
                 WHERE task_id IS NOT NULL ORDER BY event_id"
            )
            .join("\n"),
            self.rows(
                "SELECT kind || ' ' || substr(text, 1, 200) FROM events
                 WHERE task_id IS NULL AND kind <> 'profile_unhealthy'
                 ORDER BY event_id DESC LIMIT 20"
            )
            .join("\n"),
        )
    }

    fn wait<T>(&self, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
        wait_for_or(what, &mut f, || self.diagnostics())
    }

    fn wait_status(&self, status: &str) {
        self.wait(&format!("the task to be {status}"), || {
            (self.task_status() == status).then_some(())
        });
    }

    /// Wait for the attempt `n` (`A<n>`) to end, and give its outcome.
    fn wait_ended(&self, n: usize) -> String {
        let id = format!("A{n} ended ");
        self.wait(&format!("the attempt A{n} to end"), || {
            self.attempts()
                .into_iter()
                .find_map(|a| a.strip_prefix(&id).map(str::to_string))
        })
    }

    /// The output of the act `key` (`result` or `ask`) of the exec run of
    /// the attempt `A<n>` on `cli`, once that run did it.
    fn act(&self, cli: Cli, n: usize, key: &str) -> Value {
        self.wait(&format!("the exec run of A{n} to do its {key}"), || {
            self.exec_runs(cli)
                .into_iter()
                .find(|r| {
                    r["argv"]
                        .to_string()
                        .contains(&format!("/attempts/A{n}/inbox/START.md"))
                })
                .and_then(|r| r.get(key).cloned())
        })
    }

    /// The agent of the attempt `A<n>` on `cli` wrote its `result.json`
    /// with `horch result`.
    fn assert_result_written(&self, cli: Cli, n: usize) {
        let out = self.act(cli, n, "result");
        assert_eq!(
            out["exit"], 0,
            "the agent of A{n} could not write its result: {}",
            out["stderr"]
        );
    }

    fn needs_you_open(&self, kind: &str) -> Vec<String> {
        self.rows(&format!(
            "SELECT key FROM needs_you WHERE kind = '{kind}' AND state = 'open'"
        ))
    }
}

/// The 1 task of `persona` runs in exec mode on `cli` and reaches `done`.
fn task_runs_to_done(name: &str, persona: &str, cli: Cli) {
    let w = World::new(
        name,
        &Spec {
            persona,
            fallback: None,
            runs: &[(cli, &[DOES_THE_TASK])],
        },
    );
    // The wrapper ran the CLI in exec mode with the START.md of the
    // attempt (FR-6: the mode of the profile decides the launch).
    w.assert_result_written(cli, 1);
    w.wait_status("done");
    let runs = w.exec_runs(cli);
    assert_eq!(runs.len(), 1, "{}", w.diagnostics());
    assert!(
        runs[0]["exec_prompt"]
            .as_str()
            .unwrap_or_default()
            .contains("T0"),
        "{}",
        runs[0]
    );
    assert_eq!(w.attempts(), ["A1 ended done"], "{}", w.diagnostics());
    assert_eq!(w.used(), "1", "{}", w.diagnostics());
}

#[test]
fn exec_claude_task_runs_to_done() {
    horch_e2e::expected::check(file!(), "exec_claude_task_runs_to_done", || {
        task_runs_to_done("p7-exec-claude", "opus", Cli::Claude);
    });
}

#[test]
fn exec_codex_task_runs_to_done() {
    horch_e2e::expected::check(file!(), "exec_codex_task_runs_to_done", || {
        task_runs_to_done("p7-exec-codex", "codex-sol", Cli::Codex);
    });
}

#[test]
fn exec_agy_task_runs_to_done() {
    horch_e2e::expected::check(file!(), "exec_agy_task_runs_to_done", || {
        task_runs_to_done("p7-exec-agy", "antigravity", Cli::Agy);
    });
}

/// A task of `opus` with the fallback `codex-sol`: the exec run of A1 on
/// Claude Code shows a usage limit, and A2 on Codex does the task.
fn usage_limit_world(name: &str) -> World {
    World::new(
        name,
        &Spec {
            persona: "opus",
            fallback: Some("codex-sol"),
            runs: &[
                (Cli::Claude, &["usage_limit"]),
                (Cli::Codex, &[DOES_THE_TASK]),
            ],
        },
    )
}

/// RR-11 in exec mode: the stream of Claude Code shows a usage limit. The
/// wrapper ends the attempt with `quota`, the attempt burns nothing, and
/// fleetd restarts the task on its fallback `codex-sol`, which does it.
#[test]
fn exec_usage_limit_restarts_on_the_fallback_and_burns_no_attempt() {
    horch_e2e::expected::check(
        file!(),
        "exec_usage_limit_restarts_on_the_fallback_and_burns_no_attempt",
        || {
            let w = usage_limit_world("p7-exec-quota");
            assert_eq!(w.wait_ended(1), "quota", "{}", w.diagnostics());
            w.assert_result_written(Cli::Codex, 2);
            w.wait_status("done");
            assert_eq!(
                w.attempts(),
                ["A1 ended quota", "A2 ended done"],
                "{}",
                w.diagnostics()
            );
            assert_eq!(w.exec_runs(Cli::Claude).len(), 1, "{}", w.diagnostics());
            assert_eq!(w.exec_runs(Cli::Codex).len(), 1, "{}", w.diagnostics());
            // Only the attempt on the fallback counts.
            assert_eq!(w.used(), "1", "{}", w.diagnostics());
        },
    );
}

/// An exec agent that asks a question exits. Its attempt ends with
/// `asked`, and the task waits in `blocked_on_user`.
#[test]
fn exec_question_exits_and_the_task_waits_in_blocked_on_user() {
    horch_e2e::expected::check(
        file!(),
        "exec_question_exits_and_the_task_waits_in_blocked_on_user",
        || {
            let w = World::new(
                "p7-exec-ask",
                &Spec {
                    persona: "opus",
                    fallback: None,
                    runs: &[(Cli::Claude, &["ask_file"])],
                },
            );
            let ask = w.act(Cli::Claude, 1, "ask_file");
            assert_eq!(
                ask["exit"], 0,
                "the agent of A1 could not write its question file: {}",
                ask["stderr"]
            );
            assert_eq!(w.wait_ended(1), "asked", "{}", w.diagnostics());
            w.wait_status("blocked_on_user");
            let found = walk(&w.fleet_root().join("tasks"))
                .into_iter()
                .any(|p| p.parent().is_some_and(|d| d.ends_with("questions")));
            assert!(found, "no question file: {}", w.diagnostics());
            assert_eq!(w.exec_runs(Cli::Claude).len(), 1, "{}", w.diagnostics());
        },
    );
}

/// FM-20: Antigravity has no compaction setting. Its stream passes 256000
/// context tokens, so the wrapper ends the attempt with `context_limit`.
/// The attempt burns nothing, and the next attempt gets the handoff file
/// in the same worktree and does the task.
#[test]
fn agy_long_context_ends_with_context_limit_and_the_next_attempt_has_the_handoff() {
    horch_e2e::expected::check(
        file!(),
        "agy_long_context_ends_with_context_limit_and_the_next_attempt_has_the_handoff",
        || {
            let w = World::new(
                "p7-exec-agy-limit",
                &Spec {
                    persona: "antigravity",
                    fallback: None,
                    runs: &[(Cli::Agy, &["long_context", DOES_THE_TASK])],
                },
            );
            assert_eq!(w.wait_ended(1), "context_limit", "{}", w.diagnostics());
            let runs = w.wait("the exec run of A2", || {
                let runs = w.exec_runs(Cli::Agy);
                (runs.len() >= 2).then_some(runs)
            });
            let prompt = runs[1]["exec_prompt"].as_str().unwrap_or_default();
            assert!(
                prompt.contains("handoff.md"),
                "the START.md of A2 names no handoff file:\n{prompt}"
            );
            // Both attempts work in the 1 worktree of the task.
            let trees = w.rows("SELECT DISTINCT path FROM worktrees WHERE task_id IS NOT NULL");
            assert_eq!(trees.len(), 1, "{trees:?}\n{}", w.diagnostics());
            w.assert_result_written(Cli::Agy, 2);
            w.wait_status("done");
            assert_eq!(
                w.attempts(),
                ["A1 ended context_limit", "A2 ended done"],
                "{}",
                w.diagnostics()
            );
            assert_eq!(w.exec_runs(Cli::Agy).len(), 2, "{}", w.diagnostics());
            // Only the attempt after the context limit counts.
            assert_eq!(w.used(), "1", "{}", w.diagnostics());
        },
    );
}

/// FM-20, the guard of MAP.md Q2: 2 attempts in a row end with
/// `context_limit` and the task branch has no new commit between them. The
/// task does not go to `ready` again, and 1 needs-you item opens.
#[test]
fn context_limit_loop_with_no_commit_opens_1_needs_you_item() {
    let w = World::new(
        "p7-exec-agy-loop",
        &Spec {
            persona: "antigravity",
            fallback: None,
            runs: &[(Cli::Agy, &["long_context"])],
        },
    );
    // 1 wait for each step, so each step has the whole deadline.
    assert_eq!(w.wait_ended(1), "context_limit", "{}", w.diagnostics());
    assert_eq!(w.wait_ended(2), "context_limit", "{}", w.diagnostics());
    let key = format!("{CONTEXT_LIMIT_LOOP}:{}", w.task_id);
    w.wait("the needs-you item of the context limit loop", || {
        w.needs_you_open(CONTEXT_LIMIT_LOOP)
            .contains(&key)
            .then_some(())
    });
    assert_eq!(
        w.needs_you_open(CONTEXT_LIMIT_LOOP),
        [key],
        "{}",
        w.diagnostics()
    );
    assert_eq!(
        w.attempts(),
        ["A1 ended context_limit", "A2 ended context_limit"],
        "{}",
        w.diagnostics()
    );
    assert_ne!(w.task_status(), "ready", "{}", w.diagnostics());
    assert_ne!(w.task_status(), "queued", "{}", w.diagnostics());
    assert_eq!(w.exec_runs(Cli::Agy).len(), 2, "{}", w.diagnostics());
    // No attempt burned: the 2 context limits count as none.
    assert_eq!(w.used(), "0", "{}", w.diagnostics());
}

/// FR-9 (part): each new attempt gets the window 256000. A1 runs on Claude
/// Code and ends on a usage limit, and A2 runs on Codex.
#[test]
fn new_attempt_gets_the_256000_window() {
    horch_e2e::expected::check(file!(), "new_attempt_gets_the_256000_window", || {
        let w = usage_limit_world("p7-exec-window");
        let claude = w.wait("the exec run of A1 on Claude Code", || {
            w.exec_runs(Cli::Claude).into_iter().next()
        });
        let claude_argv = claude["argv"].to_string();
        assert!(
            claude_argv.contains(&format!("CLAUDE_CODE_AUTO_COMPACT_WINDOW\\\":\\\"{WINDOW}")),
            "A1 on Claude Code: {claude_argv}"
        );
        assert_eq!(w.wait_ended(1), "quota", "{}", w.diagnostics());
        let codex = w.wait("the exec run of A2 on Codex", || {
            w.exec_runs(Cli::Codex).into_iter().next()
        });
        let codex_argv = codex["argv"].to_string();
        assert!(
            codex_argv.contains(&format!("model_auto_compact_token_limit={WINDOW}")),
            "A2 on Codex: {codex_argv}"
        );
    });
}

/// Each file under `dir`, at any depth.
fn walk(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                todo.push(p);
            } else {
                found.push(p);
            }
        }
    }
    found
}
