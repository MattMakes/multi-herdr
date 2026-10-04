# F3 skill-overrides: disabled skills stay hidden in Claude Code

Single-branch work: follow `ai_docs/plans/finish/01-single-branch.md`.

## GOAL

A Claude pane launched by horch does not list skills its teammate disables
(`disabled_skills`, e.g. `herdr:herdr-worker`, `herdr:herdr-orchestrator`).

## CONTEXT

- L2 (opus-62) LA-4: Claude Code 2.1.289 still lists
  `herdr:herdr-worker` and `herdr:herdr-orchestrator` although horch sends
  `skillOverrides` with those keys off; the orchestrator pane shows the same.
- Find where those skills come from (a plugin named `herdr`? the
  `~/.claude/skills/herdr-*` symlinks, which memory calls stale?), and how
  current Claude Code disables a skill or plugin skill (read
  `https://code.claude.com/docs/en/skills` and the settings reference: the
  setting name, the key format for plugin skills, whether `--settings`
  merges it). Fix horch's launch settings accordingly, with a test of the
  written settings.
- Do not delete or edit anything in the operator's `~/.claude`; if the
  stale symlinks are the source, report it with the exact command for the
  operator.
- Also: `skills/ue-build-verify/references/git-lfs.md` dropped `*.mp4` from
  the recommended `.gitattributes` only because a plan grep matched `p4 `;
  put the line back (LFS-track video) and re-hash if provenance records it.

## FILES

own: `crates/horch-core/src/harness/claude.rs` (settings), tests,
`skills/ue-build-verify/references/git-lfs.md`, `ai_docs/reports/finish/skill-overrides.md`.

## STEPS

1. Reproduce (`claude` with horch's settings, list skills) and find the
source. 2. Fix + test. 3. The mp4 line. 4. Targeted checks; COMMITTED.
