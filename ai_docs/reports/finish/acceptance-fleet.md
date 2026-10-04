# L2 acceptance-fleet: LA-3, LA-4, LA-5 on the operator's Mac

Unit `acceptance-fleet`, branch `ds/acceptance-fleet`, worker opus-62, 2026-10-04.
Design: `ai_docs/designs/2026-10-02-dataset-competition-design.md` §8.

## Summary

| Check | Result |
|---|---|
| LA-3 session discovery | PASS |
| LA-3 resume | FAIL, then fixed (opencode). PASS after `14ced79`. |
| LA-3 `horch cost` equals an A0 ledger copy | PASS (totals equal). Found and fixed 2 unpriced models (`5e6895e`). |
| LA-4 only activated skills visible | PASS for horch's own skills on Claude and Codex. 1 defect in disabled plugin skills (follow-up, not fixed here). |
| LA-5 marketplace install pins a SHA, spawn works offline | PASS |

The fixes are in 3 commits:

- `5e6895e` Usage: Price Sonnet 5.5 and Gemini 3.1 Pro, and pin every roster model to a price.
- `14ced79` Harness: Type an opencode resume's prompt once the agent is idle, and make tell wait out a starting agent.
- `71abaa9` Messaging: Give a freshly registered role time to show its agent before tell types.

No oracle or golden file changed.

## Environment

- herdr 0.8.2, claude 2.1.289, codex-cli 0.160.0, opencode 1.18.34.
- horch was built from the branch (`cargo build --release`). Every run used that binary on `PATH`.
- The A0 binary was built from `8e90066`, the last A0 commit. At `575c2c2` the `horch` binary does not compile, because `chrono` is not declared; `8de1f0a` fixed that.
- The real workers ran in separate herdr workspaces: `la-fleet` (w2S) and `la-offline` (w2W). Nothing was spawned into the fleet workspace w2F. Both workspaces are closed.
- Scratch repo and evidence: `/Users/mascott/projects/multi-herdr/.worktrees/_scratch/la-fleet/` (git-ignored). The first run used `/tmp/la-fleet/repo`. I moved it there when the operator asked.
- `ANTHROPIC_API_KEY` is set in the operator's shell profile. I unset it in every pane I drove. `ps eww` on a spawned codex pane and a spawned claude pane showed 0 copies of the variable (`harness/launch.rs` `FORBIDDEN_ENV`).

## LA-3

### Session discovery and `horch cost` against an A0 ledger copy (fleet w2F)

1. I copied the project ledger (`~/.local/state/horch/-Users-mascott-projects-multi-herdr.json`, 72 records) to `_scratch/la-fleet/a0-state/`.
2. I ran `horch cost --json` with `HORCH_STATE_DIR` on that copy, first with the A0 binary and then with the branch binary.

Results:

- Total cost is equal: $283.65126285 in both. The rerun was $284.55276425 in both, because live sessions had grown.
- Rows, sessions and tokens are equal for all 70 priced records. The 2 `runtime-only` records have no session id and are listed as not priced by both binaries.
- `horch sessions` output is byte-identical between A0 and the branch (905 lines).
- Discovery: all 70 rows resolved a transcript file that exists (claude and codex).
- The only difference is the `expected_unused` column. A0 cannot parse today's roster (`agent: antigravity` is unknown to it), so it loads no roster and reports no expected skills. This is expected.

Defect found (fixed in `5e6895e`): `claude-sonnet-5-5` had no price. Claude Code now resolves `--model sonnet` to `claude-sonnet-5-5`, so every sonnet teammate showed $0.00 with a "partly unpriced" mark. The dataset `UsageMeter` reads the same table (`usage/money.rs`), so sonnet candidates counted 0 against the budget ceiling (opus-61 confirmed this). The fix:

- It adds `claude-sonnet-5-5`: $2 input, $10 output, $0.20 cache read; cache writes 1.25x and 2x.
  Source: <https://platform.claude.com/docs/en/about-claude/pricing>, read 2026-10-04.
