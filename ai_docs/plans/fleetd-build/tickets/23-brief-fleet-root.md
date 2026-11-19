# The brief of a spawn carries the fleet root, so a worker pane uses the fleet.toml of its fleet

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --lib messaging && cargo test -p horch-core --lib execution && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo test -p horch-e2e --test taskledger && cargo fmt --all --check`
**Own:** `crates/horch-core/src/messaging/brief.rs`, `crates/horch-core/src/execution/service.rs`, `crates/horch-core/src/execution/lifecycle.rs`

## Goal

A worker pane reads `fleet.toml` from the fleet root of the process that spawned it, also when
that process had an explicit `FLEET_ROOT`.

## Context

Review finding F1 of T-262 (the wrapper heartbeat). A fresh pane does not inherit `FLEET_ROOT` or
`HORCH_LEDGER_DB` of the spawner. T-262 (commit f4e254b8) made `enter_context` in
`crates/horch-core/src/execution/lifecycle.rs` set the fleet root to the folder `fleet` beside the
ledger of the brief. 1 gap stays: a spawner with an explicit `FLEET_ROOT` that is not beside its
ledger. The pane then reads the heartbeat period, and each other key, from the wrong `fleet.toml`.

## Steps

1. Write the failing test first: a spawner context with an explicit fleet root in another folder;
   the brief; `enter_context`; assert that the worker context has the fleet root of the spawner.
2. Add the resolved fleet root of the spawner to `Brief` (`messaging/brief.rs`). Keep an old
   brief with no such field valid: then the fleet root is the folder beside the ledger, as today.
3. `execution/service.rs` writes the field. `enter_context` applies it before a read of
   `fleet.toml`.
4. The brief is a file that a worker can read. The field is a path, not a secret. Check it as a
   path that the spawner wrote: it must be absolute.
5. Run the Check.

## Constraints

- Before the commit, run `git remote -v` and `git config user.email`. The remote must be
  `git@git-alt:MattMakes/multi-herdr.git` and the email `m4ttplu5@gmail.com`. If not, send BLOCKED.
- No co-author line. Never write the em dash character. NEVER use `ANTHROPIC_API_KEY`.
- Do not use a shared `CARGO_TARGET_DIR`. Never install a development build. Each test uses
  temporary folders, never the live ledger.
- If a frozen oracle or a golden file of the brief changes, check each changed line by hand and
  name the files in your done summary.
- Run `horch ledger progress` each 5 min during a long run.
- If the GitHub SSH connection times out at `horch ledger finish`, run it again, 3 times maximum.

## Done when

The Check passes, and the new test fails without the change.
