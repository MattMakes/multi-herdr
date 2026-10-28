# 14: horch note --fact and a host line in every brief (P20, P28)

**Issue:** [#56](https://github.com/MattMakes/multi-herdr/issues/56) · **Spec:** [#46](https://github.com/MattMakes/multi-herdr/issues/46)
**Persona:** primary `backend-developer`, fallback `opus` · **Parallel:** yes
**Check:** `cargo build --workspace --bins && cargo test -p horch-core --test golden_prompts && cargo test -p horch-core prompts && cargo test -p horch-e2e --test brief --test taskledger && env HORCH_TEAMMATES_DIR=teammates cargo run --quiet --bin horch -- teammates --check`

The GitHub issue is the source of truth. This file is its copy at handoff (2026-10-28), so that `horch ledger add-map` can queue it.

## Parent

#46

## What v0.3.0 changed (checked 2026-10-28)

- **Still needed.** `horch note` has no flags. It now writes the session ledger, mirrors the note into the task ledger, and runs the context check. The worker brief is rendered by the same function as before, with the same 4 placeholders. `horch ledger note` exists but is not named in the briefing.
- **Keep `--fact` on `horch note`.** A fact belongs to the project, not to a task, and `horch note` works in every fleet. `horch ledger note --fact` may hand off to it.
- **Ledger workers** get their brief before they claim a task. The claim output (ticket, own files, do-not-touch files) is a second place where facts could be printed. Print them in both places.
- **Naming:** the roster already has a type called `ProjectFacts` (it detects project traits for teammate offers). Pick a different name for the new facts store.
- The brief's data comes either from the spawn brief (which gained the worker id and ledger path in v0.3.0) or from values computed when the brief is rendered inside the worker pane.

## What to build

Workers start with what the fleet already knows.

- **Facts.**
  - `horch note --fact "<fact>"` appends 1 line to a per-project facts file in the state folder.
  - Duplicates are ignored. The list is capped, and the newest facts win.
  - Workers and the orchestrator can both record facts. The orchestrator's Codex exec policy already allows `horch note`; the orchestrator briefing must name it, so the rule check passes.
- **Briefs.** Every worker brief gains 2 placeholders, filled at launch:
  - the project's facts so far;
  - a host line: OS, architecture, and whether the tools are BSD or GNU (for example, no GNU `timeout` and a different `sed -i` on macOS).
- `horch ledger claim` also prints the project's facts.
- Briefing text stays in `_base`. Rust only fills values.

## Acceptance criteria

- [ ] End-to-end: a worker records a fact; a worker spawned after it has that fact in its brief, and in its claim output in a ledger run.
- [ ] Recording the same fact twice stores it once.
- [ ] A worker brief on the test host names its OS and architecture.
- [ ] Facts from another project never appear.
- [ ] The worker golden changes are in a named sanctioned block, and `teammates --check` passes (including the exec-policy rule check).

## Blocked by

None (can start immediately)
