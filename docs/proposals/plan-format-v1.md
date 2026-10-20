# Proposal: plan format v1 - plans that horch can read and check

| | |
|---|---|
| Status | Proposal. Not approved for build. |
| Date | 2026-10-20 |
| Decision asked | Run the evaluation in section 6. Build only the parts that its results justify. |
| Builds on | `skills/orchestrate/SKILL.md` §2 and §5, [fleet-workflow.md](../fleet-workflow.md), [specs/fleet-dataset.md](../specs/fleet-dataset.md), `evaluation/validator.rs` |

---

## 1. Summary

The plan file is already the contract between the orchestrator and a worker
([fleet-workflow.md](../fleet-workflow.md): "The plan file is the contract").
But no command reads it. The orchestrator checks every promise in it by hand:

- It re-runs the `DONE WHEN` tests itself (orchestrate §5.2: "A worker's
  `DONE:` is a claim, not evidence").
- It reads the diff to confirm the worker stayed inside `own` (§5.3).
- It finds file overlaps and unit order by hand (§1.2, §1.3).
- It keeps `STATUS.md` by hand.

This proposal makes the plan file an official, versioned format that horch
parses. horch then does the mechanical checks at 3 points: `horch spawn`,
`horch done` and a new `horch plan` command. The orchestrator keeps the checks
that need judgement.

The proposal does not decide that this is worth building. Section 6 is a
staged evaluation. Each stage is cheap, and each stage has a stop rule.

## 2. Where the ideas come from (clean-room note)

The operator asked that nothing be copied from other projects, so that no
credit or maintenance is owed. This design follows these rules:

1. **It extends our own format.** Every section name, field name and command
   name comes from this repository: the plan sections in orchestrate §2, the
   ledger, the fleet run rows, `GateSpec`, `UNLANDED`, `horch verdict`.
2. **It uses only general engineering ideas.** None belongs to one project.

   | idea | where it already is in this repo | general source |
   |---|---|---|
   | Checkable acceptance criteria | `DONE WHEN` (orchestrate §2) | Postconditions, as in design by contract; CI gates |
   | Run commands to decide pass or fail | `GateSpec`, `CommandValidator` (`evaluation/validator.rs`) | Every CI system |
   | Each unit owns a set of files | `FILES` `own` / `do not touch` | CODEOWNERS; ownership in build systems |
   | Record a digest of the input | `plan_digest` in the fleet start row | Content addressing; lockfiles |
   | Units that wait for other units | orchestrate §1.3 ("Serialize units that must share a file") | Make; task schedulers |
   | Mechanical checks first, then a person judges | orchestrate §5; the dataset judge after gates | Code review after CI |

3. **No text, schema or code from another project is in this document.**
   Section 8 describes the overlap check that was run on this document.
4. **Builders get only this document and this repository.** The plan files for
   the build must tell workers not to fetch or read other contract-language
   projects. A worker that has never read them cannot copy them.

This note is not legal advice. Ideas and general methods are not
copyrightable. The usual permissive licenses ask for credit only when their
text or code is copied. Rules 1 to 4 keep us out of that case.

## 3. Problems to solve

| # | problem | today's cost |
|---|---|---|
| P1 | A `DONE:` is not verified until the orchestrator runs the checks | Orchestrator context and turns per unit. Errors found late, after the worker's pane is closed, so a fix needs a fresh worker. |
| P2 | Scope is checked by reading diffs | Orchestrator context. A missed out-of-scope edit collides with another worker. |
| P3 | File overlap between live units is found by hand | A conflict that neither worker can see (orchestrate §8.2). |
| P4 | Unit order is kept in the orchestrator's head and in `STATUS.md` | Lost after a context reset or a resume. |
| P5 | Plans differ in shape from run to run | A cheaper teammate gets an unclear plan. The section list is advice, not a rule. |
| P6 | A plan can change after the worker starts | The worker and the orchestrator can work from different versions. The start row has `plan_digest`, but nothing compares it. |

We do not know how often each problem happens. Stage 0 of the evaluation
measures that first.

## 4. The format

### 4.1 File and version

- A plan stays where it is: `ai_docs/plans/<run>/<unit>.md`.
- A v1 plan starts with YAML frontmatter that has `plan: 1`.
- A file without that frontmatter is a legacy plan. horch treats it exactly as
  today. `horch plan check` gives advice on it and never refuses it.

### 4.2 Frontmatter

```yaml
---
plan: 1                       # format version, required
unit: d09-agent-list          # required; unique in the run; [a-z0-9-]+
run: harness-sweep            # required; the directory name under ai_docs/plans/
teammate: sonnet-feature      # optional; the teammate the orchestrator intends
phase: implementation         # optional; research | plan | implementation | validation
after: [d08-capabilities]     # optional; units that must be done first
check_timeout_s: 600          # optional; per check, default 600
---
```

`teammate` and `phase` are records of intent. `horch spawn` prints a NOTE when
the spawn differs from them. It does not refuse.

### 4.3 Sections

The section names are the ones in orchestrate §2. The parser accepts any
heading level and any letter case, so `## GOAL` and `### Goal` are the same.

| section | v1 rule |
|---|---|
| Goal | Required. 1 paragraph. |
| Context | Optional. Free text. |
| Files | Required. Typed lines (4.4). At least 1 `own` line. |
| Prior work | Optional. Free text. |
| Steps | Optional. Free text. |
| Constraints | Optional. Free text. |
| Done when | Required. Typed lines (4.5). At least 1 `check` or `review` line. |
| Report | Optional. Free text. |

An unknown section is kept and ignored, with a lint NOTE. The format has no
other grammar. Free text stays free, because workers read it and horch does
not.

### 4.4 `Files` lines

```markdown
## Files
- own: crates/horch/src/cmd/agentlist.rs
- own: crates/horch-core/src/harness/inventory/**
- shared: skills/README.md
- never: Cargo.lock
- read: crates/horch-core/src/harness/capabilities.rs
```

| kind | meaning | checked |
|---|---|---|
| `own` | The worker may create, edit and delete these paths. Globs allowed. | Yes |
| `shared` | Several units add lines here (fleet-workflow: "Shared files"). The worker may edit the file. | Overlap is allowed |
| `never` | The worker must not change these paths. | Yes |
| `read` | Context for the worker. | No |

Any other changed path is **outside scope**. A `never` path changed is a
**violation**. Section 5.3 says what each one does.

### 4.5 `Done when` lines

```markdown
## Done when
- check: cargo test -p horch agent_list
- check: cargo clippy -p horch -- -D warnings
- review: The help text names every column.
- review: The JSON output keeps the old field names.
```

| kind | who decides | how |
|---|---|---|
| `check` | horch | `sh -c <command>` in the worker's directory. Exit 0 is a pass. The same runner as dataset gates. |
| `review` | the orchestrator | Reads the work and records `horch verdict`. horch only lists these lines in the report. |

A line that is neither kind is a lint error in a v1 plan. Each condition must
say who checks it. That rule is the core of the format.

### 4.6 A complete example

```markdown
---
plan: 1
unit: d09-agent-list
run: harness-sweep
teammate: sonnet-feature
phase: implementation
after: [d08-capabilities]
---
## Goal
`horch agent-list` prints 1 row for each installed harness.

## Context
d08 added `Capabilities::version_flag`. Use it. Do not add a second probe.

## Files
- own: crates/horch/src/cmd/agentlist.rs
- own: crates/horch-core/src/harness/inventory.rs
- never: crates/horch-core/src/harness/capabilities.rs

## Steps
1. ...

## Done when
- check: cargo test -p horch agent_list
- review: The help text names every column.

## Report
Send `horch done` with the test count and the commit sha.
```

## 5. What horch does with a v1 plan

### 5.1 Lint: `horch plan check <path>...`

- Parses each file. Prints errors and NOTEs. Exit 0 when there are no errors.
- Errors: no `plan: 1` in a file that has frontmatter, a missing required
  section, an untyped `Done when` line, no `own` line, a bad unit name, a
  `never` glob that matches an `own` glob.
- Run-level checks across a directory: duplicate units, an `after` that names
  no unit, an `after` cycle, and 2 units whose `own` globs overlap without an
  `after` path between them. That last one is P3, found before any spawn.

### 5.2 At `horch spawn`

horch finds the plan with the existing plan rule (fleet-dataset §3.1: the first
`.md` token of the task). For a v1 plan it also does these steps:

1. **Lint** the file (5.1). An error refuses the spawn: exit 2, nothing created.
2. **Order.** Every unit in `after` must have a ledger record for this run that
   is done and has no `rework` or `rejected` as its last verdict. Otherwise the
   spawn is refused. `--ignore-after` overrides this and is recorded in the
   ledger.
3. **Overlap.** The `own` globs must not overlap the `own` globs of any live
   execution's sealed plan. Otherwise the spawn is refused. `--allow-overlap`
   overrides this and is recorded.
4. **Seal.** Copy the plan bytes to `<state root>/plans/<execution id>.md`
   (0600). Store the unit, the run and the digest on the ledger record. The
   start row already stores `plan_digest`.

The seal fixes P6 and is the main protection against tampering. A worker can
edit the plan in `ai_docs/`, but every later step reads the sealed copy.
`horch plan reseal <role>` is for the orchestrator only. It replaces the sealed
copy and records a history event.

### 5.3 At `horch done`

For a v1 plan, before the record is marked done:

1. **Scope.** horch collects the paths this unit changed:
   - Worktree mode: `git diff --name-only <base>...<branch>`, which is exact.
   - Single-branch mode: the commits since the start row's `base_sha` that
     carry the trailer `Horch-Unit: <unit>`, plus uncommitted changes to the
     unit's `own` paths. Other workers' commits are on the same branch, so
     the trailer is the only reliable link. Section 5.4 adds a way to write it.

   A `never` path changed is a violation. A path outside `own` and `shared` is
   outside scope.
2. **Checks.** horch runs every `check` line from the sealed plan with
   `CommandValidator`. It uses the same process group, timeout, environment
   removal (`FORBIDDEN_ENV`, which includes `ANTHROPIC_API_KEY`), redaction and
   capped logs as dataset gates.
3. **Result.** The outcome depends on the mode (5.5):
   - `enforce`: a failed check or a violation refuses `horch done`. The pane
     stays open, and the worker sees each failure and the log path, so it can
     fix the problem while its context is still warm. That is the main bet of
     this proposal (P1). A path outside scope prints a warning and does not
     refuse.
   - Escape hatch, like `--unlanded`:
     `horch done --unverified "<reason>" "<summary>"` finishes anyway. The
     report line then carries `UNVERIFIED: <reason>`, and the orchestrator
     must act on it.
4. **Report.** The `[role] DONE:` line gains 1 part:
   `CHECKS 2/2. SCOPE ok. REVIEW 1.` The `REVIEW n` part tells the
   orchestrator how many `review` lines it still has to judge.
5. **Record.** The run row gains an optional `plan_checks` object: each
   check's name, status, duration and log digest, plus the scope result. The
   schema version goes from `mh.fleet-run/1.0.0` to `1.1.0`, and old readers
   ignore the new field.

### 5.4 `horch commit "<message>" -- <path>...`

This is a small wrapper over the pathspec commit that fleet-workflow.md already
requires. It refuses a path that the sealed plan does not own, adds the
`Horch-Unit:` trailer, commits only the given paths, and then prints
`git show --stat HEAD`. The worker briefing already demands each of those
steps by hand. A worker that uses plain `git commit` still works. Its commits
then show as outside the unit at `done` time, which makes the gap visible.

### 5.5 Modes

`HORCH_PLAN_CHECKS=off|shadow|enforce`. `bootstrap.rs` reads it into
`RuntimeContext`, as rule `arc_05` requires.

| mode | spawn | done | report line |
|---|---|---|---|
| `off` (default until the decision) | as today | as today | as today |
| `shadow` | lint, order and overlap print NOTEs only; seal | runs scope and checks, records them, never refuses | **unchanged** |
| `enforce` | refuses as in 5.2 | refuses as in 5.3 | with `CHECKS`, `SCOPE` and `REVIEW` |

In `shadow` mode the report line stays unchanged on purpose. The orchestrator
must give its verdict without seeing horch's results. Then the verdict is an
independent label against which horch's checks can be scored (6.4).

### 5.6 `horch plan status [<run>]`

Prints the unit table from the plan directory, the ledger and the verdict
rows: unit, teammate, state, last verdict, checks and `after`. It replaces the
hand-kept `STATUS.md` (P4). It reads data and writes nothing.

### 5.7 Security

- Rule SEC-07 says a gate command never comes from the party being judged.
  That stays true. The orchestrator writes `check` lines, and horch seals
  them before the worker's agent starts. Extending SEC-07 to "config, or a plan
  sealed at spawn" is a spec change, and the build must make it explicitly.
- Checks run as the same user, with the same environment removal, as the
  worker's own shell, so they give no new power. The risk is tampering, and
  the seal covers it.
- A worker could still weaken a check by editing the tests that the check
  runs. Two defences: test files belong in `never` or in the orchestrator's
  `review` lines, and the evaluation measures this case (6.3, G2).

### 5.8 Code map

| change | where |
|---|---|
| New module: parse, lint, seal, scope, verify | `crates/horch-core/src/plan/{mod,parse,lint,seal,scope,verify}.rs` |
| Run a check list in a given directory | `evaluation/validator.rs`: let `CommandValidator` take a directory, not only a `FrozenCandidate` |
| Spawn steps 5.2 | `execution/service.rs` (plan step before the record is written) |
| Done steps 5.3 | `execution/lifecycle.rs`: 1 new `DoneSteps` method, `verify_plan` |
| `--unverified`, report parts | `crates/horch/src/cmd/messaging.rs` |
| `horch plan check/status/reseal`, `horch commit` | `crates/horch/src/cmd/plancmd.rs` (new) |
| Run row 1.1.0 | `fleet_runs/rows.rs`, `fleet_runs/facts.rs`, `docs/specs/fleet-dataset.md` |
| Mode setting | `runtime/context.rs`, `crates/horch/src/bootstrap.rs` |
| Orchestrator playbook | `skills/orchestrate/SKILL.md` §2 (write v1), §5 (checks are done; judge the `review` lines) |
| Worker briefing | `teammates/_base/fleet-worker.md` (run the checks before `done`; use `horch commit`) |
| Spec | `docs/specs/plan-format.md` with requirement IDs and tests, written only after the decision |

The `arch_scan` rules apply: typed `PlanError`, no branch on error text, no
`std::env` in core outside `runtime/`.

### 5.9 Not in v1

- Named multi-worker recipes (the critic loop, the creative council, a
  competition) declared in a plan. They may come later if v1 pays off.
- Auto-accept when every line is a `check`. The orchestrator still records
  every verdict. The evaluation tells us whether this is safe later.
- Auto-spawn when an `after` unit finishes.
- Any model call inside horch. All v1 behaviour is deterministic.

## 6. Evaluation: does it improve outcomes?

### 6.1 Hypotheses

| # | hypothesis | the part it tests |
|---|---|---|
| H1 | Many reworked units fail in ways that a `check` line or the scope rule would catch | Is there a problem to solve? |
| H2 | The v1 format alone (no enforcement) makes workers succeed more often on the first pass | Format |
| H3 | horch's checks agree with the orchestrator's verdicts, with few false refusals | Check quality |
| H4 | Enforcement at `done` raises first-pass quality at an acceptable cost | Enforcement |
| H5 | The orchestrator spends fewer tokens verifying each unit | Orchestrator load |

### 6.2 Data we already have, and 1 gap

Most of the measurement exists. One part is missing.

**Gap: no verdicts are recorded today.** fleet-dataset §5 specifies
`horch verdict`, and `fleet_runs::verdict::record_verdict` implements it in
`horch-core`. But no `horch` subcommand calls it, and the orchestrator
briefing (`teammates/_base/fleet-orchestrator.md`) never mentions it. Only
`fleet-compete.md` tells the orchestrator to run it, and that command does
not exist. So `verdicts.jsonl` is almost certainly empty on every machine.
M1 and Stage 2 depend on verdicts. **Stage P** (6.4) fixes this first. The
fix is useful whatever we decide about plans.

| need | source |
|---|---|
| The plan text and digest at spawn | `fleet/starts.jsonl` (`plan_text`, `plan_digest`, `base_sha`) |
| Tokens, cost, time and notes per worker | `fleet/runs.jsonl` (`cost_microusd`, `duration_ms`, `notes`, `tokens`) |
| The orchestrator's judgement | `fleet/verdicts.jsonl` (`accepted`, `rework`, `rejected`, plus a note) |
| Later defects | `multi-herdr-dataset outcome regression\|revert` |
| Controlled runs: the same plan, isolated worktrees, gates the worker does not see, cost limits | `multi-herdr-dataset run --plan <path> --candidates 1 --baseline <teammate>` |
| The orchestrator's own tokens | The orchestrator's run row, and `horch cost --sessions` |

### 6.3 Metrics

**Primary**

- **M1 first-pass acceptance**: the share of units whose first worker record
  ends with `accepted`, with no `rework` record for the unit.
- **M2 hidden-test pass rate** (controlled stages only): the share of runs
  whose result passes tests that the worker never saw.

**Secondary**

- **M3 cost per accepted unit**: the sum of `cost_microusd` over every record
  of a unit, rework included, divided by accepted units.
- **M4 wall time per accepted unit.**
- **M5 orchestrator verification load**: the orchestrator's tokens per unit,
  and the time from a worker's `finished_at` to its verdict.

**Guardrails** (each must stay below a limit)

- **G1 false refusals**: a check failed, but the orchestrator accepted the
  work unchanged.
- **G2 gaming**: a worker changed test files or check inputs outside `own`, or
  edited its plan after the seal.
- **G3 authoring overhead**: the orchestrator's tokens and time to write a v1
  plan compared with a legacy plan, and the lint errors per plan.
- **G4 escaped defects**: `regression` or `revert` outcomes within 14 days.

### 6.4 Stages

Each stage runs only if the previous stage passes its gate.

#### Stage P - Record verdicts (prerequisite, about 1 unit, no model spend)

1. Add `horch verdict <role-or-record-id> <accepted|rework|rejected> [--note <text>]`
   to the CLI, as fleet-dataset §5 specifies. It calls `record_verdict`.
2. Add the step to the orchestrator briefing and to orchestrate §5: after
   each check of a DONE, run `horch verdict` with a 1-sentence note that names
   the cause.
3. From then on, every fleet run gives the labels that M1, M3 and Stage 2
   need. Stage 0 and Stage 1 can run while the labels build up.

#### Stage 0 - Look back (no tokens, about 1 day of 1 worker)

On the operator's machine, over every project's `fleet/*.jsonl` (run
`multi-herdr-dataset fleet sync --all-projects` first), `ai_docs/plans/`,
`ai_docs/reports/` and `ai_docs/reflections/`:

1. Count units, and the share that needed rework or were dropped. Past runs
   have no verdicts (6.2), so use these signals instead, each with its
   source noted: a later unit or plan that fixes the same unit, a red gate
   sent back to the worker (orchestrator messages, `STATUS.md`), a resumed
   or re-spawned unit, and the reflections. These labels are weaker than
   verdicts. Report them as an estimate.
2. Sort every rework or drop into 1 cause: tests failed, build broke, out of
   scope, file conflict, incomplete, wrong approach, other.
   A second reader sorts a random 20 % of them, to check that the causes are
   applied the same way.
3. For each plan, count the `DONE WHEN` items that a command could check.
4. Where `ai_docs/reports/` or git history allow it, compare the changed files
   with `own`.

**Gate to Stage 1 (H1):** the share of units with rework or rejection is at
least 10 %, and at least a third of those causes are "tests failed", "build
broke", "out of scope" or "file conflict". If not, stop. Adopt only the format
and `horch plan check`, which cost little, and skip enforcement.

#### Stage 1 - Format only, controlled (tokens, no Rust)

Test H2 with the dataset harness. No code changes are needed.

1. **Corpus.** Take 20 to 30 finished units from the start rows: each has
   `plan_text`, `base_sha` and a merged result. Choose units whose merged
   result added or changed tests. Spread them over teammates and sizes.
2. **Hidden tests.** For each unit, the tests from the merged result become
   gates. Store them outside the repository and copy them in at gate time, so
   the worker never sees them.
3. **2 versions of each plan.** Arm A is the original plan. Arm B is the same
   plan rewritten into v1 by 1 fixed procedure. A second reviewer confirms that
   arm B adds no fact that arm A lacks: only structure, and each `DONE WHEN`
   item typed as `check` or `review`. Arm B tells the worker to run its `check`
   lines before `horch done`. That instruction is part of the format being
   tested.
4. **Runs.** `run --plan` refuses a plan that is not committed at the base
   commit (fleet-dataset §7.2). `ai_docs/` is not in git. So for each unit and
   arm, make an eval branch: the unit's `base_sha` plus 1 commit that adds only
   that arm's plan file. Each arm gets its own branch, so a worker never sees
   the other arm's plan. On that branch run
   `multi-herdr-dataset run --plan <arm file> --candidates 1 --baseline <teammate>`.
   Use 1 cheap teammate for every unit (for example `sonnet`), so the
   teammate does not vary between the arms. Run the arms in random order.
   Before the pilot, confirm with 1 dry round that `--candidates 1` plans only
   the baseline slot, and that a round with 1 candidate is decided by its gates
   as expected.
5. **Pilot first:** 6 units × 2 arms × 2 repeats = 24 runs. Use the spread of
   the results to choose the full size. Plan for about 25 units × 2 arms × 3
   repeats = 150 runs.

**Analysis:** M2 per arm, with runs paired by unit. Use McNemar's test on the
paired pass/fail results, and a bootstrap confidence interval on the
difference per unit. Also compare M3 and M4.

#### Stage 2 - Shadow mode in real fleet work (small build)

Build only 5.1, the seal in 5.2, and 5.3 in `shadow` mode. That is about a
third of section 5. Run normal fleet work with `HORCH_PLAN_CHECKS=shadow` for
2 to 3 weeks, or at least 60 units.

The orchestrator does not see the check results (5.5), so its verdicts are an
independent label. For each unit, compare horch's result with the verdict:

| | verdict `accepted` | verdict `rework` or `rejected` |
|---|---|---|
| checks and scope pass | agree | **miss**: the problem needed judgement |
| checks or scope fail | **false refusal** (G1) | **catch**: enforcement would have sent it back early |

This measures H3 directly, and gives the size of H4's possible gain: the
catches are the units that enforcement would have fixed while the worker
still had its context.

**Gate to Stage 3:** catches are at least 50 % of the rework and rejected
units, and false refusals are at most 5 % of the `done` calls.

#### Stage 3 - Enforcement, controlled (tokens)

Build the rest of 5.3 (`enforce` mode). Re-run the Stage 1 corpus with 2 arms:
arm B (format only) and arm C (format and enforcement). Use the same
teammate, repeats and analysis. Also measure G2: in arm C, do workers change
tests to make a check pass?

#### Stage 4 - Decide

| result | decision |
|---|---|
| Stage 0 gate fails | Adopt the format and lint only. Stop. |
| H2 holds (arm B ≥ arm A on M2, and M3 is at most 10 % higher) | Make v1 the plan format in the orchestrate skill, even if enforcement does not pay. |
| Stage 2 gate fails | Keep shadow data for one more cycle, or stop at format and lint. |
| H4 holds: arm C beats arm B on M2 by at least 10 points, or rework falls by at least a third; M3 is at most 15 % higher; G1 ≤ 5 %; no G2 cases | Build the rest. Change the default mode to `enforce`. Write `docs/specs/plan-format.md`. |
| Mixed results | Keep `shadow` on. Re-evaluate after 100 more units. |

The operator should confirm or change these limits **before** Stage 1 starts.
Limits chosen after the data is in are not a fair test.

### 6.5 Cost

The dataset default estimate is about $1.60 per candidate on `sonnet`
(dataset-config.md, `budget`). Measured spend replaces the estimate after the
pilot.

| stage | runs | estimate |
|---|---|---|
| P | 0 | No model spend. About 1 build unit. |
| 0 | 0 | No model spend. About 1 worker-day. |
| 1 pilot | 24 | about $40 |
| 1 full | about 150 | about $240 |
| 2 | 0 extra | Normal fleet work. The build is about 3 to 4 units. |
| 3 | about 150 | about $240, plus about 2 to 3 build units |

Each round uses `--budget-usd`, so a stage cannot spend more than its limit.

### 6.6 Threats to validity

- **One repository.** The corpus comes from multi-herdr work. Godot, Swift and
  Unreal projects can behave differently. Add 5 or more units from another
  project before the final decision, if such fleet data exists.
- **Rewrite bias.** A careful rewrite into v1 can add clarity that is not
  caused by the format. The fixed procedure and the second reviewer in Stage 1
  step 3 limit this. Report the cases where the reviewer had to remove
  something.
- **Model variance.** One run says little. That is why the stages use repeats
  and paired analysis, and why the pilot sizes the full run.
- **The orchestrator sees the arm.** Shadow mode hides the checks from it. The
  controlled stages use hidden tests, not the orchestrator, as the judge.
- **Hidden tests are a narrow view.** Tests miss design quality. For a sample
  of each arm, run the dataset judge blind as a secondary signal.
- **Model or price changes during the evaluation.** Run each stage's arms over
  the same days. Do not compare across a model release.

## 7. Rollout if adopted

1. Ship `horch plan check` and the format in the orchestrate skill. Legacy
   plans keep working.
2. Ship `shadow` as the default for 1 cycle in every project.
3. Change the default to `enforce`. Keep `--unverified` and `off`.
4. After 3 months, decide whether to remove legacy plan support.

## 8. Overlap check on this document

On 2026-10-20, every 8-word sequence in this document (4 554 of them) was
compared with the text files of the OpenProse repository (`openprose/prose`,
default branch, shallow clone). The result was 0 shared sequences. A 6-word
check also found 0. A search for that project's own terms for its kinds and
sections found none in use here.

The check is `scripts/eval/text-overlap.py`:

```bash
git clone --depth 1 https://github.com/openprose/prose /path/outside/the/repo/prose
python3 -I scripts/eval/text-overlap.py docs/proposals/plan-format-v1.md /path/outside/the/repo/prose
```

Run it again on every later version of this document, on the build's plan
files and on the new spec, before they are committed. Only the person who runs
the check reads the other project. Builders do not (section 2, rule 4).

## 9. Questions for the operator

1. Do you agree with the stop rules in 6.4, or do you want other limits?
2. Can Stage 0 read your machine's `fleet/` data and `ai_docs/` (it stays
   local), or should a worker on your machine produce only the counts?
3. Which teammate should run the controlled stages? `sonnet` is cheaper.
   `opus` is closer to real use.
4. Should `review` lines also go to a reviewer teammate (`codex-reviewer`)
   instead of the orchestrator? That changes M5 and is out of v1 scope unless
   you want it.
