# Native skill discovery checks

Executed on 2026-09-17 without model requests. For OpenCode, pi and Prime, three repository skill folders (`brainstorm`, `research-codebase`, `trace`) were copied into an ephemeral directory. These checks exercise native discovery or native package loaders; they do not prove successful authenticated model sessions.

| Harness | Installed version | Executed check | Observation |
| --- | --- | --- | --- |
| Claude Code | 2.1.274 | Stream-JSON control initialization, generated `horch` plugin containing `create-plan` | Native commands returned `horch:create-plan`; no user/model message sent. |
| Codex | 0.154.0 | App-server initialization followed by `skills/list` | A temporary probe skill under private `CODEX_HOME/skills` appeared at its exact path. A control using only `skills.config` did not discover an external path. |
| OpenCode | 1.18.2 | `opencode debug skill --pure` with `OPENCODE_CONFIG_CONTENT` containing `skills.paths` | All three selected skills discovered at their temporary absolute paths. |
| pi | 0.85.1, installed package metadata | Native `parseArgs`, `loadSkills`, `formatSkillsForPrompt` modules | `--no-skills --skill <bundle>` loaded exactly the three selected skills, no diagnostics. |
| Prime Agent | 0.9.4, also confirmed by CLI | Native `parseArgs`, `loadSkills`, `formatSkillsForPrompt` modules | `--no-skills --skill <bundle>` loaded exactly the three selected skills, no diagnostics. |

## OpenCode procedure and limits

The executed environment assigned temporary `XDG_DATA_HOME`, `XDG_CONFIG_HOME`, `XDG_CACHE_HOME`, and `XDG_STATE_HOME`, plus:

```json
{"skills":{"paths":["<temporary-directory>/skills"]}}
```

`OPENCODE_DISABLE_CLAUDE_CODE=true`, `OPENCODE_DISABLE_PROJECT_CONFIG=true`, and `OPENCODE_DISABLE_MODELS_FETCH=true` were also set. Execution took place in an empty temporary work directory. Output was redirected directly to a temporary file and parsed as JSON. The command exited successfully and reported the exact temporary `SKILL.md` location for each selected skill.

Discovery was **additive**: the output also included 24 existing user/built-in skills. These settings do not isolate OpenCode from all other skill locations, including the user's `.agents` catalog. This proves configured bundle discovery, not exclusive phase-only global discovery. The debug command emits complete skill content; that diagnostic representation does not establish how much reaches a model prompt. Provider/auth configuration was neither printed nor examined.

## pi and Prime procedure and limits

For each installed package, a temporary Node script imported its actual `dist/cli/args.js` and `dist/core/skills.js` modules and executed:

```javascript
const args = parseArgs(['--no-skills', '--skill', bundle]);
const result = loadSkills({
  cwd: temporaryWorkDirectory,
  agentDir: temporaryAgentDirectory,
  skillPaths: args.skills,
  includeDefaults: !args.noSkills,
});
const catalog = formatSkillsForPrompt(result.skills);
```

Both returned `noSkills: true`, the explicit temporary path, the three expected names, and an empty diagnostics array. Formatted catalogs measured 1,172 characters for pi and 1,383 for Prime in this run (absolute path length affects the count). Both omitted the distinctive opening sentence of the brainstorm workflow body, consistent with a metadata catalog rather than eager body inclusion.

Installed source inspection confirmed the real resource loader's `noSkills` branch preserves explicit `additionalSkillPaths`, calls `loadSkills` with `includeDefaults: false`, and CLI startup passes parsed flags into that loader. This wiring confirmation is source evidence; full resource-loader startup was not executed. Prime CLI help also reports `--skill` and `--no-skills`.

The installed pi bundled CLI failed even for `--version` under Node.js 22.9.0 with `TypeError: webidl.util.markAsUncloneable is not a function` in bundled Undici initialization. Its package declares Node >=22.19.0. A follow-up `PATH=/opt/homebrew/bin:$PATH pi --version` succeeded with the already installed Node 25.5.0 and printed `0.85.1`. Standalone native skill modules also executed successfully. A full model session was not run. No packages, user settings, runtime installations or global PATH were changed.

## Claude and Codex procedure

Claude ran in an empty temporary directory with a temporary `CLAUDE_CONFIG_DIR`, empty setting sources and MCP servers, the generated plugin directory, and `disableBundledSkills`/`disableWorkflows` settings. Its stream-JSON control `initialize` response reported success and included `horch:create-plan` among 30 commands. This validates plugin discovery and namespacing without starting an inference request.

Codex ran `app-server` with a temporary `CODEX_HOME`. JSON-RPC `initialize`, `initialized`, then `skills/list` for `/tmp` returned the probe skill at `skills/horch-probe/SKILL.md`. Repeating with the skill outside the home and a `skills.config` enablement entry returned no probe skill. Ambient user and generated system skills also appeared. The launcher integration test separately verifies replacing only the private home's skills symlink, preserving the original user skill file, and cleaning the private home on early errors.
