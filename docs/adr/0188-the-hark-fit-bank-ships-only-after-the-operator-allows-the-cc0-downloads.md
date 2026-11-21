# The hark fit bank ships only after the operator allows the CC0 downloads

The hark bank file `fit.json` ships only after the operator allows the 15
CC0 source downloads (ARCHITECTURE 15, bank Q5). Until then only
`forge04.json`, `forge_v2.json` and `pluck.json` ship.

## Consequences

- No test requires `fit.json`. `skills/hark/engine/tests/bank/conftest.py`
  expects the other 3 files, and `test_bank.py` `test_at_1` checks
  `fit.json` entries only when the file is present.
