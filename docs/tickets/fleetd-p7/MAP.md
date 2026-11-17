# Map: fleetd build, Phase 7 (docs sync and conformance, then exec mode)

Spec: `docs/specs/fleetd/design.md`

| # | Title | Blocked by | Wave |
|---|---|---|---|
| 01 | [The module files of Phase 7 in worker_profiles and task_execution, and their rows in the fitness rules and the architecture doc](01-modules.md) | T-567 | 1 |
| 02 | [The next schema version: the event words of the health of a profile and the outcome context_limit](02-schema.md) | T-567 | 1 |
| 03 | [The platform start function: 1 type that starts each process that is not a fleet process, and always removes each forbidden name](03-platform-start.md) | T-567 | 1 |
| 04 | [Move the process starts of runtime, procid and the launcher onto the platform start function](04-move-runtime.md) | 03 | 2 |
| 05 | [Move the process starts of the harness adapters onto the launcher or the platform start function](05-move-harness.md) | 04 | 3 |
| 06 | [Move the process starts of workspace, messaging and telemetry onto the launcher or the platform start function](06-move-workspace-messaging-telemetry.md) | 04 | 3 |
| 07 | [Move the process starts of the task ledger and vcs onto the launcher or the platform start function](07-move-taskledger-vcs.md) | 04 | 3 |
| 08 | [Move the process starts of execution, routing and fleetd onto the launcher or the platform start function](08-move-execution-routing-fleetd.md) | 04 | 3 |
| 09 | [Move the process starts of the horch CLI commands onto the platform start function](09-move-horch-cli.md) | 04 | 3 |
| 10 | [Move the process starts of horch-marketplace and herdr-install onto the platform start function](10-move-marketplace-installer.md) | 03 | 2 |
| 11 | [Widen herdr-docs-sync into the library agent-docs-sync: 1 source entry for each tool in docs-sources.toml, the sitemap and llms_txt kinds, and 1 folder for each version](11-agent-docs-sync-crate.md) | T-567 | 1 |
| 12 | [CHANGES.md for each tool: the changed pages and the changed flags, env vars and settings keys, and the also_git source of Codex](12-docs-changes-and-git-source.md) | 11, 03 | 2 |
| 13 | [The Claude Code adapter in exec mode: the run-to-completion launch, no dialog, 256000 as the compaction window, and the usage-limit signal of the stream](13-claude-exec.md) | 05, 01 | 4 |
| 14 | [The Codex adapter in exec mode: codex exec --json, -a never, 256000 as the compaction limit, and the usage-limit signal of the stream](14-codex-exec.md) | 05, 01 | 4 |
| 15 | [The Antigravity adapter in exec mode: agy -p with stream-json, --mode auto, the soft denials, and the token count of the stream](15-antigravity-exec.md) | 05, 01 | 4 |
| 16 | [The fake agents of Phase 7: exec mode for fake-claude, fake-codex and fake-antigravity, the probes of the conformance checks, a usage limit and a long context](16-fakes.md) | 13, 14, 15 | 5 |
| 17 | [The wrapper runs an exec attempt: it starts the CLI with START.md, renders the stream into the pane, turns events into heartbeat activity, and sees the exit at once](17-wrapper-exec.md) | 08, 13, 14, 15, 16 | 6 |
| 18 | [The 256k context limit for a CLI with no compaction setting: the wrapper ends the attempt with context_limit, the task goes on with a handoff, and a loop with no new commit stops](18-context-limit.md) | 02, 17 | 7 |
| 19 | [Exec mode is the default worker mode: each CLI with an exec adapter runs exec, the seats that interview you stay interactive, and the background wait applies only to interactive attempts](19-exec-default.md) | 17 | 7 |
| 20 | [A worker profile is complete: the adapter refuses a profile with no CLI, model, sandbox, writable paths, context budget, timeouts or restart policy](20-complete-profile.md) | 19 | 8 |
| 21 | [256000 is the context of each seat: context-windows.md and the compact_window of each teammate file](21-context-256k.md) | 20 | 9 |
| 22 | [horch docs sync and the docs_sync job: the version through the launcher, the pages into the fleet root, and a job result with no repository](22-docs-sync-command-and-job.md) | 12, 01, 04 | 3 |
| 23 | [The conformance suite: 9 checks of the design as real launches in a throwaway task folder, with a verdict for each check and conformance.json](23-conformance-suite.md) | 01, 16, 17 | 7 |
| 24 | [The check "Operator boundary" of the suite for each adapter: from inside the sandbox, the token read, the channel connect and the request write are each refused](24-operator-boundary-check.md) | 23 | 8 |
| 25 | [The conformance job: the executor runs the suite, writes conformance.json into the fleet root, and its input key holds the hash of the adapter](25-conformance-job.md) | 22, 24 | 9 |
| 26 | [The health of a profile: a failed conformance run or a dialog marks it unhealthy, a passed run marks it healthy again, and dispatch skips an unhealthy profile](26-profile-health.md) | 02, 23 | 8 |
| 27 | [A drift becomes an idea: a failed conformance run adds 1 idea "Update the <tool> adapter for <version>" with the diff and the failed checks](27-drift-idea.md) | 26, 25 | 10 |
| 28 | [fleetd queues the docs_sync and conformance jobs: each day, on a new horch binary, on a new CLI binary, and a conformance run after each docs_sync run](28-docs-jobs-schedule.md) | 01, 25 | 10 |
| 29 | [horch status shows each hold on dispatch with its cause: a health floor, pacing, an unhealthy profile, a git job that holds the repository, a missing fast gate, and not_before](29-status-holds.md) | 26 | 9 |
| 30 | [Wire Phase 7 into fleetd: the schedule of the docs jobs, the health marks after each conformance result, the drift idea, and the context_limit outcome](30-pass-wiring.md) | 18, 19, 26, 27, 28, 29 | 11 |
| 31 | [The arch_scan rule fitness_no_process_start_outside_the_launcher, and no child of a worker, a job, a check, docs sync or conformance has the forbidden name](31-launcher-rule.md) | 05, 06, 07, 08, 09, 10, 22, 25, 30 | 12 |
| 32 | [The e2e test of docs sync and conformance: no repository, a daily new run, a drift that marks the profiles unhealthy and becomes an idea, and the recovery](32-p7-docs-sync-e2e.md) | 30, 16 | 12 |
| 33 | [The e2e test of exec mode: a task runs to completion in exec mode on each CLI, a usage limit restarts it on the fallback, and a long context hands off](33-p7-exec-e2e.md) | 30, 21 | 12 |
| 34 | [The release test of Phase 7: an interactive worker that is live at the update ends its task as before, and the next attempt of its task runs in exec mode](34-p7-release.md) | 30, 21 | 12 |
| 35 | [The docs for a new user: install, start fleetd, add an idea and read the status with the new commands, in the README, the docs, the skills and the teammate files](35-user-docs.md) | 21, 22, 29 | 10 |
| 36 | [Gate of Phase 7 and of the whole build: each rule of the gate green, empty lists of expected failures, the run-in-flight test, the table of each id of the design, the pending CI rows and the deviations](36-gate.md) | 01, 02, 03, 04, 05, 06, 07, 08, 09, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35 | 13 |

