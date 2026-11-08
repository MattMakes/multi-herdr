# 41: Close the open items of hark integration

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `PYTHONPATH=skills/hark/engine /Users/mascott/.local/state/hark/venv/bin/python -m pytest skills/hark/engine/tests -q -m 'not ml and not live'`
**Own:** `skills/hark/`, `docs/specs/hark/listen-find.md`

## Goal

The open items that T-33 (hark integration) listed are fixed, or recorded as operator decisions, and the hark suite still passes.

**Side: clean.** Never open the source. Read only docs/specs/hark/ and the repo. Rules: `/Users/mascott/projects/multi-herdr/ai_docs/plans/hark/CLEANROOM.md`.

## Open items from T-33 and the decision for each

1. The launch kit preset uses mallet/pluck, which the take audit refuses. Decision: give the launch preset the closest passing bank sound for that role, by measured profile (onset, centroid, decay), and log the swap in the kit data with a reason.
2. 15 derived forge_v2 takes fail `sfx check` (OPEN_DERIVED). Decision: re-derive each with the next-closest render that passes the check. A take that has no passing render is dropped from the bank and listed in a `dropped` field with its reason.
3. The riser table order has no spec rule. Decision: sort by ascending duration, then by id. Document it in the code.
4. `find`: an onset with no attack gives no stroke. Decision: keep the behaviour and add 1 sentence to `docs/specs/hark/listen-find.md` section F5 that states it.
5. `doctor` reports vocab_stats.json missing as a problem, and ARCHITECTURE 12 ships no file. Decision: doctor reports it as info, with the command that builds it.
6. `forge --check` on a missing folder exits 0 with no output. Decision: exit 1 with 1 line on stderr.
7. Dead stand-in code remains in `tests/*/conftest.py`. Decision: remove it. Keep every test green.
8. Every ML command prints a torch "Redirects" warning on stderr. Decision: filter that 1 warning where hark imports torch, by its exact message, not by category.

## Steps

1. Fix items 1-8 as decided. Add or update a test for items 1, 2, 5, 6 and 8.
2. Run the check, the ml tier with `HARK_OFFLINE=1` (`-m ml`), and the complexity sweep with the installed horch on your changed functions.
3. Commit exact files. Run `horch ledger finish`, then `horch ledger done`.

## Out of scope

- fit.json (needs an operator decision on real CC0 recordings).
- EVS-11 (xfail stays).
- The size of the horch binary.

## Done when

- The check passes. Each item has its fix and test, or a reason in DONE.
