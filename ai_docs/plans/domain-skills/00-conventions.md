# Conventions for the domain-skills run (Swift and Unreal Engine)

Read `ai_docs/plans/design-skills/00-conventions.md` first. All of it applies
(integration branch `design-skills`, worktrees under
`/Users/mascott/projects/multi-herdr/.worktrees/<unit>`, branch `ds/<unit>`,
the gate, the merge protocol, the hard rules). This file adds what differs.

## Sources (read-only, pinned)

- Swift: `/Users/mascott/projects/multi-herdr/.worktrees/_sources/swift/<owner>_<repo>/`,
  pins in `.../swift/PINS.txt` (repository, full commit).
- Unreal: `/Users/mascott/projects/multi-herdr/.worktrees/_sources/unreal/unreal-engine-skills/`,
  pin in `.../unreal/PINS.txt`.

## The two reports are the specification

- Swift: `ai_docs/reports/swift-fleet-skills-2026-10.md` (verdicts in §4,
  house style in §5, teammates in §6, horch changes in §7, provenance in §7.3).
- Unreal: `ai_docs/reports/unreal-engine-wave.md` (vendor and adapt rules,
  the 2 own skills, 11 teammates, persona rules, code changes).
- Where a report says "verify" or "check", do it, and record the evidence in
  your report. Where a report leaves a choice open, choose, and say why.

## Vendored and adapted skills (differs from the design run)

- **Vendor** = a verbatim copy of the upstream skill directory (SKILL.md and
  references), excluding dotfiles such as `.DS_Store`.
- **Adapt** = a curated copy with exactly the edits the report lists, plus
  the fleet rules: no subagents, no human gate (a message to the
  orchestrator instead), no machine paths, no `${CLAUDE_*}` variables.
- Provenance: one entry per skill with `sources` (repository URL, full
  40-hex revision from PINS.txt, path, sha256 of each upstream file you
  copied or adapted), `adaptation` ("verbatim" or the list
  of edits), and `vendored: true` (V0 adds this flag; vendored skills are
  exempt from the 12 KB / 160 KB budget, not from the text-only rule).
- Skill directory names must equal the `name:` in SKILL.md.
- Domain skills attach by name (`skills:`), never by phase.
