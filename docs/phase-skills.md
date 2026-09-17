# Phase-scoped skills

The fourteen skills in [skills/](../skills/README.md) are repo-owned adaptations of `public-skills`, with source revision and hashes recorded in [provenance.json](../skills/provenance.json). They are compiled into `horch`; an installed binary works without either source checkout. No skill download or global installation happens when a worker starts.

| Phase | Fleet catalog |
|---|---|
| research | brainstorm, research-codebase, trace, handoff |
| plan | create-plan, pre-flight, handoff |
| implementation | execute, tdd, debug, check, handoff |
| validation | check, code-analysis, code-review, security-review, document, handoff |

A teammate's `phase` selects its default catalog; `skills` adds specific bundled names, deduplicated. Additional names are available, not an instruction to load them eagerly. Unknown names and conflicting settings fail before a pane or working ledger entry is created.

```sh
horch skills --phase implementation --json
horch spawn codex-sol --phase research "Investigate the storage migration"
horch spawn sonnet --phase plan "Write a plan from ai_docs/research/storage.md"
horch spawn prime --phase implementation "Implement ai_docs/plans/storage.md"
horch spawn opencode-pickle --phase validation "Validate the completed storage change"
horch spawn --resume RECORD_ID --phase validation "Check the final diff"
```

Resume retains its recorded phase unless explicitly overridden. Old ledger entries fall back to the teammate default. Skill selection changes on the next launch; assigning new work to an already running pane does not hot-reload its catalog. Prefer a fresh session for a new phase when the old conversation is unnecessary: a resumed conversation may still contain previously loaded skill bodies. Pass findings, plans and evidence as file paths.

## Native integration

Each launch writes only selected skill folders into a unique directory under `HORCH_STATE_DIR/skill-bundles` (the normal state root when unset). Native loaders expose descriptions and load bodies on demand. The initial briefing names the phase and selected skills; it never pastes their workflows. Ordinary completion and preparation errors remove the bundle. A forcibly killed process may leave its private directory behind.

| Harness | Integration | Context behavior |
|---|---|---|
| Claude Code | Generated `horch` skills-only plugin via `--plugin-dir`; names such as `horch:create-plan` | Keeps operator settings and denials. Defaults `disableBundledSkills` and `disableWorkflows` to true; explicit teammate JSON can override them. Selected workflows stay available through the native Skill tool. |
| Codex | Selected folders linked into the launch's private `CODEX_HOME/skills` | Native skill discovery; auth/config remain linked to the operator's home, and fleet execpolicy stays scoped. Other user/project and built-in skills may remain visible. |
| OpenCode | Selected directory added to `skills.paths` through a child-only `OPENCODE_CONFIG_CONTENT` overlay | Preserves existing inline settings, paths, provider values and denials. Other native catalogs remain discoverable; `--pure` disables external plugins, not all skills. |
| pi | `--skill <selected-directory>` | Explicit paths still load under the roster's `--no-skills`; extensions, templates and themes retain their existing controls. |
| Prime Agent | `--skill <selected-directory>` | Same lazy skill discovery. Daemon/session flags precede the positional prompt delimiter; the fleet owns daemon cleanup. |

Native discovery and controls are documented by [Claude Code](https://code.claude.com/docs/en/skills), [Codex](https://learn.chatgpt.com/docs/build-skills), [OpenCode](https://opencode.ai/docs/skills/), [pi](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/skills.md), and [Prime Agent](https://github.com/PrimeIntellect-ai/prime-agent/blob/main/packages/coding-agent/docs/skills.md). OpenCode's [`skills.paths` schema](https://opencode.ai/config.json) and [inline config precedence](https://opencode.ai/docs/config/) establish its adapter contract. Codex's private-home discovery was additionally exercised against the installed runtime; per-skill enablement configuration alone does not add a discovery root.

## Measuring context

`horch skills --phase PHASE --json` reports catalog bytes, an explicitly approximate metadata token count (bytes / 4), and the size of each deferred skill file. These are fleet catalog costs, not whole-request token measurements: native wrappers, paths, ambient skills, tools, repository instructions and conversation history add context.

Measured from this bundle: research 415 metadata bytes (~104 tokens), plan 329 (~83), implementation 468 (~117), validation 661 (~166). Extra teammate skills add to these figures. These estimates do not include deferred workflow bodies.

The linked [Claude context article](https://www.aihero.dev/how-to-kill-the-bloat-in-claude-codes-system-prompt) recommends measuring before removing features. Use Claude's `/context` before and after a representative launch; inspect `/skills` for duplicates. For Codex inspect `/skills` or app-server `skills/list`; OpenCode's `debug skill` lists actual native discovery; pi/Prime's native skill catalog and session usage expose their selected resources. None of these catalog listings proves a model invoked the skill. Keep MCP servers and tools that the task needs; do not disable tool families blindly to reduce a token count.

## Compatibility and limits

See [runtime checks](runtime-skill-checks.md) for exact tested versions and methods. Authentication must already work. Managed policies can reject startup settings, hide skills or deny reads; bundles do not override these policies. Skill availability does not grant tool or command permission.

Native Windows Codex still uses the legacy shared-rules fallback and cannot safely isolate this skill bundle. Phase-enabled Codex launches are rejected before writing shared rules or allocating worker panes. Use WSL for phase skills, or explicitly set `phase: null` and `skills: []` on a custom Codex teammate to use the legacy launcher. The other adapters use portable files and native flags; Windows runtime validation was not performed.

pi 0.85.1 requires Node >=22.19.0. This machine's default Node 22.9.0 fails before pi parses its CLI. An installed Node 25.5.0 runs it successfully when its directory leads PATH; this change does not modify the user's Node installation or PATH globally.
