# Dev and test builds keep only line tables, so each worktree build is smaller and faster

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo build --workspace --bins --locked && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc --locked && cargo fmt --all --check`
**Own:** `Cargo.toml`

## Goal

A debug build of the workspace needs less disk and less time. Each worker of the fleet builds the
workspace from zero in a new worktree, and the disk of the build machine limits how many workers
run. A panic of a test still prints file and line.

## Context

The operator asked for a faster build of fleetd (2026-11-18). A worktree `target/` folder is 8 to
10 GB today. The root `Cargo.toml` has only `[profile.release]`. The dev and test profiles use
the default `debug = true` (full debug information).

## Steps

1. Measure before: in your new worktree run `cargo build --workspace --bins --locked` and
   `cargo test --workspace --locked --no-run`. Note the wall time of each and `du -sh target`.
2. Add to the root `Cargo.toml`:

   ```toml
   [profile.dev]
   debug = "line-tables-only"

   [profile.test]
   debug = "line-tables-only"
   ```

   If a build script, a test or a guard test reads the profile and fails, stop and send QUESTION.
3. Measure after: `cargo clean`, then the same 2 commands. Note the wall times and `du -sh target`.
4. Check that a failing assert still prints its file and line (run 1 test that you make fail in a
   scratch edit, then undo the edit).

## Constraints

- Change only the root `Cargo.toml`. No change to the release profile.
- Simplified Technical English. No em dash.
- Do not run the whole test suite; `--no-run` is enough for the measure.

## Done when

- The Check passes.
- The done summary gives the 2 sizes and the 4 wall times, before and after. If the size goes
  down by less than 20 percent, say so; the orchestrator then decides to keep or to revert.

## Report

Use `horch ledger done` with the summary. It is the only DONE path.