## The check of the run

```
cargo test --workspace --locked
```

As in Phase 0 (`docs/tickets/fleetd-p0/MAP.md`): this command passes when each test that is not on
the list of expected failures passes, and each test on the list fails for its listed reason. The
Check of each ticket also runs the guard tests:
`cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`.

Self-hosting danger: this phase changes the worker mode that the build fleet itself uses. No worker
installs a development build, and each test runs the development binary only with temporary folders
(build rule 1). Each ticket says it.

Expected failures: no line of Phase 7 is in `crates/horch-e2e/expected-failures/` today. After this
phase each list is empty. Ticket 36 checks it (REQUIRED item 1).

Network: the docs sync reads the public docs of each CLI in production only. No test uses the
network: each test gives the pages through a local fixture. No ticket sends project text, a name or
an email address to an outside service.

## Waves

- Wave 1: 01, 02, 03, 11
- Wave 2: 04, 10, 12
- Wave 3: 05, 06, 07, 08, 09, 22
- Wave 4: 13, 14, 15
- Wave 5: 16
- Wave 6: 17
- Wave 7: 18, 19, 23
- Wave 8: 20, 24, 26
- Wave 9: 21, 25, 29
- Wave 10: 27, 28, 35
- Wave 11: 30
- Wave 12: 31, 32, 33, 34
- Wave 13: 36

