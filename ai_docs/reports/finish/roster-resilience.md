# F7 roster-resilience report

Plan: `ai_docs/plans/finish/f7-roster-resilience.md`.

## Outcome

One overlay file that does not read or parse no longer stops the roster from
loading. `horch spawn`, `horch teammates`, `horch fleet`, `horch route`,
`horch agent-list` and the orchestrator briefing load every other teammate.
Each of them prints 1 `warning:` line per broken file to stderr. The line
names the file, the parse error and what is used instead.
`horch teammates --check` and `horch doctor` still report the file as a
problem, so the gate stays strict.

## Behaviour per layer

- Built-ins: `Roster::builtin` still fails on a parse error. The test
  `f7_builtin_roster_always_parses` pins that every built-in parses.
- Overlay teammate with a new name: it is not in the roster.
  `Roster::require(name)` returns the parse error, not "unknown teammate".
- Overlay teammate that overrides a built-in: the built-in stays. The warning
  says "using the built-in '<name>' instead".
- Overlay teammate that overrides an earlier overlay: the earlier file stays.
  The warning says "using the earlier <path> instead".
- A higher layer that parses clears the warning for that name.
- `_base/` files get the same treatment as teammate files.
- An unreadable directory still fails the load, as before.

Example warning:

    warning: teammate 'newcomer' did not load from /x/teammates/newcomer.md:
    parsing frontmatter: requires[0]: unknown variant `no-such-tool`, ...;
    teammate 'newcomer' is not available

## Files

- `crates/horch-core/src/roster/repository.rs`: `BrokenFile`,
  `Roster::load_warnings`, tolerant `Roster::overlay`, `require` error.
- `crates/horch-core/src/roster/validation.rs`: `Roster::check` starts with
  `load_warnings`.
- `crates/horch-core/src/roster/mod.rs`: module doc.
- `crates/horch/src/cmd/mod.rs`: `load_roster` prints the warnings;
  `load_roster_unwarned` does not.
- `crates/horch/src/cmd/teammatescmd.rs`, `crates/horch/src/cmd/doctor.rs`:
  use `load_roster_unwarned`, so a file is reported once, as a problem.
- Tests: 5 in `crates/horch-core/src/roster/tests.rs` (`f7_*`), 2 in
  `crates/horch/tests/roster_resilience.rs`.

## Not changed

- Paths that call `Roster::load_layered` directly print no warning:
  `horch-core/src/execution/lifecycle.rs` (worker launch),
  `horch/src/dataset/run.rs`, `horch/src/dataset/judge_job.rs`.
  They now load instead of failing. The spawn step before a worker launch
  already printed the warning.

## Checks

- `cargo test -p horch-core --lib roster::`: 35 of 35 pass.
- `cargo test -p horch --test roster_resilience`: 2 of 2 pass.
- `cargo test -p horch-core --test nfr --test skills_catalog --test judge_input --test coordinator`: pass.
- `cargo test -p horch --bin horch -- teammatescmd doctor`: 15 of 15 pass.
- `cargo clippy -p horch-core -p horch --all-targets -- -D warnings`: clean.
- `rustfmt --check` on the changed files: clean.
