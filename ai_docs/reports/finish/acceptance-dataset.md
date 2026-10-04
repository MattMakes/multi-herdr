# L1 acceptance-dataset: report

Unit `acceptance-dataset`, branch `ds/acceptance-dataset`, worker opus-61,
2026-10-04. Plan: `ai_docs/plans/finish/l1-acceptance-dataset.md`. Checks:
dataset design §8, LA-6 to LA-12, run on the operator's Mac with real
herdr 0.8.2, claude 2.1.289 and codex-cli 0.160.0.

## Result

| Check | Result | Evidence (below) |
|---|---|---|
| LA-6 preflight numbers | PASS | §LA-6 |
| LA-7 trust dialogs | FAIL for a new repo; PASS for a repo the operator already trusts | §LA-7 |
| LA-8 real judge | FAIL before the fix (every judge crashed); PASS after commit 1 | §LA-8 |
| LA-9 promotion targets | PASS (4 targets); 1 message defect fixed (commit 3) | §LA-9 |
| LA-10 export and readiness | PASS | §LA-10 |
| LA-11 kill -9 and resume | PASS; 1 provisioning defect found and fixed (commits 2, 3) | §LA-11 |
| LA-12 key | not run as written (project rule); hermetic test plus a real check: PASS for the agents, 1 finding for the pane wrapper | §LA-12 |

Spend: 7 real rounds (the plan allowed 6; the orchestrator approved a
7th for the key check). `horch cost` total $1.09, with sonnet priced at the
claude-sonnet-5 rate through `--pricing`, because the built-in table has no
claude-sonnet-5-5 price (finding F2). 2 haiku calls to test the schema fix
cost under $0.01. Budget per round: `--budget-usd 4` (approved; see F1).

## Commits on ds/acceptance-dataset

1. `Judge: Drop the top-level keys --json-schema refuses`:
   `crates/horch-core/src/harness/headless.rs`. `headless_command` passes
   `cli_schema(schema)`: the schema without the top-level `$schema`, `$id`,
   `allOf`, `anyOf`, `oneOf`, `if`, `then`, `else`. The strict parser still
   checks the full schema (the winner rule in the top-level `allOf`
   included). Tests: `headless_passes_the_cli_schema_when_supported` (the
   old `headless_passes_the_schema_text_when_supported` asserted the
   verbatim text, which is the defect, so it was replaced),
   `cli_schema_only_drops_the_refused_top_level_keys`,
   `cli_schema_keeps_every_field_the_strict_parser_checks` (properties,
   required fields, `schema_version` const, verdict enum, assessment and
   score fields against `JUDGMENT_FIELDS`, `ASSESSMENT_FIELDS`, `VERDICTS`
   and the rubric components, so the API-side schema cannot drift below the
   local one).
2. `Dataset: Stop the round for the operator when a worktree cannot be made`:
   `competition/coordinator.rs`, `competition/state.rs`,
   `tests/coordinator.rs`. A worktree creation error records
   `round.needs_intervention{source: operator}` with the reason and the
   `cleanup <round> --force` hint; new transition
   `(Provisioning, round.needs_intervention:operator, NeedsIntervention)`;
   `drive` returns at once for a NEEDS_INTERVENTION round. No event format
   changes. Test `provisioning_failure_needs_the_operator_and_cleans_up`
   (fails before: `coordinator.start` returned Err): no candidate starts, 0
   anomalies, the reason names the label, `cleanup --force` removes the
   worktree made and leaves the foreign directory.
3. `Dataset: Put an explicit worktree root under the experiment id`:
   `crates/horch/src/dataset/run.rs`. `--worktree-root R` now gives
   `R/<experiment>/<label>` (the default root was already per experiment).
   The outcome line names the latest stop (`latest_intervention`: the last
   `round.needs_intervention` or `promotion.conflicted`). Tests
   `an_explicit_worktree_root_is_per_experiment`,
   `the_latest_intervention_names_a_later_conflict`. Verified on real round
   7: the worktree was `la-wt/01a1075b-…/A`.

Targeted checks (merge-train rule): `cargo test -p horch-core --lib
harness::headless` 6/6, `--lib competition` 7/7, `--test coordinator` 5/5,
`--test competition_planner` 10/10, `cargo test -p horch --lib dataset::run`
2/2, `--test dataset_cli` 2/2, `HORCH_REQUIRE_GIT=1 cargo test -p horch-e2e
--test dataset` 21/21, `cargo clippy -p horch -p horch-core --all-targets`
clean, `cargo fmt` clean.

