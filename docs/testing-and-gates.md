# Testing and the gate

Every commit must pass the gate. This page tells you what the gate runs, how
to read a failure, and the rules for oracles, goldens and fakes.

## Run the gate

```bash
HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate
```

`just gate` runs `scripts/phase-gate.sh` (11 steps). It stops at the first failure and
prints `GATE GREEN` at the end. On a full machine it takes about 4 minutes.

Warning: never run the gate or a test under `git rebase -x`, or from a git
hook. Rebase first, then run the gate as its own command. On 2026-10-03 a gate
run under `git rebase -x` inherited `GIT_DIR`, and test fixtures that ran
`git init` and `git commit` wrote into the real repository. The gate script now
unsets every variable that aims git at a repository (`GIT_DIR`,
`GIT_WORK_TREE`, `GIT_INDEX_FILE`, `GIT_COMMON_DIR`, `GIT_OBJECT_DIRECTORY`,
`GIT_CONFIG*`, and the others in the list), including the numbered
`GIT_CONFIG_KEY_<n>` and `GIT_CONFIG_VALUE_<n>`. Each git child that horch
starts has the same variables removed (`REPO_ENV` in
`crates/horch-marketplace/src/git.rs`, 18 names plus the numbered pairs). That is a second line
of defence. Do not depend on it.

| # | step | a failure means | open first |
|---|---|---|---|
| 1 | `no_spec_todo` (`git grep` for an unresolved spec marker) | a file holds an unresolved spec marker; the step prints each hit | the file and line in the output; close the marker |
| 2 | `cargo fmt --all --check` | the formatting differs | run `cargo fmt --all` |
| 3 | `cargo build --workspace --all-targets` | code or a test does not compile | the first `error[...]` in the output |
| 4 | `cargo build --workspace --bins` | a binary does not compile (the e2e tests need all of them) | the same |
| 5 | `cargo clippy --workspace --all-targets -- -D warnings` | a clippy warning; every warning is an error | the first `warning:` in the output |
| 6 | `cargo test --workspace --no-fail-fast` | a test failed; every test runs, so read the summary at the end | the failing test file |
| 7 | `env HORCH_TEAMMATES_DIR=teammates cargo run --quiet --bin horch -- teammates --check` | a file in `teammates/` breaks a roster rule | the named teammate file; rules in `crates/horch-core/src/roster/validation.rs` |
| 8 | `scripts/check-req-coverage.sh` | a requirement ID has no test | the ID's requirement table in `docs/specs/` |
| 9 | `scripts/check-deps.sh` | a crate has a dependency that is not allowed | the `Cargo.toml` you changed |
| 10 | `scripts/verify-telemetry-e2e.sh` | the hermetic telemetry story changed | `crates/horch-e2e/tests/scenario.rs` |
| 11 | `godot_skills` (the Python checks in `scripts/godot/`) | a Godot skill names an unknown engine API, a code block does not parse, or a cross-reference does not resolve; the Godot and dotnet checks skip when those tools are absent | the first failing script in the output, then the named file in `skills/godot-*/` |

`HORCH_REQUIRE_GIT=1` and `HORCH_REQUIRE_SQLITE=1` turn a skip into a
failure. Without them, a test that cannot find `git` (for example in
`crates/horch-core/tests/vcs.rs`, `coordinator.rs`, `promotion.rs`,
`crates/horch-marketplace/tests/marketplace.rs`, `crates/horch/tests/skills_cli.rs`,
`crates/horch-e2e/src/harness.rs`) or a working `sqlite3`
(`crates/horch-e2e/tests/fakes.rs`) returns early and passes. Always set
both on a development Mac.

## Run the gate when many gates run

Ten full gates at once (load 40 on 18 cores) made each gate slow and flaky.
While you work, run only the tests you touch:

```bash
cargo test -p <crate> --test <file>
cargo clippy -p <crate>
rustfmt --check <files>
```

In single-branch mode (see [fleet-workflow.md](fleet-workflow.md)), a worker
runs only the tests it touches and takes no gate slot. The orchestrator runs 1
full gate on the tip, in `.worktrees/_gate`.

