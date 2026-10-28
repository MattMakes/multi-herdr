# Map: the task ledger
Spec: [#28](https://github.com/MattMakes/multi-herdr/issues/28) · design: [docs/specs/task-ledger.md](../../specs/task-ledger.md)

The tickets live on GitHub as sub-issues of #28, labelled `ready-for-agent`.

## Tickets
| # | Issue | Title | Blocked by | Wave |
|---|---|---|---|---|
| 01 | [#29](https://github.com/MattMakes/multi-herdr/issues/29) | Exact process identity on Windows | - | 1 |
| 02 | [#30](https://github.com/MattMakes/multi-herdr/issues/30) | The ledger store and `horch ledger status` | - | 1 |
| 03 | [#31](https://github.com/MattMakes/multi-herdr/issues/31) | Read the agent's working timer | - | 1 |
| 04 | [#32](https://github.com/MattMakes/multi-herdr/issues/32) | Runs, queueing and claiming | #30 | 2 |
| 06 | [#34](https://github.com/MattMakes/multi-herdr/issues/34) | Workers record their PIDs, session and checkin | #30 | 2 |
| 05 | [#33](https://github.com/MattMakes/multi-herdr/issues/33) | Queue a ticket map, plan workers, fallback, cancel | #32 | 3 |
| 07 | [#35](https://github.com/MattMakes/multi-herdr/issues/35) | Stop by recorded PIDs, ban greedy kills | #29, #34 | 3 |
| 08 | [#36](https://github.com/MattMakes/multi-herdr/issues/36) | Finish pipeline core | #32 | 3 |
| 09 | [#37](https://github.com/MattMakes/multi-herdr/issues/37) | PR finish step | #36 | 4 |
| 10 | [#38](https://github.com/MattMakes/multi-herdr/issues/38) | Complexity-sweep finish step | #36 | 4 |
| 11 | [#39](https://github.com/MattMakes/multi-herdr/issues/39) | Code-sweep finish step via reviewer tasks | #32, #36 | 4 |
| 12 | [#40](https://github.com/MattMakes/multi-herdr/issues/40) | Progress notes and orchestrator wakes | #34, #36 | 4 |
| 13 | [#41](https://github.com/MattMakes/multi-herdr/issues/41) | Reap and replace | #32, #35, #36 | 4 |
| 15 | [#43](https://github.com/MattMakes/multi-herdr/issues/43) | Import and `ledger unfinished` | #32, #36 | 4 |
| 14 | [#42](https://github.com/MattMakes/multi-herdr/issues/42) | The watchdog | #31, #34, #41 | 5 |
| 16 | [#44](https://github.com/MattMakes/multi-herdr/issues/44) | Briefings switch | #33, #37-#40, #42, #43 | 6 |
| 17 | [#45](https://github.com/MattMakes/multi-herdr/issues/45) | End-to-end run | #44 | 7 |

## Critical path
#30 -> #34 -> #35 -> #41 -> #42 -> #44 -> #45
