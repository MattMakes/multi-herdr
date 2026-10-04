# F5 candidate-idle: report

Unit F5, single-branch work on `design-skills`, worker opus-61,
2026-10-04. Plan: `ai_docs/plans/finish/f5-candidate-idle.md`. Evidence
that started it: `ai_docs/reports/finish/acceptance-dataset.md`, finding
F4 (codex-terra could not commit in its worktree, stopped without
`horch done`, and held its slot for the 900 s deadline).

## Result

| Item | Status | Commits |
|---|---|---|
| 1. Codex candidates without `.git` writes | done | 19e002a, 87ea4dd |
| 2. Idle nudge and end, config keys | done | a9cec5b, f9bb7e2 |
| 3. Fleet Codex worker nudge | decided: no; reason recorded in the design | 8ba6cd6 |
| 4. PRE-14 honours `CLAUDE_CONFIG_DIR` | done | 74ae219, 98f35c7 |
| 5. Exit 3 for a plan with no candidate | done | 09cf4d6 |
| Design text | done | 5ee614b, 8ba6cd6, 98f35c7 |

## 1. Codex candidates in worktrees

- `teammates/_base/competition-candidate.md` now says: "Do not commit.
  Leave your changes in the work tree, including new files. The coordinator
  commits your work when you finish. Your sandbox may not allow git to
  write here." The done step says "When the work is complete" (no longer
  "complete and committed").
- The freeze path already handles uncommitted and untracked work:
  `WorktreeManager::freeze` runs `commit_all` (all changes and untracked
  files, fixed freeze identity), then reads the head from the main
  repository. No code change was needed there.
- The Codex sandbox is not widened to the common `.git` (plan rule).
- Test: the fake candidate mode gets `commit: "blocked"`
  (`crates/horch-e2e/src/lib.rs`): it runs `git add -A` and `git commit`
  with `GIT_INDEX_FILE=/dev/null/index`, so both fail with no side effect,
  and records `commit_failed`. `cmp_07_blocked_commit_work_is_frozen`
  checks: the codex candidate's commit failed, both candidates are frozen
  with the changed tracked file (`notes.txt`) and the new file (`new.txt`),
  both are eligible, and each branch holds exactly one commit on the base,
  `candidate <label> frozen`.

## 2. Idle nudge and end

- Config (`competition/config.rs`, `Caps`): `idle_nudge_after_s` = 120,
  `idle_end_after_s` = 180; 0 turns the rule off; `validate` refuses a
  nudge period with an end period of 0. Reason for the periods: the LA
  candidates finished a small task in 20 to 45 s, so 2 minutes at the
  prompt means the agent stopped; 3 more minutes is one turn to run
  `horch done`; 5 minutes in total is far below the deadline (900 s in the
  LA config, 3600 s by default).
- Rule (`competition/observe.rs:IdleWatch::look`): `idle` starts or
  continues a stretch; any other status ends it. After the nudge period the
  coordinator types `IDLE_NUDGE` once with `messaging::delivery::send_line`
  (herdr `agent prompt` first). After the nudge, an idle stretch of the end
  period ends the candidate as `Failed(Cancelled{reason:
  "idle_without_done"})` through `end_candidate` (record, pane, event). Work
  restarts the stretch but never earns a second nudge.
- Decisions:
  - The ended candidate is frozen and validated but not eligible: without
    `horch done` nobody says it finished. With item 1, a Codex candidate no
    longer stops at a failed commit, so this should be rare.
  - The idle state is in memory only. A resumed coordinator counts again.
    No event records the nudge, so the event formats and
    `schema_version` stay unchanged; stderr reports it.
  - herdr reports `idle` only for a detected agent, so a pane without
    agent detection (or the e2e fake-herdr) is never nudged.
- Tests: `idle_watch_nudges_once_then_ends` (unit),
  `idle_candidate_is_nudged_once_then_ends` and
  `idle_rule_off_waits_for_the_deadline` (`tests/coordinator.rs`, with
  FakeWorkspace `set_agent_states` and a round clock that moves 60 s per
  tick), `idle_periods_default_parse_and_validate` (config).
  `round_with` now delegates to `round_cfg`, which takes a config tweak
  and returns the workspace calls.

## 3. Fleet Codex workers

Not done, on purpose. A fleet worker at its prompt is often waiting
correctly: after `QUESTION:` the protocol says wait for the orchestrator and
never run `horch done` while the question is open. A nudge to finish would
push it to stop early. The orchestrator already sees each pane's
`agent_status` and the ledger notes, so it has the context to nudge. The
fleet's Codex commit problem is handled by assignment: git-writing units go
to Claude teammates. Recorded in dataset design 4.11.5.

## 4. `CLAUDE_CONFIG_DIR`

Checked on claude 2.1.289 with no model call: with
`CLAUDE_CONFIG_DIR=<dir>`, `claude config list` wrote `<dir>/.claude.json`
(plus `backups/`, `projects/`, `sessions/`). `RuntimeContext.inherited`
gets `claude_config_dir`; `claude_config_file(home, dir)` (now in
`harness/trust.rs` after the orchestrator's move, 9e79de5) picks
`$CLAUDE_CONFIG_DIR/.claude.json`, else `~/.claude.json`. The reason label
is `$CLAUDE_CONFIG_DIR/.claude.json`, like `$CODEX_HOME/config.toml`. Test
`claude_trust_follows_claude_config_dir` (trusted only in the config dir,
then only in home; both directions). The context test pins the new field.

## 5. Empty plan exit

`run` exits 3 (budget/quota refusal, dataset design B2 exit table) when
the plan has no candidate and PRE-08 is the only failed check; the report
is still recorded and the experiment aborted. Any other failed check keeps
exit 4. The unreachable old empty-plan branch after preflight is removed.
Test `pre_08_empty_plan_exits_3_before_worktree_or_model` (e2e: exit 3,
events created/preflight/aborted, failed checks exactly PRE-08, no
worktree, no agent launch).

## Checks (targeted, on the tip)

- `cargo test -p horch-core --test coordinator`: 8 passed, 1 ignored.
- `cargo test -p horch-core --lib competition::observe`: 4 passed;
  `competition::config`: 2 passed; `runtime::context`: 5 passed.
- `cargo test -p horch-core --test preflight`: 25 passed.
- `cargo test -p horch --lib claude_trust_follows`: 1 passed.
- `cargo build --workspace --bins`, then
  `HORCH_REQUIRE_GIT=1 cargo test -p horch-e2e --test dataset`: 24 passed.
- `cargo clippy -p horch-core -p horch -p horch-e2e --all-targets -- -D warnings`: clean.
- `rustfmt --check` on every file I changed: clean.

## Gotchas

- The e2e dataset tests run the prebuilt `multi-herdr-dataset` from
  `target/`; run `cargo build --workspace --bins` first, or they test a
  stale binary (my first exit-3 run failed for that reason).
- Commit 19e002a went in with one formatting diff in
  `crates/horch-e2e/tests/dataset.rs` (a `;` let the commit run after a
  failed `fmt --check`); 87ea4dd fixes it.

## Not done / follow-ups

- No real-agent check of the nudge: it needs a live round, and the plan
  gave no model budget. A future LA run can confirm that herdr reports a
  Codex candidate at its prompt as `idle`.
