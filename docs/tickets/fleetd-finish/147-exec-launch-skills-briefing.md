# A worker that fleetd launches in exec mode gets no skills briefing

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-e2e --test skills_exposure --locked && scripts/dev-sandbox.sh cargo test -p horch-core --test harness_defaults --locked && cargo fmt --all --check`
**Own:** `crates/horch-core/src/harness/launch.rs`, `crates/horch-core/src/harness/claude.rs`, `crates/horch-core/src/harness/codex.rs`, `crates/horch-e2e/expected-failures/skills_exposure.txt`, `crates/horch-core/tests/harness_defaults.rs`

## Goal

On `main`, a worker that horch launches gets a skills briefing at the start of its prompt: 1 line for each skill of its persona (`- horch:<name>: <description>`) and 1 line for each operator skill that is not installed (`Skipped: operator skill <name> is not installed on this host`). `Bundle::briefing_in` makes this text, and `command_with_skills_in` (`crates/horch-core/src/harness/launch.rs`, about line 338) puts it before the prompt. The exec path of fleetd (`horch spawn <persona> --attempt <id>`) does not call it: the exec prompt is only the path of `inbox/START.md` (`crates/horch-core/src/harness/claude.rs`, about line 183). So a worker of fleetd does not know its skills. Task T-107 found this.

## Read first

- `crates/horch-e2e/tests/skills_exposure.rs`: the tests `operator_skills_e2e_exposure_claude` and `operator_skills_e2e_check_warns_on_a_missing_name`, and `crates/horch-e2e/expected-failures/skills_exposure.txt` (T-107 lists the 2 tests there with the name of this ticket).
- `crates/horch-core/src/harness/launch.rs`: `command_with_skills_in`, `run_exec_flow`, and where `START.md` gets its text.
- `docs/specs/fleetd/design.md`: the task files of an attempt (`inbox/START.md`, `task.yaml`), to decide the correct place of the briefing.

## Steps

1. Write the failing test first in `crates/horch-core/tests/harness_defaults.rs`: an exec launch of claude and of codex with a persona that has 1 installed skill and 1 missing operator skill gives the 2 briefing lines to the agent. Name: `exec_launch_gives_the_skills_briefing`.
2. Fix: the exec launch gives the same briefing text as the interactive launch. Use the one function `Bundle::briefing_in`; do not copy its text. Choose the place by the design: the start of the exec prompt, or a section of `START.md`. Write the choice and its reason in the commit message.
3. Remove the 2 entries from `crates/horch-e2e/expected-failures/skills_exposure.txt` (delete the file if it is empty). Run the 2 tests 3 times.

## Out of scope

Other harnesses with no exec adapter. The skill store.

## Done when

The new test passes, the 2 e2e tests pass 3 times with no expected-failures entry, and the Check passes.

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

