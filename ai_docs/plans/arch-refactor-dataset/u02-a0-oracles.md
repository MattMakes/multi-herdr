# U02 a0-oracles: freeze behavior oracles from the pre-refactor code

Unit slug: `a0-oracles`. Branch: `ard/a0-oracles`. Phase: A0. Requirement: ARC-01.

## GOAL

Committed oracle files capture what the pre-refactor code does today, and
tests compare the current code against them. Later phases (A1 to A12) must
keep these tests green without changing any oracle file.

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Read in `00-master-plan.md`: "Hard constraints", §3 "A0 Baseline" (the
  "Freeze oracles" list), and the phases that consume the oracles:
  A4 (`arc_09_argv_matches_baseline`), A5 (`arc_12_decisions_match_baseline`),
  A6 (`arc_17_*` legacy ledgers), A9 (`skl_08_*` briefings), A11 (`mkt_09_*`
  legacy `horch skills` output).
- The oracle DATA is frozen forever. The comparison TEST CODE may change in
  later phases (call sites move), but its expected files never change.
- Other units run at the same time. U05 (A1) changes some signatures, for
  example `resume: bool` becomes `SessionMode`. Whoever merges second adapts
  the call sites in the test code. You write against the base code.
- Generate the oracles from the base code (575c2c2 plus integration commits).
  Do not change any file under `crates/*/src/` to generate them. If a value is
  not reachable through a `pub` API, send `QUESTION:` before you change source.

## FILES

own (all new):
- `crates/horch-core/tests/baseline_oracles.rs`
- `crates/horch-core/tests/oracles/**`
- `crates/horch/tests/baseline_cli.rs`
- `crates/horch/tests/oracles/**`
- `ai_docs/reports/arch-refactor-dataset/a0-oracles.md`

do not touch: every other file. In particular, do not touch `crates/*/src/`,
`scripts/`, `justfile`, `ai_docs/designs/`, `ai_docs/gates/`.

## STEPS

1. Create the worktree (conventions §2).
2. Write a small bless helper in each test file, in the style of the existing
   `HORCH_BLESS` helpers (`crates/horch-e2e/tests/scenario.rs:77`): with
   `HORCH_BLESS=1` it writes the expected file; without it, it compares and
   fails with a diff hint. Also make it fail when the expected file is absent.
3. Oracle 1, launch argv and env (consumer: A4 `arc_09`):
   - For every teammate in the repo roster (`teammates/*.md`, loaded with
     the roster loader that `horch teammates --check` uses, with the repo
     `teammates/` dir passed explicitly), and for each session mode
     {fresh with a fixed session id, resume with a fixed session id,
     unmanaged / no session}, build the launch command with
     `horch_core::launch::command` (and `command_with_skills` when a teammate
     has skills; use a bundle built from a fixed temp path).
   - Record: program, every argv element, every env var set or removed on the
     `Command` (`get_envs()`), and the current dir. Replace every temp path
     and home path with stable placeholders (`<TMP>`, `<HOME>`, `<PROMPT>`).
     Use a fixed prompt text.
   - Isolate the process environment: the builders may read env vars today
     (for example `OPENCODE_CONFIG_CONTENT`, `CODEX_HOME`, `HORCH_*_BIN`).
     Find each such read with `grep -rn 'std::env' crates/horch-core/src`.
     Pin each relevant variable to a fixed value or remove it inside the test,
     and record the pinned values in the oracle file header. Tests in one
     binary run in parallel threads, so serialize env-touching tests with one
     `static` `Mutex`.
   - If a teammate cannot build a command (for example agent `none`), record
     the error text instead.
   - Write one JSON file per teammate:
     `crates/horch-core/tests/oracles/launch/<teammate>.json`.
4. Oracle 2, routing decisions (consumer: A5 `arc_12`):
   - Inputs: every quota fixture in
     `crates/horch-core/tests/fixtures/telemetry/quota/*.json` (12 today; the
     master plan says 7, and more is fine), every teammate in the repo roster,
     flags {none, exact, force} (`balance_policy::GateFlags`), and modes
     {auto, advise, off} (`BalanceMode`).
   - Call `balance_policy::decide` and, where it applies, `resolve`. Find how
     the existing BAL tests build a `QuotaView` from a fixture and reuse that
     path. Record the decision as the JSON that `horch route --json` prints
     today (find its serializer in `crates/horch/src/cmd/route.rs`), plus the
     resolved teammate name.
   - Write one file per fixture:
     `crates/horch-core/tests/oracles/routing/<fixture>.json`, keyed by
     `<teammate>|<flags>|<mode>` with sorted keys.
   - Also write `crates/horch-core/tests/oracles/routing/fallback_problems.json`
     with `balance_policy::fallback_problems` for the repo roster.
