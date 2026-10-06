# Skills, domain teams and the roster fields

This page tells you how skills reach a worker, how a domain team is offered
only where the project needs it, and which commands show you the result. Read
it before you add a skill or a teammate.

For the files to change, use the recipes: [add-skill.md](recipes/add-skill.md)
and [add-teammate.md](recipes/add-teammate.md).

## Four ways a worker gets a skill

A teammate file has 4 fields that attach skills. Each has a different cost in
the worker's briefing.

| field | what the worker gets | briefing cost |
|---|---|---|
| `phase` | the bundled skills of that phase | names only |
| `skills` | named skills, expected | name and description |
| `available_skills` | named skills, offered | name only, under "Also available" |
| `operator_skills` | skills from a directory on the operator's machine, expected | name and description |

Precedence when a name is in more than 1 place: `skills` and `operator_skills`
first, then `phase`, then `available_skills`.

`available_skills` saves briefing text. It does not save the harness's own
skill-list cost: the harness lists the description of every skill that the
launch materializes. Keep the list short (the Unreal teammates use at most 3).

Show what is configured:

```bash
horch teammates --matrix          # skills, available skills, offer gate, price
horch teammates --matrix --json   # the same, with offer_when and requires
horch skills                      # the bundled catalog and its context cost
horch skills show ue-build-verify # 1 skill: digest, copied files, verbatim flag
```

## Bundled skills: adapted, verbatim, own text

`skills/` holds 3 kinds of bundle. `skills/copied.json` has 1 entry for
each: `{"name", "copied_files", "verbatim"}`.

| kind | what it is | `verbatim` | size budget |
|---|---|---|---|
| adapted | a curated copy with a set of edits and the fleet rules (no subagents, no approval gate, no machine paths) | no | `SKILL.md` at most 12 KB, directory at most 160 KB |
| verbatim | an unchanged copy of a skill directory, without dotfiles | `true` | exempt |
| own text | written here; `copied_files` is empty | no | as adapted |

Rules that the tests enforce (`crates/horch-core/tests/skills_catalog.rs`):

- Every bundled skill has a `copied.json` entry, and every listed copied file
  exists in the skill directory.
- A skill in `REPO_ORIGINAL` has no copied files; every other skill has at
  least 1.
- A skill directory holds only `.md` and `.txt` files. `skill-creator` is the
  one exception. No skill holds a licence or notice file.
- The build skips every file and directory whose name starts with `.`.

The Godot checks in the gate (`scripts/godot/*_check.py --strict-own
skills/copied.json`) report a problem in a copied file and fail on a problem
in any other file.

### Refresh a copied skill

1. Copy the new files into `skills/<id>/`, without dotfiles. For an adapted
   skill, re-apply its edits.
2. Update `copied_files` in `skills/copied.json` when the file list changes.
3. Run the targeted check, then the gate:

   ```bash
   cargo test -p horch-core --test skills_catalog
   horch skills show <id>     # prints the new digest
   ```