- It maps the alias `sonnet` to `claude-sonnet-5-5`. The price is the same as `claude-sonnet-5`, so old transcripts keep their cost.
- It adds `gemini-3-1-pro` and `gemini-3.1-pro-preview` (antigravity teammate): $2 input, $12 output, $0.20 cache read. Cache writes are priced as plain input.
  - Source: <https://ai.google.dev/gemini-api/docs/pricing>, read 2026-10-04, paid tier.
  - These are the rates for prompts of 200k tokens or less. Prompts over 200k cost $4 / $18 / $0.40, and 1 row cannot hold both rates. The code comment says this.
- It adds the test `usage::tests::every_builtin_teammate_model_has_a_price_or_is_free`. Every built-in teammate with an agent and a model must price, or be free or local (`is_free`). Before the fix it failed for 12 teammates (11 sonnet teammates and antigravity). After the fix it passes.
- With the fix, the w2F ledger copy prices at $318.99 instead of $283.65. The difference is the sonnet-5-5 sessions.

### Real workers in a separate workspace (w2S)

Workers: `sonnet`, `codex-terra` and `opencode-pickle`, each with a 1-line task (write a file).

- All 3 finished. Each got a session id: claude `b63eaa51-…`, codex `01a10722-…`, opencode `ses_ef8dd457…`. Session discovery: PASS for all 3 harnesses.
- Cost before resume (scratch ledger copy) and after resume were equal: $0.0303, 7 calls. This was before the price fix, so the sonnet row read $0.00.

### Resume defect (fixed in `14ced79` and `71abaa9`)

`horch spawn --resume <opencode record> '<task>'` started an opencode pane that sat idle. The new task never arrived, and the ledger said `working`. I reproduced this 3 times: twice through horch, and once with plain `opencode --session <id> --prompt '<text>'`. opencode 1.18.34 opens the session and ignores `--prompt`.

I measured when typed text is safe:

- `herdr agent prompt` succeeds 1.6 s after launch, while herdr reports `agent_status: unknown`. opencode drops that text.
- The same text sent after herdr reports `idle` (4.9 s after launch) arrived and ran.
- A 4.8 KB multi-line prompt sent after `idle` arrived as 1 submission.
- `horch tell` to the resumed role 2 s after registration exited 0, and the text was lost. So tell and assign had the same drop. This was a live test with the old binary.

The fix:

- `Harness::resume_prompt_typed` (default false; opencode true). On such a resume, `run_flow_code` starts a thread that waits until herdr reports the pane `idle`, then submits the prompt with `send_line_with` (`delivery::deliver_when_idle`). The wait times out after 120 s (`Readiness::DEFAULT`). On a timeout, `type_resumed_prompt` does 2 things: it writes a `note` on the worker's record ("resumed task not delivered: …"), and it sends a `[<role>] BLOCKED:` line to the orchestrator pane if one is registered.
- The opencode resume argv still carries `--prompt`, because the A0 launch oracles freeze it (`baseline_oracles.rs`: "fix the code, not the file"). It is a no-op today. **Risk:** if a later opencode honours `--prompt` beside `--session`, the task arrives twice. The code comment on `OpenCode::resume_prompt_typed` says to re-check this on an upgrade.
- `horch tell` (and so `horch assign`) now calls `delivery::send_line_when_ready`:
  - When herdr sees an agent that is still starting (`unknown`), tell waits until that agent leaves the starting state.
  - When herdr sees no agent and the role registered less than 30 s ago (`TELL_GRACE`, `HORCH_TELL_GRACE_MS` through `RuntimeContext.settings.tell_grace`), tell first waits up to the grace for an agent to show.
  - A pane that never shows an agent gets the line after the grace, on the old path.
  - A busy agent (`working`), and a pane with an old role, get the line at once as before. Tell does not wait for idle, so the orchestrator can still reach a busy worker.
