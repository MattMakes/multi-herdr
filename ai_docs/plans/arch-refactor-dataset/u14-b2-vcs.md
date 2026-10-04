# U14 b2-vcs: GitClient, WorktreeManager, CommandValidator

Unit slug: `b2-vcs`. Branch: `ard/b2-vcs`. Phases: B2 (vcs/git), B3 (worktree,
validator). Requirements: CMP-04, CMP-08, CMP-09, SEC-07 (validator part).

## GOAL

`horch-core` can drive git through a typed `GitClient` (real implementation
`GitCli` over `horch_marketplace::git::GitRunner`), create, freeze and remove
candidate worktrees deterministically, and validate a frozen candidate by
running operator-configured gate commands with timeouts, redacted capped logs
and a stripped environment. All tested on temp repos. Nothing calls it yet.

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Read in `00-master-plan.md`: §2 "vcs" and "evaluation", §3 "B2" (vcs line),
  §3 "B3" items 1 and 5 (freeze and CommandValidator), §4 CMP-04, CMP-08,
  CMP-09, SEC-07, NFR-07 rows.
- The design doc is your main spec:
  `ai_docs/designs/2026-10-02-dataset-competition-design.md` §4.5 (VCS) and
  §4.6 (Validation). Where it differs from this plan, the design wins,
  except where this plan says "deviation".
- Merged building blocks (read their reports in
  `ai_docs/reports/arch-refactor-dataset/`):
  - `horch_marketplace::git::GitRunner` (`new(bin)`, `with_env`, `run`,
    `output`; it removes `ANTHROPIC_API_KEY` and repo-locating `GIT_*` vars).
    See `a8-marketplace.md`.
  - `horch_core::fsx`, `measure::{digest, redact}`, `ids::ExecutionId`.
  - `crates/horch-e2e/src/harness.rs` `Harness::with_git()` pins
    `GIT_CONFIG_GLOBAL`, `GIT_CONFIG_NOSYSTEM`, author, committer and dates.
    Use the same pins in your temp-repo test helper.
- Deviation (faults): `runtime::fault::Faults` comes from A2 (in flight).
  `CommandValidator` takes a plain `faults: BTreeSet<String>` for now.
- Deviation (git bin): the caller passes the git binary path. Nothing in your
  files reads `HORCH_GIT_BIN`; A2's `RuntimeContext` provides it later.
- `horch-core` gains a dependency on `horch-marketplace` (allowed by the
  master plan: `horch → horch-core → horch-marketplace`). Update the
  `allowed_core` list in `scripts/check-deps.sh` and in `nfr_05` in
  `crates/horch-core/tests/nfr.rs` with `horch-marketplace`. U16
  `a9a-skills` adds the same dependency in parallel. Use exactly:
  `horch-marketplace.workspace = true` in `[dependencies]` (alphabetical
  position), `horch-marketplace` appended to `allowed_core` in
  `scripts/check-deps.sh`, and `"horch-marketplace"` appended to the core
  list in `nfr_05`. If U16 merged first, keep its version.
- Parallel units: U13 `b1-measure` (owns `measure/*` new files and
  `competition/model.rs`), U15 `b4-evaluation` (owns `evaluation/{judgment,parser,winner,rubric}.rs`),
  A2/A3/A5 refactors. You create `evaluation/mod.rs`; U15 also adds lines to
  it. Expect a trivial merge conflict there; keep both sides.

## FILES

own:
- `crates/horch-core/src/vcs/{mod,git,worktree}.rs` (new)
- `crates/horch-core/src/evaluation/mod.rs` (new or add your line)
- `crates/horch-core/src/evaluation/validator.rs` (new)
- `crates/horch-core/src/lib.rs` (add `pub mod evaluation; pub mod vcs;`)
- `crates/horch-core/Cargo.toml` (add `horch-marketplace.workspace = true`)
- `Cargo.lock`
- `scripts/check-deps.sh`, `crates/horch-core/tests/nfr.rs` (the allowlist entry only)
- `crates/horch-core/tests/vcs.rs` (new)
- `ai_docs/reports/arch-refactor-dataset/b2-vcs.md`

do not touch: every other file.

## STEPS

