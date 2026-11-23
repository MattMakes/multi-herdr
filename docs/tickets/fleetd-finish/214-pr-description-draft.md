# Draft of the description of the pull request to main: what fleetd changes, in detail

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** docs
**Review:** none
**Check:** `test -s docs/specs/fleetd/gates/pr-description.md && grep -q '^## What changed' docs/specs/fleetd/gates/pr-description.md && grep -q '^## Decisions for the operator' docs/specs/fleetd/gates/pr-description.md && grep -q '^## How it was proved' docs/specs/fleetd/gates/pr-description.md && cargo fmt --all --check`
**Own:** `docs/specs/fleetd/gates/pr-description.md`

## Goal

The operator wants 1 pull request to `main` that holds each change of fleetd, with a description that explains in detail what changed. The run branch `fleet/fleetd-finish` has more than 1200 commits on top of `main` (`git rev-list --count origin/main..fleet/fleetd-finish`). Write the description now as a file in the repository, from evidence. A later task updates the numbers at the end and puts the text on the pull request. This task writes only the file. It opens no pull request and edits no pull request.

## Reader

The operator: an engineer who designed fleetd (`docs/specs/fleetd/design.md`) and did not watch the build. They read the description to decide if the pull request can merge. Each claim must be true and checkable. Plain words. Short sentences. No praise words. Use they/them for a person.

## Sources (read in this order)

1. `docs/specs/fleetd/design.md`: the summary, the phases of the migration plan, the table "Where each rule becomes green".
2. `git log --first-parent --oneline origin/main..fleet/fleetd-finish` and, for the build run before this one, `git log --oneline origin/main..fleet/fleetd-build`. Group by phase and by area; do not list each commit.
3. `git diff --stat origin/main...fleet/fleetd-finish` (the size: files, lines, new crates) and `git diff --name-status origin/main...fleet/fleetd-finish -- crates | cut -f1 | sort | uniq -c`.
4. The gate and report files in `docs/specs/fleetd/gates/` (each `fleetd-p*.md`, `pre-gate-p4.md`, `pre-gate-p5.md`, `pre-gate-p6.md`, `ci-run-3.md`, `ci-run-4.md` if it exists, `merge-main-plan.md`, `merge-main-proof.md`, `macos-check-sandbox-lan.md`) and the review `docs/specs/fleetd/reviews/end-review-a.md`.
5. The decisions log `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/decisions.md` (long: search it for "OPERATOR", "PRODUCT", "SECURITY", "DONE") and `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/goal-prompt.md` (the done criteria).
6. `crates/horch-e2e/expected-failures/*.txt` on the run branch (what is still listed and who owns it), and `git grep -n '#\[ignore' fleet/fleetd-finish -- crates`.

## The file `docs/specs/fleetd/gates/pr-description.md`

Sections, in this order. Target length: 250 to 450 lines of Markdown.

- `# fleetd: a supervised daemon that runs the fleet` and 1 paragraph: what the pull request does and what it replaces.
- `## Status`: draft; not to merge before the operator decisions; the branch tip that the text describes (`git rev-parse fleet/fleetd-finish`); what is not done yet (read `horch ledger status --run fleetd-finish` for the open tasks, and name each with 1 line).
- `## What changed`: the body. 1 subsection for each area, each with: what the old behavior was, what the new behavior is, the main modules (paths), and the design rules that it implements (ids). Areas at least: the daemon and its pass loop (heartbeat, budgets, wake); push dispatch, leases and attempts; `result.json`, the verifier and the check and gate jobs; worktree custody (states, rescue, removal, disk budget); landing, bundles and the feature branch; ideas, sub-orchestrators, questions and answers (with the resume of a session); the operator boundary (the operator token, the command channel, the request catalog, FR-10, SR-8, the worker wrapper); sandboxes and the platform crate `horch-platform` (macOS Seatbelt, Linux bwrap with the PID namespace, Windows state); the ledger schema (the versions and what each adds; after the merge of main the fleetd steps are 9 to 17: see `merge-main-proof.md`); the CLI (new commands, retired commands and what replaces each); the test harness and the e2e crate (the fake herdr, the fake CLIs, expected-failures, `scripts/dev-sandbox.sh`); CI changes.
- `## Behavior changes that a user sees`: a table: before, after, why. Include each retired command and each new requirement of a host (for example bubblewrap, socat and util-linux `setsid` on Linux).
- `## Faults found and fixed in the finish run`: the product faults (not the test faults) that the run `fleetd-finish` found, 1 line each with the task number: search the decisions log for "PRODUCT" and "SECURITY". Examples: the launch window of SR-8 (T-172), the lost work at a launch timeout (T-195), the runaway job host chain of a unit test (T-287), the heartbeat gap of a 10000-row scan (T-292), the Linux job sandbox leak (T-309), the false Pass of the operator-boundary probe (T-320), resume with the answer (T-300).
- `## How it was proved`: the gates of each phase with their result; the 3 pre-gates; the end review (0 Critical, 3 Important: the state of each); the CI runs with their run ids and counts; the proof of the trial merge. Write "verified" with the evidence file for each line, or "not verified".
- `## Not proved, and known gaps`: each thing that no test or no OS run proves (Windows, real CLIs, Antigravity), each expected-failure entry and ignore mark that remains with its owner, each "not verified" row of the proof files.
- `## Decisions for the operator`: 1 numbered list. Each item: the question, the options, the recommendation of the run, the file with the evidence. Take them from the decisions log ("OPERATOR"), the end review ("Operator decisions") and the merge plan (Q1 to Q3). Known items: comment-sweep under fleetd; the release and the CHANGELOG line; the macOS check sandbox and the LAN address (R8-F2); RR-12 attempt burn at a failed resume; the 3 `i308_fleet_steward` expected failures; Windows fleetd as a release target; FR-7 and SR-8 as literal design text; the `unlanded` key of `result.json`; `ui-prop-tester` and the platform rules; unhealthy review profiles; the design lines with old schema numbers.
- `## Release steps (not done by this pull request)`: only what the operator must do, from the design and the handoff notes; say that no CHANGELOG line is added and why.
- `## How to review`: the order to read (design first, then which directories), and the commands of the full check: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked`, `cargo test --workspace --locked` through `scripts/dev-sandbox.sh`.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. Run no test suite: this task reads and writes text. `cargo fmt --all --check` is the only cargo command.
2. A number in the text comes from a command that you ran or a file that you name. Do not round up. Do not write that something passes when the source says "not verified".
3. No name of a person, no email address, no attribution line, no praise. Do not copy secrets, tokens or the content of `~/.horch`. Never read, set or pass `ANTHROPIC_API_KEY`.
4. Do not open, edit, merge or mark ready a pull request. Do not push a branch other than through `horch ledger finish`. Do not edit `CHANGELOG.md` or `docs/specs/fleetd/design.md`.
5. Write 1 `horch ledger progress` line each 10 minutes.

## Done when

The Check passes; each section has content from a named source; each open decision of the sources is in the list.
