# 07: Stale horch warning and version at boot (P06)

**Issue:** [#49](https://github.com/MattMakes/multi-herdr/issues/49) · **Spec:** [#46](https://github.com/MattMakes/multi-herdr/issues/46)
**Persona:** primary `backend-developer`, fallback `opus` · **Parallel:** yes
**Check:** `cargo build --workspace --bins && cargo test -p horch selfupdate && cargo test -p horch-e2e --test update`

The GitHub issue is the source of truth. This file is its copy at handoff (2026-10-28), so that `horch ledger add-map` can queue it.

## Parent

#46

## What v0.3.0 changed (checked 2026-10-28)

- **Still needed as written.** The self-update notice is unchanged:
  - It still returns early when stderr is not a terminal, on `--no-update`, or with the agent-CLI update setting off.
  - It is called only from `doctor` and before a fleet starts.
  - Nothing compares the running binary with the installed one, and the boot does not print the version.
- **Silence switch:** `HORCH_NO_SELF_CHECK` already exists. Reuse it, and do not add a new one.
- v0.2.1's release cooldown covers only agent CLIs, not horch itself.
- The task-ledger watchdog now starts after almost every command, so the stale check on hot paths must stay strictly offline (no fetch).
- **Tests to extend:** the update end-to-end tests (which now have local-registry helpers) and the boot-notice unit tests.

## What to build

No session runs a stale horch without being told.

- `horch fleet` prints `horch <version>` when it boots.
- `horch doctor`, `horch spawn` and `horch inbox` print 1 warning line on stderr when this horch is stale:
  - the cached latest release (at any age) is newer than the running version, or
  - the installed horch on PATH is a different binary from the running one.
- The check is offline: it makes no network call of its own.
- It does not depend on a terminal, and is not silenced by `--no-update` or by turning off agent-CLI updates. `HORCH_NO_SELF_CHECK` silences it, for a deliberate pin.
- The existing boot notice keeps its 24 h refresh of the cache.

## Acceptance criteria

- [ ] With a cached latest tag newer than the running version, `spawn`, `inbox` and `doctor` each print 1 stale warning, also when stderr is a pipe and agent-CLI updates are off.
- [ ] With a cache 30 days old, the warning still appears.
- [ ] When the installed horch differs from the running binary, the warning names both.
- [ ] `HORCH_NO_SELF_CHECK` suppresses the warning.
- [ ] No network call happens in `spawn` or `inbox` (the test uses an unreachable release URL and sees no delay).
- [ ] Fleet boot output includes the version line.

## Blocked by

None (can start immediately)
