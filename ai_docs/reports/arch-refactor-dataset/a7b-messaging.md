# a7b-messaging report (A7 part b)

Branch `ard/a7b-messaging`. Requirements ARC-20 and ARC-21; the A7 gate for
tile and balance. Behavior is unchanged.

## Module map

| Before | After |
|---|---|
| `mailbox.rs` | `messaging/mailbox.rs` (`git mv`); `mailbox.rs` is a shim |
| `message.rs` | `messaging/message.rs` (`git mv`); `message.rs` is a shim |
| `workspace/herdr.rs` `send_line`, `wait_for_tail`, `squash`, `tail_needle` | `messaging/delivery.rs`, free functions over `&dyn WorkspaceClient` |
| `cmd/tilecmd.rs` `gather`, `apply`, `declined`, `balance_tab`, `balance_all`, `restore_focus`, `TileLock`, `roles`, `run`, `workspace_of`, `after_change`, `settle_after_close` | `workspace/arrange.rs` |
| `cmd/layoutcmd.rs` `report`, `holds`, `verdict` | `workspace/arrange.rs` |
| `cmd/balancecmd.rs` `equalize`, `equalize_quietly`, `orchestrator_of` | `workspace/arrange.rs` (+ new `balance_target`) |
| `cmd/messaging.rs::done` (the steps) | `execution/lifecycle.rs::done` |

`tilecmd.rs`, `layoutcmd.rs` and `balancecmd.rs` now hold parsing and printing
only. Re-exports for `cmd/spawn.rs` (not my file): `tilecmd::after_change` and
`balancecmd::equalize_quietly`.

## Delivery API (`horch_core::messaging::delivery`)

- `send_line(ws: &dyn WorkspaceClient, pane, message) -> Result<()>`: the real
  waits (`Timing::DEFAULT`).
- `send_line_with(ws, pane, message, &Timing)`: the same with explicit waits.
- `Timing { tail_timeout 15 s, poll_start 100 ms, poll_max 1 s, settle_short 1 s,
  settle_long 2 s (message over 1500 bytes), second_enter 1 s }`.
  `arc_20_default_timing_is_unchanged` pins these values.
- Order: `agent_prompt`; if it fails: `pane_send_text`, poll `pane_read(visible)`
  for the squashed 16-char tail, then `enter`, and a second `enter` 1 s later
  only when the tail showed. A timeout is not an error: it settles and sends
  one `enter`, as before.
- `Herdr::send_line` stays as a one-line delegate. Caller: `cmd/smoke.rs`.
- `WorkspaceClient` needed no new methods.

## done order (`horch_core::execution::lifecycle`)

`done(ws: &dyn WorkspaceClient, steps: &dyn DoneSteps, req: &DoneRequest)`:

1. strip `[role] DONE:` from the summary; `steps.mark_done` (error stops);
2. `steps.report("[role] DONE: <summary>")` only for
   `ReportTarget::Orchestrator`; an error is logged, then the steps go on;
3. `ws.pane_get(pane)` (error stops); workspace = `req.workspace`, else the
   pane's own (error stops);
4. `steps.unregister(workspace, role)`;
5. `steps.settle(workspace)`;
6. `ws.pane_close(public pane id)`.

`ReportTarget { Orchestrator, None }` is defined here (design 4.5 shape,
serde `snake_case`). A6 can move it to `execution/plan.rs` and re-export it.
`DoneSteps` is a trait (`mark_done`, `report`, `unregister`, `settle`), so
the order is testable without a ledger. The CLI implements it as `CliDone`
in `cmd/messaging.rs` (ledger, `horch tell`, mailbox, `arrange::settle_after_close`).
The design's `done(ctx, store, ws, summary)` signature waits for the store
(A6); `run_worker` is not here.

## Herdr binary

- `Herdr` is `Herdr { bin: PathBuf }`, built with `Herdr::with_bin(path)`.
  `workspace/herdr.rs` has no `agent::` and no `ProcessEnv` use.
