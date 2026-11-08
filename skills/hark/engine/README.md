# hark engine

The Python package behind the `hark` command. The launcher `skills/hark/scripts/hark` runs it with the hark venv
python (`~/.local/state/hark/venv`, or `HARK_PYTHON`). The specs are in `docs/specs/hark/`. `ARCHITECTURE.md` there
fixes paths and contracts. The unit specs fix behaviour.

## Layout

| path | what |
|---|---|
| `hark/cli.py`, `hark/__main__.py` | the command table and dispatch |
| `hark/core/` | decode (ffmpeg), config, measurements and features, JSON and text reports |
| `hark/models/` | CLAP and AST loaders, the Hub pins, the ML install prompt (`require_ml`) |
| `hark/listen/` | describe, match, rank, find, compare, ab, realness, profile; `eval/` holds the held-out evaluation |
| `hark/timeline/` | timeline, sheet, critique |
| `hark/forge/engine/` | the forge: render, search, critic, polish, refsets |
| `hark/forge/recipes/<ui,impacts,motion,risers>/` | the synthesis recipes; their seed banks are in `hark/forge/seeds/` |
| `hark/forge/bank/` | the sound bank, kits and sfx commands; shipped bank files in `data/` |
| `hark/forge/kits/` | kit presets |
| `hark/fit/analysis/`, `hark/fit/pipeline/` | fit: analyse a recording and write a recipe model |
| `hark/surfaces/` | gallery, llm critique (agy), MCP server |
| `hark/doctor.py` | `hark doctor` |
| `hark/data/` | vocabulary, realness model, launch profile |
| `tests/<area>/` | tests per area: core, listen, timeline, forge, recipes_ui, recipes_impacts, recipes_motion, recipes_risers, bank, fit_analysis, fit_pipeline, surfaces |

## Install

- Base: `python3 -m venv ~/.local/state/hark/venv`, then install numpy, scipy, soundfile, soxr, librosa, pillow,
  matplotlib (versions in `pyproject.toml`). The launcher offers this when the venv is absent. ffmpeg and ffprobe
  must be on PATH.
- ML extras (describe, match, rank, find, compare, ab, realness, timeline, sheet, critique, profile):
  `<py> -m pip install --index-url https://download.pytorch.org/whl/cpu "torch>=2.6" && <py> -m pip install "transformers>=4.40"`.
  Then `hark doctor --fetch` downloads the pinned models to `<HARK_HOME>/hf/` (CLAP about 776 MB, AST about 346 MB).
  `hark doctor --load` times the model loads.
- MCP server: `<py> -m pip install "mcp>=2"`.

## Tests

Run from the repository root:

```
PYTHONPATH=skills/hark/engine ~/.local/state/hark/venv/bin/python -m pytest skills/hark/engine/tests -q -m 'not ml and not live'
```

- The whole run takes about 9 minutes. For one area, give its folder: `skills/hark/engine/tests/bank`.
- `-m ml` runs the tests that load CLAP or AST. They need the ML extras and the fetched models.
- `live` tests call an external program (agy). They run only with `HARK_LIVE=1`.
- Tests marked "needs-data" skip until their data exists, for example `HARK_EVAL_ASSETS` for the held-out
  evaluation sets.
- `tests/<area>/conftest.py` can install a small stand-in for a module of another area. A stand-in installs only
  when the real module is absent, so with the whole engine present every test runs against the real modules.

## Rebuild data

Run each command from `skills/hark/engine` with `PYTHONPATH=.` and the venv python. The plan for every asset is
`ARCHITECTURE.md` section 12.

| asset | command | notes |
|---|---|---|
| `hark/forge/bank/data/pluck.json` | `python -m hark.forge.bank build --pluck -o hark/forge/bank/data/pluck.json` | 3 voices |
| `hark/forge/bank/data/forge04.json` | `python -m hark.forge.bank build --forge04 -o hark/forge/bank/data/forge04.json` | Members that fail the take audit are logged and left out (exit 0). `--strict` refuses the file instead (exit 1). |
| `hark/forge/bank/data/forge_v2.json` | `python -m hark.forge.bank build --derive -o hark/forge/bank/data/forge_v2.json` | One entry per catalog id. An id with no named family member gets the closest render, and the log names it. The log also lists every measure outside the catalog tolerances. |
| `hark/forge/bank/data/fit.json` | `python -m hark.forge.bank add-fit <model> --id <id> --source <url>` per recording | Only after the operator allows the CC0 downloads. Until then the file is absent. |
| bank check | `hark kit init --preset <name> -o kit.json`, `hark kit check kit.json`, `hark kit render kit.json --out takes`, `hark sfx check takes/*.wav` | audit the presets and their takes |
| `hark/data/listen/vocab.json` | hand-written; check with `python -m hark.listen.vocabulary` | |
| `hark/data/listen/vocab_stats.json` | `python -m hark.listen.vocabulary --build-stats REF... -o <path>` | Not shipped. Without it the term bias is 0 and the logit scale comes from the model. |
| `hark/data/listen/realness_model.json` | `python -m hark.listen.realness --train <labelled dir> -o <path>` | The shipped file is the zero-shot fallback. |
| `hark/data/timeline/launch_profile.json` | `hark profile <films> -o <path>` | Use 5 or more reference films. |
| held-out evaluation | `python -m hark.listen.eval.run_eval --out <dir> {embed,stats,analyze,heldout,kind,find,timing}` | Needs `HARK_EVAL_ASSETS`. |
| forge benchmark | `python -m hark.forge.engine.eval.bench --out <dir>` | Needs a refs registry. |

After a bank rebuild, run the `bank` test area. The shipped-bank tests list the ids that are known to fail an audit
in `tests/bank/conftest.py` (`OPEN_FORGE04`, `OPEN_DERIVED`).