The tickets of 1 wave run at the same time. Only ticket 36 is `Parallel: no`. No 2 tickets that can
run at the same time own the same file: the generator of this map checked each pair, and a pair that
shares a file is ordered by its blockers. The chains on 1 file:

- `crates/horch-core/src/harness/launch.rs`: 03 -> 05.
- `crates/horch-core/src/harness/claude.rs`, `codex.rs`, `antigravity.rs`: 05 -> 13, 14, 15.
- `crates/horch-core/src/execution/lifecycle.rs`: 08 -> 17 -> 18.
- `crates/horch-core/src/fleetd/fleet_runtime/application/holds.rs`: 08 -> 29.
- `teammates/`, `crates/horch-core/tests/oracles/`: 19 -> 20 -> 21 -> 35.
- `crates/horch-core/tests/golden/`: 21 -> 35.
- `crates/agent-docs-sync/`, `Cargo.lock`: 11 -> 12 -> 22.
- `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs`, `crates/horch/src/cmd/jobcmd.rs`,
  `crates/horch-core/src/fleetd/worker_profiles/adapters/docs_dir.rs`: 22 -> 25.
- `crates/horch-core/src/fleetd/worker_profiles/domain/conformance.rs`, `application/conformance.rs`,
  `adapters/conformance_runner.rs`: 01 -> 23 -> 24.
- `docs/architecture.md`: 01 -> 22.

Each ticket with no blocker in this map (01, 02, 03, 11) is blocked by `T-567`, the gate of Phase 6.

The groups of the phase:

- Foundation: 01, 02 (module files, the schema step).
- 1 door for each process (SR-7): 03 to 10 (the platform start function and the move of each direct
  start, by folder), 31 (the rule and the e2e test).
- Docs sync: 11, 12, 22 (the library, CHANGES.md, the command and the job).
- Exec mode: 13 to 15 (the adapters), 16 (the fakes), 17 (the wrapper), 18 (the 256k limit and
  `context_limit`), 19 (the default), 20 (the complete profile), 21 (256000 for each seat).
- Conformance and health: 23, 24, 25 (the suite, "Operator boundary", the job), 26 (the health of a
  profile), 27 (the drift idea), 28 (the schedule), 29 (the holds in `horch status`).
- Wiring and tests: 30 (the pass), 32 to 34 (e2e and release).
- Docs and gate: 35, 36.

## Critical path

03 -> 04 -> 05 -> 13 -> 16 -> 17 -> 23 -> 24 -> 25 -> 27 -> 30 -> 31 -> 36

## Decisions that hold

- A guard whose fact is unknown does not hold.
- What the user reads keeps the words of today, and a new event has a word that says what happened.
- A job starts only through the launcher.
- Only fleetd writes the ledger, through `transition()`.
- Each effect is a committed row first.
- The scans and the duties of the pass have a work budget and a cursor.
- OS-specific code is only in `crates/horch-platform`.
- No sandbox for a profile on an OS means no dispatch of that profile and 1 needs-you item.
- Each operator command is checked on the server side. No agent has operator rights.
- The fleet has no budget cap and no cap on parallel work (decision D8). No ticket adds one.
- Each persona of `teammates/` keeps its fallback rule: a fallback never lowers confidentiality,
  and a persona with a written no-fallback reason keeps it (task T-261).
- No test reads the real load, the real limits or the real clock of the machine, no test depends on
  the speed of the host, and no test starts a real agent CLI.

## Open questions

