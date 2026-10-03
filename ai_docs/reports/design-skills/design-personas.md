# D07 design-personas report

Branch `ds/design-personas`. The gate is green. Every persona loads with its final skills.

## Final persona table

| name | phase | harness | model | effort | skills (final) | MCP servers | purpose |
|---|---|---|---|---|---|---|---|
| `design-director` | research | claude | opus | high | art-direction, ui-taste, brand-identity | none | Writes the direction contract and storyboard; no production code. |
| `design-critic` | validation | claude | opus | high | ui-taste, ui-redesign | playwright | Scored review with screenshots at 390, 768, 1440 px; never edits code. |
| `landing-page-builder` | implementation | claude | opus | medium | landing-page, ui-taste, art-direction, motion-gsap | playwright, chrome-devtools, context7 | Builds conversion-led landing and marketing pages. |
| `design-system-engineer` | implementation | claude | sonnet | medium | design-system, ui-taste, brand-identity | playwright, context7 | Tokens, themes, components, brand kit in code. |
| `motion-engineer` | implementation | claude | opus | medium | motion-gsap, ui-taste | playwright, chrome-devtools, context7 | GSAP scroll scenes and transitions, reduced-motion safe. |
| `visual-prototyper` | implementation | codex | gpt-5.6-sol | medium | design-imagery, ui-taste | none (Codex has no per-teammate MCP) | Generated images and a static prototype. |
| `designer` (existing) | research | claude | opus | medium | + ui-taste, art-direction | none | Unchanged role; now loads the taste and direction skills. |
| `frontend-developer` (existing) | implementation | claude | opus | medium | tdd + ui-taste, design-system | unchanged | Unchanged role; now loads the taste and system skills. |

Fallbacks: opus seats fall back to `codex-sol`, `design-system-engineer` to
`codex-terra`, `visual-prototyper` to `opus`.

## Decisions

- No persona added, none merged. `designer` already covers flows, states,
  copy and accessibility, so an accessibility-and-content designer would
  duplicate it. A mobile app designer has no build skill behind it
  (`design-imagery` covers mobile mockups only), so it would be a prompt with
  no method. `landing-page-builder` and `motion-engineer` touch the same
  pages but own different files and different skills; the pipeline
  serializes them.
- `design-system-engineer` runs on sonnet at medium. The direction contract
  supplies the judgement, and tokens and components are rule-bound work. Its
  brief tells the orchestrator to spawn opus for a system with no direction.
- `design-director` runs at high, like the planners (`staff-engineer`,
  `product-lead`): every builder works from its contract.
- `design-critic` denies `Agent`, `Edit` and `NotebookEdit`, but keeps
  `Write`. This differs from `architect-reviewer`, which also denies
  `Write`. The plan asks the critic to write its critique to a file; a Write
  deny would push it to shell redirection, which is a weaker control than a
  persona rule plus a visible tool. The persona limits Write to the critique
  and the screenshots.
- `visual-prototyper`: Codex loads bundled skills through the private
  `CODEX_HOME` (`harness/codex.rs` `attach_skills`). Image generation is the
  Codex feature `image_generation`, stable and on in codex-cli 0.160.0, and it
  does not need sandbox network, so the seat keeps `permission_mode: auto`
  (network off). Without the tool it writes prompts and placeholders. Codex
  has no per-teammate `mcp_servers`, so it screenshots only when a browser
  tool works in its sandbox, and otherwise names the file for a Claude
  teammate. It does not commit unless told (Codex can refuse `.git` writes).
- Builders verify at 390, 768 and 1440 px before `horch done`.
- D04 request: every persona that chooses colour or type says that brand
  inputs (guidelines, logo colours, brand fonts) override the palettes and
  font pairings in the design skills. `motion-engineer` has no such line; it
  does not choose colour or type.
- D01 request: `design-critic` uses `ui-taste` in review mode and
  `ui-redesign` steps 1 to 4 only (classify, baseline, scan, audit).
- `designer` and `frontend-developer` each get one paragraph that names
  their new skills. `designer` leaves a full direction contract to
  `design-director`.
- The orchestrate "Design work" section is section 9, after "Things that go
  wrong", so the existing section numbers stay.
- `brief_description` of `motion-engineer` has no colon: a colon followed by
  a space broke the YAML frontmatter.

## Test and oracle changes (approved by the orchestrator)

- `crates/horch-core/tests/baseline_oracles.rs` and `skills_catalog.rs`: the 6
  new names join `SKIP_NEW_TEAMMATES`. No new oracle files. Routing oracles
  unchanged.
- `crates/horch-core/src/roster/validation.rs`: `design-director` joins the
  research arm and `design-critic` the validation arm of
  `builtin_phase_defaults_are_portable_and_disabling_conflicts`; the claude
  fleet-pane count goes from 19 to 24 (comment 18 to 23).
- `crates/horch/tests/oracles/skills/skills-json.txt`: orchestrate
  `skill_file_bytes` 6992 to 8385, from the "Design work" section. No other
  CLI oracle changes.
- `crates/horch-core/tests/oracles/skills/designer-{research,plan,implementation,validation}.txt`:
  the briefing changes from "Available native skills: ..." to "Skills you
  are expected to use on this task", with 2 lines, `horch:ui-taste` and
  `horch:art-direction`, then "Also available in this phase: ..." with the
  same phase skills as before.
- `crates/horch-core/tests/oracles/skills/frontend-developer-{research,plan,implementation,validation}.txt`:
  2 lines added after `horch:tdd`: `horch:ui-taste` and
  `horch:design-system`.
- `crates/horch-core/tests/oracles/launch/designer.json` and
  `frontend-developer.json`: 3 lines each, one per mode. Each is the
  `--append-system-prompt` skills briefing, with the same text change as
  the skills oracles. No argv, env or settings change.
- `horch skills --phase <phase>` output is unchanged for all 4 phases:
  persona skills attach by name.

## Gotchas

- Rebase conflicts: `SKIP_NEW_TEAMMATES` in both test files and the phase
  arm in `validation.rs` conflict with D08 (`antigravity`). I kept both
  sides: the list is alphabetical, one name per line, and `antigravity`
  keeps its `None` arm.
- `cargo test` takes one name filter. Bless `oracle_skills_match` and
  `oracle_launch_matches` in 2 runs.
- A colon followed by a space inside `brief_description` breaks the YAML.

## Outside my scope (not fixed)

- `horch-e2e` `tests/dataset.rs` fails intermittently under machine load
  (load average 6 to 8): PRE-06, PRE-07 and PRE-12 report "harness version
  unresolved: codex". `crates/horch-core/src/competition/preflight.rs:889`
  gives the version probe a 2 s timeout. Alone, the target passes 21 of 21.
- The plan asks `design-critic` to copy the `architect-reviewer` tool
  restrictions and also to write a file; these conflict (see Decisions).

## Follow-ups

- Run 1 real design pipeline (director, builder, critic) and measure cost
  with `horch cost` before tuning efforts with `tune-fleet`.
- `visual-prototyper` cannot take screenshots unless a browser tool works in
  the Codex sandbox. The orchestrate section routes screenshots to the
  critic.
