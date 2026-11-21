# Scope: an `hc` adapter for fleetd

Task T-351. Main released a second pane host in v0.5.2 (PR #363): `hc`, which
is `herdr-core` plus the Herdr desktop app. fleetd runs its workers only on
`herdr`. This report lists what an `hc` adapter for fleetd needs and what it
costs, so that the operator can decide before the fleetd pull request merges.

Sources: `merge-main-proof-2.md` (T-330, sections H2 and "For the operator"),
`gh pr view 363`, `git show 50e63b2a --stat`, `docs/specs/pane-host.md` and
`docs/adr/0003-two-pane-hosts-behind-one-handle.md` of `origin/main`, and the
code of the trial branch `keep/fleetd-finish-trial-merge-main` (`3fd534f7`).
All paths and line numbers are of that branch. Paths that start with
`fleetd/`, `workspace/`, `harness/` or `execution/` are under
`crates/horch-core/src/`. Paths that start with `cmd/` are under
`crates/horch/src/`.

Each claim has a label:
- **verified (read)**: I read the code or the document.
- **verified (test)**: a test of T-330 passed for it.
- **not verified**: no evidence. I did not start a real `hc`, a real Herdr app
  or any test suite (rules 1 and 2 of the ticket).

## Facts

What works on `hc` after the merge:
1. These commands of main work on `hc`: `horch doctor` (`hc_doctor` 4 / 0),
   `horch mail courier` (`hc_courier` 1 / 0), `horch tell`, and the
   `PaneHost` contract (`pane_host_contract` 2 / 0 on the fakes). **verified (test)**
2. The handle `PaneHost` (`workspace/host.rs`) gives these calls on `hc`:
   `pane_get`, `pane_list`, `pane_read`, `pane_split`, `pane_run`,
   `agent_prompt`, `pane_send_keys`, `pane_close`, `workspace_create`,
   `workspace_list`, `tab_list`, `tab_new`, `workspace_close`. **verified (read)**
3. `horch spawn` has an `hc` placement path, `open_hc_pane`
   (`cmd/spawn.rs:783`). It uses `workspace::place`. `ExecutionService::split`
   calls it only when the spawn has no `--from-pane`
   (`execution/service.rs:271`). **verified (read)**

What is refused on `hc` after the merge:
1. `horch fleetd` and `horch fleetd check` call `refuse_hc`
   (`cmd/fleetd.rs:128`, `:137`). They fail with `HC_REFUSED`. **verified (test)**:
   `the_hc_backend_is_refused_with_its_fix_and_herdr_is_taken`, `hc_from_the_environment_is_refused_by_fleetd`.
2. `horch fleet` calls `refuse_hc` (`cmd/recipes.rs:228`) before it makes a
   workspace. **verified (test)**: `hc_fleet_start_is_refused_and_makes_no_space`.
3. `horch spawn` runs only when fleetd starts it (`cmd/spawn.rs:162`). So no
   path of the run branch puts a worker in an `hc` pane. **verified (read)**
4. `PaneHost::pane_shell_pid` returns `Unsupported` on `hc`
   (`workspace/host.rs:252`). **verified (read)**
5. `pane_send_text` returns `Unsupported` on `hc` (ADR 0003). **verified (read)**

What main had and the merge removes:
1. 6 tests of `hc_fleet_start.rs`: a fleet start on `hc`, the placement of
   workers 1 to 5, the start with the window closed, and the OpenCode refusal.
   The trial replaced them with 2 refusal tests. **verified (read)**
2. The watchdog check of a worker on its recorded backend (`hcb_10` of main).
   fleetd checks each worker on `herdr`. **verified (read)**
3. The docs of main still describe `hc` fleets: `README.md`, ADR 0003,
   `docs/live-checks/hc-backend.md`, `docs/specs/pane-host.md`,
   `docs/testing-and-gates.md`, `docs/architecture.md`, `GLOSSARY.md`.
   **verified (read)** in `merge-main-proof-2.md`, item 3.

## Gaps

"Handle on `hc`" says whether `PaneHost` already gives the same call for `hc`.

| # | Place | Function | What it calls on `herdr` now | Handle on `hc` | What is missing | Unit |
|---|---|---|---|---|---|---|
| G1 | Launch of a worker | `PushSpawns::spec`, `spawn_spec`, `launch_pane` (`cmd/fleetd.rs:1031`, `:1054`, `:1067`) | `pane_get` for the recorded pane. Else `workspace_list` and a match of the label to the project folder name, then the first pane of `pane_list` | Yes, all 3 calls | With the Herdr window closed, the `hc` fallback of `workspace_list` gives `label: None` (`workspace/hc.rs:637`), so the label match fails. Phase 6 runs have no orchestrator pane, so the label match is the usual path. The run row has `workspace_id` (`fleetd/task_execution/application/launch.rs:353`): the launch can use it on both hosts. **verified (read)** | U4 |
| G2 | Launch helper | `push_launch_spec` (`cmd/recipes.rs:408`) | Starts `horch spawn <persona> --run <slug> --exact --attempt <id> --from-pane <pane>` with `HORCH_WORKSPACE_ID` and the fleet env | Not a host call | (1) The env has no `HORCH_BACKEND`. The spawn takes the backend from the execution record of the workspace, else from its context, which is `herdr` (`cmd/spawn.rs:166`, ADR 0003). No record of a fleetd workspace on `hc` exists, so the spawn picks `herdr`. **verified (read)**. (2) With `--from-pane`, the spawn splits that pane and skips `workspace::place`. The split works on `hc`, but the layout of ADR 0003 (2 columns, then tabs `workers <N>`) is lost. **verified (read)** | U4 |
| G3 | Worker rows | fleetd inserts of `workers` (`fleetd/task_execution/adapters/sqlite.rs:493`, `fleetd/fleet_runtime/adapters/watchdog_duties.rs:1632`) | No host call | Not a host call | Neither insert writes `backend`, so each row gets the default `herdr`. No fleetd code reads `workers.backend`. **verified (read)** | U2 |
| G4 | Stop of an attempt | `effects::stop_process` (`fleetd/fleet_runtime/adapters/effects.rs:75`) | No host call. OS signals to the recorded PID and start time, and to its group when the PID leads it | Not a host call | Works on any host if the wrapper leads its process group. If it does not, the stop goes to the wrapper only, and the agent lives until the pane close. Depends on S2. **not verified** on `hc` | U5 |
| G5 | Liveness and the screen read of the stall ladder, the watchdog duties | `HerdrObserver::observe` (`fleetd/fleet_runtime/adapters/watchdog_duties.rs:604`), built in `with_pass` (`cmd/fleetd.rs:1179`) | `pane_get` (status and `state_change_seq`) and `pane_read(pane, "visible")` | Yes: `hc get` and `hc read` (`workspace/hc.rs:397`, `:422`) | `with_pass` builds every host handle with `Backend::Herdr`. It must take the backend of the worker row. `state_change_seq` on `hc` is `status_ms`, a time in ms, not a counter (HCB-05). The stall ladder only compares it for a change, so a time works. **verified (read)** for the calls, **not verified** for the ladder on real `hc` | U3 |
| G6 | Orphan scan and pane close (I5, FM-22) | `Live::panes`, `Live::close_pane` (`cmd/fleetd.rs:1364`, `:1382`) | `workspace_list`, `pane_list`, `pane_shell_pid`, `pane_close` | `pane_close` yes (`hc kill`, then `hc rm`). `pane_shell_pid` no | On `hc` each pane is `InPane::Unknown`, so fleetd never closes an orphan pane there. That is safe but leaks panes. `hc get --json` gives a `pid` field (`docs/live-checks/hc-backend.md`, line 251: `"argv":["/bin/zsh","-l"]`, `"pid":66085`), and `TerminalRecord` ignores it (`workspace/hc.rs:98`). The scan lists only 1 host: it must list each reachable host (`Hosts::reachable`). **verified (read)** | U1, U3 |
| G7 | Workspace and tab layout | `horch fleet` (`cmd/recipes.rs:228`) | `workspace_create` with the project folder name | Yes: `herdr-app do space.new`, which needs the window open (HCB-07) | The refusal goes. The fleet start must record that the workspace is on `hc`, so that the launch (G2) and the rows (G3) use it. Tabs past 4 workers come from `workspace::place` and `tab_new`, which `hc` has. **verified (read)** | U2, U4, U7 |
| G8 | Doorbell | `Live::doorbell` (`cmd/fleetd.rs:1490`), `delivery::send_line` (`messaging/delivery.rs:190`) | `agent_prompt` with the fixed line, then `pane_send_text` as the fallback | Yes: `agent_prompt` is `hc send`. `send_line` stops at the prompt error when the fallback is `Unsupported` | Only the backend choice of G5. FR-7 holds: the line is a constant. `hc send` refuses a blocked terminal (`hcb_06_blocked_terminal_refuses_the_prompt`), so a doorbell to a blocked pane fails and the next pass sends it again. **verified (test)** for the call (`hcb_06_send_line_uses_only_the_prompt_on_hc`), **not verified** for the retry on `hc` | U3 |
| G9 | Needs-you notice | `notify::notify` (`fleetd/ideas/adapters/notify.rs:31`), `waiting::remind` (`fleetd/ideas/application/waiting.rs:184`) | `notification_show` | Yes: a no-op on `hc` (HCB-06) | No banner on `hc`. The item is still in `needs-you/` and in the ledger. No unit is needed: the notice is a hint, not the record. **verified (read)** | none |
| G10 | Mail courier | `cmd/mail.rs:96`, `check_login` (`cmd/mail.rs:470`) | `PaneHost::for_workspace`, `pane_read` | Yes | Nothing. **verified (test)**: `hc_courier` 1 / 0 | none |
| G11 | `horch doctor` | `cmd/doctor.rs` | Checks the resolved host | Yes (HCB-13) | With option (a), doctor says that `hc` is reachable and does not say that fleetd refuses it. With option (b), nothing. **verified (read)** | U8 (option a only) |
| G12 | Conformance job | `fleetd/worker_profiles/adapters/conformance_runner.rs` | No host call (a search for `PaneHost` and `herdr` in the file finds none) | Not a host call | Nothing. **verified (read)** | none |
| G13 | Test harness | `Harness::fleet_workspace` (T-890), `fake-herdr`, and main's `fake-hc`, `fake-herdr-app`, `hc_fake.rs` | The fleetd e2e tests use `fake-herdr` only | The fakes exist and build (`fakes` 61 / 0) | (1) No `fleet_workspace` for `hc`. (2) The `pid` of a fake terminal is `10000 + n` (`crates/horch-e2e/src/hc_fake.rs:294`). The real PID is in `state["pids"]`, so a `pane_shell_pid` test on the fake gets a PID that no process has. (3) The fake starts the pane command in a new process group, not a new session (`hc_fake.rs:509`). That matches the real core only if S2 says so. **verified (read)** | U6 |
| G14 | Service env and the daemon | `fleetd` (`cmd/fleetd.rs:137`), the service env (`execution/service.rs:306`) | 1 backend for the whole daemon, from its context | Not a host call | fleetd runs as an OS service with 1 env, and a fleet can be on either host. So fleetd must pick the host for each run and row (ADR 0003: "code handed a workspace or record uses `PaneHost::for_workspace` or `PaneHost::for_backend`"), not from `HORCH_BACKEND`. **verified (read)** | U3, U7 |
| G15 | Sandbox sockets of a worker | `LaunchEnv::from_context` (`harness/launch.rs:140`), `overlay_horch_paths` (`harness/claude.rs:1018`) | A worker with `horch_access: full` gets `$HERDR_SOCKET_PATH` in `allowUnixSockets` | On `hc` it gets the `herdr-core` socket and the app socket | See S1. This is a blocker. **verified (read)** | U5 |

## Security

### S1: the `herdr-core` socket opens a path around SR-8 and FR-10. Blocker.

1. ADR 0003 says that the `herdr-core` socket "can run programs and write
   files outside the sandbox (`machine.exec`, `file.write`)". On `hc`, Claude's
   Bash sandbox "is no longer an isolation boundary". **verified (read)**
2. A worker with `horch_access: full` gets that socket on `hc`
   (`harness/claude.rs:1036`, `:1042`). `full` is the default of
   `teammates/_template.md:389`. The persona `report` gets only its report
   socket. **verified (read)**
3. SR-8 needs the worker sandbox to deny 3 things: a read of
   `/fleet/run/operator.token`, a connection to the command channel, and a
   write into `/fleet/run/requests/`. A worker that asks `herdr-core` to run a
   program gets a process outside the sandbox, so none of the 3 deny rules
   apply to it. **verified (read)** for the rules, **not verified** as a run.
4. The peer check (`fleetd/fleet_runtime/adapters/peer_process.rs`) walks the
   parent chain of the caller and matches the recorded wrapper and agent, and
   their group and session. A program that `herdr-core` starts has
   `herdr-core` as its parent, and a new group and session of its own. No fact
   ties it to the attempt, so the answer is `No`. The module already names the
   same gap for a double fork, and says that the worker sandbox guards it. On
   `hc` that guard is gone. **verified (read)** for the code, **not verified**
   for the parent and group of a `machine.exec` child.
5. FR-10: `horch idea approve` refuses only inside a fleet process, by the same
   fact. A process from `machine.exec` is not a fleet process, so it can
   approve an idea. FM-32 ("A model approves its own assumptions") is then
   open. **verified (read)** for the rule, **not verified** as a run.
6. The same class of risk is known on `herdr`: R1 of
   `docs/security/sandboxed-pane-horch-access.md` ("Run any command outside
   the sandbox through the herdr socket"), with the fix C1, `horch_access:
   report`. This report does not check whether each fleetd persona on `herdr`
   is safe. That question is outside this ticket. **verified (read)** for R1.

So an `hc` adapter must give no host socket to any fleetd worker. A fleetd
worker reports through files (FR-7, Phase 5), so the fix fits the design. The
cost: every fleetd persona on `hc` must run with `horch_access: report`, or
the launch must drop the host sockets of a worker. Profile validation must
refuse the rest on `hc`, as it refuses a profile with no sandbox (SR-8).

### S2: how `hc` starts a pane process, and the wrapper record

1. The wrapper record does not use the pane host. The wrapper (`horch worker`)
   writes its own PID into its heartbeat file before its agent starts
   (`taskledger/supervisor.rs:539`). fleetd records the PID and start time on
   the worker row (`fleetd/task_execution/adapters/attempt_files.rs:438`), and
   the wrapper waits for that record (`wait_recorded`,
   `taskledger/supervisor.rs:576`). This is the same on `herdr` and on `hc`.
   **verified (read)**
2. The pane command is the same on both hosts: `pane_run` types
   `exec horch worker <role>` with its env (`execution/service.rs:310`,
   `workspace/paneshell.rs:54`). On `hc`, `pane_run` is `hc send`, which pastes
   the line and presses Enter (`workspace/hc.rs:456`). The `exec` replaces the
   pane shell, so the wrapper has the PID of the shell. **verified (read)**
3. On `herdr`, the peer check relies on this: "a herdr pane starts it as the
   leader of a session, so its PID also names its group and its session"
   (`peer_process.rs`, `FleetProcess`). On `hc`, the shell of a terminal is
   started by `herdr-core`. `hc get` shows `"parent": null`, but that field is
   the parent terminal of `hc`, not the OS parent. **verified (read)**
4. Whether the OS parent of an `hc` pane shell is `herdr-core`, and whether the
   shell leads its own session and group, is **not verified**. The live check
   of main does not print a PPID, a PGID or a SID. A program that a terminal
   emulator starts on a pty usually calls `setsid`, but that is not evidence
   for `herdr-core`.
5. If the shell does not lead its group, 3 things change. The group test of
   the peer check misses, but the walk of the parent chain still finds the
   wrapper. The stop of G4 goes to the wrapper only. `group_left` gives
   `GroupLeft::None` (`effects.rs:120`). None of these lets a process in the
   attempt pass as a non-fleet process, while its parent chain is intact.
   **verified (read)**
6. A live check on a real `hc` closes S2 (unit U9). Rule 2 of this ticket
   forbids it here.

## Units

Each unit is 1 ticket for 1 worker. Sizes: S is under half a day, M is about
1 day, L is more than 1 day. These units are for option (b), except U8, which
has a form for each option.

| # | Title | Files | The test that proves it | Size | Blocked by |
|---|---|---|---|---|---|
| U1 | `pane_shell_pid` on `hc` from the `pid` of `hc get` | `workspace/hc.rs`, `workspace/host.rs`, `crates/horch-e2e/src/hc_fake.rs` | A unit test parses `"pid":4123` of a terminal record into `pane_shell_pid`. A `pane_host_contract` case on `fake-hc` gets the PID of the real fake process | S | U6 for the contract case |
| U2 | The backend of a run and of each worker row | `taskledger/schema.rs` (a new version after `PHASE_7_VERSION` if `runs` needs a `backend` column), `fleetd/task_execution/adapters/sqlite.rs`, `fleetd/fleet_runtime/adapters/watchdog_duties.rs`, `cmd/recipes.rs` | A bind in a run whose workspace is on `hc` writes `workers.backend = 'hc'`. `horch fleet --backend hc` records `hc` on the run | M | none |
| U3 | 1 host handle for each run and row in the pass | `cmd/fleetd.rs` (`with_pass`, `Live`, `LiveIdeas`, `PushSpawns`), `fleetd/fleet_runtime/adapters/watchdog_duties.rs` (`HerdrObserver`) | Unit tests with 2 hosts: an `hc` row is observed and doorbelled on `hc`, and a `herdr` row on `herdr`. `Live::panes` lists the panes of both hosts. A host that does not answer gives no snapshot for its panes | M | U1, U2 |
| U4 | The launch of an attempt on `hc` | `cmd/fleetd.rs` (`launch_pane`), `cmd/recipes.rs` (`push_launch_spec`), `cmd/spawn.rs` or `execution/service.rs` (`split`) | The launch finds the workspace by the `workspace_id` of the run, also with the window closed. The spawn env has `HORCH_BACKEND=hc`. A spawn on `hc` places the worker with `workspace::place` | M | U2 |
| U5 | No host socket for a fleetd worker on `hc`, and profile validation for it | `harness/launch.rs`, `harness/claude.rs`, `harness/codex.rs`, the profile validation of `fleetd/worker_profiles/` | The settings of each fleetd worker launch on `hc` have no `herdr-core` socket and no app socket in `allowUnixSockets`. A persona with `horch_access: full` on `hc` is refused with a reason. A Codex launch on `hc` gets no `HC_TERMINAL_SOCKET` | M | none |
| U6 | The test harness for `hc` fleets | `crates/horch-e2e/src/harness.rs`, `crates/horch-e2e/src/hc_fake.rs`, `crates/horch-e2e/src/bin/fake-hc.rs` | `Harness::fleet_workspace` makes a space on `fake-hc` and `fake-herdr-app`. `hc get` of the fake gives the real PID of the terminal process. The fake starts that process as U9 says the real core does | M | U9 for the process model |
| U7 | `hc` fleets through fleetd, end to end | `cmd/fleetd.rs` and `cmd/recipes.rs` (remove `refuse_hc` and `HC_REFUSED`), `crates/horch-e2e/tests/hc_fleet_start.rs` | e2e on the fakes: `horch fleet --backend hc` makes a space. A push attempt starts in an `hc` pane, its wrapper is recorded, its doorbell is sent, and its stall read works. An orphan `hc` pane is closed. Workers 5 and 6 go to a tab `workers 2`. A run on `herdr` beside it is not touched | L | U1 to U6 |
| U8 | Docs and release notes | `README.md`, `docs/adr/0003-two-pane-hosts-behind-one-handle.md`, `docs/specs/pane-host.md`, `docs/fleet-workflow.md`, `docs/testing-and-gates.md`, `docs/architecture.md`, `GLOSSARY.md`, `CHANGELOG.md`; for option (a) also `cmd/doctor.rs` | Option (a): each doc says that fleetd runs only on `herdr`, the CHANGELOG says that `hc` fleets are not available, and `horch doctor` on `hc` prints that fleetd refuses `hc`. Option (b): each doc describes `hc` fleets through fleetd. `docs_sync` and `teammates --check` pass | S | option (b): U7 |
| U9 | Live check of the process tree of an `hc` pane (run by the operator) | `docs/live-checks/hc-backend.md` | On a real `hc`, for 1 pane that runs `exec horch worker`: its PPID, PGID and SID, and that `hc get` `pid` is the PID of the wrapper. And: the parent, group and session of a process that `machine.exec` starts | S | none |

A change to `docs/specs/fleetd/design.md` is also needed for option (b): the
design names `herdr` as the only pane host (section "herdr"), and SR-8 must
name the `hc` sockets. This report does not write it (rule 4).

Gaps with no unit: G9 (the notice is a hint), G10 (works), G12 (no host call).

## Options for the operator

### (a) Merge fleetd with `hc` refused, and say so

Facts:
1. The code is ready: `refuse_hc` and its 3 tests are in the trial. **verified (test)**
2. Work left: U8 in its option-(a) form, size S. **verified (read)**
3. An operator who uses `hc` fleets of v0.5.2 loses them at the update to the
   fleetd release. `horch fleet --backend hc` then fails with `HC_REFUSED`,
   which names the fix (`--backend herdr`). **verified (test)**
4. `horch doctor`, `horch mail courier` and `horch tell` keep working on
   `hc`. **verified (test)**
5. A ledger row with `backend = 'hc'` can come only from horch 0.5.2 of main.
   fleetd checks it on `herdr`. **not verified** as a behavior
   (`merge-main-proof-2.md`, item 5).
6. Option (b) stays possible later. The units do not change.

### (b) Build the `hc` adapter before the merge

Facts:
1. Work: U1 to U7 and U9, plus U8 in its option-(b) form and a change to the
   design. Sizes: 2 S, 5 M, 1 L, and the S of U8. That is about 7 to 9 worker
   days, plus the live check of U9 by the operator. **not verified** (an
   estimate from reading the code)
2. The order: U2, U5 and U9 first, in parallel. Then U1, U4 and U6. Then U3.
   Then U7. Then U8.
3. U5 is a blocker. Without it, a worker on `hc` can run a program outside its
   sandbox through `herdr-core`, and SR-8 and FR-10 do not hold (S1).
   **verified (read)**
4. U9 needs a real Herdr app and a real `hc`. A worker of this fleet must not
   start them, so the operator runs it or allows it. **verified (read)**: rule 2
   of this ticket.
5. The merge of the fleetd pull request waits for all units.

### (c) Keep the old claim-and-spawn flow of main for `hc` only

Facts:
1. The run branch retired that flow: the orchestrator pane, `claim_on`, the
   CLI wakes, `horch spawn` outside fleetd, and the old watchdog (FM-16,
   ticket 48 of fleetd-p6). The trial took "ours" in 10 conflicted files for
   that reason (rows 11, 21, 24, 25, 27, 28, 29, 30, 31 and 33 of
   `merge-main-proof-2.md`). Option (c) brings that code back. **verified (read)**
2. It makes 2 control paths: fleetd for `herdr` runs, and the old watchdog
   with an orchestrator for `hc` runs. Phase 1 of the design stops each old
   watchdog at the start of fleetd (`take_over`, `cmd/fleetd.rs:1150`),
   so that "1 control path acts on each run". fleetd would need a rule to
   leave `hc` runs alone. **verified (read)**
3. It breaks these rules of the design:
   - RR-5: from Phase 5, only fleetd writes the ledger. The old claim and
     the old worker commands write the ledger directly.
   - FM-16 and OR-3: no long-lived orchestrator conversation.
   - D3: everything is an idea. The old flow takes tasks without the idea
     dispatcher and the plan validator.
   **verified (read)** for these 3. FR-7 (only the fixed doorbell goes into a
   terminal) may break too, if the old flow types mail lines: **not verified**.
4. It does not fix S1. An `hc` worker in the old flow has the same
   `herdr-core` socket. The old flow has no SR-8 peer check. **verified (read)**
5. Each test and each doc then has 2 forms, 1 for each flow. **not verified**
   as a count.
6. Work: restore the retired code for `hc` only, guard it from fleetd, and
   restore the 6 tests of main. **not verified** as a size. It is at least as
   large as option (b), because the retired code must work beside the
   Phase 5 ledger rules.

### What the design text supports

The design (`docs/specs/fleetd/design.md`) names 1 pane host, `herdr` (section
"herdr", line 214), and gives no requirement for a second one. So option (a)
is the option that the design supports as it is written. Option (b) is
consistent with the rules of the design, but it needs a design change: a pane
host section for `hc`, and SR-8 text for the `hc` sockets. Option (c)
contradicts RR-5, FM-16, OR-3, D3 and the 1-control-path rule of
Phase 1, so the design does not support it. This report does not choose.
