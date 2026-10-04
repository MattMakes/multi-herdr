# U14 b2-vcs: GitClient, WorktreeManager, CommandValidator

Branch `ard/b2-vcs`. Phases B2 (vcs/git) and B3 (worktree, validator).
Requirements CMP-04, CMP-08, CMP-09, SEC-07 (validator part). Nothing calls
this code yet; the B3 coordinator is the first caller.

## What exists

- `crates/horch-core/src/vcs/{mod,git,worktree}.rs`
- `crates/horch-core/src/evaluation/{mod,validator}.rs`
- `crates/horch-core/tests/vcs.rs` (9 tests)
- `horch-core` depends on `horch-marketplace` (`horch-marketplace.workspace = true`).
  The allowlists in `scripts/check-deps.sh` and `nfr_05` name it.

## API

The API follows `ai_docs/designs/2026-10-02-dataset-competition-design.md`
§4.5 and §4.6. Additions are marked "(extra)".

```rust
// vcs::git
let git = GitCli::new(git_bin)                // caller resolves HORCH_GIT_BIN
    .with_env("GIT_CONFIG_GLOBAL", cfg);      // (extra) tests pin config
git.diff_digest(dir, base, head)?             // (extra) streamed sha256 of the full patch
// plus every GitClient method of design §4.5

// vcs::worktree
let mgr = WorktreeManager { git: &git };
spec.branch()  // "mh/exp/<exp8>/r<idx>/<label>"   (extra helper)
spec.path()    // <root>/<label>                    (extra helper)
mgr.create(&spec)? -> PathBuf
mgr.freeze(&spec, &execution_id, at)? -> FrozenCandidate
mgr.remove(&spec)?

// evaluation::validator
let v = CommandValidator::new(gates, artifacts, faults)   // faults: BTreeSet<String>
    .with_env("K", "V");                                  // (extra) test-only
v.validate(&frozen)? -> ValidationReport
```

## Rules

- Identity and date. `freeze` commits as
  `multi-herdr-dataset <dataset@multi-herdr.invalid>` (author and
  committer). The date is `at` as RFC 3339 with `Z`
  (`SecondsFormat::Secs`). Git stores and prints it as `+00:00`.
  `frozen_at` keeps the `Z` form. The message is `candidate <label> frozen`.
- `commit_all` runs `add -A`, then `diff --cached --quiet`. Exit 0 means
  nothing to commit and returns `None`. The commit uses `--no-verify`,
  `--allow-empty-message`, `--cleanup=strip` and `commit.gpgsign=false`.
  Hooks are off through `GitRunner` (`core.hooksPath=/dev/null`).
- `cherry_pick` sets only the committer identity. Git keeps the original
  author.
- The patch for `diff_patch` and `diff_digest` uses fixed options:
  `--no-color --no-ext-diff --no-textconv --no-renames --full-index
  --src-prefix=a/ --dst-prefix=b/`. Git writes it with `--output=` to a
  scratch file under `git rev-parse --git-path`. Horch reads it in 64 KiB
  pieces. The scratch file is removed on drop.
- `diff_numstat` uses `-z --no-renames`. A binary file has `None` counts.
- Every rev, ref, range and branch argument that is empty or starts with `-`
  is refused before git runs.
- `update_ref_cas` returns `false` when the ref does not hold
  `expected_old` (an all-zero old value means "must not exist"). Any other
  failure is an error.
- Labels and `exp8` must be `[A-Za-z0-9_-]` and must not start with `-`.
  `base_sha` must be hex.

## Log capping

- Each gate writes stdout and stderr to a raw file
  `<artifacts>/<label>/.gate-<n>.raw` (0600). The raw file is removed after
  it is stored.
- A raw log of at most 256 KiB is redacted whole. If redaction makes it
  longer than 256 KiB, horch cuts the end and appends a marker line.
- A larger raw log keeps its head and tail (each about 128 KiB). Each piece
  is cut at a line boundary before redaction, so no secret is split. A
  marker line `[horch: <n> bytes omitted from the middle of this log]`
  separates them. Only those two pieces are read.
