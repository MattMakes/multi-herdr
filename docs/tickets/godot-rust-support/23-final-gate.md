# 23: Full gate on the run branch

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus` · **Parallel:** yes
**Kind:** report
**Check:** `just gate`

Spec: `docs/specs/godot-rust-support.md` (Requirement Table and Further Notes).
Waits for: 03 (complexity-sweep: GDScript route through gdcomplexity.py); 09 (godot-native-engineer owns gdext, and rust-gdext.md is complete); 11 (code-sweep and comment-sweep know Godot, C# and Rust); 12 (Suggest the default check for the project stack); 13 (Measure worktree build and import cost, then seed the Godot cache); 14 (Codex panes get a writable CARGO_HOME); 15 (Close the godot-run.sh Codex pane live check); 16 (Correct godot-perf-measure and the percentile convention); 18 (Correct godot-optimization physics and pooling claims); 19 (Correct godot-multithreading thread and loading claims); 22 (hfr_bench.py, and wire godot-hfr-hunt to the performance engineer). Wave 8.

## Goal

The run branch passes the full gate with every GRS row covered.

## Steps

1. Run `just gate` on the run branch.
2. Run `scripts/check-req-coverage.sh GRS-01 GRS-02 GRS-03 GRS-04 GRS-05 GRS-06 GRS-07 GRS-08 GRS-09 GRS-10 GRS-11 GRS-12 GRS-13 GRS-14 GRS-15 GRS-16 GRS-17 GRS-18 GRS-19 GRS-20 GRS-21 GRS-22 GRS-23 GRS-24 GRS-25 GRS-26 GRS-27 GRS-28 GRS-29 GRS-30 GRS-31 GRS-32 GRS-33 GRS-34 GRS-35 GRS-36 GRS-37 GRS-38 GRS-39 GRS-40 GRS-41 GRS-42 GRS-43 GRS-44` and confirm 0 missing.
3. Report each failure to the orchestrator with the ticket that owns the file. Do not fix it here.
4. Run `horch ledger finish`. When every step passes, run `horch ledger done "<summary>"`.

## Done when

`just gate` passes, or the orchestrator has a list of each failure and its owning ticket.