## LA-6 preflight numbers

`preflight.completed` against the system (Activity Monitor's sources):

| Fact | Preflight | System |
|---|---|---|
| disk free | 2 253 420 072 960 B | `df -k`: 2 200 574 248 KiB = 2 253 388 029 952 B (32 MB written between the reads) |
| disk total | 3 996 276 899 840 B | `df -k`: 3 902 614 160 KiB, exact |
| RAM total | 137 438 953 472 B | `hw.memsize` 137 438 953 472, exact |
| RAM available | 83 806 765 056 B | `vm_stat` free+inactive+speculative = 83.5–84.7 GB over the reads |
| CPUs | 18 | `hw.ncpu` 18 |
| max processes | 10 666 | `kern.maxprocperuid` 10 666, `ulimit -u` 10 666 |
| max open files | 1 048 576 | `ulimit -n` 1 048 576 |

Note: the effective per-process file limit is `kern.maxfilesperproc`
245 760, below the rlimit. PRE-05 divides the rlimit; the verdict does not
change on this Mac (245 760 / 256 is still 960 candidates). Available RAM
leaves out purgeable pages, so it is conservative against Activity Monitor.

## LA-7 trust dialogs

- Round 1 (repo `/private/tmp/la-dataset/proj`, default worktree root under
  `~/.local/state`): no stall seen. Trust entries for the repo root
  appeared in `~/.claude.json` (`projects."/private/tmp/la-dataset/proj"
  .hasTrustDialogAccepted = true`) and `~/.codex/config.toml`
  (`[projects."/private/tmp/la-dataset/proj"] trust_level = "trusted"`).
  Who accepted them is not known.
- Round 2 (new repo `.worktrees/_scratch/la-dataset/proj`, inside the
  trusted `/Users/mascott/projects/multi-herdr`, `--worktree-root` under
  the same trusted folder): both panes stalled. Claude (pane `w2V:p2`,
  herdr status `blocked`): "Accessing workspace: …/wt-root/A. Quick safety
  check: Is this a project you created or one you trust? … ❯ No, exit /
  Yes, I trust this folder". Codex (pane `w2V:p3`, herdr status `idle`):
  "Note: You're in a subdirectory of a Git project. Trusting will apply to
  the repository root: …/_scratch/la-dataset/proj. Trust this folder? …
  › 1. Trust and continue 2. Quit". Nobody answered; at 900 s both
  candidates ended `timed_out`, the round was REJECTED `no_eligible`,
  cleanup ran and the workspace closed (the timeout path works).
- Rounds 3–7 ran from a linked worktree of the multi-herdr repo (orphan
  branch `la/base`): no dialog.

Conclusion: both harnesses key trust on the main repository root of the
worktree (the git common dir's parent), not on the worktree path and not
on a parent folder. A parent entry (`/Users/mascott/projects/multi-herdr`)
did not trust a separate repo under it. Fleet worktrees under
`multi-herdr/.worktrees/` never prompt and have no entries of their own.
So the default candidate worktree root (`~/.local/state/horch/multi-herdr/
<project>/worktrees/<exp>`, outside the project folder) does not cause the
prompt, and making the default `<repo>/.worktrees/` would not prevent it.
What matters is whether the operator trusted the repo itself once.

Product fix: landed on design-skills as PRE-14 (commit 2f2d672, opus-55,
plan f2-trust-preflight) from this evidence. The proposal was: PRE-13
reads both trust stores for the repo root (`~/.claude.json`
`projects[<root>].hasTrustDialogAccepted`, `~/.codex/config.toml`
`projects."<root>".trust_level`) for the harnesses in the plan, and fails
with the one-time commands (`cd <root> && claude`, accept, exit; the same
with `codex`). The coordinator reports a candidate pane that herdr shows
`blocked` at a trust dialog at once, not after the deadline. PRE-13 today
always warns, because `trusted_parents` is always empty
(`crates/horch/src/dataset/preflight.rs`).

## LA-8 real judge

Before commit 1, every judge attempt crashed in 1 s (round 1: `judge.failed
{kind: crashed, code: 1}` twice, then NEEDS_INTERVENTION). `job.log`:

    Error: --json-schema is not a valid JSON Schema: no schema with key or ref "https://json-schema.org/draft/2020-12/schema"

With `$schema` removed, a haiku test call returned:

    API Error: 400 tools.0.custom.input_schema: input_schema does not support oneOf, allOf, or anyOf at the top level

With `$schema`, `$id` and the top-level `allOf` removed, the call returned
valid `structured_output`. After commit 1, rounds 3–7 judged:

- `--json-schema`: available (`claude --help` lists it) and accepted.
- Bundle unchanged: round 3's `judge-input/manifest.json` lists 7 files;
  every sha256 still matched after the judge ran; every file time is
  before `judge.started`. The bundle names no model, harness or cost.
- Strict parse: 5 of 5 answers accepted, 0 rejected (rounds 3, 4, 5, 6, 7).
- Verdicts: tie (round 3, NEEDS_INTERVENTION with `tie_break: disabled`),
  winner B (round 4, after `tie_break: utility u1`), winner with
  confidence 0.6 (round 5, NEEDS_INTERVENTION below 0.7), winner A (rounds
  6 and 7 with `min_confidence: 0.5`).
- The judge ran on sonnet/medium (`judge.model` in the scratch
  `dataset.yaml`) to keep the cost low, not on the default opus/high.

## LA-9 promotion targets

Round 4 (`--promote-to la/t-clean`, checked out clean in a worktree):
PROMOTED, `la/t-clean` fast-forwarded 00daa1f → 59a77ac, the checkout
followed and stayed clean. A second `promote` of a promoted round is
refused (receipt exists), which is correct.

Round 6 (DECIDED, then `promote --to`):

| Target | Result | Exit | Ref |
|---|---|---|---|
| `la/t-dirty` (checked out, local edit) | NEEDS_INTERVENTION: "is checked out with local changes" | 5 | unchanged, edit kept |
| `la/t-moved` (moved, conflicting `slugify`) | `promotion.conflicted{paths: [textutil.py]}`, NEEDS_INTERVENTION | 5 | unchanged |
| `la/t-free` (not checked out) | PROMOTED via `update_ref_cas`, fast forward | 0 | 00daa1f → 655ef3c |

`rollback` moved `la/t-free` back to 00daa1f; a second `rollback` is a
no-op with the same message. Defect (fixed in commit 3): the `la/t-moved`
attempt printed the old `la/t-dirty` reason, because the outcome line read
only the projection's `needs_intervention`. A round 5 NEEDS_INTERVENTION
below confidence is refused by `promote` ("without a winner"), as designed.

## LA-10 export and readiness

Before any round: `export` wrote 0 rows (empty file, sha256 e3b0…b855).
After the rounds: 2 exports 1 s apart, 4 rows each, sha256 10bd9946…610d
for both, `cmp` identical. `readiness`: `not_ready`; judged rounds 4/50,
arms with ≥ 10 runs 0/4, distinct tasks 1/30; 2 arms (sonnet: 4 runs, 1
win; codex-terra: 3 runs, 1 win). That is correct for this data.

## LA-11 kill -9 and resume

Round 4, coordinator killed by exact pid (`kill -9 <pid>`) 3 times:

1. RUNNING, right after both `candidate.spawned` → `resume`.
2. JUDGING, right after `judge.started` (the detached judge job survived)
   → `resume` adopted the judge result.
3. REVALIDATING, 5 s after `winner.selected`, while the 20 s revalidation
   gate ran → `resume` finished: PROMOTED.

Invariants: 2 `candidate.spawned`, 1 `judge.started`, 1 judgment file, 1
receipt, `la/t-clean` advanced once, 0 duplicate idempotency keys, `horch
sessions --all` shows exactly 1 execution per label. `rebuild`: 23
events, 0 torn lines, 0 anomalies, "projection: identical to the live
fold". The PROMOTING window itself (CAS publish) is too short to hit
with a manual kill; the hermetic `cmp_13_crash_every_boundary` covers
`abort-after-promotion-started` and `abort-after-update-ref`. An orphan
`sleep 20` gate process outlived the killed coordinator and exited by
itself.

Found on the way (fixed, commits 2 and 3): round 4's first start failed
after preflight with "creating the worktree of A: …/la-wt/A is a worktree
on mh/exp/89432c6b/r0/A, not on mh/exp/f13cbd35/r0/A". Round 3 (NEEDS_
INTERVENTION) had kept `la-wt/A`, and `--worktree-root` was not per
experiment. The round stayed in PROVISIONING, and every resume would fail
the same way. Resume worked after `cleanup --force` of round 3.

## LA-12 key

Not run as written: the project rule forbids setting
`ANTHROPIC_API_KEY`, even to a sentinel. The hermetic test
`sec_08_e2e_no_api_key_in_any_child` covers it. Real checks without a key:

- `grep -rl ANTHROPIC_API_KEY` over the 3 dataset state dirs and the
  ledger: 0 files.
- `ps eww` during round 7: candidate claude (pid 23010) 0 hits; judge job
  (33082) 0 hits; its claude child (33088) 0 hits; coordinator 0 hits.
- Finding F5: the candidate pane's `horch worker` wrapper (pid 22834) had
  1 hit. The herdr server (pid 5443) carries `ANTHROPIC_API_KEY` from the
  operator's shell (value length 108; the value was not printed), and every
  pane shell inherits it. The wrapper removes it before it starts the
  agent, so no agent and no agent child sees it.

## Findings for other units (not fixed here)

- F1 (cost estimate): PRE-09 projects a fixed `DEFAULT_TOKEN_ESTIMATE` per
  candidate ($1.60 sonnet, $1.72 codex-terra) whatever the task; no config
  key changes it, so `--budget-usd 2` always fails with 2 candidates. Real
  spend was about $0.07 per candidate. Proposal: a `budget.expected_tokens`
  key in `dataset.yaml` (per model or global), and later an estimate from
  the measured runs of the same task family.
- F2 (pricing, owned by opus-62/L2): `canonical_model` maps the alias
  `sonnet` to `claude-sonnet-5`, but transcripts name `claude-sonnet-5-5`,
  which has no price. `horch cost` shows $0.00 for sonnet, and the dataset
  `UsageMeter` counts sonnet candidates as 0 µ$, so the budget ceiling does
  not stop them. Evidence sent to opus-62.
- F3 (fleet env): inside a fleet pane, `HORCH_PROJECT_DIR` (and the other
  `HORCH_*` identity variables) overrides the cwd, so `multi-herdr-dataset
  run` from a fleet pane targets the fleet's repo. My first preflight-only
  run did that; it was refused at preflight and left a 3-event experiment
  dir at `~/.local/state/horch/multi-herdr/-Users-mascott-projects-multi-herdr`
  (removal was denied to me; the operator can delete it). Proposed
  precedence: an explicit `--project` flag, then the git top level of the
  cwd, then `HORCH_PROJECT_DIR` only when the cwd is not in a git repo.
  `run` should print the target repo and its HEAD as its first line,
  before preflight.
- F4 (codex in linked worktrees): in round 5 codex-terra could not commit
  ("Git cannot create the worktree index lock because its shared
  .git/worktrees/B path is not writable"), then went idle without `horch
  done`, so the coordinator waited the full 900 s deadline and the
  candidate ended `timed_out`. The orchestrator plans a follow-up unit.
- F5 (key in the pane wrapper): see LA-12. Proposal: the candidate pane
  command starts with `env -u` for every `FORBIDDEN_ENV` name, so no
  process in the pane holds the key.
- F6 (status): `status` shows the experiment as PLANNED while its round
  runs or is COMPLETE; only the round state moves.
- F7 (empty dirs): after cleanup, `<root>/<experiment>/` and
  `<root>/_promote/` stay as empty directories.
- F8 (docs): the docs say nothing about the `--worktree-root` layout; after
  commit 3 it is `<root>/<experiment>/<label>`.

## Scratch artefacts and cleanup

Created in the multi-herdr repo (all removed at the end, see the done
summary): branches `la/base`, `la/t-clean`, `la/t-dirty`, `la/t-free`,
`la/t-moved`; worktrees `.worktrees/_scratch/la-base`, `la-t-clean`,
`la-t-dirty`; candidate branches `mh/exp/89432c6b/r0/{A,B}`,
`mh/exp/f13cbd35/r0/{A,B}`, `mh/exp/b7a40065/r0/{A,B}`,
`mh/exp/1d2b52d5/r0/A`, `mh/exp/737ba3cc/r0/A`. Outside the repo:
`/tmp/la-dataset/` (round 1 repo) and `.worktrees/_scratch/la-dataset/`
(round 2 repo, binaries, logs). No dataset workspace is left open; `w2F`
was not touched.
