# 08: The Windows test job of CI

<!-- horch-ticket: 1 -->
**Persona:** primary `sonnet-sweep`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `actionlint .github/workflows/ci.yml && grep -q 'test-windows:' .github/workflows/ci.yml && grep -q 'continue-on-error: true' .github/workflows/ci.yml`
**Own:** `.github/workflows/ci.yml`

Spec: `docs/specs/fleetd/design.md`, "Migration plan" item 1 (Phase 0): "CI runs the test suite on
Windows; today the Windows job only builds".

## Goal

CI runs the test suite on Windows. The job may fail until Phase P, so it does not block a pull request.

## Requirements

No requirement becomes green in Phase 0. This ticket gives the mechanism "the Windows test job of CI
(0)" of the row Phase 0 in the table "What each gate needs".

## Context

- `.github/workflows/ci.yml` has the jobs `test` (matrix `ubuntu-latest`, `macos-15`, runs
  `cargo test --workspace --locked`), `docs`, `release-lint` and `build-windows` (runs
  `cargo build --workspace --locked` on `windows-latest`).
- CI runs only on a pull request to `main` or by `workflow_dispatch`. This ticket cannot see the
  Windows result. Say so in your summary.
- Many tests of today do not compile or do not pass on Windows. Phase P fixes them or skips them on
  Windows by name. So the new job has `continue-on-error: true` until the gate of Phase P removes
  that line (orchestrator answer to Q2 of the plan).

## Steps

1. Add the job `test-windows` to `.github/workflows/ci.yml`, after `build-windows`. Copy the 3 setup
   steps of `build-windows` exactly (checkout, rust-toolchain, rust-cache, with the same pinned SHAs).
   Then add the step `- run: cargo test --workspace --locked`.
2. Give the job `continue-on-error: true`. Put this comment on the line above it:
   `# Until Phase P of fleetd: many tests do not run on Windows yet. The gate of Phase P removes this line.`
3. Do not change the other jobs. Keep `build-windows`.
4. Run the Check.

## Constraints

1. This fleet runs on the installed `horch` binary and on the live ledger
   `/Users/MASCOTT/.horch/ledger.sqlite3`. The code that you change is the code of that tool.
   Do not break the fleet that builds it:
   - Each test, and each `horch` or `fleetd` process that you build and start, uses temporary
     folders for `HORCH_DATA_DIR`, `HORCH_STATE_DIR` and `HORCH_LEDGER_DB`. Never point a
     development build at the live ledger or at the live state folders.
   - Never install a development build. Never run `horch self-update`, `cargo install` or a
     release script from a worktree.
   - Never start a development `fleetd`, watchdog or service unit on this machine outside a test
     that uses temporary folders. Never register a launchd job, a systemd unit or a scheduled task
     on this machine.
   - The only `horch` commands that you run on the live fleet are the worker commands of your own
     task (`horch ledger ...`, `horch tell orchestrator ...`).
2. NEVER use `ANTHROPIC_API_KEY`. Never read, print, set, export, pass or depend on it. Never
   write code, config, docs or teammate files that use it. Each `claude` run uses the claude.ai
   subscription login. Remove the variable from each `claude` command:
   `env -u ANTHROPIC_API_KEY claude ...`. If a task seems to need an API key, stop and ask.
3. Do not use a shared `CARGO_TARGET_DIR`. Build in the `target/` folder of your worktree. Run
   `cargo clean` in your worktree before `horch ledger finish` only if the disk has less than
   20 GB free (`df -h /`).
4. Before the first commit, run `git remote -v` and `git config user.email`. The remote must be
   `git@git-alt:MattMakes/multi-herdr.git` and the email must be `m4ttplu5@gmail.com`. If not,
   stop and send BLOCKED.
5. Commit messages have no co-author line. Never write the em dash character; use `-`. Never edit
   `CHANGELOG.md` or a generated file by hand.
6. Write the tests first and see them fail for the correct reason. Then write the code.
7. The source of truth is `docs/specs/fleetd/design.md` (FINAL). If the code of today and the
   design do not agree on a point that the ticket does not settle, stop and send QUESTION. Do not
   change `docs/specs/fleetd/` or `ddd/`.
8. Change only the files of `Own`. If the work needs another file, send QUESTION.
9. Prefer quality, simplicity and robustness to speed. No feature that the design does not name.
10. If the GitHub SSH connection times out at `horch ledger finish`, run it again, 3 times maximum.

## Done when

- The Check passes.
- The diff adds only the job `test-windows` and its comment.
- The summary says that the Windows result comes from CI on the next pull request to `main`.
