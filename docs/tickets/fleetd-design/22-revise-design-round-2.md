# 22: Revise the fleetd design, round 2 (A19 to A21)

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** no
**Kind:** docs
**Review:** none
**Check:** `test -s docs/specs/fleetd/amendments.md && grep -q 'Phase P' docs/specs/fleetd/design.md && grep -q 'PL-6' docs/specs/fleetd/design.md && grep -q '06 S14' docs/specs/fleetd/design.md && grep -q 'D16' docs/specs/fleetd/design.md && ! grep -q 'separate decision' docs/specs/fleetd/design.md && ! grep -q '—' docs/specs/fleetd/design.md && ! grep -q '—' docs/specs/fleetd/amendments.md`
**Own:** `docs/specs/fleetd/design.md`, `docs/specs/fleetd/amendments.md`
**Do not touch:** `ddd/`, each other file in `docs/specs/fleetd/`

## GOAL

`docs/specs/fleetd/design.md` states amendments A19, A20 and A21, and the amendments file is on the branch as `docs/specs/fleetd/amendments.md`.

## CONTEXT

- `docs/specs/fleetd/design.md` is the design of a daemon named `fleetd`. It passed a review in ticket 10 (`docs/specs/fleetd/design-review-2.md`, verdict PASS).
- After that, a DDD run made a model of the design in `ddd/`. The operator read the model and made decision D16. The DDD run also found 2 small faults in the design.
- The orchestrator wrote the 3 changes as amendments A19 (D16), A20 and A21. They are in the section "Revision round 2" at the end of `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/amendments.md`. That section is your instruction.
- That file is not in git. The committed reviews cite its A-ids. So step 2 puts the file on the branch.
- The decisions file is `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/decisions.md`. Read D1 and D16 there.
- The inventory of the OS-specific code is `docs/specs/fleetd/preservation-06-wiring-and-platform.md`, section 6 and section 7.
- You are a fresh session on purpose. You are not the author of the design.
- Ticket 23 is next. A fresh judge checks only your change.

## STEPS

1. Run `git remote -v` and `git config user.email`. The remote must be `git@git-alt:MattMakes/multi-herdr.git`. The email must be `m4ttplu5@gmail.com`. If 1 is different, send `BLOCKED:` and end your turn.
2. Copy `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/amendments.md` to `docs/specs/fleetd/amendments.md`. Change nothing in it. Commit.
3. Read "Terms to use everywhere" and the section "Revision round 2" of that file. Read section 6 and section 7 of `docs/specs/fleetd/preservation-06-wiring-and-platform.md`.
4. Check 2 facts that A19 uses. Run `grep -l horch-core crates/*/Cargo.toml`. The result must not name `herdr-install` or `horch-marketplace`. Then read the `[dependencies]` of `crates/herdr-install/Cargo.toml` and of `crates/horch-marketplace/Cargo.toml`. If a fact makes a leaf crate `crates/horch-platform` impossible for 1 of the 4 crates, send a `QUESTION:` with the fact and end your turn.
5. Apply A19. Search the design for `platform layer`, `Platform layer`, `PL-2`, `separate decision`, `178`, `Phase 1`, `build-windows` and `cfg(`. Make each place agree with A19. For the interface list: go through each row of the table of section 6 and make sure that the list in PL-2 and in the row "Platform layer" names a concern that holds it. Commit.
6. Apply A20. Search the design for `landed` and `ready`. Add the rule and the table "States that share a name". Correct each sentence that gives a task the state `landed`. Do not rename a state. Do not change a `CHECK (state IN ...)` list. Commit.
7. Apply A21. Add row 06 S14 to the Preservation analysis. Commit.
8. In the `## Amendments` table of the design, add 1 row "D16" with its decision in 1 or 2 sentences and the sections that you changed, and 1 row "Revision round 2: A19 to A21". Change the D1 row so that it no longer says "a separate decision". In the Open questions section, give the question about the existing OS-specific code its answer (D16), in the form that the section already uses. Commit.
9. Run the **Check** command of this ticket. It must exit 0.
10. Run `git diff f6243ab2 HEAD --stat`. It must name only the 2 files in **Own**.

## CONSTRAINTS

- Change only what A19 to A21 and step 8 need. Do not improve any other part of the design.
- Do not renumber Phases 0 to 7. Do not renumber a requirement, a failure mode or an invariant.
- Keep the voice of the design. It speaks to the operator as "you". Write short sentences in the active voice.
- Do not add a design idea of your own. If A19 to A21 leave a gap or contradict the design, send a `QUESTION:` to the orchestrator and end your turn.
- Do not use the em dash.
- Do not edit a file other than the 2 in **Own**.
- Do not start a subagent or a nested agent CLI.

## DONE WHEN

- The check passes.
- The Migration plan has a Phase P between Phase 0 and Phase 1 with a gate "PL-2 and PL-6 green".
- Phase 1 no longer says that the platform layer lands in it. Phase 0 says that CI runs the test suite on Windows.
- No sentence gives a task the state `landed`.
- Each row of the table of section 6 of the inventory has a concern in the interface list.

## REPORT

Finish with `horch ledger finish`, then `horch ledger done`. In the DONE line, give for each of A19, A20 and A21 the lines of the design that you changed, and each place where you had to choose a wording that the amendment did not give.