Q1 and Q2 are answers of the orchestrator (QUESTION of T-568). The other points are choices of the
planner where the design and the code of today do not agree, or where the design leaves a point open.
Each ticket follows them, and the orchestrator can change each one.

- **Q1. The scope of SR-7 (answer of the orchestrator, option 2).** `rg` finds 143 `Command::new` in 55 files
  under `crates/*/src` today, test code included. Each moves behind the launcher (a process of fleetd, the wrapper,
  a job, docs sync or conformance, also the git calls of a job) or behind 1 platform start function
  that the launcher and the other commands both use and that always removes each forbidden name.
  The allow-list of the rule has only these 2 files. Test binaries and test code are not production
  code. The form of the start function: the type `horch_platform::start::Command`, which removes each
  forbidden name just before each start; `FORBIDDEN_ENV` moves to `start.rs`, and
  `harness/launch.rs` re-exports it. Tickets 03 to 10, 31.
- **Q2. The outcome of the 256k limit (answer of the orchestrator, option 1).** The design says that
  the wrapper ends the attempt with a handoff, and names no outcome. The new outcome `context_limit`
  burns no attempt; the task moves to `ready` at once in the same worktree with the handoff file of
  rung 6. Guard: when 2 attempts in a row of 1 task end with `context_limit` and the task branch has
  no new commit between them, the task does not go to `ready` again, and 1 needs-you item opens. No
  new column or counter. Tickets 02, 18.
- **Q3. The background wait after Phase 7.** fleetd-p2 Q23 keeps a background wait while a child of
  the agent runs, and says "Phase 7 removes it with exec mode". Choice: an exec attempt has no
  background wait; an interactive attempt (sub-orchestrators, orchestrator seats, personas marked
  interactive) keeps option 1 of fleetd-p2 Q23. Ticket 19.
- **Q4. The docs sync crate.** The design: "generalise it into `agent-docs-sync`" and "Docs sync is a
  `horch` subcommand". Choice: `git mv crates/herdr-docs-sync crates/agent-docs-sync`, a library with
  no binary, used by `horch docs sync`. `docs-sources.toml` is built into the crate. The previous
  version of a tool is the entry before the current one in `<out>/<tool>/versions.json`. The
  repository folder `herdr-docs/` stays, and `horch docs sync --cli herdr --out herdr-docs` refreshes
  it as the old binary did. Tickets 11, 12, 22.
- **Q5. The adapters of the suite.** The 9 checks other than "Operator boundary" run for Claude Code,
  Codex and Antigravity, the 3 CLIs of the adapter table and of `docs-sources.toml`. "Operator
  boundary" runs for each of the 6 adapters (also OpenCode, pi and Prime), as the gate says "for each
  adapter". OpenCode, pi and Prime have no exec adapter, so they stay interactive by default, and no
  conformance run marks their profiles. herdr is a docs source only. Tickets 19, 23, 24.
- **Q6. The conformance tests and the real CLIs.** The design: "Each is a real launch". No test starts
  a real agent CLI, so each test runs the suite against the fake agents, which apply the rules of
  their rendered config through the platform sandbox (fleetd-p4 ticket 17, fleetd-p5 ticket 06). The
  real CLI runs only in the production `conformance` job. Tickets 16, 23, 24.
- **Q7. A complete profile (FR-6).** A field is complete when it has a value in the file, in its
  `base`, or a default of the design. `skills` and `mcp_servers` may be empty lists. The exempt
  teammates of fleetd-p2 Q8 are not worker profiles. Ticket 20.
- **Q8. The version in the subject of a job.** A pass does not wait for a version command. Choice:
  the subject of `docs_sync` and `conformance` is `<cli> <binary id>`, the hash of the identity of the
  CLI binary (`exeid`). The job runs `version_cmd` through the launcher and records the version text
  in its result and as the folder name. Tickets 22, 25, 28.
- **Q9. The hash of the adapter.** The input key of `conformance` holds "the hash of its adapter".
  Choice: the sha256 of the rendered conformance launch of the adapter (argv and config text) for 1
  fixed reference profile. Ticket 25.