- `send_line`/`send_line_with` and `Timing::DEFAULT` are unchanged. ARC-20 still pins them.
- The e2e harness sets `HORCH_TELL_GRACE_MS=0`, because fake panes never show an agent.

Tests (FakeWorkspace; `FakeWorkspace::set_agent_states` scripts what `pane_get` reports):

- `crates/horch-core/tests/messaging.rs`:
  - `deliver_when_idle_waits_for_idle`
  - `deliver_when_idle_times_out_without_typing`
  - `send_line_when_ready_waits_only_for_a_starting_agent`
  - `send_line_when_ready_gives_a_fresh_role_time_to_show_its_agent`: no agent, then a starting agent, then idle. The line is delivered after the agent shows. A pane that never shows an agent falls back after the grace.
- `harness/launch.rs`:
  - `only_an_opencode_resume_types_its_prompt`
  - `an_undelivered_resume_prompt_is_recorded_and_reported`: the record note and the BLOCKED line to the orchestrator pane.
- `runtime/context.rs` covers `HORCH_TELL_GRACE_MS`.

Live verification with the fixed binary:

- `horch spawn --resume` of the opencode record: the task arrived and ran (`fixresume` appended).
- `horch tell` 3 s after role registration, after `14ced79`: the text was still lost. herdr showed no agent yet, which led to `71abaa9`.
- After `71abaa9`: tell waited 5 s, then the text arrived (`toldgrace` appended).

## LA-4: activated skills only

- One Claude worker (`sonnet`, phase research) and one Codex worker (`codex-terra`, phase validation) each wrote the skill list they could see.
- **Codex: PASS.** It listed exactly `check, code-analysis, code-review, document, handoff, security-review`. These are the 6 validation skills. Its private `CODEX_HOME` (`~/.local/state/horch/codex-home/codex-terra-2-…`) has `skills -> skill-bundles/<record>/skills`, which holds those 6 plus codex's own `.system`.
- **Claude, horch skills: PASS.** It listed exactly 4 `horch:` skills (`brainstorm, handoff, research-codebase, trace`). These are the 4 research skills. The plugin dir `skill-bundles/30cb2681-…/skills` holds exactly those 4.
- **Claude, defect (not fixed, follow-up):** `herdr:herdr-worker` and `herdr:herdr-orchestrator` are still listed. The `sonnet` teammate disables them, and horch passes `"skillOverrides":{"herdr:herdr-worker":"off","herdr:herdr-orchestrator":"off",…}` in `--settings`. The opus-62 pane (this session) also lists both. So on Claude Code 2.1.289 a plugin-prefixed `skillOverrides` key has no effect from `--settings`. `teammates/_template.md` says this works (verified on 2.1.278). Options for the owner:
  - set `enabledPlugins: {"herdr@matts-robot-skills": false}` for teammates that disable all herdr skills; or
  - re-verify the key format against 2.1.289.
- The other visible skills are the operator's ambient skills and plugins. Teammates without `inherit_plugins: false` inherit them on purpose.

## LA-5: marketplace install pins a SHA, spawn works offline

- I installed into an isolated store (`XDG_DATA_HOME=_scratch/la-fleet/data`):
  `horch skills install skill-marketplace@game-design-v0.3.1 --path plugins/dev/skills/whats-next`.
