# Make the guard tests of the build branch pass again

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo fmt --all --check`
**Own:** `crates/horch-core/tests/nfr.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/sqlite.rs`, `crates/horch-core/src/messaging/report.rs`

## Goal

The guard tests of `horch-core` and the format check pass on the run branch. 3 small faults
landed because the Check of each ticket was narrow.

## Steps

1. Run the Check and read each failure.
2. `nfr_05_no_new_runtime_crates` fails with "horch-core gained 'toml'". The ticket of the fleet
   config added the crate `toml` on purpose. Add `toml` to the list, with its reason, in the
   style of the list.
3. `no_core_type_is_named_plain_ledger` (test `architecture_doc`) fails on a test module that
   declares `struct Ledger` in `crates/horch-core/src/fleetd/fleet_runtime/adapters/sqlite.rs`.
   Rename that test struct to `TestLedger`. Change no production name.
4. `cargo fmt --all --check` reports `crates/horch-core/src/messaging/report.rs`. Run rustfmt on
   that file only.
5. If the Check shows another failure of the same kind (a list or a name that a landed ticket
   did not update), correct it and name the file in your done summary. If it shows a failure of
   another kind, send QUESTION.

## Constraints

- Before the commit, run `git remote -v` and `git config user.email`. The remote must be
  `git@git-alt:MattMakes/multi-herdr.git` and the email `m4ttplu5@gmail.com`. If not, send BLOCKED.
- No co-author line. Never write the em dash character. NEVER use `ANTHROPIC_API_KEY`.
- Do not use a shared `CARGO_TARGET_DIR`. Never install a development build. Each test uses
  temporary folders, never the live ledger.
- If the GitHub SSH connection times out at `horch ledger finish`, run it again, 3 times maximum.

## Done when

The Check passes.
