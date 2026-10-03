# U21 a7b-messaging: messaging modules, delivery over WorkspaceClient, arrange, done order

Unit slug: `a7b-messaging`. Branch: `ard/a7b-messaging`. Phase: A7 (part b).
Requirements: ARC-20, ARC-21 (and the A7 gate for tile/balance).

## GOAL

Messaging lives in `crates/horch-core/src/messaging/` (`mailbox`, `brief`
(exists), `message`, `delivery`), message delivery runs over the
`WorkspaceClient` trait, `tilecmd.rs` orchestration moves to
`workspace/arrange.rs`, and `done` has one fixed, tested order. The herdr
client gets its binary from `RuntimeContext`. Behavior is unchanged.

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Read in `00-master-plan.md`: §1 layout (`workspace/arrange.rs`,
  `messaging/{mailbox,brief,delivery,message}.rs`), §3 "A7", §4 ARC-19..21
  and ARC-27 rows.
- Design doc `ai_docs/designs/2026-10-02-architecture-refactor-design.md`:
  `WorkspaceClient` (§4.7 there wants `PaneId` newtypes and
  `send_text`/`send_keys` names), messaging, the A7 section.
- Merged reports: `a7a-workspace.md` (the trait has 11 methods with `&str`
  ids; `Herdr` methods outside the trait: `send_line`, `wait_output`,
  `integration_status`, `pane_focus*`, `pane_move*`, `pane_resize`,
  `tab_*`; `workspace_create(label, focus, cwd: Option<&str>)`;
  `FakeWorkspace` with `fail_next`, `set_screen`, `calls`),
  `a2-runtime.md` (`messaging/brief.rs` exists; `Herdr` still resolves its
  binary via `agent::herdr_bin()`; `tilecmd::settle_after_close` keeps its
  own setsid code; switch to `runtime::process::spawn_detached`;
  `workspace/herdr.rs` and `message.rs` still use `ProcessEnv` shims, which
  you remove).
- Source today: `crates/horch-core/src/workspace/herdr.rs`: `send_line`
  (about 481), private `wait_for_tail` (502), `squash` (526),
  `tail_needle` (533) and their tests. `crates/horch-core/src/mailbox.rs`,
  `crates/horch-core/src/message.rs` (`ensure_tag`, `strip_done_prefix`,
  `spool_dir`, `spool`, `reference_line`). `crates/horch/src/cmd/messaging.rs`
  (`tell`, `inbox`, `assign`, `note`, `done` at 103, `register`).
  `crates/horch/src/cmd/tilecmd.rs` (`gather`, `apply`, `balance_tab`,
  `balance_all`, `restore_focus`, `run`, `tile`, `workspace_of`,
  `after_change`, `settle_after_close`).
- Parallel units: U18 `a4-harness` (harness, `worker.rs launch_agent`,
  `recipes.rs pane_launch`), U19 `a6a-store` (`ledger.rs`,
  `execution/{legacy,store}.rs`, `execution/mod.rs`), U20
  `a11-marketplace-cli` (`main.rs` skills subcommands). A later A6 unit adds
  `execution/lifecycle.rs::run_worker`; you create `execution/lifecycle.rs`
  with `done` only.
- Do not keep `resume: bool` / `no_tile: bool` anywhere (A1 grep rule).

## FILES

own:
- `crates/horch-core/src/messaging/{mailbox,message,delivery}.rs` (new; moved)
- `crates/horch-core/src/messaging/mod.rs` (your lines)
- `crates/horch-core/src/mailbox.rs`, `message.rs` (become shims)
- `crates/horch-core/src/workspace/herdr.rs` (remove the delivery functions
  after moving them; take the herdr binary as a constructor argument; keep a
  `Herdr::new()` that resolves through the context-free default only if a
  caller outside your files still needs it, and list it in the report)
- `crates/horch-core/src/workspace/client.rs` (add `send_line`-level methods
  only if delivery needs them; keep `&str` ids in this unit)
- `crates/horch-core/src/workspace/arrange.rs` (new)
- `crates/horch-core/src/execution/lifecycle.rs` (new; `done` only), `execution/mod.rs` (your line)
- `crates/horch/src/cmd/tilecmd.rs`, `crates/horch/src/cmd/messaging.rs`,
  `crates/horch/src/cmd/layoutcmd.rs`, `crates/horch/src/cmd/balancecmd.rs`
  (call sites)
