# Skill authoring rules (design-skills run)

Every skill unit (D01 to D06) follows these rules. Read
`skills/README.md` ("Deliberate adaptations") first: the same spirit applies.

## Shape

- `skills/<id>/SKILL.md` with YAML frontmatter `name: <id>` and
  `description:`. The description starts with "Use when ..." and is at most
  300 characters. It states the trigger, not the content.
- Body budget: at most 12 KB (aim for 6 to 10 KB). It holds the workflow,
  the rules that apply every time, and an index of references.
- `skills/<id>/references/*.md`: deep material that the worker loads only
  when a step needs it (style catalogs, API details, checklists, examples).
  The whole skill directory is at most 160 KB. Text only: no images, no
  binaries, no scripts, no CSV. Convert useful data tables to compact
  Markdown tables.
- Body sections, in this order: `When to use`, `Inputs`, `Workflow`
  (numbered steps, each with the check that proves it is done), `Rules`,
  `Review checklist`, `References` (one line per file: when to load it).

## Make it ours

- Combine and deduplicate. State each rule once. When sources disagree,
  choose one and say why in your report.
- Rewrite in our voice: direct, imperative, specific. Short code examples and
  exact API facts may stay close to the source. Do not paste long passages.
- Remove: marketing language, persuasion and "elite" framing, fixed response
  rituals, mandatory interactive approval gates (the worker asks the
  orchestrator with `horch tell orchestrator "[<role>] QUESTION: ..."`),
  machine paths, provider-specific tool names.
- Harness-neutral: a skill must work in Claude Code, Codex, OpenCode, Pi and
  Prime. Say "a browser tool, if available" instead of a specific MCP name.
- Self-contained: no runtime download, no upstream script, no required other
  skill. You may name a related skill as optional ("if the motion-gsap skill
  is available, ...").
- Fleet rule: never tell the worker to start a subagent or a nested agent CLI
  (no `gemini`, `claude -p`, `codex exec`). A generate-then-review loop runs
  inside the worker's own session, or the worker asks the orchestrator for
  a second worker.
- Verification over vibes: every design rule that can be checked gets a
  concrete check (a measurement, a screenshot comparison, a contrast ratio,
  a list of banned patterns to search for).

## Provenance

After you rebase on the `skills-infra` unit (D00), add one entry per skill to
`skills/provenance.json` with the multi-source schema D00 defines: for each
source file you adapted, the repository URL, the pinned revision from
`PINS.txt`, the path, the sha256 of the original file
(`shasum -a 256 <file>`), and `license: "MIT"`. Plus one `adaptation` line.
Add one row per skill to the table in `skills/README.md`.

## Check

- `cargo run --quiet --bin horch -- skills show <id>` prints the skill.
- The gate is green (D00 adds the size budget test).
- In your report: the source-to-skill map (which source file went where),
  what you dropped and why, and 3 example task lines that should trigger the
  skill.
