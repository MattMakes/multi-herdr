# 06: Detect a logged-out orchestrator and alert the operator (P32)

**Issue:** [#62](https://github.com/MattMakes/multi-herdr/issues/62) · **Spec:** [#46](https://github.com/MattMakes/multi-herdr/issues/46)
**Persona:** primary `backend-developer`, fallback `opus` · **Parallel:** yes
**Check:** `cargo build --workspace --bins && cargo test -p horch-e2e --test mail`

The GitHub issue is the source of truth. This file is its copy at handoff (2026-10-28), so that `horch ledger add-map` can queue it.

## Parent

#46

## What v0.3.0 changed (checked 2026-10-28)

- **Still needed as written.** Nothing detects a login prompt. The courier never reads pane text, and nothing calls `herdr notification show`; fake herdr only lists the subcommand.
- **Build on:**
  - the herdr pane read the watchdog's observer now uses;
  - the screen-fixture pattern of the task ledger's working-timer parser (saved pane screens, 1 table entry per format).
- Task-ledger wakes go through the same tell path, so a wake sent while the orchestrator needs a login should record that in its wake event.

## What to build

An expired Claude login is noticed in seconds, not after 29 minutes.

- On each pass, the courier reads the orchestrator pane's recent text and matches the Claude login prompt ("Please run /login").
- The match strings live in 1 small table with saved-screen fixtures, so a changed prompt is a 1-line fix.
- On a match:
  - the courier records the state;
  - it sends 1 herdr notification with a title and body that say the orchestrator needs `/login`;
  - it stops typing redeliveries into that pane.
- While the state holds, `horch tell orchestrator` prints that the orchestrator needs a login, and still queues the mail. A task-ledger wake records the same in its event.
- The state clears when the prompt is gone from the pane, and redelivery resumes.

## Acceptance criteria

- [ ] End-to-end: with a fake-herdr pane whose text holds the login prompt and pending mail, the courier sends exactly 1 notification, and types nothing into the pane.
- [ ] A `tell` in that state prints the login message, and leaves the mail pending.
- [ ] After the pane text changes, the courier redelivers and sends no second notification.
- [ ] Fake herdr supports `notification show`, and records the call for the test.
- [ ] The prompt matcher has a saved-screen fixture for the current Claude Code login prompt.

## Blocked by

- #60 (Pitfalls 04: Worker mail is delivered until acknowledged)
