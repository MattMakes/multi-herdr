# W1 ue-vendor report

Branch `ds/ue-vendor`. Source: `quodsoler/unreal-engine-skills` at `f3742d7b688690810df369802b90430324e380b9` (MIT).

## Totals

- 31 skill directories under `skills/ue-*/`: 30 verbatim, 1 adapted (`ue-project-context`).
- 121 files: 90 `.md` files and 31 `LICENSE` files. No other file types.
- 2,108,373 bytes added to the bundle. `build.rs` embeds them, so the binary grows by about that much.
- `skills/provenance.json` grows from 54,312 to 100,498 bytes. It has 31 new entries, each with `vendored: true`, `license: "MIT"` and a sha256 per file (upstream bytes, `LICENSE` included).
- `horch skills` (built binary) lists all 31 `ue-*` skills.

## Checks

- `diff -r` of each of the 30 verbatim skills against the source shows only the added `LICENSE`.
- Directory name equals `name:` in every `SKILL.md`.
- `skills/provenance.json` change is pure addition (0 lines removed).
- `grep` finds no `/Users/`, `CLAUDE_*`, or `ANTHROPIC_API_KEY` in `skills/ue-*`.

## Gate: red, for 3 reasons that V0 fixes

`just gate` fails only in `-p horch-core --test skills_catalog`, 3 tests:

- `skills_bundled_size_budget`: `SKILL.md` files are 20 to 35 KB (limit 12 KB). V0 exempts vendored skills.
- `skills_bundled_text_only`: it rejects the `LICENSE` files. V0 accepts a file named `LICENSE`.
- `skl_01_bundled_catalog_versions_and_digests`: it says the upstream is "not a pinned upstream". This is a third cause, not named in the plan. `ds/vendoring-infra` replaces the hard-coded pin list with a shape check (repository URL, 40-hex revision, license), so it fixes this one too. I did not touch the test.

All other gate steps pass. I have not run the gate against V0 plus this branch. Run it after the V0 merge and REBASE.

## Decisions

- Provenance order: the existing entries are not sorted by name. I appended the 31 `ue-*` entries after them, sorted by name. I did not reorder old entries.
- `LICENSE` is listed as a source in each entry (path `LICENSE`), because it is a copied upstream file.
- `ue-project-context` adaptation: Step 2 "Correct the draft" is now "Ask about the unknowns" (one `QUESTION:` to the orchestrator, unknowns in the skill's order: conventions, framework classes, networking, then save/streaming/AI, then team). The fallback questionnaire is now "Fallback: no source tree". Step 1 scan, "What to scan", plugin checklist, document template, Deprecated table and the no-guessing rule are unchanged. The full edit list is in its `adaptation` field. The `description:` front matter is unchanged: it holds trigger phrases only.
- File modes: the source files are read-only. rsync copied that mode, so I ran `chmod u+rwX,go+rX` on the copies.

## Odd in upstream

- Every `SKILL.md` is large: 20 to 35 KB, 946 KB in total. The briefings cost the descriptions only, but the bundle is big. The conventions exempt vendored skills from the budget.
- Upstream `ue-project-context` has `description:` text that says "the user says ...". These are trigger phrases, so I kept them.

## Follow-ups (not done)

- P-UE must attach the skills to the UE teammates by name.
- No skill is in any `Phase`, as the plan says.