- The lock records `requested_revision: game-design-v0.3.1` and `resolved_commit: 652e8d281a7fe5f91de9f86d626c3ae5ccc64fe0`. That commit is the tag's peeled commit (`git ls-remote`). The digest is `sha256:1367…4969`.
- `HORCH_GIT_BIN=/nonexistent horch skills doctor`: `1 skill(s) checked, 0 problem(s)`.
- A scratch teammate `la-sonnet` (a copy of `sonnet` with `skills: [whats-next]`) was spawned with `HORCH_GIT_BIN=/nonexistent`. Its worker loaded the skills offline. The worker listed `horch:whats-next` with the 5 phase skills. The bundle copy is byte-identical to the store (`diff -r`).
- **Gotcha (not a code change):** a pane that `horch spawn` splits does not inherit the spawner's env. herdr also does not apply workspace `--env` to split panes. The worker process then re-resolves skills against the default store and fails with "unknown bundled skill 'whats-next'". With the default `~/.local/share/horch` store this cannot happen. I ran the worker command in its pane with the env exported. Follow-up: carry `data_root` in the brief, as `teammates_dir` already is, so spawner and worker always read one store.

## Cost

- Real workers: $0.13 in the `/tmp` run and $0.17 in the `_scratch` run, so about $0.31 in total.
- Workers used: 3 sonnet sessions, 2 codex-terra sessions and opencode-pickle (free). No top-tier model was used.

## Other findings (not fixed, outside scope)

- **Trust dialogs:** `_scratch/la-fleet/repo` is its own git root. Claude and Codex both showed a trust dialog there and waited, so I accepted it by hand. A scratch repo inside the trusted folder is still a new git root, so it is not automatically trusted.
- `horch done` from a worker with no registered orchestrator (scratch workspace): opencode records done and exits. Codex reported "Blocked from notifying the orchestrator" and did not run `done`, so its record stayed `working` until I closed it.
- `horch inbox` keeps the roles of closed panes (for example `opencode-pickle-2 w2S:p5` after the pane closed).
- A resume of a record whose pane was closed without `horch done` refuses ("still marked working"). The operator needs `horch ledger done <id> …` first.
- The opencode-pickle worker's done summary said "Nothing is uncommitted" while its file was uncommitted. This is the model's report quality, not a horch defect.

## Files changed

- `crates/horch-core/src/usage.rs`: prices, `sonnet` alias, roster price test.
- `crates/horch-core/src/harness/{mod,opencode,launch}.rs`: `resume_prompt_typed`, typed resume prompt, failure note and BLOCKED line, tests.
- `crates/horch-core/src/messaging/{delivery,mailbox}.rs`: `Readiness`, `deliver_when_idle`, `send_line_when_ready`, `TELL_GRACE`, `Mailbox::registered_at`.
- `crates/horch-core/src/workspace/{model,testing}.rs`: `Pane.agent`, `Pane.agent_status`, `FakeWorkspace::set_agent_states`. I also fixed a RefCell double borrow in `pane_list`.
- `crates/horch-core/src/runtime/context.rs`: `Settings.tell_grace` (`HORCH_TELL_GRACE_MS`).
- `crates/horch/src/cmd/messaging.rs`: tell uses `send_line_when_ready` with the grace.
- `crates/horch-e2e/src/harness.rs`: `HORCH_TELL_GRACE_MS=0`.
- `crates/horch-core/tests/messaging.rs`: 4 new tests.
- Changed existing test: none in the end. An interim change to `phase_skills_are_native_on_all_harnesses_and_resume_keeps_prompt_last` was reverted when the resume argv kept `--prompt`.

## Gate

`just gate` through `gate-slot.sh` on this branch fails in 2 targets. Both fail the same way on a clean `design-skills` at `a49faec`, so this branch does not cause them:

- `horch-core --test arch_scan`:
  - `arc_10_harness_match_only_in_harness`: `HarnessKind` matches in `competition/preflight.rs` and `horch/dataset/preflight.rs`.
  - `arc_05_no_ambient_env_in_core`: `std::env::var_os("PATH")` in `harness/claude.rs:160`.
- `horch-e2e --test e2e`: `tel_02_fleet_writes_orchestrator_record` ("timed out waiting for the orchestrator's claude").

Every other gate target passes, as do the targeted checks after the last rebase: fmt, clippy with 0 warnings, horch-core lib 425 of 425, `messaging`, `baseline_oracles`, and `horch`.
