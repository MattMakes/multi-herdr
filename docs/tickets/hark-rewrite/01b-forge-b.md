# 01b: Rewrite forge fx, viz, schema, tonal below the similarity threshold

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** yes
**Kind:** refactor
**Review:** none
**Check:** `PYTHONPATH=skills/hark/engine /Users/mascott/.local/state/hark/venv/bin/python -m pytest skills/hark/engine/tests/forge -q -m 'not ml and not live'`
**Own:** `skills/hark/engine/hark/forge/engine/fx.py`, `skills/hark/engine/hark/forge/engine/viz.py`, `skills/hark/engine/hark/forge/engine/schema.py`, `skills/hark/engine/hark/forge/recipes/tonal.py`

## Goal

Every code file in your Own paths scores under 30 % token similarity and has no run of 5 or more identical non-trivial lines against the source, with behaviour unchanged.

**Side: clean.** Never open the source, its repo, or `~/.local/state/hark-cleanroom/src-*`. Rules: `/Users/mascott/projects/multi-herdr/ai_docs/plans/hark/CLEANROOM.md`.

## Context

- The similarity gate (T-39) wrote `docs/specs/hark/SIMILARITY.md`. Unrelated code scores at most 24 %; the threshold is 30 %. Read the rows for your files: token %, identical runs, shared block line ranges (our lines only).
- The overlap comes mostly from shared non-public identifiers, local names, comment wording and statement order that the specs carried over. Public names stay: CLI commands, flags, JSON field names, and the function signatures in `docs/specs/hark/ARCHITECTURE.md`.
- Orchestrator decisions: JSON data assets are accepted as facts (do not edit JSON); prose is judged by copied lines and passes. Only code files are in scope.
- The operator wants speed: work file by file, worst first.

## Prior work

T-50 (worker opus-87) rewrote the other forge engine files (dsp.py 29.2 %, tone.py 29.4 %, bit-identical) and landed them. Read its summary: `horch ledger show T-50`. Its reusable tools are in `/Users/mascott/.local/state/hark-cleanroom/rewrite-tools/` (rename.py, rebind.py, npimport.py, perfn.sh, m.sh, harness_*.py); read each tool's top comment before use. They are our own tools, not source.

Your files only: engine/fx.py, engine/viz.py, engine/schema.py, recipes/tonal.py. 2 other workers rewrite the other remaining forge files in parallel.

## Steps

1. List your files from SIMILARITY.md with their scores, worst first. Measure only your files with the gate's oracle: `python3 /Users/mascott/.local/state/hark-cleanroom/gate-tools/measure.py --ours <file>` (if its options differ, read only its `--help`; never open its source paths). It prints numbers and our line ranges only.
2. For each file: rename non-public identifiers, restructure (split, merge, reorder helpers), rewrite comments and docstrings in your own words, and remove any 5+ line run. Keep every public name and signature. Behaviour must not change: renders, goldens and JSON outputs stay bit-identical; run the area tests after each file.
3. Re-measure each file. Stop on a file when it is under 30 % with no 5-line run.
4. Run the check, and the complexity sweep with the installed horch on your changed functions (0 breaches).
5. Commit after each file or 2 (exact paths). Run `horch ledger finish`, then `horch ledger done` with before/after scores per file.

## Done when

- Every code file in your Own paths is under 30 % with no 5-line run, or the DONE report names each file still over and why (for example a public signature list).
- The check passes.