5. Oracle 3, legacy ledgers (consumer: A6 `arc_17`):
   - Write 4 hand-made ledger fixtures under
     `crates/horch-core/tests/oracles/ledgers/`:
     - `bash-era.json`: the oldest format. Read `crates/horch-core/src/ledger.rs`
       for the serde defaults and the comments that name old fields; use only
       the fields that old format had.
     - `pre-effort.json`: records with no `effort` field.
     - `pr14-substituted.json`: records with `via` and `substitution_reason`.
     - `orchestrator.json`: a ledger with an orchestrator record and worker records.
   - For each fixture, load it with the ledger API, then record what the
     loader returns (as `Debug` or JSON) in `<name>.loaded.json`, and record
     the bytes that a load-then-save round trip writes in `<name>.saved.json`.
     Use a temp state dir.
6. Oracle 4, skills (consumer: A9 `skl_08`, A11 `mkt_09`):
   - For every teammate × phase {research, plan, implementation, validation},
     record the skills briefing text that the base code renders into the
     worker prompt. Find the renderer in `crates/horch-core/src/skills.rs`.
     Replace the bundle path with `<BUNDLE>`. Write
     `crates/horch-core/tests/oracles/skills/<teammate>-<phase>.txt`.
   - In `crates/horch/tests/baseline_cli.rs`, run the built `horch` binary
     (`env!("CARGO_BIN_EXE_horch")`) with `skills --json`, `skills`, and
     `skills --phase <p>` for each phase. Read `crates/horch/src/main.rs` for
     the exact flags. Set `HORCH_TEAMMATES_DIR` to the repo `teammates/`,
     `HOME` and `HORCH_STATE_DIR` to temp dirs, and remove
     `ANTHROPIC_API_KEY`. Write the outputs to `crates/horch/tests/oracles/skills/`.
7. Oracle 5, `horch sessions` render (consumer: A6):
   - In `baseline_cli.rs`, copy each ledger fixture from step 5 into a temp
     state dir as the project ledger, run `horch sessions` and
     `horch sessions --json`, and write the outputs to
     `crates/horch/tests/oracles/sessions/`. Find how the binary maps a
     project dir to a ledger file (`ledger.rs` state_root and slug) and set
     `HORCH_STATE_DIR` and `HORCH_PROJECT_DIR` (or the cwd) to match.
8. Add `arc_01_baseline_oracles_present` in `baseline_oracles.rs`. It asserts
   that every oracle directory above exists, that it has the expected count
   of files (one per teammate, fixture, and so on), and that no file is empty.
9. Bless once (`HORCH_BLESS=1 cargo test -p horch-core --test baseline_oracles`
   and `-p horch --test baseline_cli`). Then run the tests twice without
   `HORCH_BLESS`: both runs pass, so the output is deterministic. Read a
   sample of each oracle kind. Confirm no absolute path from your machine and
   no time-dependent value is in any file (`grep -rn /Users` must be empty).
10. Commit: `A0: Freeze pre-refactor oracles`. Write the report, listing for
    each oracle: the API that produced it, the env vars pinned, and how a
    later phase should call it. Commit it.
11. Run the full gate. Follow conventions §6 to finish.

## CONSTRAINTS

- Hermetic: no real harness binary runs. Building a `Command` does not run it.
- The test names that later phases add (`arc_09_*`, `arc_12_*`, ...) are NOT
  yours. Name your comparison tests `oracle_launch_matches`,
  `oracle_routing_matches`, `oracle_ledgers_match`, `oracle_skills_match`,
  `oracle_cli_skills_match`, `oracle_cli_sessions_match`, plus
  `arc_01_baseline_oracles_present`.
- Keep each oracle file deterministic: sorted keys, stable order, `\n` endings.

## DONE WHEN

- All 5 oracle kinds exist and the comparison tests pass twice in a row.
- `arc_01_baseline_oracles_present` passes.
- `grep -rn /Users crates/*/tests/oracles` prints nothing.
- The full gate is green.

## REPORT

- `horch note` after each oracle kind.
- `QUESTION:` if any value needs a source change to reach.
- `horch done` summary: oracle counts per kind, pinned env vars, gotchas.
