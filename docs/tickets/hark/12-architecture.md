# 12: Architecture: module map, contracts, venv and test plan of hark

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** yes
**Kind:** docs
**Review:** none
**Check:** `test -s docs/specs/hark/ARCHITECTURE.md && /Users/mascott/.local/state/hark/venv/bin/python -c 'import numpy, scipy, soundfile'`
**Own:** `docs/specs/hark/ARCHITECTURE.md`, `skills/hark/engine/pyproject.toml`, `skills/hark/engine/hark/__init__.py`, `skills/hark/engine/tests/conftest.py`

## Goal

`docs/specs/hark/ARCHITECTURE.md` fixes every name and contract that 2 or more implementers share, so that 11 implementers build hark in parallel and their parts fit at the first try. The base venv exists.

**Side: clean.** Read only the specs and the repo.

## Context

- The operator wants a sound engineer for video SFX and game SFX. The fleet rebuilds an open-source sound tool for coding agents, clean-room, as our own engine `hark`, plus 2 skills and new teammates.
- Read the clean-room rules first: `/Users/mascott/projects/multi-herdr/ai_docs/plans/hark/CLEANROOM.md`. They are absolute. Your side is named below.
- Read the decisions: `/Users/mascott/projects/multi-herdr/ai_docs/plans/hark/decisions.md`.
- The package is `hark`. Its engine lives at `skills/hark/engine/hark/`. Never use the source's names for our things.

## Inputs

- `docs/specs/hark/core-io.md` (and its parts)
- `docs/specs/hark/listen.md` (and its parts)
- `docs/specs/hark/timeline.md` (and its parts)
- `docs/specs/hark/forge-core.md` (and its parts)
- `docs/specs/hark/recipes-impacts.md` (and its parts)
- `docs/specs/hark/recipes-motion.md` (and its parts)
- `docs/specs/hark/bank-kit-sfx.md` (and its parts)
- `docs/specs/hark/fit.md` (and its parts)
- `docs/specs/hark/surfaces.md` (and its parts)
- `docs/specs/hark/skills-and-docs.md` (and its parts)
- `docs/specs/hark/agy-audio-probe.md` if it exists.

## Fixed layout (do not change)

- `skills/hark/engine/hark/core/` audio I/O, config, features, report, api; `hark/cli.py`, `hark/__main__.py`, `hark/doctor.py`, `hark/models/` (model hub, CLAP, AST, the optional-install prompt).
- `hark/listen/` describe, match, rank, find, compare, realness, profile, vocabulary; data in `hark/data/listen/`.
- `hark/timeline/` timeline, sheet, video, critique; data in `hark/data/timeline/`.
- `hark/forge/` forge engine: `hark/forge/__init__.py`, `hark/forge/engine/`, `hark/forge/recipes/__init__.py` (registry), `hark/forge/recipes/tonal.py`.
- `hark/forge/recipes/impacts/` and `hark/forge/seeds/impacts/`; `hark/forge/recipes/motion/` and `hark/forge/seeds/motion/`.
- `hark/forge/bank/` bank, sfx, kit and their data; `hark/forge/kits/`.
- `hark/fit/` audio to recipe code.
- `hark/surfaces/` gallery, MCP server, the agy critique.
- Tests: `skills/hark/engine/tests/<area>/` with areas core, listen, timeline, forge, recipes_impacts, recipes_motion, bank, fit, surfaces.
- Launcher: `skills/hark/scripts/hark`.

## Steps

1. Read every spec. List every cross-unit call and data flow.
2. Write ARCHITECTURE.md: for each module, its public functions and classes with exact signatures and types, its data files and their schemas, and which module may import which (no cycles). Fix the recipe contract, the parameter schema, the audio buffer type (numpy float32 mono or stereo, sample rate), the result/report object, the CLI command table (command, module, function), the MCP tool table, and the exit codes.
3. Fix the optional ML rule: a function `hark.models.require_ml(feature)` raises a typed error, and the CLI turns it into the install prompt (TTY: ask y/N, then pip install the ML extras into the hark venv; non-TTY: exit 3 with the exact command). Base deps load lazily so `measure`, `sfx`, `kit` and forge start fast.
4. Fix the data asset plan: for each asset of the specs, the module that rebuilds it, the command that rebuilds it, and the fallback for NEEDS-DATA assets.
5. Write `pyproject.toml` (base deps; extras `ml` = torch CPU + transformers, `mcp`, `dev` = pytest), `hark/__init__.py` (version only), `tests/conftest.py` (shared fixtures: synthetic tones, temp dirs via tmp_path, a marker `ml` that skips when torch is absent).
6. Create the base venv: `python3 -m venv /Users/mascott/.local/state/hark/venv`, then install the base deps and pytest into it. Do not install torch. Record the versions in ARCHITECTURE.md.
7. Write the test plan: per area, the acceptance tests of its spec that become pytest cases.
8. Commit only the Own paths. Run `horch ledger finish`, then `horch ledger done`.

## Done when

- ARCHITECTURE.md covers steps 2-4 and 7 for every spec unit.
- The venv imports numpy, scipy and soundfile.

