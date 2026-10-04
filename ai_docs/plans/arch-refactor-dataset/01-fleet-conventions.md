# Fleet conventions for the arch-refactor-dataset run

Every unit plan in `ai_docs/plans/arch-refactor-dataset/` points here. Read
this file completely before you start. These rules apply to every unit.

## 1. Sources of truth

- The master plan is `ai_docs/plans/arch-refactor-dataset/00-master-plan.md`.
  Read the sections that your unit plan names. Do not edit the master plan.
- Spec A and Spec B (the operator's original specs) are NOT in the repository
  yet. The orchestrator asked the operator for them. The master plan is the
  source of truth until they arrive.
- If a step needs exact spec text that the master plan does not give, do this:
  1. Implement the most direct reading of the master plan.
  2. Put a `SPEC-TODO(<Spec A|B> §<n>): <what is unknown>` comment at that place.
  3. Send `horch tell orchestrator "[<role>] NOTE: SPEC-TODO <section>: <what>"`.
  4. Continue. Do not wait.

## 2. Git: worktrees, branches, never the shared tree

- The orchestrator owns the integration worktree
  `/Users/mascott/projects/mh-wt/integration` on branch `arch-refactor-dataset`.
  Do not edit, commit, checkout or build in that directory.
- Do not run `git checkout` or `git switch` in `/Users/mascott/projects/multi-herdr`.
  Other sessions use that tree.
- Create your own worktree from the current integration branch:

  ```
  git -C /Users/mascott/projects/multi-herdr worktree add -b ard/<unit> /Users/mascott/projects/mh-wt/<unit> arch-refactor-dataset
  cd /Users/mascott/projects/mh-wt/<unit>
  ```

  `<unit>` is the slug that your unit plan gives.
- Do all work in your worktree. Use absolute paths in every command.
- Commit on `ard/<unit>` only. Do not push. Do not open a PR. Do not merge
  into `arch-refactor-dataset`. The orchestrator merges.
- Write commit subjects as `<phase>: <imperative summary>`, for example
  `A1: Add identity newtypes`. In the body, list what changed and quote the
  CHECKLIST lines you checked (see §4), when
  `ai_docs/gates/architecture-refactor/CHECKLIST.md` exists in your base.

## 3. Parallel work: shims and shared files

Several units run at the same time on different branches. These rules keep
the branches mergeable.

- When you move a module, leave a re-export shim at the old path, for example
  `pub use crate::harness::HarnessKind as Agent;` or
  `pub use crate::roster::*;`. Callers that other units edit must still
  compile. Only phase A12 deletes shims.
- Edit only the files that your unit plan lists under `own`. If you must
  change another file, ask the orchestrator first with `QUESTION:`.
- `crates/horch-core/src/lib.rs` gets new `pub mod` lines from many units. Add
  your lines in alphabetical order. Do not reorder or reformat other lines.
  A merge conflict in `lib.rs` is expected; resolve it by keeping both sides.
- Do not edit `ai_docs/gates/architecture-refactor/CURRENT_PHASE`. The
  orchestrator adds a phase to it when the last unit of that phase merges.

## 4. The gate

Every commit must pass the gate. Run it from your worktree root.

- If `just gate` exists in your base, run `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate`.
- If it does not exist yet, run these commands. Each one must exit 0:

  ```
  cargo fmt --all --check
  cargo build --workspace --all-targets
  cargo build --workspace --bins
  cargo test --workspace
  HORCH_TEAMMATES_DIR=teammates cargo run --quiet --bin horch -- teammates --check
  ./scripts/check-deps.sh
  ./scripts/verify-telemetry-e2e.sh
  ```

- Also run `./scripts/check-req-coverage.sh --phase <your phase>` when the
  script supports `--phase`. Every requirement ID of your phase that your unit
  covers must have a test whose name starts with the lowercase ID, for example
  `ARC-02` needs a test named `arc_02_...`.
- Name every new test with its requirement ID prefix exactly as the master
  plan's traceability tables (§4) spell it.
- Do not change existing tests to make them pass, unless your unit plan says so.
- Never regenerate a golden prompt file under `crates/horch-core/tests/golden/`.
  Never re-bless a serialization golden. A format change bumps the schema
  version.

## 5. Hard rules

- NEVER read, print, set, export or pass `ANTHROPIC_API_KEY`. Every child
  process that horch starts must have it removed (`FORBIDDEN_ENV`). If you run
  `claude` yourself, use `env -u ANTHROPIC_API_KEY claude ...`.
- No new crates. The only addition is `sha2` (already in `Cargo.lock`), for
  `horch-core` and `horch-marketplace`. No `thiserror`, no `proptest`, no
  `tokio` or other async runtime, no HTTP crate. Write error enums by hand
  with `Display` and `std::error::Error`.
- Domain code does not call `std::env`. Pass values in as parameters. Only
  `runtime/` and the `horch` binary's bootstrap read the process environment
  (phase A2 builds this; until then, do not add new `std::env` reads in core).
- Tests are hermetic: no network, no real harness binary, no herdr server, no
  file outside a temp dir. Real `git` is allowed only on temp repos.
- Keep the code style of the surrounding files: comment density, naming,
  error handling with `anyhow` in application code.

## 6. Messages and the merge protocol

Write every message in Simplified Technical English: short sentences, active
voice, one fact per sentence, exact paths and commands.

- Progress: `horch note "<one line>"` after each major step.
- Questions: `horch tell orchestrator "[<role>] QUESTION: <question>"`. Then
  wait for the answer.
- Blockers: `horch tell orchestrator "[<role>] BLOCKED: <cause>"`. Then wait.
- When your unit is complete:
  1. Run `git -C <your worktree> rebase arch-refactor-dataset`. Resolve
     conflicts. Keep both sides of `lib.rs` module lists.
  2. Run the full gate (§4) again. It must be green.
  3. Send exactly one line:
     `horch tell orchestrator "[<role>] NOTE: READY-TO-MERGE ard/<unit> <short-sha>. Gate green. Tests added: <count>. See <report path>."`
  4. Wait for the reply. Do not run `horch done` yet.
  5. If the reply is `REBASE`, repeat steps 1 to 4.
  6. If the reply is `MERGED`, run `horch done "<summary>"`. The summary has:
     files changed, tests added, decisions, gotchas, and every SPEC-TODO.
- Write a short report to `ai_docs/reports/arch-refactor-dataset/<unit>.md`
  and commit it on your branch. Include decisions, gotchas, and anything that
  a later phase must know.