- call sites of `Herdr::new()` elsewhere that must pass the binary (one-line
  edits; list them in the report)
- `crates/horch-core/tests/messaging.rs` (new)
- `ai_docs/reports/arch-refactor-dataset/a7b-messaging.md`

do not touch: `workspace/{layout,tile,balance,paneshell}.rs` (byte-for-byte
rule), U18/U19/U20 files, oracle and golden data.

## STEPS

1. Create the worktree (conventions §2).
2. Move `mailbox.rs` → `messaging/mailbox.rs`, `message.rs` →
   `messaging/message.rs` with `git mv`, then shim the old paths. Replace the
   `ProcessEnv` shim reads in `message.rs` (`spool_dir`) with a parameter
   (the temp root from ctx).
3. `messaging/delivery.rs`: move `send_line`, `wait_for_tail`, `squash`,
   `tail_needle` (and their tests) out of `workspace/herdr.rs`, written as
   free functions over `&dyn WorkspaceClient` (add the trait methods they
   need, for example `pane_read`, which exists). Keep `Herdr::send_line` as
   a one-line delegate if callers outside your files use it.
   Delivery order and timing stay identical.
4. `Herdr` gets its binary from the caller: `Herdr::with_bin(path)`, built
   from `ctx.bins.harness.herdr` in the CLI; remove `agent::herdr_bin()`
   use from `workspace/herdr.rs`.
5. `workspace/arrange.rs`: move the orchestration in `tilecmd.rs`
   (`gather`, `apply`, `balance_tab`, `balance_all`, `restore_focus`, `run`,
   `after_change`) into core, over `&dyn WorkspaceClient` where the trait
   covers the calls and `&Herdr` where it does not (tab and focus calls).
   `tilecmd.rs` keeps CLI parsing and printing only. `settle_after_close`
   uses `runtime::process::spawn_detached`.
6. `execution/lifecycle.rs`: `pub fn done(...)` with the fixed order:
   mark done in the ledger → report to the orchestrator (skipped when
   `report_to` is `None`; take a `ReportTarget { Orchestrator, None }`
   parameter, defined here if A6 has not defined it) → unregister the
   mailbox entry → settle the grid → close the pane. Each step logs and
   continues on failure exactly as today's `cmd/messaging.rs::done` does
   (read it first and keep its error handling). `cmd/messaging.rs::done`
   calls it.
7. Tests in `crates/horch-core/tests/messaging.rs` with `FakeWorkspace`:
   - `arc_20_send_line_prompt_first`: delivery calls `agent_prompt` first
     and does not fall back when it succeeds.
   - `arc_20_fallback_waits_tail`: when `agent_prompt` fails, delivery
     sends text, waits until the screen tail (`set_screen`) shows the
     needle, then sends Enter; with a screen that never shows it, it times
     out with the same error as today.
   - `arc_21_done_order`: the recorded call order is ledger done, report,
     unregister, settle, close; with `report_to: None` the report step is
     absent; with the report failing, the later steps still run.
   - The moved `tail_needle` and `squash` tests keep their names and bodies.
   - `horch tile --plan` on the existing tile fixtures prints the same
     output as your base (diff it; record the command in the report).
8. Gate after each step. Commits: `A7: Move mailbox and message into messaging/`,
   `A7: Delivery over WorkspaceClient`, `A7: Herdr binary from RuntimeContext`,
   `A7: Move tile orchestration into workspace::arrange`,
   `A7: Fixed done lifecycle order`, `A7: Add ARC-20 and ARC-21 tests`.
9. Write and commit the report. Follow conventions §6.

## DONE WHEN

- `arc_20_send_line_prompt_first`, `arc_20_fallback_waits_tail`,
  `arc_21_done_order` pass; the existing message race tests pass unchanged.
- `tilecmd.rs` holds no herdr call sequence (only parsing and printing).
- `workspace/herdr.rs` has no `agent::` and no `ProcessEnv` use.
- `just gate` is green.

## REPORT

- `horch note` after each commit.
- `horch done` summary: module map, delivery API, done order, remaining
  `Herdr::new()` callers, gotchas.