- The stored file is `<artifacts>/<label>/gate-<n>-<name>.log`, 0600,
  written with `fsx::write_atomic`. `n` starts at 1. Characters in the gate
  name other than `[A-Za-z0-9_-]` become `_`. `log_digest` is sha256 of the
  stored bytes.

## Validator behaviour

- Each gate runs `sh -c <command>` with cwd = the worktree and stdin null.
  It runs in its own process group (`CommandExt::process_group(0)`).
  On timeout, horch sends `killpg(SIGKILL)`. After a normal exit, horch
  also kills the group, so background stragglers stop.
- Env: the process env, plus `with_env` entries, plus
  `CARGO_TARGET_DIR=<worktree>/target`, minus `launch::FORBIDDEN_ENV`.
- Status: exit 0 is `Passed`. A non-zero exit is `Failed { code }`. A
  signal is `Failed { code: 128 + signal }`. A failed `sh` spawn is
  `Error { reason }`. The fault `fail-gate:<name>` gives
  `Error { reason: "fault" }` and the gate does not run. Its log has one
  line.
- `mechanical_score` = passed / total, 0.0 with no gates. `eligible` =
  every gate passed. With 0 gates, `eligible` is `true` (empty `all`).
  The B3 coordinator must decide if that is correct.
- `GateStatus` serializes with `"kind"` as the tag, snake_case.
- `validation_id` = `ids::mint_v7(clock::now())`.

## Deviations

- `GitClient::diff_digest` is an extra trait method. `freeze` needs the
  digest of the uncapped patch, and the patch can be large.
- `CommandValidator.faults` is `BTreeSet<String>` until A2 adds
  `runtime::fault::Faults`.
- `CommandValidator.env` and `with_env` are extra. The SEC-08 style test
  uses them to put `ANTHROPIC_API_KEY=SENTINEL` into the gate env.
- `GateSpec` has no serde derives. The config layer maps its own type to
  `GateSpec`.

## Security review (focused, this unit)

Scope: `vcs/git.rs`, `vcs/worktree.rs`, `evaluation/validator.rs`. Trust
boundary: the candidate worktree is untrusted (an agent wrote it). The
config and the artifacts dir are trusted.

- FIXED. A candidate can rewrite the `.git` file in its worktree to point
  at another git dir. `freeze` then committed into that git dir and read
  the head and diff from it. Now `freeze` checks that
  `refs/heads/<branch>` in the main repo equals the head that the worktree
  reports. It reads `is_ancestor`, numstat and the digest from the main
  repo. Test: `freeze_refuses_a_worktree_redirected_to_another_repo`.
- ACCEPTED. Candidates run as the same user, so a candidate can write
  `.git/config` of the main repo (for example `core.fsmonitor`). Git then
  runs that command when horch runs git. Gates run candidate code by
  design. `GitRunner` removes `ANTHROPIC_API_KEY` from these children.
  A sandbox for candidates is out of scope.
- LIMITATION. A gate that calls `setsid` leaves its process group, and the
  timeout kill does not reach it.
- LIMITATION. Gates get the full process env minus `FORBIDDEN_ENV`. Other
  secrets (for example `GITHUB_TOKEN`) reach gates. Logs are redacted.
- LIMITATION. A line longer than about 128 KiB without a newline can be
  cut inside a secret at the head/tail cut. A short fragment can then
  escape redaction.

## Gotchas for B3 and B5

- Callers of `diff_patch` should pass the main repo as `dir`, not the
  candidate worktree (see the security review).
- `freeze` runs `git add -A`. A validator run sets
  `CARGO_TARGET_DIR=<worktree>/target`. If the repo does not ignore
  `target/`, a second freeze after validation commits it.
- `create` compares worktree paths after `canonicalize`. Git prints
  resolved paths (`/private/var/...` on macOS).
- `remove` keeps the branch. A later `create` checks the branch out again
  if `base_sha` is still an ancestor of its tip.
- `GitCli::version` runs in `.` (the process cwd).
