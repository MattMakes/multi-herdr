# U1 ue-lfs: Unreal teammates on the latest engine and Git LFS

Unit slug: `ue-lfs`. Branch: `ds/ue-lfs`.

## GOAL

The Unreal teammates and the 2 house skills (`ue-build-verify`,
`ue-editor-scripting`) assume the latest released Unreal Engine and Git LFS,
with no Perforce instructions left, and the version facts are checked.

## CONTEXT

- Read first: `ai_docs/plans/finish/00-conventions.md`,
  `ai_docs/reports/unreal-engine-wave.md` (open questions), the
  `ue-teammates` and `ue-own` reports in `ai_docs/reports/domain-skills/`.
- Operator decisions: latest engine; Git LFS.
- Find the latest released UE version (Epic's release notes, web). If it is
  newer than 5.8, the vendored `ue-*` skills (5.8) stay verbatim; the
  personas say which skill advice may be stale and to check the engine
  headers (persona rule 3 already says this; make it name the version).
  Re-check W2's "not verified" flags against the latest docs and update the
  skills' marks.
- Git LFS replaces every Perforce rule:
  - binary assets (`.uasset`, `.umap`, and the usual UE binary types) are
    LFS-tracked and *lockable* (`.gitattributes` with `filter=lfs diff=lfs
    merge=lfs -text lockable`); give the recommended `.gitattributes` in
    `ue-project-context` guidance or in `ue-build-verify` (house skills only;
    never edit the vendored ones);
  - a worker that must change a lockable asset runs `git lfs lock <path>`
    only when the orchestrator assigned that asset, reports a lock held by
    someone else as `BLOCKED`, and never `--force`-unlocks;
  - a read-only lockable file is the LFS signal of "not locked": report,
    never `chmod`;
  - worktrees: LFS objects are shared through the common `.git/lfs`; note
    `git lfs install` per machine and `git lfs pull` in a new worktree.
- The 11 `teammates/ue-*.md` personas: replace Perforce wording (rule 4 and
  any `p4` mention) with the LFS rules above.

## FILES

own: `teammates/ue-*.md`, `skills/ue-build-verify/`, `skills/ue-editor-scripting/`,
their provenance `adaptation` text, `teammates/README.md` (UE section),
`ai_docs/reports/finish/ue-lfs.md`.

do not touch: the 31 vendored `skills/ue-*` dirs.

## STEPS

0. Worktree. 1. Version research (sources in the report). 2. Skills. 3.
Personas. 4. `grep -rni 'perforce\|p4 ' teammates/ue-* skills/ue-build-verify skills/ue-editor-scripting`
prints nothing. 5. Gate. Report.
