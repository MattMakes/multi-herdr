# Live check: telemetry on this Mac

The telemetry design's local acceptance steps L1-L10 ([§17 of
`specs/telemetry.md`](../specs/telemetry.md)) run against the real harnesses,
the real ledgers and the real herdr: the fixtures match reality, cost and usage
agree, the quota probes tell the truth, and the collector survives a crash.
Item U-06.

## How to run

```bash
scripts/live/telemetry.sh                                  # everything except L7
LIVE_TELEMETRY_KILL_COLLECTOR=1 scripts/live/telemetry.sh  # also L7
```

It needs `jq`, `perl`, `horch`, a running herdr server, and `just` (L10). A
missing tool is `SKIP`. It writes under `.worktrees/_scratch/live-telemetry/`
(a private `XDG_STATE_HOME` with a copy of the ledgers, the cost and usage
JSON, the perf log). A full run takes about 6 minutes.

Paid steps, each a 1-line task: **L6** starts 1 `sonnet` worker, **L8** starts 1
`researcher` worker that the gate substitutes with `codex-sol`. L3 and L4 run
the quota probes (no model session). **L7 sends `kill -9` to the shared live
collector** and restarts it with `horch telemetry ensure`, so it runs only with
`LIVE_TELEMETRY_KILL_COLLECTOR=1`.

