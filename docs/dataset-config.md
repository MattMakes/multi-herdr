# `dataset.yaml` reference

`multi-herdr-dataset run` reads `<repo>/.multi-herdr/dataset.yaml` when it
exists. Every key is optional. An unknown key is an error. A `run` flag
overrides the key with the same name. The source is
`crates/horch-core/src/competition/config.rs` (`load`, `merge`, `validate`);
the design is §4.11 of
[`specs/dataset-competition.md`](specs/dataset-competition.md).

`<repo>` is the repo the command targets: `--project <dir>`, else the git
top level of the current directory (the full order is in
[command-flow.md](command-flow.md), "Which repo a dataset command targets").
In a linked worktree (`git worktree add`), the git top level is the linked
worktree, not the main repository. So a command run there reads the linked
worktree's `.multi-herdr/dataset.yaml`, starts the candidates from its HEAD
and keeps its own dataset directory. To target the main repository from a
linked worktree, pass `--project <main repository>`.

## Top-level keys

| key | default | flag | meaning |
|---|---|---|---|
| `candidates` | `3` | `--candidates N` | candidates per round, at least 1 |
| `strategy` | `diverse` | `--strategy` | how candidates are picked; `diverse` is the only value |
| `budget` | see below | `--budget-usd X` | the money limits |
| `judge` | see below | `--judge` (mode only) | the judge |
| `baseline` | none | `--baseline T` | the teammate in slot A |
| `gates` | `[]` | none | the validation commands every candidate runs |
| `caps` | see below | none | time, parallelism, disk and size limits |
| `exclude` | `[]` | none | teammates that never run as candidates |
| `promote_to` | none | `--promote-to B` | promote the winner onto branch B |
| `worktree_root` | `<dataset root>/worktrees` | `--worktree-root DIR` | where the candidate worktrees go; relative to the repo |
| `allow_dirty` | `false` | `--allow-dirty` | run on a dirty work tree (candidates start from HEAD) |
| `prune_branches` | `false` | none | delete each candidate branch at cleanup |
| `retain_transcripts` | `false` | none | keep copies of candidate transcripts (SEC-03) |

The worktree layout is in [command-flow.md](command-flow.md), "Where the
candidate worktrees go".

## `budget`

All amounts are whole micro-dollars (1 000 000 = $1).

| key | default | meaning |
|---|---|---|
| `hard_usd_micro` | `0` (no ceiling: PRE-09 refuses the run) | the hard ceiling; `--budget-usd` sets it in dollars. PRE-09 refuses a run whose projection plus the judge reserve is above it |
| `soft_usd_micro` | 80 % of the hard ceiling | PRE-09 warns when the projection plus the judge reserve is above it, and the run goes on; only the hard ceiling refuses |
| `judge_reserve_usd_micro` | 10 % of the hard ceiling | held back for the judge; the live limit is hard minus reserve |
| `expected_tokens.all` | none | the expected tokens of 1 candidate on any model |
| `expected_tokens.models.<model>` | none | the same for 1 roster model id, such as `sonnet` (exact match) |

Rules: the hard ceiling must be at least the soft limit, and the reserve
must be below the hard ceiling. With no ceiling, the soft limit and the
reserve must be 0 too.

Each `expected_tokens` entry has the kinds `input`, `cache_write_5m`,
`cache_write_1h`, `cache_read` and `output`. A kind left out is 0. An entry
must give at least 1 token. Without an entry, PRE-09 uses the measured usage
of at least 3 earlier candidates of the same task on the same model, else
the default (200 000 input, 3 000 000 cache read, 60 000 output tokens:
$1.60 on sonnet). The live budget meter projects each running candidate's
model with the same estimate (by model, not by label).

```yaml
budget:
  hard_usd_micro: 2000000
  expected_tokens:
    all: { input: 20000, cache_read: 400000, output: 8000 }
    models:
      opus: { input: 40000, cache_write_1h: 12000, cache_read: 800000, output: 16000 }
```

## `judge`

| key | default | meaning |
|---|---|---|
| `mode` | `auto` | the only mode: the judge runs when every candidate is frozen |
| `model` | `opus` | the judge model |
| `effort` | `high` | the judge effort |
| `timeout_s` | `900` | the judge job timeout |
| `policy.min_confidence` | `0.7` | a winner below this confidence needs the operator |
| `policy.tie_break` | `{mode: disabled}` | `disabled` (a tie needs the operator) or `{mode: utility, v: u1}` |

`policy` has no per-field default: give both fields or neither.

## `gates`

A list. Each gate:

| key | default | meaning |
|---|---|---|
| `name` | required | the gate name in logs and validation records |
| `command` | required | the shell command, not empty |
| `timeout_s` | required | greater than 0 |
| `required` | `true` | `false`: a failed gate does not make the candidate ineligible |

A candidate is eligible for the judge when every required gate passes. A gate
with `required: false` still runs and counts in the mechanical score (passed
gates / all gates). Promotion revalidates the integrated commit with the same
rule. Source: `CommandValidator::validate` in
`crates/horch-core/src/evaluation/validator.rs`.

```yaml
gates:
  - { name: test, command: cargo test, timeout_s: 600 }
  - { name: lint, command: cargo clippy -- -D warnings, timeout_s: 300, required: false }
```

## `caps`

| key | default | meaning |
|---|---|---|
| `candidate_deadline_s` | `3600` | a candidate still running then ends as `timed_out` |
| `max_parallel` | none | candidates at a time, at least 1; preflight's safe N is the lower of this and the CPU and memory bounds |
| `disk_headroom_bytes` | `10737418240` (10 GiB) | free disk kept after worktrees and builds; below it no new candidate starts |
| `log_cap_bytes` | `262144` | bytes per gate log in the preflight disk estimate |
| `output_cap_bytes` | `1048576` | bytes of output per candidate in the preflight disk estimate |
| `idle_nudge_after_s` | `120` | idle seconds without `horch done` before 1 nudge; 0 turns the idle rule off |
| `idle_end_after_s` | `180` | seconds after the nudge before a still-idle candidate ends; at least 1 while the nudge is on |