The rest of this section applies to parallel worktree runs. Run the full gate
once, just before you report ready. The slot wrapper `.worktrees/gate-slot.sh`
is an optional, operator-local script. It is git-ignored and absent from a
fresh clone. When it exists, it lets 3 gates run at a time (set
`GATE_SLOTS` to change the number). A slot is a directory in
`/tmp/horch-gate-slots` that holds the pid of its holder. A slot whose holder
is dead is reclaimed.

```bash
HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 .worktrees/gate-slot.sh just gate   # only if the script exists
```

`just verify` is an older, shorter check from the telemetry design (build,
test, coverage, telemetry e2e, deps, `rustfmt` on changed files, `teammates
--check`). Use `just gate` for a commit.

## Requirement coverage

`scripts/check-req-coverage.sh` reads every spec in `docs/specs/*.md`. A
row `| TEL-05 | ...` in a table whose second header cell is `Requirement` or
`Check` defines the ID `TEL-05`. Each defined ID needs at least 1 test whose
name starts with the lowercase ID: `tel_05_...`.

With no arguments the script checks every ID in every spec. The script fails
when `docs/specs/` has no spec or the specs define no ID. To check one ID, run
`scripts/check-req-coverage.sh TEL-05`.

When you add a requirement table to a spec, add the tests in the same
commit, or the gate fails.

## Dependencies

`scripts/check-deps.sh` holds the allowed direct dependencies of
`horch-core`, `horch` and `horch-marketplace`. Do not add a crate. No HTTP
crate and no async runtime. If you think you need one, ask first.

## Oracles, goldens and fixtures

Oracles freeze the behavior that existed before the architecture refactor
(phase A0). Goldens freeze a rendered output. A diff in either means
user-visible behavior changed.

| location | what | test |
|---|---|---|
| `crates/horch-core/tests/oracles/launch/` | 1 launch argv/env/settings file per A0 teammate | `crates/horch-core/tests/baseline_oracles.rs` |
| `crates/horch-core/tests/oracles/skills/` | skill briefings per teammate and phase | `baseline_oracles.rs`, `skills_catalog.rs` |
| `crates/horch-core/tests/oracles/routing/` | routing decisions per quota fixture | `baseline_oracles.rs` |
| `crates/horch-core/tests/oracles/ledgers/` | ledger reads | `baseline_oracles.rs` |
| `crates/horch/tests/oracles/` | `horch skills` and `horch sessions` output | `crates/horch/tests/baseline_cli.rs` |
| `crates/horch-core/tests/golden/` | rendered briefings and execpolicy files; `export-1.0.0.jsonl`; `worker-run-1.0.0.json` | `golden_prompts.rs`, `dataset_export.rs`, `measure.rs` |
| `crates/horch/tests/golden/` | telemetry screen frames | unit tests in `crates/horch/src/cmd/telemetry.rs` |
| `crates/horch-e2e/tests/golden/` | the telemetry e2e frame | `crates/horch-e2e/tests/scenario.rs` |
| `crates/horch-core/tests/fixtures/harness/` | launch snapshots from before the harness move | `crates/horch-core/tests/harness.rs` |

`arc_01_baseline_oracles_present` in `baseline_oracles.rs` counts the oracle
files. It expects 33 launch oracles (`TEAMMATES_AT_A0`).

## HORCH_BLESS

`HORCH_BLESS=1` makes a comparison test write its expected file instead of
comparing. The 7 files that read it behave in 2 ways:

| file | behavior under `HORCH_BLESS=1` |
|---|---|
| `crates/horch-core/tests/baseline_oracles.rs` | overwrites the oracle |
| `crates/horch/tests/baseline_cli.rs` | overwrites the oracle |
| `crates/horch/src/cmd/telemetry.rs` (unit tests) | overwrites the golden |
| `crates/horch-e2e/tests/scenario.rs` | overwrites the golden |
| `crates/horch-core/tests/dataset_export.rs` | writes the golden only when it is absent |
| `crates/horch-core/tests/measure.rs` | writes the golden only when it is absent |
| `crates/horch-core/tests/harness.rs` | writes the fixture only when it is absent |

The rules:

- Do not bless to make a test pass. Bless only when your plan says that this
  output changes.
