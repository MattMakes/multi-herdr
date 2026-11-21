# 2 hermetic story tests (and maybe 1 routing test) wait for the old text of a launch refusal

**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test scenario --test routing_provenance && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/scenario.rs`, `crates/horch-e2e/tests/routing_provenance.rs`

## Fault

On the tip of the run branch (proved on base 25660806 by task T-384) `the_hermetic_story` and `the_hermetic_story_with_an_ollama_that_starts_in_7_s` in `crates/horch-e2e/tests/scenario.rs` time out after 120 s: line 192 waits for the text `launch_failed: REFUSED: opus cannot start.`, and the ledger shows the refusal as `its spawn failed: REFUSED: ...`. Task T-254 (commit 893eb71d, `end_failed_spawn`) changed the end reason of a failed spawn: the designed reason has the prefix `its spawn failed: `. Tasks T-300 and T-336 moved 2 other tests to it. `crates/horch-e2e/tests/routing_provenance.rs:139` holds the same old text.

## Steps

1. Run both files first and write down each red test with its message (a test that is green needs no change).
2. Read `end_failed_spawn` and how the reason is made: what is the full designed text for this case (where it is shown: the attempt end reason, an event, the status line, a message to the orchestrator)? If the product still makes a text with `launch_failed:` in another place (an event kind or a wake that these tests read), find out why that text does not come: a retired wake of the orchestrator (FM-16) is not a reason to wait for another text blindly: assert the designed effect that takes its place and name the design id in the commit message.
3. Move each stale assert to the full designed text, as an exact match where the test has the whole text. No weaker assert, no longer wait. The rest of each story must still run to its end: a story that stopped at this line until now can show the next stale assert; fix each in the same way, and send `QUESTION:` when a later step fails for a cause that is not a stale text.
4. Run each file 3 times.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`. Run each `cargo test` through `scripts/dev-sandbox.sh`.
2. Never read, set or pass `ANTHROPIC_API_KEY`. Never write or read `~/.horch`, `~/.local/bin`, `/fleet`. Never install the build. Never start a real ollama: the story uses its fake.
3. No flaky test, no `#[ignore]`, no weaker assert, no expected-failures entry.
4. Never run pkill, killall or `rm -rf /tmp/horch-e2e-*`. Never touch a process of another fleet. 1 `horch ledger progress` line each 5 minutes while a long test runs.
5. `git fetch` and `git rebase fleet/fleetd-finish` before each `horch ledger finish`.
6. Before DONE: no process of the unit is alive, and the sandbox folder is removed.

## Done when

The Check passes 3 times and the commit message names T-254 and each changed assert.
