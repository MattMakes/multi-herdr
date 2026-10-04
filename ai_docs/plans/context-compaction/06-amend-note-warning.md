# Brief 06: amend the design and plan with a context warning in `horch note`

## GOAL
Edit `ai_docs/designs/context-compaction.md` and
`ai_docs/plans/context-compaction/05-implementation.md` so that `horch note`
warns a worker when its own context is over its threshold. Both files stay
self-contained and consistent.

## CONTEXT
The operator approved this change on 2026-09-28. The design (decision D3)
checks context only when the orchestrator handles a worker message or spawns.
`horch note` records progress in the ledger and never reaches the
orchestrator. A worker on a long quiet task is therefore not checked until the
harness's own auto-compaction fires (467,000 for Claude today), and that path
writes no handoff. A `horch note` call is already a worker stopping point
(design §5.2 step 3), so it is the right place for a self-check.
The design and plan will be committed and built by a remote Claude cloud
session. Keep the Tier 1 (cloud) checks runnable with no herdr.

## FILES
- own: `ai_docs/designs/context-compaction.md`,
  `ai_docs/plans/context-compaction/05-implementation.md`.
- do not touch: every other file. No source edits.

## PRIOR WORK
- The design and plan were written by staff-engineer-1. Read both in full first.
- `horch note` is `crates/horch/src/cmd/messaging.rs:71` (`pub fn note`), wired at
  `crates/horch/src/main.rs:304`. Workers have `HORCH_ROLE`, `HORCH_RECORD_ID`,
  `HORCH_SESSION_ID` in their env. No unit owns `messaging.rs` today.

## BEHAVIOR TO SPECIFY
1. After `horch note` records the note, it computes a `Reading` for the
   calling worker's own session with the U1 readers and the U3 threshold
   (`compact_at`, `HORCH_COMPACT_AT`, 300,000, `min` with 0.8 x native trigger).
2. If the reading is over the threshold, `horch note` prints one warning
   message to stdout. The text is a new message key in
   `teammates/_base/compaction.md` (prompts are data; Rust only substitutes
   placeholders such as role, tokens, threshold, handoff path).
3. The warning tells the worker: you are at a stopping point now; load the
   handoff skill; write `ai_docs/handoffs/<role>-whats-next.md`; run
   `horch note "handoff: <path>"`; send
   `horch tell orchestrator "[<role>] NOTE: COMPACT-READY <path>"`; end the turn.
   This skips the orchestrator's `--request` round trip. The orchestrator
   then runs `horch compact <role>` exactly as in design §5.2 step 7.
4. Repeat control: decide and justify. For example, do not print the warning
   when the ledger already has `compact-requested` or a `compacted` event after
   the last reading below threshold, or print it at most once per N notes.
   The `horch note "handoff: ..."` call in step 3 must not trigger a second warning.
5. Failure is silent: any error while reading context (no transcript, unknown
   harness, unreadable file, no `HORCH_SESSION_ID`) never changes the exit code
   or output of `horch note`. The note is always recorded first.
6. Cost: the check is a tail read (about 0.03 s). State that.
7. Orchestrator: `horch note` does not work in the orchestrator pane
   (no `HORCH_*` env). State that the orchestrator keeps its own
   `horch context --over` checkpoints. No change there.

## STEPS
1. Design: update §2 (decision summary, D3), §5.1, §5.2 (add the
   self-warning path next to the `--request` path), §8.1 (Rust surface:
   `messaging.rs`), §8.2 (new message key), §8.3 (worker prose: "If `horch note`
   prints a context warning, follow it"), §9.1 (a new Tier 1 check with an
   exact command and expected output, runnable against fixtures with no herdr,
   using the existing transcript or home override), §9.2 (a local check), and
   §12 risks if needed. Check: `grep -n "note" ai_docs/designs/context-compaction.md`
   shows the new text in each of those sections.
2. Plan: give `crates/horch/src/cmd/messaging.rs` to U4 (it already depends on
   U1, U2, U3). Add the new message key to U3's steps and tests. Add the prose
   line to U5's steps and its sanctioned golden block. Add the new check to U7.
   Add an acceptance requirement row (AR11). Check: the ownership table still
   has no file with 2 owners.
3. Re-read both files end to end. Check: every cross-reference (§ numbers,
   check ids C*, L*, AR*) resolves.

## CONSTRAINTS
- Keep the existing structure and numbering. Add, do not renumber, where possible.
- Plain exact English.

## DONE WHEN
- Both files describe the note warning consistently, with a Tier 1 check and
  an owner for `messaging.rs`.

## REPORT
- `horch tell orchestrator "QUESTION: ..."` if a decision needs the operator.
- `horch done` summary: the sections changed, the repeat-control rule, the new
  check id, and the unit that owns `messaging.rs`.
