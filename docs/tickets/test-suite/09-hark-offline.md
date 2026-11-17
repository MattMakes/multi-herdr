# 09: The hark test suite never reaches the network

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cd skills/hark/engine && env -u HF_HUB_OFFLINE PYTHONPATH=. /Users/mascott/.local/state/hark/venv/bin/python -m pytest tests/timeline/test_critique.py -q -m 'not ml and not live'`
**Own:** `skills/hark/engine/tests/conftest.py`, `skills/hark/engine/tests/timeline/test_critique.py`

## Goal

`test_c11_input_errors` and every other non-`live` hark test pass with no network access and no `HF_HUB_OFFLINE` set by the caller.

## Context

- T-48 (opus-84) found that `tests/timeline/test_critique.py::test_c11_input_errors` calls the Hugging Face Hub on every run. The Hub's progress output goes to stderr and breaks the test's exact-stderr assertion. T-48 made `scripts/phase-gate.sh` set `HF_HUB_OFFLINE=1`, which hides the problem only inside the gate.
- Test-suite goal from the operator: faster and more reliable tests. A unit test that touches the network is unreliable.

## Steps

1. Find out which call reaches the Hub (model lookup, `huggingface_hub` import side effect, or a model download in an input-error path). Write the cause in DONE.
2. Make the suite hermetic in `tests/conftest.py`: an autouse fixture (or a session-start setting) that sets `HF_HUB_OFFLINE=1` and `TRANSFORMERS_OFFLINE=1` for tests not marked `live` or `ml`. If the input-error path should not load a model at all, say so in DONE as a product follow-up; do not edit product code.
3. Keep the exact-stderr assertion; it is the useful part of the test.
4. Run the Check, then the full non-ml hark suite with `-n auto` if pytest-xdist is installed in the venv.

## Done when

- The Check passes without `HF_HUB_OFFLINE` in the environment.

## Report

Commit only your exact files. Run `horch ledger finish`, then `horch ledger done` with: the cause, the fix, and the test counts.
