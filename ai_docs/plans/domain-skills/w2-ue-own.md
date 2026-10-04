# W2 ue-own: write `ue-build-verify` and `ue-editor-scripting`

Unit slug: `ue-own`. Branch: `ds/ue-own`.

## GOAL

Two house skills, written in our voice within the 12 KB SKILL.md budget:
`ue-build-verify` (the change → build → test → report loop every UE
implementer runs before `DONE:`) and `ue-editor-scripting` (asset and
Blueprint changes through the Python Editor Script Plugin and a headless
commandlet). Every command-line flag is checked against Unreal Engine 5.8
documentation or source, and the evidence is in your report.

## CONTEXT

- Read first: `ai_docs/plans/domain-skills/00-conventions.md`,
  `ai_docs/plans/design-skills/01-skill-authoring.md` (house skill format),
  and `ai_docs/reports/unreal-engine-wave.md` "Write two of our own".
- Use the upstream `ue-testing-debugging` and `ue-module-build-system`
  (in `/Users/mascott/projects/multi-herdr/.worktrees/_sources/unreal/...`)
  as the reference for commands. Point to their files by skill name
  (`ue-module-build-system`'s `common-build-errors.md`), and do not copy
  their text.
- No engine is installed here. Check flags against Epic's published docs
  (use web search/fetch) and the upstream skills. Mark any flag you could
  not confirm as "unverified on 5.8" in the skill, not silently.
- Fleet rules the skills must carry: long builds run in the background (the
  Bash foreground limit is 10 minutes); Live Coding active → BLOCKED and do
  not close the editor; one UBT per working copy, so build only when the
  orchestrator assigns it; Perforce read-only file → report, never `chmod`;
  a `DONE:` names the target, configuration, automation filter and result.
- `ue-editor-scripting` makes rule 2 of the UE personas ("never edit
  `.uasset` bytes") workable: assets change only through editor Python or
  commandlets, and the skill must say so. It must also say how to verify
  the asset change (reload, a test, or a log line).
- These are house skills: `vendored` is false and the budget applies.
  Provenance names the upstream skills you consulted as references
  (`adaptation: "own text; consulted ..."`), with their pins.

## FILES

own: `skills/ue-build-verify/`, `skills/ue-editor-scripting/`, their
provenance entries and README rows, `ai_docs/reports/domain-skills/ue-own.md`.

do not touch: Rust code, teammates, other skills.

## STEPS

0. Create the worktree.
1. Research the flags (Build.bat/Build.sh/RunUBT, UnrealEditor-Cmd
   automation, `-run=pythonscript`, `-ExecutePythonScript`). Record each
   source URL in the report.
2. Write `ue-build-verify`. Gate. Commit.
3. Write `ue-editor-scripting`. Gate. Commit.
4. Report: the flag table (flag, purpose, source, verified yes/no).