- **Q10. The health of a profile: no table, and a schema step.** The design names no table for it
  (fleetd-p2 Q16 also added none). Choice: the health is the last event `profile_unhealthy` or
  `profile_healthy` of the profile. The 2 event words need the next schema version, because a CHECK
  lists the event words. The release row of the design says "Ledger: None"; the step changes only
  the allowed words and no row. This is a deviation for the operator. Tickets 02, 26.
- **Q11. "Each day".** A fixed 86400 s on the fleet clock since the last `docs_sync` run of the CLI
  started. No new key in `fleet.toml`. "On `horch self-update`": `horch self-update` restarts fleetd
  on the new binary (OR-6), and the first pass sees a horch binary identity that differs from the one
  in the result of the last run. Ticket 28.
- **Q12. The hold "a git job that holds the repository".** No ticket of fleetd-p3 or fleetd-p4 names
  it. Choice: it is the rule of 1 git writer for each repository (fleetd-p3 Q12), shown as a cause
  derived from the `jobs` rows. No new rule. Ticket 29.
- **Q13. The list of expected failures after Phase 7.** The design does not say to remove the
  mechanism (amendment A22). It stays with empty lists. Ticket 36.
- **Q14. The usage-limit signal of the CLI.** fleetd-p2 Q9: before Phase 7 the signal is the pool
  reading. The design table: "The adapter maps the usage-limit and outage signal of the CLI in exec
  mode and in interactive mode to the outcome `quota` (the exact signal to confirm in conformance)".
  Choice: Phase 7 adds the signal of the exec stream for Claude Code, Codex and Antigravity. The
  interactive mode keeps the pool reading. Tickets 13 to 15, 17.
- **Q15. An unhealthy profile has no fallback.** The design names the fallback only for a usage limit
  and an outage. A task of an unhealthy profile waits, with its hold shown. Ticket 26.
- **Q16. 256000 for each seat.** The design names the windows of `context-windows.md`. A teammate
  `compact_window` wins over that file, and 93 teammate files set one today. Choice: each window and
  each `compact_window` becomes 256000, also the 150000 of the Sonnet and Terra seats. Ticket 21.
- **Q17. Module files.** Ticket 01 makes each new file of Phase 7 (1 file for each concern), in place
  of `application/commands.rs` and `CliConformanceRunner` of `ddd/08-code/implementation-plan.md`,
  Slice 7. Ticket 01.
- **Q18. The stream forms of the CLIs.** Each adapter ticket records the forms that it parses, with
  the doc page, in `crates/horch-core/tests/fixtures/streams/<cli>-exec.jsonl`, and the fakes emit the
  same lines. A form that the docs do not give is a QUESTION, not a guess. Tickets 13 to 16.

Risks of the phase:

- **R1.** Self-hosting: exec becomes the default worker mode of the fleet that builds this code. The
  build fleet uses the installed `horch`, so nothing changes for it until a release. After the
  release, a live interactive worker goes on, and its next attempt runs in exec mode (ticket 34).
- **R2.** The move of about 140 process starts (tickets 04 to 10) touches many files that all phases use.
  A changed argument, stdin or environment rule is a change of behavior. The launch oracles must not
  change in tickets 04 to 10.
- **R3.** Windows: the platform start function (ticket 03) and the platform sandbox have Windows code
  that this Mac cannot compile. CI gives the result.
- **R4.** The stream forms and the usage-limit signal of the real CLIs are proved only by the
  production `conformance` job, not by a test. The first real run can fail a check and mark each
  profile of that CLI unhealthy, so dispatch to that CLI stops until the drift idea is done. This is
  the design, and it shows in `horch status` and `needs-you/`.
- **R5.** A model whose real context is below 256000 cannot reach the window. The CLI may refuse the
  value or never compact. The check "Context limit" reads the effective value back for each CLI.
- **R6.** Phases 2 to 6 are built at the same time. A file that they name can change its name. Each
  ticket reads the ticket that makes the file.