The full rules are in [skills/README.md](../skills/README.md#copied-files).

## Offering a domain team only where it fits

A fleet orchestrator reads the roster in its briefing. A domain team (Unreal,
Swift) would add noise to a project that has no use for it. Two fields control
this.

`offer_when` is a list of file-name globs. Any 1 match makes the teammate
visible to the orchestrator.

```yaml
offer_when: ["*.uproject"]
offer_when: ["*.xcodeproj", "*.xcworkspace", "Package.swift"]
```

- A glob supports `*` and `?`, is case-sensitive, and matches an entry name
  (a file or a directory). A glob with `/` never matches, so `--check`
  refuses it, and it refuses an empty glob.
- The project is read at the top level and 1 level down. Dot-directories
  (`.git`, `.worktrees`) are not read.
- Only the 2 fleet orchestrator panes and `horch doctor` apply the filter.
  `horch spawn <name>`, `horch teammates` and the worker briefings do not.
  So `horch spawn ue-qa-engineer` works in any project.

`requires: [xcode]` names a tool on the operator's machine. `horch doctor`
checks it only when the project is offered a teammate that names it. It is an
enum: a typo fails when the roster loads.

## Operator skills (skills that cannot ship here)

Apple's Xcode 27 skills are an example. Export them on your machine:

```bash
xcrun agent skills export --output-dir ~/.agents/skills   # Xcode 27 or later
```

`swift-developer` and `apple-platform-developer` already name 2 of them. For
another teammate, name them in a local copy of its file:

```yaml
operator_skills:
  dir: ~/.agents/skills
  names: [swiftui-whats-new-27]
```

- The launch copies each `<dir>/<name>/` into the skill bundle and checks
  the copy's tree digest. The version is `operator+<digest12>`.
- On Claude the skill appears as `horch:<name>`, like a bundled skill.
- A host that lacks the skill is normal, because each host exports its own
  copy. When `dir` or `<dir>/<name>/` does not exist, `horch teammates --check`
  prints a `warning:` line and passes. The launch skips the skill, and the
  briefing says "Skipped: operator skill <name> is not installed on this
  host", so the worker tells the orchestrator.
- `horch teammates --check` fails on a name that is named twice, a name that a
  bundled skill already has, a `dir` that exists but cannot be read, an
  invalid or unreadable `SKILL.md`, and a subagent skill (`device-interaction`,
  or a `SKILL.md` with "SUBAGENT skill" or "Agent tool"). The name rules apply
  on every host. There is no `operator:` prefix: rename 1 of the 2 skills.
- The ledger record that `horch spawn` writes lists the operator skills that
  the launch copied.

## What `horch doctor` checks

`horch doctor` needs a herdr server for its first check. It prints a warning
for each of the others and still exits 0.

| check | fails when |
|---|---|
| herdr installed and reachable | `herdr` is not on `PATH`, or `herdr workspace list` fails (exit 1) |
| roster | `horch teammates --check` has problems; doctor counts the teammates and the ones offered in this project |
| `requires:` | an offered teammate needs `xcode`, and `xcodebuild -checkFirstLaunchStatus` fails or `xcodebuild` is missing |
| harness | a harness binary is found but `--version` fails; the quota probe marks it broken and routing skips its teammates |
| effort overrides | `CLAUDE_CODE_EFFORT_LEVEL`, `maxEffortLevel` in `~/.claude/settings.json`, or `model_reasoning_effort` in `~/.codex/config.toml` overrides every teammate's effort |

Run it from the project directory, so the offer filter reads that project.

## The domain teams

Each team is a set of `teammates/*.md` files plus the skills they carry. The
tables of members, models and fallbacks are in
[teammates/README.md](../teammates/README.md). This section says how to use
each team.

| team | offered when the project has | skills | rules every member follows |
|---|---|---|---|
| Unreal Engine (11 `ue-*`) | `*.uproject` | 31 copied `ue-*` skills, plus 2 house skills, `ue-build-verify` and `ue-editor-scripting` | read `.agents/ue-project-context.md` first; never write asset bytes; check the engine headers; 1 build at a time; a `DONE:` that names target, configuration and filter; the Git LFS lock rules |
| Godot (19 `godot-*`) | `project.godot` | 64 `godot-*` skills: renamed copied skills, combined skills with own-text references, and own skills such as `godot-build-verify` and `godot-scene-files`; a `*.csproj` adds the 2 C# skills to each builder | read `.agents/godot-project-context.md` first; never touch `.godot/` or `.import`, commit `.uid` sidecars; check APIs with `godot --doctool`; headless only, 1 import at a time; a `DONE:` that names the Godot version, the parse check and the tests; addon skills only at the pinned addon version |
| Swift and Apple (7) | `*.xcodeproj`, `*.xcworkspace` or `Package.swift` | 28 Swift and Apple skills | check the toolchain first; the builders carry `mobilebuildmcp` |
| Design (6 new, 2 changed) | no gate: always offered | 8 design skills | brand inputs override the palettes and font pairings in the skills |
| Blender (`blender-artist`) | `*.blend` or `*.uproject` | `blender-ue-pipeline`, `blender-modeling`, `blender-rigging`, `blender-baking` | Blender Lab MCP server; hands FBX exports to the Unreal team |

### Unreal Engine and Git LFS

- Target: UE 5.8 (hotfix 5.8.3 on 2026-10-04). The 31 copied skills target
  it, so they stay verbatim (30 verbatim, `ue-project-context` adapted).
- Version control is **Git LFS**, not Perforce. Binary assets are LFS files
  marked `lockable`. A worker locks an asset with `git lfs lock` only when the
  orchestrator assigned it. It reports a lock held by someone else as
  `BLOCKED:`. It never runs `git lfs unlock --force`. It never makes a
  read-only file writable with `chmod`.
- A new worktree needs `git lfs pull`. All worktrees of 1 clone share the
  objects in `.git/lfs`.
- A worker keeps its locks and lists them in `DONE:`. It unlocks only when the
  orchestrator says so, because `git lfs unlock` needs a clean status.
- The recommended `.gitattributes` and the lock rules are in
  `skills/ue-build-verify/references/git-lfs.md`.
- Every UE teammate runs on Claude. A Codex pane has no tested UE path, and
  native Windows refuses a Codex teammate that has skills. Only `ue-tech-lead`
  and `ue-code-reviewer` have a `codex-sol` fallback.
- Check the whole team: `horch teammates --check`. Watch the briefing size
  with `horch teammates --matrix`.

### Core specialists

The core specialists carry own-text skills for the part of their work that the
phase catalog does not cover. Each attaches by name and belongs to no phase.

| teammate | skills it adds |
|---|---|
| `product-lead` | `product-requirements`: problem, scope lists, acceptance criteria, first slice |
| `staff-engineer` | `system-design`: forces, options, failure modes, rollout order; names `api-contracts` and `data-migrations` |
| `architect-reviewer` | `architecture-review`: boundaries, contracts, coupling, reversibility, ranked by consequence |
| `backend-developer` | `api-contracts` (validation, error model, idempotency, evolution) and `data-migrations` (expand/contract, online DDL, backfills) |

### Blender and the Unreal team

`blender-artist` models, retopologizes, UV-unwraps, bakes, rigs and exports
assets for Unreal. It runs on Claude (opus, medium) and carries 4 skills:
`blender-ue-pipeline` (names, axes, export, hand-off), `blender-modeling`
(topology, UVs, texel density, LODs, collision), `blender-rigging` (skeleton,
weights, shape keys, actions) and `blender-baking` (normal, AO, base color and
ORM maps). The helper functions in the last 3 were run against the `bpy`
5.0.1 module.

- It drives Blender through the Blender Lab MCP server (v1.0.3), pinned by
  commit in `teammates/blender-artist.md`. The server has no telemetry, no
  network tools and no API key.
- Warning: `uvx blender-mcp` installs a different, community project. Use the
  pinned launch line in the teammate file.
- Headless tools (`*_for_cli`) need only a `blender` binary on `PATH`, or
  `BLENDER_PATH` set to it. Live tools need Blender 5.1 or later with the
  Blender Lab add-on open, and the operator must say so.
- Without `blender` on `PATH` or `BLENDER_PATH`, `horch doctor` warns first.
  Without it, the worker reports `BLOCKED:` on the first run.
- The add-on and the community add-on both default to port 9876. Run only 1.
- It writes 1 FBX per asset and a `<AssetName>.handoff.md` (scale, axes,
  bounds in centimetres, facing). `ue-technical-artist` and
  `ue-tools-engineer` import the files; they send Blender work to
  `blender-artist`.
- The Git LFS lock rules from the Unreal section apply to the exports.

### Swift and Apple, MobileBuildMCP, App Store Connect

- The panes that build carry `mobilebuildmcp` (formerly XcodeBuildMCP),
  pinned at 2.7.1. Each teammate switches on the workflows it needs in the
  server's `env`. The Swift panes turn off its telemetry.
- A builder or QA pane reports `BLOCKED:` when the toolchain check fails
  (`xcodebuild -version`, `swift --version`, `xcrun simctl list devices
  available`). A reviewer reviews the source and says it had no build.
- `app-release-preparer` reads its App Store Connect credential from a file
  that the operator owns. The key setup is in the Apple section of the
  top-level [README](../README.md). Its Bash commands run in an OS sandbox
  that reaches only `api.appstoreconnect.apple.com`. It never uploads and
  never submits: it writes the exact upload command, and a human runs it.
  It ends every other App Store Connect write at a `--dry-run` plan for the
  orchestrator.
- `swift-developer` and `apple-platform-developer` set `operator_skills` for
  Apple's Xcode 27 skills (see "Operator skills"). A host without the export
  gets a warning, not a failure.

### The design team

`design-director` (research), `design-critic` (validation), and the builders
`landing-page-builder`, `design-system-engineer`, `motion-engineer` and
`visual-prototyper` use 8 bundled design skills: `art-direction`,
`brand-identity`, `design-imagery`, `design-system`, `landing-page`,
`motion-gsap`, `ui-redesign` and `ui-taste`. `designer` and
`frontend-developer` also load some of them.

- The orchestrator runs a pipeline: director writes the direction contract,
  builders build, critic reviews at 390, 768 and 1440 px. The "Design work"
  section of the `orchestrate` skill says how.
- `design-critic` never edits code. It writes its critique to a file.
- `visual-prototyper` runs on Codex. It cannot take screenshots unless a
  browser tool works in the Codex sandbox, so a Claude teammate screenshots
  for it.

## `horch agent-list`: which harnesses can this machine run

```bash
horch agent-list              # runs each CLI's --version
horch agent-list --no-probe   # runs no harness binary
horch agent-list --json
```

For each of the 6 harnesses (`claude`, `codex`, `opencode`, `pi`, `prime`,
`antigravity`) it prints: the status (`available` or `unavailable`), the usage
pool state, the binary, the version, the efforts the CLI takes, the
capabilities (`resume`, `headless`, how skills are exposed), and the models
that roster teammates run on it. It needs no herdr server and sends no model
request.

### The Antigravity harness

- The binary is `agy`. Set `HORCH_ANTIGRAVITY_BIN` to use another path.
- Efforts are `low`, `medium` and `high`. The pane takes the prompt with
  `--prompt-interactive`.
- It cannot expose skills (`skills=none` in `horch agent-list`). A teammate on
  it has no `phase` and no `skills`, and `horch spawn` refuses one that has.
- It uses the `google` usage pool, which has no probe. Its prompts can train
  the provider's models until the operator confirms the opt-out, so send it
  public and open-source work only.
- The shipped teammate is `antigravity`.

## `multi-herdr-dataset`: competitive rounds

`multi-herdr-dataset` is a separate binary. Every candidate and the judge are
model calls that cost money.

```bash
multi-herdr-dataset run "<task>" --candidates 3 --budget-usd 12.50
multi-herdr-dataset status          # experiments and rounds with their state
multi-herdr-dataset readiness       # the dataset per arm, the verdict and the gaps
multi-herdr-dataset export          # decided rounds as JSONL
multi-herdr-dataset resume          # after a crash or a kill
```

Preflight refuses a run before any worktree or model call. Use `--budget-usd`
every time. The model is in
[specs/dataset-competition.md](specs/dataset-competition.md), and the recipe for
a new event is [add-dataset-event.md](recipes/add-dataset-event.md).
