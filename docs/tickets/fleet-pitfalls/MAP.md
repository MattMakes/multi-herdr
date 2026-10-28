# Map: fleet pitfalls
Spec: [#46](https://github.com/MattMakes/multi-herdr/issues/46) · the 33 pitfalls (P01-P33) from 8 fleet sessions, checked again against v0.3.0

The tickets live on GitHub as sub-issues of #46, labelled `ready-for-agent`. Each row links its
ticket file, so the whole map can be queued in one task-ledger run:

```bash
horch ledger add-map docs/tickets/fleet-pitfalls/MAP.md
```

## Tickets
| # | Issue | Title | Ticket | Blocked by | Wave |
|---|---|---|---|---|---|
| 01 | [#47](https://github.com/MattMakes/multi-herdr/issues/47) | Mailbox registry lives in the state folder (P09) | [01-mailbox-in-state-folder.md](01-mailbox-in-state-folder.md) | - | 1 |
| 02 | [#48](https://github.com/MattMakes/multi-herdr/issues/48) | horch inbox --roles, --peek, and mail printed last (P17) | [02-inbox-flags.md](02-inbox-flags.md) | - | 1 |
| 07 | [#49](https://github.com/MattMakes/multi-herdr/issues/49) | Stale horch warning and version at boot (P06) | [07-stale-horch-warning.md](07-stale-horch-warning.md) | - | 1 |
| 08 | [#50](https://github.com/MattMakes/multi-herdr/issues/50) | Spawn and fleet warn about a live orchestrator and high host load (P08, P15) | [08-live-orchestrator-and-load.md](08-live-orchestrator-and-load.md) | - | 1 |
| 09 | [#51](https://github.com/MattMakes/multi-herdr/issues/51) | sessions and context show only your workspace; multi-line summaries indented (P16, P21) | [09-sessions-scope.md](09-sessions-scope.md) | - | 1 |
| 10 | [#52](https://github.com/MattMakes/multi-herdr/issues/52) | Codex session found by a token unique to its launch (P10) | [10-codex-session-token.md](10-codex-session-token.md) | - | 1 |
| 11 | [#53](https://github.com/MattMakes/multi-herdr/issues/53) | Compact automatically when an over-threshold worker writes its handoff (P12) | [11-auto-compact.md](11-auto-compact.md) | - | 1 |
| 12 | [#54](https://github.com/MattMakes/multi-herdr/issues/54) | Doctor probes teammate services and warns about a proxy (P33, P24) | [12-doctor-probes.md](12-doctor-probes.md) | - | 1 |
| 13 | [#55](https://github.com/MattMakes/multi-herdr/issues/55) | Quota parsers read real replies (P26) | [13-quota-parsers.md](13-quota-parsers.md) | - | 1 |
| 14 | [#56](https://github.com/MattMakes/multi-herdr/issues/56) | horch note --fact and a host line in every brief (P20, P28) | [14-facts-and-host-line.md](14-facts-and-host-line.md) | - | 1 |
| 17 | [#57](https://github.com/MattMakes/multi-herdr/issues/57) | Orchestrator decisions log and a command card on resume (P22, P31) | [17-decisions-log-and-command-card.md](17-decisions-log-and-command-card.md) | - | 1 |
| 18 | [#58](https://github.com/MattMakes/multi-herdr/issues/58) | Teammates state their sandbox limits and pre-allow read-only commands (P04, P07) | [18-teammate-permissions.md](18-teammate-permissions.md) | - | 1 |
| 19 | [#67](https://github.com/MattMakes/multi-herdr/issues/67) | horch done never outlives the agent's command timeout (P18) | [19-done-report-cap.md](19-done-report-cap.md) | - | 1 |
| 20 | [#68](https://github.com/MattMakes/multi-herdr/issues/68) | The unregistered-worktree scan skips missing paths and other people's worktrees (P27, P30) | [20-unregistered-scan.md](20-unregistered-scan.md) | - | 1 |
| 21 | [#69](https://github.com/MattMakes/multi-herdr/issues/69) | Worktrees from a PR ref, scratch worktrees, and dropping a worktree (P02) | [21-worktree-from-scratch-drop.md](21-worktree-from-scratch-drop.md) | - | 1 |
| 22 | [#70](https://github.com/MattMakes/multi-herdr/issues/70) | The watchdog does not reap a worker that is waiting on reviewers or an answer (new in v0.3.0) | [22-watchdog-waiting-state.md](22-watchdog-waiting-state.md) | - | 1 |
| 03 | [#59](https://github.com/MattMakes/multi-herdr/issues/59) | Inbox shows worker status and open questions; horch peek (P03) | [03-inbox-status-and-peek.md](03-inbox-status-and-peek.md) | #48, #70 | 2 |
| 04 | [#60](https://github.com/MattMakes/multi-herdr/issues/60) | Worker mail is delivered until acknowledged (P01) | [04-delivered-means-acknowledged.md](04-delivered-means-acknowledged.md) | #47, #67 | 2 |
| 15 | [#63](https://github.com/MattMakes/multi-herdr/issues/63) | Worker briefing rules that keep workers moving (P07, P11, P14, P19, P23, P24) | [15-worker-briefing-rules.md](15-worker-briefing-rules.md) | #56, #70 | 2 |
| 05 | [#61](https://github.com/MattMakes/multi-herdr/issues/61) | Delivery tests at fleet start and in doctor (P01) | [05-delivery-tests.md](05-delivery-tests.md) | #60 | 3 |
| 06 | [#62](https://github.com/MattMakes/multi-herdr/issues/62) | Detect a logged-out orchestrator and alert the operator (P32) | [06-login-detection.md](06-login-detection.md) | #60 | 3 |
| 16 | [#64](https://github.com/MattMakes/multi-herdr/issues/64) | Orchestrator skill rules - honest reports, delegate, fan out (P05, P08, P13, P15, P25) | [16-orchestrator-skill-rules.md](16-orchestrator-skill-rules.md) | #59 | 3 |


## Critical paths
- #67 -> #60 -> #61, #62 (the tell and report path, in that order)
- #48, #70 -> #59 -> #64 (inbox status, then the skill that teaches it)
- #56, #70 -> #63 (the worker briefing)

## Pitfall coverage
Every pitfall P01-P33 has a ticket.

| Pitfall | Tickets |
|---|---|
| P01 messages to a Claude orchestrator lost | #60, #61 |
| P02 no PR-ref, scratch or drop worktrees | #69 |
| P03 the orchestrator can't see worker state | #59 |
| P04 Codex steps the sandbox can't do | #58 |
| P05 unverified claims reported as fact | #64 |
| P06 stale horch binary | #49 |
| P07 auto mode denies planned steps | #63, #58 |
| P08 no check for work in flight | #50, #64 |
| P09 mailbox registry in $TMPDIR | #47 |
| P10 Codex workers swap session ids | #52 |
| P11 a QUESTION blocks with no limit | #63, #70 |
| P12 compaction waits on a lost COMPACT-READY | #53 |
| P13 the orchestrator does worker work | #64 |
| P14 the no-background rule causes sleep-polling | #63, #70 |
| P15 parallel full test runs overload the host | #50, #64 |
| P16 sessions and context mix in other fleets | #51 |
| P17 inbox acknowledges piped output | #48 |
| P18 done hangs, then exits 137 | #67 |
| P19 context warning after the work is finished | #63, #53 |
| P20 every worker rediscovers the same facts | #56 |
| P21 multi-line DONE summaries show as 1 line | #51 |
| P22 the orchestrator re-proposes rejected designs | #57 |
| P23 a worker skipped a plan step | #63 |
| P24 the compression proxy cuts reads short | #63, #54 |
| P25 read-only fan-out under-parallelized | #64 |
| P26 the quota probe always says unknown | #55 |
| P27 the worktree scan fails on a deleted path | #68 |
| P28 GNU timeout and sed -i on macOS | #56 |
| P29 workers send DONE twice | #63, #64 |
| P30 done lists the operator's own worktrees | #68 |
| P31 the orchestrator forgets command forms | #57 |
| P32 a login expiry stalls the orchestrator | #62 |
| P33 Playwright MCP, Docker and sandbox outages | #54 |

## Handoff notes
- **Checked against v0.3.0 (2026-10-28).** Each issue has a "What v0.3.0 changed" section. The task ledger (#28, #65) fixed none of these pitfalls. It made #60 and #52 more urgent, and added the reap risk in #70.
- **The issues are the source of truth.** The ticket files here copy them at handoff, so that `add-map` can queue them. If an issue changes, update its file, or queue from the issue.
- **Needs the operator:**
  - #55 needs real quota replies captured on a logged-in machine.
  - #58's allow-list half needs a live check that Claude auto mode honors `--allowedTools`.
- **Personas:** every ticket commits code, and a Codex worker cannot write `.git`, so every primary and fallback is a Claude persona (#58).
- **Shared files:**
  - The briefing goldens are touched by #48, #56, #57, #58, #63, #64 and #69. Only #56 -> #63 and #59 -> #64 are ordered by blockers. A land that conflicts there is resolved by keeping both named blocks.
  - The orchestrate skill has 478 bytes left under its 12 KB budget. #48, #58 and #69 keep their skill edits to 1 line each. #64 owns the rest, and rewrites text rather than adding to it.
- **Tests:** each ticket's `**Check:**` runs only the tests for what it touches. Run the full gate (`HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate`) once, on the run branch, before the run's pull request merges.
