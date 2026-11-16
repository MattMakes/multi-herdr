# 17: Gate of Phase P: PL-2 and PL-6 green on macOS, Linux and Windows

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** no
**Kind:** quality
**Review:** light
**Check:** `cargo test -p horch-core --test arch_scan && scripts/pl6/check.sh && test -s docs/specs/fleetd/gates/fleetd-pp.md`
**Own:** `docs/specs/fleetd/gates/fleetd-pp.md`, `scripts/pl6/windows-gaps.txt`, `crates/`

Spec: `docs/specs/fleetd/design.md`, Migration plan, item 2 (Phase P): "*Gate:* PL-2 and PL-6
green", the tables "What each gate needs", "Where each rule becomes green" and "What a release
does to a run in flight" (row Phase P).

## Goal

The gate of Phase P is green, with proof for each row on the 3 OSes, in
`docs/specs/fleetd/gates/fleetd-pp.md`.

## Requirements

- **PL-2**: the `arch_scan` rule of ticket 16 passes.
- **PL-6**: on each of macOS, Linux and Windows, each test has the same result before the move and
  after it. A test that passes before the move passes after it. A test on the list of expected
  failures still fails. A test that fails on Windows for a gap that exists today is fixed, or it
  is skipped on Windows by name with the row of section 6 that states the gap.
- The "run in flight" test of Phase P is the test of PL-6 (table "What a release does to a run in
  flight", row Phase P).

## Context

- `scripts/pl6/` (ticket 02) has `pl6.py record`, `pl6.py compare [--gaps <file>]`, `check.sh`,
  `BASE` (the commit before the first move) and `baseline-macos.txt`. Read its `README.md`.
- CI runs on the draft pull request of `fleet/fleetd-build` at each push, on Linux, macOS and
  Windows. You can read CI and not start it (a start gives 403):
  `gh run list --repo MattMakes/multi-herdr --branch fleet/fleetd-build`,
  `gh run view <id> --repo MattMakes/multi-herdr --log` and `--log-failed`.
- Phase 0 keeps the list of expected failures and a test runner rule that reads it. Use the list
  of expected failures of Phase 0 as Phase 0 states it; the orchestrator gives its path if you
  cannot find it.
- Section 6 of `docs/specs/fleetd/preservation-06-wiring-and-platform.md` has the rows of the
  Windows gaps of today.

## Steps

1. Run the Check on this Mac. Run the runner rule of the list of expected failures of Phase 0,
   and see it pass.
2. Find the CI run of the commit in `scripts/pl6/BASE` and the CI run of the tip of
   `fleet/fleetd-build`. If 1 of them does not exist, or its test log is not complete for an OS
   (`pl6.py record` refuses it), send BLOCKED to the orchestrator with the commit and the OS. Do
   not guess a result.
3. For Linux and for Windows: save each log, record it with `pl6.py record`, and compare the 2
   records with `pl6.py compare`.
4. For each Windows test that fails before the move and after it, for a gap of today: fix it if the
   fix is small and changes no behavior on macOS and Linux, or skip it on Windows by name with
   `#[cfg_attr(windows, ignore = "preservation-06 <row>: <gap>")]`. Add its line to
   `scripts/pl6/windows-gaps.txt`. A test on the list of expected failures stays as it is.
5. Correct each other small fault that the compare shows. A fault that is not small (more than
   about 50 lines, or a change of behavior) goes to the orchestrator as a QUESTION with a proposed
   fix task. Each change that you make needs a new CI run of Linux and Windows. Commit it, run
   `horch ledger progress "waiting: CI of <commit>"`, send a NOTE to the orchestrator that names
   the commit, and end your turn. The orchestrator puts the commit on the branch of the draft pull
   request and tells you when its CI is in. Then do steps 2 to 4 again for that commit.
6. Write `docs/specs/fleetd/gates/fleetd-pp.md`: 1 row for each gate row (PL-2; PL-6 on macOS;
   PL-6 on Linux; PL-6 on Windows; the run in flight test), with its test name, the commit, the CI
   run id where it applies, and the result. Add the list of the Windows gaps of
   `scripts/pl6/windows-gaps.txt` with their rows, and each flaky test that you saw.
7. Run the Check. Commit.

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

Rules of this phase:

- `Own` holds `crates/` only so that you can correct small faults and add the skip of a Windows
  gap test. Do not add a feature, and do not move a test.
- A gate row is green only with a result of its OS. Never write green for Linux or Windows
  without its CI run id.
- The design file is not yours: write only `docs/specs/fleetd/gates/fleetd-pp.md` under
  `docs/specs/`.

## Done when

- The Check passes.
- `docs/specs/fleetd/gates/fleetd-pp.md` shows PL-2 and PL-6 green on macOS, Linux and Windows,
  each with its test name, commit and result.