1. Create the worktree (conventions §2).
2. `vcs/git.rs`: `NumstatLine { added: Option<u32>, deleted: Option<u32>, path: String }`
   (binary files have `None`), `CherryPick`, `CheckoutLocation`,
   `GitIdentity`, the `GitClient` trait and `GitCli` exactly as design §4.5.
   `GitCli::new(git_bin: PathBuf)` builds a `GitRunner` with
   `GIT_TERMINAL_PROMPT=0` and `LC_ALL=C`. `commit_all` runs
   `git -c core.hooksPath=/dev/null -c commit.gpgsign=false add -A` then
   `commit --no-verify --allow-empty-message` with author and committer name,
   email and date from `GitIdentity` set through the env of that one command;
   returns `None` when there is nothing to commit. `update_ref_cas` uses
   `git update-ref <ref> <new> <old>` and returns `false` (not an error) when
   the old value does not match. `cherry_pick` returns `Conflict { paths }`
   and leaves the cherry-pick aborted (`git cherry-pick --abort`).
   `diff_patch` caps at `cap_bytes` on a UTF-8 boundary and reports truncation.
3. `vcs/worktree.rs`: `WorktreeSpec`, `FrozenCandidate`, `WorktreeManager`
   exactly as design §4.5.
   - Branch `mh/exp/<exp8>/r<idx>/<label>`; path `<root>/<label>`.
   - `create` is idempotent: if the worktree exists on the right branch at
     or after `base_sha`, return its path; if the path exists with another
     branch, error.
   - `freeze`: commit everything with identity
     `multi-herdr-dataset <dataset@multi-herdr.invalid>` and date `at`
     (RFC 3339), message `candidate <label> frozen`; record `head_sha`,
     `diff_numstat(base, head)`, `diff_digest` = sha256 of the full patch
     (uncapped bytes, computed by streaming), `frozen_at`. Freezing twice
     with no new changes returns the same `head_sha`.
   - `remove` runs `worktree remove --force` and keeps the branch.
4. `evaluation/validator.rs`: design §4.6.
   - `GateSpec` comes from the caller (config); there is no other way to add
     a gate (no method takes a command string from a candidate or a judge).
   - Each gate: `sh -c <command>` with cwd = the worktree, a per-gate
     timeout (poll `try_wait`, kill the process group on timeout; use
     `libc::setpgid`/`killpg` under `cfg(unix)`), env with `ANTHROPIC_API_KEY`
     removed and `CARGO_TARGET_DIR=<worktree>/target` set, stdout+stderr
     captured to `<artifacts>/<label>/gate-<n>-<name>.log` (0600), redacted
     with `measure::redact`, capped at 256 KiB (keep the head and the tail,
     with a marker line), `log_digest` over the stored bytes.
   - `mechanical_score` = passed / total (0.0 when no gates); `eligible` =
     every gate passed.
   - Fault names supported: `fail-gate:<name>` makes that gate report
     `Error { reason: "fault" }` without running.
5. Tests in `crates/horch-core/tests/vcs.rs`. A helper makes a temp repo
   with real git (pin `GIT_CONFIG_GLOBAL` to an empty temp file,
   `GIT_CONFIG_NOSYSTEM=1`, author/committer/dates); skip when git is absent
   unless `HORCH_REQUIRE_GIT=1`.
   - `cmp_04_n_worktrees_same_base_modify_same_file`: 3 worktrees from one
     base; each edits the same file differently and freezes; 3 distinct
     head SHAs, each with parent = base, and the main checkout is untouched.
   - `cmp_08_freeze_deterministic_sha_and_numstat`: the same edits frozen in
     2 separate temp repos at the same `at` give the same head SHA and numstat.
   - `cmp_09_gates_per_candidate_with_timeouts`: 3 gates (pass, fail with
     code 3, `sleep 5` with a 300 ms timeout); statuses Passed, Failed{3},
     TimedOut; the sleep process is gone after the call; logs exist, 0600.
   - `sec_07_gates_only_from_config`: a candidate whose worktree contains a
     file `gates.sh` and a judge-like JSON naming a command; the validator
     runs only the configured gates (assert by a marker file that only the
     configured gate creates).
   - `validator_strips_api_key_and_redacts`: a gate prints
     `$ANTHROPIC_API_KEY` and `sk-ant-...`; with `ANTHROPIC_API_KEY=SENTINEL`
     set in the test's command env (not the process env; pass it in through a
     test-only constructor arg), the log has no `SENTINEL` and the key
     pattern is redacted.
   - `git_cherry_pick_conflict_reports_paths`, `git_update_ref_cas_mismatch_is_false`.
6. Gate after each step. Commits: `B2: Add GitClient over the marketplace git runner`,
   `B3: Add WorktreeManager`, `B3: Add CommandValidator`, `B3: Add VCS and validator tests`.
7. Write and commit the report (API, identity and date rules, log capping,
   gotchas for B3/B5). Follow conventions §6.

## DONE WHEN

- The 7 named tests pass with `HORCH_REQUIRE_GIT=1`.
- `scripts/check-deps.sh` passes with the new dependency.
- `just gate` is green.

## REPORT

- `horch note` after each commit.
- `horch done` summary: API, deviations, gotchas.