- Run 1 test name at a time: `HORCH_BLESS=1 cargo test -p horch-core --test baseline_oracles oracle_skills_match`.
  `cargo test` takes only 1 name filter.
- After you bless, run `git status` and `git diff --stat`. Only the files your
  plan names may change. Put the diff summary in your report.
- A new teammate gets no oracle file. Add its name to `SKIP_NEW_TEAMMATES` in
  `crates/horch-core/tests/baseline_oracles.rs` and in
  `crates/horch-core/tests/skills_catalog.rs` (alphabetical, 1 name per
  line). A new launch oracle file makes `arc_01_baseline_oracles_present`
  fail, because it counts exactly 33.
- An absent-only golden (`export-1.0.0.jsonl`, `worker-run-1.0.0.json`) is a
  schema. A change to it is a schema version change, not a bless.
- The briefing goldens (`worker-*.txt`, `fleet-orchestrator.txt`,
  `execpolicy-*.txt` in `crates/horch-core/tests/golden/`) have no bless.
  They hold the OLD text. When you change a line in `teammates/_base/`, add
  a named block with the reason to the matching test in
  `crates/horch-core/tests/golden_prompts.rs` (for example
  `every_worker_briefing_differs_only_where_sanctioned`). Do not overwrite
  the golden.

## Hermetic tests and fakes

Tests use no network, no real agent CLI and no herdr server. The fakes are
binaries in `crates/horch-e2e/src/bin/`:

| fake | stands in for |
|---|---|
| `fake-claude.rs` | `claude` |
| `fake-codex.rs` | `codex` |
| `fake-opencode.rs` | `opencode` |
| `fake-pi.rs` | `pi` |
| `fake-prime.rs` | `prime-agent` |
| `fake-antigravity.rs` | `agy` |
| `fake-herdr.rs` | `herdr` (panes, workspaces, state in a file) |
| `fake-ollama.rs` | `ollama` |

`FAKES` in `crates/horch-e2e/src/harness.rs` maps each command name to its
fake. `Harness::new` links them into a temp `bin/` first on `PATH`. On unix
the link is a symlink to the built file in `target/`. A hard link made macOS
`syspolicyd` kill a fake with SIGKILL under load ("Malware rejection" in the
unified log), which showed as a flaky e2e test.
`HORCH_NOW` pins the clock (`crates/horch-core/src/clock.rs`). `HORCH_FAULT`
stops a process at a named point (`crates/horch-core/src/runtime/fault.rs`);
the e2e tests use it to test crash and resume.

## e2e gotchas

- Run `cargo build --workspace --bins` before `cargo test -p horch-e2e`. The
  e2e tests run the built `horch`; they do not rebuild it, so they can test a
  stale binary.
- Never write into a harness `bin/` entry. It can be a link to
  `target/debug/fake-*`, and a write changes the built fake. Use
  `Harness::write_bin`. `check_built_fakes` fails the test when a built fake
  changed. After a mistake, delete the damaged `target/debug/fake-*` and
  rebuild.
- `HORCH_E2E_KEEP=1` keeps a test's temp dir. Judge bundle dirs are mode 0500:
  run `chmod -R u+w <dir>` before you delete them.
- macOS kills a copied system binary (exit 137). Symlink it instead.
- `sqlite3` reads an argument that starts with `-` as an option. Feed SQL on
  stdin.
- Use your own `CARGO_TARGET_DIR` per worktree (the default `target/` inside
  the worktree is fine). Never share a target dir between worktrees.

## Live checks (not in the gate)

| command | needs | checks |
|---|---|---|
| `horch smoke messaging` | a herdr server | the messaging primitives, 2 panes, no agents |
| `horch smoke fleet` | a herdr server | spawn, ledger, report, pane self-close with a fake agent |
| `horch smoke tile` | a herdr server | the tiler across 2 tabs |
| `just verify-perf` | time | the collector budgets over a 1 GB corpus (`nfr_02`) |

Run the smoke checks after a herdr upgrade. They spend no tokens.

## Read next

- [architecture.md](architecture.md) for the arch-scan rules.
- The recipes in [README.md](README.md#recipes): each one lists its tests.
