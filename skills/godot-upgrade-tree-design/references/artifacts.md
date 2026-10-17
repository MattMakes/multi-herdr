# Artifacts to build

This file belongs to the `godot-upgrade-tree-design` skill. The terms (diag tree, chapter, class, policy) are defined in its SKILL.md.

- **Diag tree generator**: reads the shipped tree data and writes the hub data file; keeps the `price` block; the data
  export runs it; after a cull it writes the culled tree as active and the full tree beside it.
- **Diag runtime**: 1 global object with the API of `contract-template.md`; its own save and state file.
- **2 hooks**: 1 in the stat pipeline, 1 guard in the shipped shop; plus a test that diag off equals shipped stats.
- **Bot policy flag and snapshot flags**: `--diag-policy`, a snapshot per chapter start, per boss try, per shop.
- **Sweep driver**: plays N policies with own slots, out folders and traces; supports a tag, resume, run cap, seed.
- **Calibration driver**: tries per chapter, run and time caps, invalid on a script error, a table of OK, FAST, WALL.
- **Policy files and generator**: JSON per policy (contract section 5); `make_policies.py` from `script-make-policies.md` writes the set (copy the block into a file).
- **Report**: `report.py` from `script-report.md`, with the fixtures and tests of `script-test-report.md` (copy each block into a file); writes `report.md`, `report.html`, `balance.json`.
- **Wall finder**: replays a boss try per spoke with +1 and +3; writes `wall.json`.
- **Value bot driver**: pure scoring functions with tests; replays per candidate; writes the buy positions.
- **Tuning script and fun band file**: configs x panel x seeds; resume; a score-only mode; JSON of target bands.
- **Playstyle report**: chapter minutes, failed boss tries, tries at the hardest boss, stall runs per trace.
- **Wing audit checklist**: node, stat, readers, opening run, cost in runs, dead or felt, capture pair.
- **Economy target checks**: T1 to T3 as tests in the balance model.
- **Cue table**: stat key to cue, plus 1 anchor-point method per class model.
- **Telemetry logger, report and protocol**: JSON lines; per-tester answers; observer sheet; readiness gate list.
- **Decision file**: numbered items with facts, evidence, options, recommendation, and the owner's answer verbatim.
