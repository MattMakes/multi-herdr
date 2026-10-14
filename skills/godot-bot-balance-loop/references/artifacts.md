# Bot balance loop: more artifacts

This file is part of `godot-bot-balance-loop`. `SKILL.md` lists the core artifacts. These artifacts serve the
shared pipeline, wall triage, the full-run bot and telegraph parity.

- **Shared skill draw**: 1 seeded module that adds misses, used by every tool.
- **Tool generator**: copies peer tools and applies shared patches by asserted exact-match replacement.
- **Row setter**: edits 1 data row by unique text, asserts 1 match, re-exports, and restores on exit with a `trap`.
- **Wall finder**: replays a snapshot as a baseline, then with +1 level per upgrade, and ranks the probes.
- **Full-run bot and segment driver**: real-input play with snapshots, a state file, resume and stop flags.
- **Findings format**: kind (GAMEPLAY, BALANCE, PERF, BUG, POLISH), evidence, 2 or 3 options, a recommendation.
- **Proposal file**: before and after tables per row, the lever, the seeds and the log tags.
- **Telegraph parity test**: per attack, record telegraph and hit, and assert kind, centre and size agree.
