# 05: Delivery tests at fleet start and in doctor (P01)

**Issue:** [#61](https://github.com/MattMakes/multi-herdr/issues/61) · **Spec:** [#46](https://github.com/MattMakes/multi-herdr/issues/46)
**Persona:** primary `backend-developer`, fallback `opus` · **Parallel:** yes
**Check:** `cargo build --workspace --bins && cargo test -p horch-e2e --test smoke --test e2e && cargo test -p horch doctor`

The GitHub issue is the source of truth. This file is its copy at handoff (2026-10-28), so that `horch ledger add-map` can queue it.

## Parent

#46

## What v0.3.0 changed (checked 2026-10-28)

- **Still needed as written.** Neither `horch doctor` nor `horch fleet` has a messaging probe.
- **Build on** `horch smoke messaging`: a 2-pane scratch workspace, register 2 roles, type an arithmetic echo, read the pane. Factor out a quick version of it.
- Every horch command except the supervisor and the watchdog itself now starts the task-ledger watchdog if it is not running. A probe run from `doctor` or `fleet` triggers that, which is harmless.

## What to build

A broken delivery path is caught before any worker relies on it.

- **Fleet start.**
  - After the orchestrator starts, `horch fleet` enqueues 1 test message: "run horch inbox to confirm delivery".
  - It prints whether the message was acknowledged within a fixed time.
  - If it was not, it prints a warning that names the step that failed (no relay, not idle, not acknowledged) and leaves the mail pending for the courier.
- **Doctor.**
  - `horch doctor` runs a quick version of the messaging smoke test: a scratch workspace, 2 panes, a typed echo that prints a marker only if it runs.
  - It cleans up on success, and prints a warning on failure.
  - Exit code 0 is kept unless herdr is unreachable.

## Acceptance criteria

- [ ] End-to-end: with a fake orchestrator that runs `horch inbox` at start, `fleet` prints that delivery is confirmed.
- [ ] With a fake orchestrator that never runs `inbox`, `fleet` prints the warning and the mail stays pending.
- [ ] `horch doctor` against the fake herdr passes the delivery test, and leaves no workspace behind.
- [ ] With typing failing in the fake herdr, doctor warns and exits 0.

## Blocked by

- #60 (Pitfalls 04: Worker mail is delivered until acknowledged)
