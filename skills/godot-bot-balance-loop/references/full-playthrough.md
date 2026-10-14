# Bot balance loop: full playthrough bot

This file is part of `godot-bot-balance-loop`. Read it when you need a bot that plays the whole game from a fresh save.

A full-run bot answers "does the whole game play from a fresh save to the end, and where does a strong player
stall?" before a human sees it.

1. Read the existing autoplay and fight bot code and reuse their steering. Run the import in a new worktree first; a
   worktree with no class cache can hang the tool at the autoload stage.
2. Drive the real flow (title, run, results, shop, next run) with real input actions, not by setting positions. List
   each place where the tool must call an API in place of an input. Never force an encounter, a level or a resource.
3. Write an explicit strong-player policy: route, encounter line, shop rule, next-level pick and a farm rule (for
   example farm a beaten level after a bad boss try). Turn on helpers that script mode turns off, such as a coach.
4. Add options: fresh or mid-game start, a seed, captures, and a measure mode with no captures. Captures stall
   frames and distort frame times.
5. Log 1 JSON line per run: level, run time, end reason, pay per currency, buys, encounter result and errors. Log
   every wait and fight end with its cause.
6. Save a snapshot plus the tool state at each chapter start and before each encounter try. Add a resume option that
   only the first segment reads, and stop flags (stop at encounter X after N tries).
7. Split play into segments under about 9 minutes with a driver that switches by exit code. A segment restart acts
   as a game restart, so per-process statics reset.
8. Do a short windowed check (3 runs and 1 shop visit) and a fast headless dry run to the end before the official run.
9. Watch the official run with a filtered log monitor that reports only chapter ends, softlocks, script errors,
   refused buys and driver failures. Detect a stuck state and report it; do not hang.
10. At a wall (tries over the limit, power flat), stop and report the tries, the power and the unspent bank. Then use
    `references/wall-triage.md`. Runs after a bot wall are over-levelled and are not balance evidence.
11. Write chapter-end tables (runs, minutes, tries per encounter, power at the meeting) and a findings file. Label
    each item GAMEPLAY, BALANCE, PERF, BUG or POLISH, with evidence, 2 or 3 options and a recommendation. Do not fix
    the items in the same unit.