- `Herdr::new()` and `Default` are removed. No caller remains. Every caller
  now passes `ctx.bins.harness.herdr`. One-line edits outside my unit files:
  - `cmd/doctor.rs` (1), `cmd/telemetry.rs` (1), `cmd/spawn.rs` (1),
    `cmd/smoke.rs` (3), `cmd/recipes.rs` (3), `cmd/worker.rs` (2).
  - `recipes.rs` and `worker.rs` are U18 files. A merge conflict there is a
    one-line conflict: keep U18's line and change `Herdr::new()` to
    `Herdr::with_bin(&ctx.bins.harness.herdr)`.
  - A new `Herdr::new()` call from another branch fails to compile. That is
    deliberate: a silent default would ignore `HORCH_HERDR_BIN`.
- `worker.rs::start_harvest` builds its handle after `ctx.apply_overrides`, so
  it uses the brief's `HORCH_HERDR_BIN` (before: the process environment). Both
  hold the spawner's value in practice.

## Tests added: 4 (+1 CLI guard)

- `tests/messaging.rs`: `arc_20_send_line_prompt_first`,
  `arc_20_fallback_waits_tail`, `arc_20_default_timing_is_unchanged`,
  `arc_21_done_order`.
- `cmd::tilecmd::tests::the_telemetry_label_matches_the_one_tiling_skips`:
  core holds `arrange::TELEMETRY_WORKSPACE_LABEL`, a copy of
  `cmd/telemetry.rs`'s `WORKSPACE_LABEL` (I did not edit telemetry.rs beyond
  the one line). The test keeps them equal.
- Moved unchanged (names and bodies): `tail_needle_survives_a_soft_wrap_inside_a_box`
  (now `messaging::delivery::tests`), `a_declined_move_names_the_command_and_says_nothing_was_lost`,
  `the_tile_lock_excludes_a_second_tiler_and_is_released_on_drop`,
  `a_stale_lock_is_taken_over_rather_than_waited_on` (now `workspace::arrange::tests`;
  only the `Direction` path changed). The message tests in `messaging/message.rs`
  and the mailbox tests moved with their files, unchanged.

## tile --plan check

A stub herdr (`/tmp/a7b/fx/herdr`, a shell script that `cat`s JSON fixtures for
`workspace list`, `tab list`, `pane list` and `pane layout`) served 3
workspaces: `w5` (orchestrator + 5 workers over 2 tabs), `w6` (1 tab, 2
workers), `w0` (the telemetry space), and `w9` (missing). Command, run with the
base binary (built from `b484434`) and with this branch:

```
env -u HORCH_WORKSPACE_ID -u HERDR_PANE_ID TMPDIR=/tmp/a7b/fx/tmp \
  HORCH_HERDR_BIN=/tmp/a7b/fx/herdr horch tile --plan --workspace <w5|w6|w0|w9>
```

Also `horch layout --workspace <w5|w6>` and `horch balance --dry-run`
(`--workspace w5`, `--workspace w6`, `--pane w5:p4`). All 9 outputs and exit
codes are byte-identical to the base.

## Decisions

- `message::spool_dir(state_root)` takes the state root. The plan said "the
  temp root from ctx", but the spool has always lived under the state root
  (`<state>/messages`); the temp root would change behavior.
- `arrange` functions take `&Herdr`, not `&dyn WorkspaceClient`: every one of
  them calls a tab, layout, move, resize, focus or `workspace_list` method,
  and `Mailbox::resolve_in` takes `&Herdr`. None is covered by the trait alone.
- `settle_after_close` uses `runtime::process::spawn_detached`.
- `workspace/mod.rs` got `pub mod arrange;` (needed for the new file).

## Gotchas for later phases

- A6 (`a6b-service`): `lifecycle::done` takes `DoneSteps`; fold `mark_done`
  into `ExecutionStore` when the store owns `done`. Add `run_worker` beside it.
- A12: delete the shims `mailbox.rs`, `message.rs`, `Herdr::send_line`, and the
  re-exports in `tilecmd.rs` and `balancecmd.rs`.
- `ledger::state_root()` has no caller in core any more (its last one was
  `message::spool_dir`). It stays because `ledger.rs` is not my file.
- `WorkspaceClient` still uses `&str` ids and the `pane_send_text` /
  `pane_send_keys` names (design 4.7 wants `PaneId` and `send_text`/`send_keys`).

## Outside my scope (not fixed)

- `crates/horch-core/tests/execution_store.rs:15` warns: unused import
  `KIND_ORCHESTRATOR` (from the base after the a6a-store merge).