How the steps run (they refine the wording of the design's table):

- L2 compares per record and per token class (usage `records[]` against cost
  `rows[]`), skips records whose transcript changed in the last 5 minutes, and
  runs twice: on a fresh private store (readers agree) and on the live store.
- L3 and L4 probe in the private state dir, because `horch quota --refresh` is a
  no-op while a collector is live. L4 isolates the probes: the Claude probe's
  cwd is `<tmp>/horch-quota-<pid>` and the Codex probe's cwd is the script's
  scratch dir, so other panes' transcripts do not count.
- L8 shifts the dated quota fixtures to the present. The committed
  `claude-exhausted-codex-ok.json` is dated 2026-09-28, so against today's clock
  its `7d 100%` window has already reset and the gate says `SPAWN`.
- L9 derives its expectation from the real pools (both are `ok` now; the
  design text expected `REFUSED` while both were exhausted until 2026-10-02) and
  drills `REFUSED` on the shifted `all-exhausted.json`.

## Steps the operator must do

**L3 probe truth.** The script prints the pool table and the numbers from
`horch quota --refresh`. In an interactive Claude session run `/usage`; in Codex
run `/status`. Every window must be within 1 percentage point, with the same
reset time (the horch table rounds the reset to the minute; `horch quota --json`
has the full timestamp). Record the result in the table below.

**L5 singleton in herdr** (it starts 2 orchestrators, which this check never
does):

1. `herdr workspace list`: note the count of workspaces labelled `horch telemetry`
   (expect 1), the focused pane, and the `pid` in
   `~/.local/state/horch/telemetry/collector.json`.
2. `mkdir -p ~/scratch-a ~/scratch-b`; in each, `git init && horch fleet`.
3. `herdr workspace list | grep -c 'horch telemetry'` is still 1; the focused
   pane did not change; the `collector.json` pid did not change.
4. Close both scratch fleets.

## Run 2026-10-06 (UTC 2026-10-07), first pass

Tools: horch 0.1.0 (installed 17:32 MST), claude 2.1.292, codex-cli 0.160.0,
herdr 0.8.2, pi 0.85.x, opencode, repo HEAD `4fd322f`. Paid: L6 (1 `sonnet-1`
worker, 1-line task), L8 (1 `researcher` worker run as `codex-sol`, 1-line task,
run twice: the first run's check was wrong, see L8).

| step | claim it proves | result | evidence |
|---|---|---|---|
| L1 fixture drift | the key paths each reader uses exist in real files | PASS | claude 11 of 11, codex 12 of 12, pi 4 of 4, opencode 6 of 6 key paths present. New real key paths not in the fixtures (listed by the script, not failed): claude 116-124, codex 65, pi 27, opencode 6. Prime: SKIP, no stable real session to sample. |
| L2 cost parity, fresh store | `horch usage` equals `horch cost` per record and per class (TEL-10) | PASS | 106 settled records equal on 5 token classes and on cost within 1e-6 USD; 15-16 records changed in the last 5 minutes were skipped |
| L2 cost parity, live store | the same on the collector's own store | **FAIL** | 18 of 106 settled records differ, 0 on tokens, all on cost: `usage` says 0.0 where `cost` says up to 0.92 USD (all `sonnet`). The store holds 1547 `claude-sonnet-5-5` and 221 `codex-auto-review` events with `cost_usd: null`. Cause: the collector pid 52443 started 2026-10-04 08:39 and runs a binary older than commit `5e6895e`, which priced Sonnet 5.5, and `usage` counts a null cost as 0.0. 75 null events are dated 2026-10-07. `codex-auto-review` has no price in `usage.rs`. Reported; unit W9 fixes it (read-time pricing, unpriced events listed, stale-collector restart). Re-run after W9. |
| L3 probe truth | the pool windows equal `/usage` and `/status` | SKIP (operator) | `horch quota --refresh` read: claude 5h 7%, 7d 50% (resets 2026-10-09T14:00Z), 7d fable 0%; codex 7d 39% (resets 2026-10-10T01:20Z). The operator compares with `/usage` and `/status` (procedure above). |
| L4 probe hygiene | the probes write no transcript | PASS | 3 refreshes, both probes read (claude `get_usage`, codex `app-server`); 0 new transcript dirs for the Claude probe cwd (`horch-quota-*`), 0 new Codex rollouts in the probe cwd |
| L5 singleton in herdr | 1 collector workspace, focus unchanged, same pid | SKIP (operator) | needs 2 new fleets; procedure above |
| L6 live latency | a worker's row appears within 5 s of its first response | **FAIL** | first store event for the new `sonnet-1` record seen 7.7 s after the first assistant line (limit 5 s, poll step 0.5 s). Spec §7.1 holds a Claude message until a different `message.id` arrives or the file is quiet for 2 ticks, so about 4 s of the delay is by design. Re-run after W9 and W3d. |
| L6 totals | store totals equal the transcript totals | PASS | `{"input":2,"cache_write_1h":10896,"cache_read":12267,"output":87}` on both sides |
| L7 crash recovery | no loss, no duplicates after `kill -9` | SKIP | opt-in: it kills the shared collector. The classifier refused the first full run. Run with `LIVE_TELEMETRY_KILL_COLLECTOR=1` after W9 and W3d land; the restart also moves the collector to the new binary. |
| L8 gate drill | the gate substitutes `codex-sol` and the record shows it | PASS | on the dated fixture shifted to now: `SUBSTITUTED: researcher runs on codex-sol`; record `researcher-13` has agent `codex`, `routing.resolved: codex-sol`, store events with `via: codex-sol`; the worker reported `DONE: done` at 01:18:40Z. A refusal drill on the shifted `all-exhausted.json` gave `REFUSED` with 2 reset times. The first pass FAILed because the script looked for `via` on the ledger record; the ledger holds `routing.resolved`, and `via` is on the events (the script is fixed). |
| L9 real gate | the gate follows the real pools | PASS | both pools `ok`: `SPAWN: opus runs as itself` |
| L10 perf, synthetic 1 GB | cold start under 30 s, steady tick under 200 ms | not run | `just verify-perf` did not compile: another worker's edit in `crates/horch-core/src/harness/opencode.rs:220` calls `Session::is_resume`, which does not exist. Re-run after W3d. |
| L10 collector RSS | under 150 MB | PASS | live collector 141.2 MiB (148.1 MB) after 2 days 9 hours on a 58 MB store. 1.9 MB under the limit in decimal MB, and a one-shot `collect --once` on the same store peaks at 190 MiB. |

Open after this pass: L3 and L5 need the operator; L2 (live store), L6, L7 and L10 re-run
after W9 and W3d (next table).

## Run 2026-10-06 (UTC 2026-10-07), after W9, W10 and W3d

Tools: horch 0.1.0 at `b322192` or later (`just install` ran; collector pid 55750
on the new binary, `exe` recorded in `collector.json`), claude 2.1.292,
codex-cli 0.160.0, herdr 0.8.2, repo HEAD `1b58733`. Paid: L6 (1 `sonnet-3`
worker), L8 (1 `researcher-*` worker run as `codex-sol`), both 1-line tasks. L7
ran with `LIVE_TELEMETRY_KILL_COLLECTOR=1`.

| step | claim it proves | result | evidence |
|---|---|---|---|
| L1 fixture drift | the key paths each reader uses exist in real files | PASS | claude 11 of 11, codex 12 of 12, pi 4 of 4, opencode 6 of 6. Prime: SKIP. |
| L2 cost parity, fresh store | `usage` equals `cost` per record and class (TEL-10) | PASS | 130 settled records equal on 5 token classes and cost within 1e-6 USD; 3 live records skipped |
| L2 cost parity, live store | the same on the collector's own store | PASS | 130 of 130 settled records equal. The first pass found 18 differing records (null `cost_usd` read as 0.0); W9's read-time pricing removed them. |
| L3 probe truth | the pool windows equal `/usage` and `/status` | SKIP (operator) | `horch quota --refresh` read: claude 5h 17%, 7d 53% (resets 2026-10-09T14:00Z), 7d fable 0%; codex 7d 43% (resets 2026-10-10T01:20Z). Compare with `/usage` and `/status` as above. |
| L4 probe hygiene | the probes write no transcript | PASS | 3 refreshes, both probes read; 0 new probe transcripts |
| L5 singleton in herdr | 1 collector workspace, focus unchanged, same pid | SKIP (operator) | needs 2 new fleets; procedure above |
| L6 live latency | a worker's row appears within 5 s of its first response | **FAIL** | first store event for `sonnet-3` seen 7.7 s after the first assistant line; the first pass gave 7.7 s too. The delay is stable, so it comes from the design: spec §7.1 holds a Claude message until a different `message.id` arrives or the file is quiet for 2 ticks (2 x 2 s), plus the next tick. Either the L6 limit or the hold rule must change; a decision for the operator. |
| L6 totals | store totals equal the transcript totals | PASS | `{"input":2,"cache_write_1h":10116,"cache_read":12267,"output":87}` on both sides |
| L7 crash recovery | no loss, no duplicates after `kill -9` | PASS | `kill -9` on pid 55750, then `horch telemetry ensure`: new collector pid 71519; totals of the L6 record unchanged; duplicate event keys 0 before and 0 after; events 111406 to 111435 (no loss). Observation: `herdr workspace list` then held 2 workspaces labelled `horch telemetry` (w34 and w3M). w34 is the original one: its pane is a shell at a prompt after `horch telemetry`, whose collector was replaced earlier; nothing closes it. L5 expects 1. |
| L8 gate drill | the gate substitutes `codex-sol` and the record shows it | PASS | `SUBSTITUTED: researcher runs on codex-sol`; the record has agent `codex`, teammate `researcher`, `routing.resolved: codex-sol`; store events carry `via: codex-sol`. `REFUSED` with 2 reset times on the shifted all-exhausted fixture. |
| L9 real gate | the gate follows the real pools | PASS | both pools `ok`: `SPAWN: opus runs as itself` |
| L10 perf, synthetic 1 GB | cold start under 30 s, steady tick under 200 ms | PASS | cold 16.2 s, steady 198.1 ms: 1.9 ms under the limit, so a slower host fails it |
| L10 collector RSS | under 150 MB | **FAIL** | the new collector (pid 71519) holds 153.9 MiB (161.4 MB) 3 minutes after the start and grows to 157.6 MiB after 4 minutes, on a 60 MB store of 111 571 events. The old binary held 141.2 MiB after 2 days on a 58 MB store. Reported as a product defect (NFR-02 RSS); not fixed here. |