- **R7.** Network: production docs sync fetches public docs and the Codex repository. It sends no
  project text, name or email address.

## Coverage

Each mechanism of the row Phase 7 in "What each gate needs" and in "New work for each phase" of the
design, each mechanism of item 9 of the Migration plan, each rule of the gate line, and each REQUIRED
item of the ticket of this map, with the 1 ticket that gives it. A mechanism of an earlier phase that
the gate needs is named with the ticket that uses it.

| Mechanism or rule | Ticket |
|---|---|
| The module files of the phase | 01 |
| The event words of the health of a profile; the outcome `context_limit` (schema) | 02 |
| The platform start function that always removes each forbidden name | 03 |
| The move of each direct process start: runtime, procid, the launcher | 04 |
| The move: the harness adapters and the version commands | 05 |
| The move: workspace, messaging, telemetry | 06 |
| The move: the task ledger and vcs | 07 |
| The move: execution (the wrapper), routing, fleetd | 08 |
| The move: the horch CLI commands | 09 |
| The move: horch-marketplace and herdr-install | 10 |
| `herdr-docs-sync` widened to every CLI: `docs-sources.toml`, the sitemap and llms_txt kinds, the version folder and the manifest | 11 |
| `CHANGES.md` (changed pages, flags, env vars, settings keys) and `also_git` | 12 |
| Exec mode of the Claude Code adapter; its usage-limit signal; `CLAUDE_CODE_AUTO_COMPACT_WINDOW=256000` | 13 |
| Exec mode of the Codex adapter; its usage-limit signal; `model_auto_compact_token_limit=256000` | 14 |
| Exec mode of the Antigravity adapter; its token count | 15 |
| The fake agents of Phase 7 | 16 |
| The wrapper runs an exec attempt | 17 |
| FM-20: the wrapper ends the attempt at 256k; `context_limit` and its loop guard | 18 |
| Exec mode as the default (item 9; release row) | 19 |
| The background wait only for an interactive attempt | 19 |
| FR-6: the adapter refuses an incomplete profile | 20 |
| `context-windows.md` set to 256000 for each seat (item 9); FR-9 by the CLI setting | 21 |
| `horch docs sync` and the `docs_sync` job executor; a run with no repository is a valid row | 22 |
| The conformance suite: 9 checks (FR-8, FR-9, SR-1, FM-18) | 23 |
| The check "Operator boundary" for each adapter (SR-8) | 24 |
| The `conformance` job executor and `conformance.json`; a run with no repository is a valid row | 25 |
| The unhealthy mark of a profile and its recovery; dispatch skips it; rung 4 marks it (FM-19) | 26 |
| A drift becomes an idea | 27 |
| fleetd queues the `docs_sync` and `conformance` jobs; a daily run starts a new run | 28 |
| OR-1 and FM-45: each hold with its cause in `horch status`; the events of a global hold | 29 |
| The health guards and pacing (Phase 1), `not_before` (Phase 2), a git job that holds the repository and the declared fast gate (Phase 4): shown as holds | 29 |
| The ideas and `horch ideas` (Phase 6): the 1 view of OR-1 and the intake of the drift idea | 29, 27 |
| The `docs_sync` and `conformance` job kinds (Phase 1), queued from Phase 7 | 28 |
| The launcher (Phase 1) | 04 |
| The adapters (today) | 13, 14, 15 |
| The Phase 7 duties in the pass | 30 |
| SR-7: the `arch_scan` rule `fitness_no_process_start_outside_the_launcher` and the forbidden-name e2e test | 31 |
| Gate tests: no repository, a daily new run, "Operator boundary" for each adapter (e2e) | 32 |
| Exec mode end to end; FM-20 end to end | 33 |
| The "run in flight" test of Phase 7 | 34 |
| REQUIRED item 5: the docs for a user | 35 |
| REQUIRED items 1 to 4: empty expected failures, the table of each id, the pending CI rows, the deviations | 36 |
| The gate of Phase 7 and the gate file `docs/specs/fleetd/gates/fleetd-p7.md` | 36 |
