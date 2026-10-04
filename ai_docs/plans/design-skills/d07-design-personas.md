# D07 design-personas: design teammates that use the design skills

Unit slug: `design-personas`. Branch: `ds/design-personas`.

## GOAL

The roster offers design specialists for taste, art direction, landing pages,
design systems, motion and image-led prototyping. Each has a sharp
`brief_description` the orchestrator can choose by, the right default phase,
model, effort, tools and MCP servers, and the design skills from D01 to D06.
The existing `designer` and `frontend-developer` also get design skills. The
orchestrator knows a design workflow.

## CONTEXT

- Read first: `00-conventions.md`, `02-skill-map.md` (the persona table and
  skill ids), `teammates/README.md`, `teammates/_template.md` (every field),
  and these existing teammates as patterns: `designer.md`,
  `frontend-developer.md` (MCP servers for a browser), `architect-reviewer.md`
  (a reviewer that reads and never edits), `qa-engineer.md`,
  `codex-sol.md` (a Codex teammate).
- The skill units D01 to D06 run in parallel. Their skill ids are fixed:
  `ui-taste`, `ui-redesign`, `art-direction`, `landing-page`,
  `design-system`, `brand-identity`, `motion-gsap`, `design-imagery`.
  `horch teammates --check` fails on an unknown skill id. So:
  - Write the personas and their prompts now.
  - Leave the `skills:` lines that name new skills out until those skills
    merge (keep the final lines in your report draft). The orchestrator
    tells you when each skill merged; then rebase and add the lines.
- The persona table in `02-skill-map.md` is the starting point. Improve it:
  you may add 1 or 2 personas if a real gap exists (for example an
  accessibility-and-content designer, or a mobile app designer), or merge 2
  if they overlap. Say why in the report.
- Persona prompt rules (the body under the frontmatter):
  - Say what the persona produces, what it never does, and how it reports.
  - Builders verify visually: screenshots at 390, 768 and 1440 px with a
    browser tool, before `horch done`.
  - `design-critic` reads and never edits (copy the `architect-reviewer`
    tool restrictions). It writes its critique to a file and reports scores.
  - `design-director` writes the direction contract and storyboard files;
    no production code.
  - `visual-prototyper` runs on Codex (check that the Codex harness can load
    skills; it can, through its private `CODEX_HOME`) and uses image
    generation only when available.
- Models: opus for direction, critique, landing pages and motion;
  sonnet is acceptable for `design-system-engineer` if its plan is precise
  (decide). Efforts per the existing pattern.
- Oracles: new teammates get new oracle files (`HORCH_BLESS=1`, conventions
  §5). `designer` and `frontend-developer` change on purpose: their skills
  oracle files change; put the diff summary in your report. Any other
  oracle or golden diff: `QUESTION:` first.
- Orchestrator awareness: add a short "Design work" section to
  `skills/orchestrate/SKILL.md`: the pipeline design-director (direction
  contract, storyboard) → builders (landing-page-builder,
  design-system-engineer, motion-engineer, frontend-developer) →
  design-critic (scored review with screenshots) → fix loop; when to use
  `visual-prototyper`; that free opencode teammates must not get
  unreleased brand work.

## FILES

own:
- `teammates/design-director.md`, `design-critic.md`,
  `landing-page-builder.md`, `design-system-engineer.md`,
  `motion-engineer.md`, `visual-prototyper.md` (new; and any persona you add)
- `teammates/designer.md`, `teammates/frontend-developer.md` (skills lines and
  a short paragraph on the new skills)
- `teammates/README.md` (the roster table)
- `skills/orchestrate/SKILL.md` (the "Design work" section only)
- new oracle files for the new teammates; the changed skills oracle files for
  `designer` and `frontend-developer`
- `ai_docs/reports/design-skills/design-personas.md`

do not touch: `skills/<other>/`, Rust code, other teammates.

## STEPS

0. Create the worktree (conventions §3).
1. Read the patterns and the source repositories' persona ideas (for example
   `cinematic-ui/agents/`, `hallmark` modes) only enough to shape prompts.
2. Write the new teammates without the new skill ids. Gate.
3. Add the "Design work" section to `skills/orchestrate/SKILL.md`. Gate.
4. As each skill merges (the orchestrator tells you), rebase and add its id
   to the `skills:` lines. Bless the oracle files. Gate.
5. Check: `horch teammates` lists every new persona under specialists;
   `horch teammates --check` passes; `horch skills --phase <phase>` is
   unchanged (persona skills are by name, not by phase).
6. Commit per step: `Teammates: Add design personas`,
   `Skills: Add design work to orchestrate`, `Teammates: Attach design skills`.
   Write and commit the report. Follow conventions §7.

## DONE WHEN

- Every persona loads with its final skills; the gate is green.

## REPORT

- The final persona table (name, phase, harness, model, effort, skills,
  MCP servers, one-line purpose), the oracle diffs, and decisions.
