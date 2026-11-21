# The question text of an idea-lead accepts line separators and bidi characters, and the status line does not escape it

**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** security
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --test idea_notes --test idea_waiting --locked && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/ideas/adapters/idea_notes_reader.rs`, `crates/horch-core/src/fleetd/ideas/application/waiting.rs`, `crates/horch-core/tests/idea_notes.rs`, `crates/horch-core/tests/idea_waiting.rs`

## Goal

Task T-43 (finding R5-F2) shows the durable question of a clarifying idea-lead to the operator. A worker writes that text, so it is untrusted (SR-6). The finding arrived after T-43 landed:

1. `check_ask` in `crates/horch-core/src/fleetd/ideas/adapters/idea_notes_reader.rs` accepts U+2028, U+2029 and the bidi characters U+200E, U+200F, U+202A to U+202E and U+2066 to U+2069. With them a worker can make the text that the operator reads look like another line or another order.
2. `waiting_text` in `crates/horch-core/src/fleetd/ideas/application/waiting.rs` puts the question in plain quotes, so a quote mark in the question is not escaped, and the question can look like the end of the quote plus text of horch.

## Steps

1. Write the failing tests first. In `crates/horch-core/tests/idea_notes.rs`: `ask_with_a_line_separator_or_a_bidi_character_is_refused` (1 case for each character named above; the note is refused with a reason that names the character class, and no question row is written). In `crates/horch-core/tests/idea_waiting.rs`: `waiting_text_escapes_a_quote_in_the_question`.
2. Fix: `check_ask` rejects those characters (find how the reader rejects control characters now, and add to the same rule; if another reader of untrusted worker text in `crates/horch-core/src/fleetd/` has a shared function for this, use it and do not write a second list). `waiting_text` shows the question with the debug format (`{q:?}`).
3. Search for other places that show the question text to the operator (`git grep -n waiting_text` and the status views). List each one in the commit message with "escaped" or "fixed here". A place outside your Own that is not escaped: send `QUESTION:` with the file and line.

## Out of scope

Other note kinds.

## Done when

Both tests pass and the Check passes.

## Rules


1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test`, `cargo run` and each manual binary through `scripts/dev-sandbox.sh` (unit A1 makes it). Until A1 lands, use the env of `ai_docs/plans/fleetd-finish/audit/isolation.md` section "Sandbox env", with the sandbox folder inside the worktree.
3. Never install the build. No `horch self-update`, `horch install`, `cargo install`, `launchctl`. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. Each Check ends with `cargo fmt --all --check`.
6. No flaky test. A test that fails 1 time in 3 runs is a bug: find the cause.
7. A unit that removes an entry of `crates/horch-e2e/expected-failures/` runs that test 3 times.
8. Before DONE: no process that the unit started is alive (`ps`), and the sandbox folder is removed. The finish step `cleanup` removes the worktree and its `target/`.
9. A unit that lands a task branch merges it with `git merge --no-ff origin/<branch>` in its worktree, resolves conflicts, and keeps each commit of the branch.

