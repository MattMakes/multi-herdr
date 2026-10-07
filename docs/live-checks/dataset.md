# Live check: the dataset idle rule on a Codex candidate

A real Codex candidate that stops at its prompt without `horch done` gets one
nudge, then ends as `Cancelled{idle_without_done}` (dataset design
[§4.11.5](../specs/dataset-competition.md), F5). Item U-45.

## How to run

```bash
scripts/live/dataset.sh
```

It needs `multi-herdr-dataset`, `herdr` (a running server), `codex` and `jq`.
It starts 1 paid candidate: `codex-luna` with a 1-line task that forbids
`horch done` and commits, budget ceiling 2 USD (projection 0.01 USD). It ends
by itself in about 2 minutes when the rule works, and at the 300 s deadline
when it does not. A missing tool is `SKIP`.

The script builds a fresh git repo in `.worktrees/_scratch/live-dataset/`
with a `.multi-herdr/dataset.yaml` (`candidates: 1`, `baseline: codex-luna`,
nudge after 40 s, end 40 s later, deadline 300 s). It samples the candidate
pane's `agent_status` every 3 s into `.worktrees/_scratch/live-dataset-run/panes.log`.
The coordinator keeps its events under
`~/.local/state/horch/multi-herdr/<project slug>/events/`.

### The Codex trust entry this check needs

Codex asks for trust per git root, and the scratch repo is its own root, so
preflight PRE-14 refuses the run until `~/.codex/config.toml` holds this entry:

```toml
[projects."/Users/mascott/projects/multi-herdr/.worktrees/_scratch/live-dataset"]
trust_level = "trusted"
```

The script prints `SKIP` with these lines when the entry is missing. Added on
2026-10-06; the file as it was before is in
`.worktrees/_scratch/live-dataset/codex-config.toml.bak` (git-ignored).
To remove it, delete those 2 lines from `~/.codex/config.toml`. Nothing else
depends on them.

| step | claim it proves | what the script checks |
|---|---|---|
| D2 one nudge | `IdleWatch` types `IDLE_NUDGE` once after `idle_nudge_after_s` | stderr names the nudge once; the candidate's Codex rollout holds the nudge line once, after about 40 s idle |
| D3 idle_without_done | a nudged candidate that stays idle ends as `Cancelled{idle_without_done}` | `candidate.failed` carries `idle_without_done`, not `timed_out` |

No event records the nudge (the event formats do not change), so D2 reads
stderr and the Codex rollout, which holds what arrived in the pane.

## Run 2026-10-06 (UTC 2026-10-07)

Tools: codex-cli 0.160.0, herdr 0.8.2, multi-herdr-dataset 0.1.0, horch 0.1.0,
repo HEAD `4fd322f`. Cost: 1 `codex-luna` session with a 1-line task (about
0.01 USD projected); the script ran twice, and a first manual run of the same
task gave the same result (3 candidates in all, all `timed_out`).

| step | result | evidence |
|---|---|---|
| D2 one nudge | PASS | stderr: `candidate A waits at its prompt without horch done; nudging it once` (1 line). The rollout holds 1 `If you are finished, run: horch done ...` user line. The candidate answered READY and went idle at 01:10:32; the nudge arrived at 01:11:17 (45 s later; the period is 40 s and the look interval adds the rest). Codex answered READY again 2 s later. |
| D3 idle_without_done | **FAIL** | `candidate.failed {"kind":"timed_out"}` at 01:15:21, exactly the 300 s deadline after `candidate.spawned` at 01:10:21. Expected `Cancelled{idle_without_done}` about 40 s after the nudge. |

Cause, from `panes.log` (herdr `agent_status` of the candidate pane, every 3 s):

```
01:10:23 unknown (2 samples)  01:10:29 working (3)  01:10:39 idle (13)
01:11:19 done (76 samples, to 01:15:19)             01:15:22 gone
```

Before the nudge the pane reports `idle`. After the nudge turn herdr reports
`done`, and it never goes back to `idle`. `IdleWatch::look`
(`crates/horch-core/src/competition/observe.rs`) takes `idle` as
`agent_status == "idle"`, so after the nudge no idle stretch starts and the end
rule never fires. The candidate holds its slot until the deadline, which is the
failure F5 was meant to remove (the LA run: a Codex candidate that could not
commit waited 900 s).

Status: reported to the orchestrator as a product defect on 2026-10-06. This
file keeps the FAIL until the rule counts `done` after a nudge; then re-run
`scripts/live/dataset.sh` and append the new table here.
