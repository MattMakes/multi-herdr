# A worker of a push attempt gets no project facts and no host line (g56_later_spawn_receives_recorded_fact)

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test fleet18_brief_worktree && scripts/dev-sandbox.sh cargo test -p horch-core --locked --test arch_scan && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/fleet18_brief_worktree.rs`

## Fault

`g56_later_spawn_receives_recorded_fact` (`crates/horch-e2e/tests/fleet18_brief_worktree.rs:352`) is red 3 of 3 on macOS and in CI run 5 on macOS and Linux. Measured by task T-397 in a kept world (its commit b42a2d47 adds the missing `[gates]` table to the `fleet.toml` of the test; start from it): the task starts, but the `claude` call of a push attempt has only the path of `START.md` as its prompt (`crates/horch-core/src/execution/lifecycle.rs:1276`). The worker briefing with the project facts and the host line is built only in the branch `(None, None)` at line 1277, which a push attempt does not use. The fact is in `state/facts`, but no file of the fleet folder and no argument of the CLI holds it. So a fact that 1 worker recorded does not reach a later worker under fleetd. Released horch (main) gives it to each spawn (requirement G56 of the fleet-18 work: search the docs and tests for `g56`).

## Steps

1. Read the design of `START.md` (`docs/tickets/fleetd-p4/07-task-yaml-and-start-md.md` and `docs/specs/fleetd/design.md`: search for START.md, brief, facts) and the code that writes `START.md` and `task.yaml` for a push attempt. Does the design name where the project facts and the host line go for a push attempt? Quote the lines. If the design is silent, send `QUESTION:` with 2 options (a section of `START.md`; a file of the attempt folder that `START.md` names) before you write product code, and with the file list (product files are outside your Own).
2. Implement: the facts that the briefing of main gives (the same source and the same filter and size bound; 1 place builds the text for both paths) reach the worker of a push attempt. A fact is data that workers wrote: it must not be able to change the instructions of the fleet (keep it in a marked data section; say how the text is bounded and escaped) and must hold no secret that the briefing of main would not give.
3. The test asserts the designed place (the file or section), not a CLI argument, and stays strict: the later worker gets the exact fact. Run the file 3 times.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`. Run each `cargo test` through `scripts/dev-sandbox.sh`.
2. Never read, set or pass `ANTHROPIC_API_KEY`. Never write or read `~/.horch`, `~/.local/bin`, `/fleet`. Never install the build.
3. No flaky test, no `#[ignore]`, no weaker assert, no expected-failures entry. No OS `cfg` outside `horch-platform`.
4. Never run pkill, killall or `rm -rf /tmp/horch-e2e-*`. Never touch a process of another fleet.
5. `git fetch` and `git rebase fleet/fleetd-finish` before each `horch ledger finish`. After the comment sweep, restore each comment that gives a rule or a design id.
6. Before DONE: no process of the unit is alive, and the sandbox folder is removed.

## Done when

The Check passes 3 times and a later worker of a push attempt gets the recorded fact.
