# 13: Quota parsers read real replies (P26)

**Issue:** [#55](https://github.com/MattMakes/multi-herdr/issues/55) · **Spec:** [#46](https://github.com/MattMakes/multi-herdr/issues/46)
**Persona:** primary `sonnet-feature`, fallback `backend-developer` · **Parallel:** yes
**Check:** `cargo build --workspace --bins && cargo test -p horch-core quota && cargo test -p horch-e2e --test e2e quo_`

The GitHub issue is the source of truth. This file is its copy at handoff (2026-10-28), so that `horch ledger add-map` can queue it.

## Parent

#46

## What v0.3.0 changed (checked 2026-10-28)

- **Still needed as written.** The quota parsers are unchanged.
- The task ledger added saved pane screens as fixtures for its working-timer parser. They are a good model for saved real replies: 1 file per format, with the CLI version in its name or header.
- Its closing note (#28) also says the timer formats are unproven against real CLIs. Whoever captures real quota replies could capture a real working-timer screen from each CLI in the same session.

## What to build

Routing stops being blind: the quota probes give known readings on the current Claude Code, Codex and Antigravity releases.

- The operator captures 1 real reply per probe on a logged-in machine with the live-check script, and scrubs it of ids.
- The saved replies replace the synthesized fixtures.
- The parsers are fixed until those replies give known readings.
- `horch quota` shows why each unknown pool is unknown (probe error, stale reading, or no matching window).

**Needs the operator:** the hermetic gate cannot capture real replies. If they aren't attached here when work starts, the worker sends `QUESTION:` for them.

## Acceptance criteria

- [ ] For each saved real reply, the parser returns a known reading with the expected windows.
- [ ] Each fixture records the CLI version it came from.
- [ ] An unparseable reply leaves the pool unknown with the parse error as its reason, shown by `horch quota`.
- [ ] The existing quota end-to-end tests still pass.

## Blocked by

None (can start immediately; needs the operator's captured replies)
