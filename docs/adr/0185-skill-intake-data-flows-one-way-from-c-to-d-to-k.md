# skill-intake data flows one way from C to D to K

skill-intake roles pass data in 1 direction, C to D to K, and each role's
sandbox writes only its own side. `skills/skill-intake/scripts/intakelib/roles.py`
`STATUS_SIDE` puts the analyst's status file in D although its brief is in
C, because the analyst's sandbox writes only D.

## Consequences

- When `roles` re-spawns a lost fleet worker, the first attempt's domain
  snapshot stays and is passed to the new attempt, because `role-result`
  checks the writes of both workers against it.
- `intakelib/verify.py` `run()` rewrites `K/fixes.json` on every verify
  run, pass or fail, so a stale fix list never outlives the verify that
  made it.
