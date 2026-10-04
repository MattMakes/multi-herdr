# P-Swift swift-teammates: the Swift and Apple teammates

Branch `ds/swift-teammates`. Worker opus-49. Date 2026-10-03.
Plan: `ai_docs/plans/domain-skills/p2-swift-teammates.md`. Specification:
`ai_docs/reports/swift-fleet-skills-2026-10.md` §1, §5, §6, §7.

## Result

| commit | content |
|---|---|
| `Skills: Match the Xcode skills to MobileBuildMCP 2.7.1` | `ios-simulator-run`, `swift-code-audit` (2 files), their provenance `adaptation` text |
| `Teammates: Add swift-developer, swift-reviewer and swift-qa-engineer` | 3 teammates; the test lists; the doctor test |
| `Teammates: Add apple-platform-developer, apple-accessibility-auditor and codex-swift-reviewer` | 3 teammates; the test lists; the doctor test |
| `Teammates: Add app-release-preparer; expand ~/ in env values` | 1 teammate; `teammate_env` `~/` expansion and its test; `_template.md` `env:` comment |
| this report and `teammates/README.md` "The Swift and Apple team" | |

- `horch teammates --check` (built binary): `roster ok: 59 teammates, 53 offered to the orchestrator`.
- Gate: green after each commit, and again after the rebase onto `723e958`
  (with D19's clippy step).

## What each file sets

Every file: `base: fleet-worker`,
`offer_when: ["*.xcodeproj", "*.xcworkspace", "Package.swift"]`. Every Claude
file: `agent: claude`, `permission_mode: auto`, `inherit_plugins: false`,
`disabled_skills: [herdr-orchestrator, herdr-worker]`. Name,
`brief_description`, phase, model, effort and skills are §6 and §1,
unchanged.

| teammate | phase | model / effort | `requires` | fallbacks | `disallowed_tools` | MCP servers (workflows) |
|---|---|---|---|---|---|---|
| `swift-developer` | implementation | opus / medium | xcode | none | Agent | mobilebuildmcp (simulator, ui-automation, swift-package), context7 |
| `apple-platform-developer` | implementation | opus / medium | xcode | none | Agent | mobilebuildmcp (simulator, ui-automation, swift-package), context7 |
| `swift-reviewer` | validation | opus / high | xcode | codex-sol | Agent, Edit, Write, NotebookEdit | mobilebuildmcp (simulator, swift-package) |
| `swift-qa-engineer` | validation | sonnet / high | xcode | none | Agent | mobilebuildmcp (simulator, ui-automation, swift-package) |
| `apple-accessibility-auditor` | validation | sonnet / high | xcode | codex-terra | Agent, Edit, Write, NotebookEdit | mobilebuildmcp (simulator, ui-automation) |
| `app-release-preparer` | implementation | sonnet / medium | xcode | none | Agent + 19 `asc` patterns | none (`{}`) |
| `codex-swift-reviewer` | validation | gpt-5.6-sol / high | none | opus | (codex: none; `features.multi_agent=false`) | none |

`apple-accessibility-auditor` also has `available_skills:
[appkit-accessibility-auditor]`, the macOS counterpart that exists in
`skills/`.

## Decisions

- **operator_skills: left out.** `SkillCatalog::with_operator_skills`
  (`crates/horch-core/src/skills/catalog.rs:379-383`) bails when the
  directory is missing, and `--check` calls it
  (`roster/validation.rs:91-94`). On this host `~/.agents/skills` has no
  `swiftui-whats-new-27`. The persona comments in `swift-developer.md` and
  `apple-platform-developer.md`, and the README, say how the operator turns
  it on.
- **XcodeBuildMCP is now `mobilebuildmcp`, pinned at 2.7.1.** Evidence below.
- **`requires: [xcode]`** on the 6 Claude teammates (all build or run the
  simulator). `codex-swift-reviewer` reads only, so it has none.
- **Fallbacks.** Builders and QA: none, for the reason the UE builders have
  none (a Codex pane is untested with Xcode and CoreSimulator under
  `workspace-write`). Also: `routing::decision::merge` takes MCP servers, env
  and tool lists from the fallback, so a fallback loses the build server.
  The 2 read-only Claude seats fall back like `architect-reviewer` and
  `qa-engineer`; their personas say to report a review with no build.
  `codex-swift-reviewer` falls back to `opus`, like `codex-reviewer`.
  `app-release-preparer`: none, because the fallback would drop its env and
  deny list.
- **House style (§5 and the SwiftAgents list, in our words)** is in the
  persona bodies of the 2 builders, `swift-reviewer` and
  `codex-swift-reviewer`. `swift-qa-engineer` carries the `.serialized` rule;
  the auditor carries the Dynamic Type rule. `swift-developer` also covers
  `@ModelActor` and passing `PersistentIdentifier` across actors, because
  `swiftdata-pro` has no `@ModelActor` coverage (swift-core report follow-up).
- **Look at the running app before `DONE:`.** The 2 personas that list
  `ios-simulator-run` (`swift-developer`, `swift-qa-engineer`) say to build,
  run, and cite `snapshot_ui` or a screenshot. The auditor reads
  `snapshot_ui` on each screen.
- **`codex-swift-reviewer` model.** `gpt-5.6-sol` as §1 says. It is in
  `~/.codex/models_cache.json` on this host (with `gpt-5.6-terra`,
  `gpt-6-sol`, `gpt-6.1-sol` and others) and in `usage.rs` prices.

## MobileBuildMCP evidence (fetched 2026-10-03)

- `https://github.com/getsentry/XcodeBuildMCP` README: badge and text name
  `mobilebuildmcp`; "Most clients can also run the MCP server on demand via
  `npx -y mobilebuildmcp@latest mcp`".
- The docs page `clients.mdx`: `claude mcp add MobileBuildMCP -- npx -y mobilebuildmcp@latest mcp`.
- `npm view mobilebuildmcp`: latest `2.7.1`, repository
  `getsentry/MobileBuildMCP`, maintainers `sentry-bot`, `itaybren`, bin
  `mobilebuildmcp`. `npm view xcodebuildmcp`: latest `2.7.0`
  (2026-07-23), repository `getsentry/XcodeBuildMCP`. So the pin is
  `mobilebuildmcp@2.7.1`, launched as `npx -y mobilebuildmcp@2.7.1 mcp`.
- Tool names: from `npm pack mobilebuildmcp@2.7.1`, `package/manifests/tools/*.yaml`
  (82 tools) and `manifests/workflows/*.yaml`. Only `simulator` is
  `defaultEnabled`; `session-management` is `autoInclude`. `ui-automation`
  (`snapshot_ui`, `tap`, `screenshot`, `type_text`, `gesture`, `batch`) and
  `swift-package` are off by default. The env variable
  `MOBILEBUILDMCP_ENABLED_WORKFLOWS` (comma-separated) is in the docs page
  `env-vars.mdx` and 14 times in the 2.7.1 build.
- Not done: I did not run the server (conventions §2: no package scripts).

### Skill edits (orchestrator ANSWER: this unit owns them)

`grep -rni 'xcodebuildmcp|describe_ui|sim_log_cap|session-set-defaults|captureConsole|mcp__' skills/`
found 2 skills:

- `skills/ios-simulator-run/SKILL.md`:
  - `describe_ui` (2 places) became `snapshot_ui`.
  - `tap` takes one `elementRef` from the latest snapshot (2.7.1
    `tap.yaml`), not `id`, `label` or coordinates; `batch` for several taps.
  - `session-set-defaults` became `session_set_defaults`; `preferXcodebuild`
    is a session default.
  - `start_sim_log_cap` and `stop_sim_log_cap` do not exist in 2.7.1:
    `build_run_sim` and `launch_app_sim` capture logs and return the log path.
    `captureConsole` does not occur in the 2.7.1 build; removed.
  - A paragraph names the workflows to enable. Screenshot fallback:
    `xcrun simctl io <UDID> screenshot <file>.png` (`xcrun simctl io` help on
    this host).
- `skills/swift-code-audit/SKILL.md:41` and
  `references/pass-checklists.md:185`: "XcodeBuildMCP build tool" became
  "MobileBuildMCP build tool" with `build_sim` / `swift_package_build`.
- `skills/provenance.json`: the 2 `adaptation` strings only.

## `app-release-preparer`

### `env:` and `~`

- Finding: `teammate_env` (`harness/launch.rs`) set each value verbatim with
  `Command::env`; no shell runs, so `~` never expanded.
- asc needs it expanded: `cleanConfigPath` in asc 5.9.1
  `internal/config/config.go:331-337` rejects a relative `ASC_CONFIG_PATH`.
- Fix: `teammate_env(teammate, home)` expands a leading `~/` with
  `roster::expand_home`, against the same home the launch uses for plugin
  and settings paths. Both callers (`agent_command`, headless judge) pass
  `LaunchEnv::home()`. Test:
  `harness::launch::tests::teammate_env_expands_a_leading_tilde_against_home`
  (covers `~/x`, `a~/b`, `~`, no home, and the built-in
  `app-release-preparer`).

### Why `ASC_CONFIG_PATH`, not `ASC_PRIVATE_KEY_PATH`

The §6 sketch sets only `ASC_PRIVATE_KEY_PATH`. asc's credential matrix
(5.9.1 `authentication.mdx`, "Credential resolution") makes that unsafe: with
incomplete env (no `ASC_KEY_ID` and `ASC_ISSUER_ID`, which the committed file
cannot carry), asc selects the keychain or config profile first and env only
fills gaps. The operator's own, possibly admin, key would win. The same page
says: "To isolate a CI job, sandbox, agent, or test run from stored config,
set `ASC_CONFIG_PATH` to an absolute path ... and set `ASC_BYPASS_KEYCHAIN=1`."
So the file sets those 2. The config file holds the key path, never the key.
The plan's rule (a path, never a key) holds.

### First move

`xcodebuild -version` and `asc --version`; `test -f "$ASC_CONFIG_PATH"` and
the `private_key_path` in it; `BLOCKED:` with the missing path; then
`asc auth status`.

### Deny patterns

`asc` is not installed on this host (`which asc`: not found), and I did not
install it. I checked the names against the asc 5.9.1 source instead: the
Homebrew formula's tarball
`https://github.com/rorkai/App-Store-Connect-CLI/archive/refs/tags/5.9.1.tar.gz`
(sha256 `4440f3e6...1a31`, equal to the formula checksum), `commands/*.mdx`
and the command names in the Go sources. The file comment says so.

- §6's blanket `asc review *` and `asc submit *` would also block
  `asc review doctor`, `asc review status` and `asc submit status`, which
  `asc-submission-health` needs for the readiness audit. So the file denies
  the write subcommands by name: `submit create`, `submit cancel`,
  `review submit`, `review submit-for-review`, `review submissions-create`,
  `review submissions-submit`, `review submissions-cancel`,
  `review submissions-update`.
- Kept blanket from §6: `publish`, `web`, `pricing`, `certificates`.
  `pricing *` also blocks the read `asc pricing availability view`; I
  accepted that cost.
- Added: `release` (`release stage` / `release run`), `api` (raw API calls
  bypass every other pattern), `profiles`, `signing` (both create or change
  signing assets), `metadata push` (swift-ship report),
  `metadata apply *--confirm*` (the live form; the dry run stays allowed),
  and `*-allowProvisioningUpdates*` (swift-ship report; the persona also
  forbids it unless the assignment names it).
- Not verified: that Claude Code matches a `*` in the middle or at the start
  of a `Bash(...)` pattern. Running `claude` to test it is outside this
  unit's rules. If it does not, the last 2 patterns deny nothing, and the
  persona rule and the API key role remain.
- Left allowed: `review details-*`, `review items-*`,
  `review attachments-*`. They change a draft, not a submission, and the
  skills gate them on the orchestrator.

## Step 4: the orchestrator briefing, with and without `Package.swift`

Method: the P-UE script from `ue-teammates.md`, with `Package.swift` instead
of `Game.uproject` (`/tmp/swift-demo.sh`). The built `horch fleet` runs
against the fake `herdr` (scenario `exec`) and fake `claude` under `env -i`.
The briefing is the longest argument of the `claude` call with
`--session-id`.

With `Package.swift` (`/tmp/swift-demo-apple.6j87`):

```
orchestrator briefing bytes: 16670
Apple roster lines: 7
  app-release-preparer         App Store release prep - archive, internal TestFlight upload, review-readiness audit, notes. Never submits.
  apple-accessibility-auditor  Apple accessibility and UI-copy audit - VoiceOver, Dynamic Type, Voice Control, labels. P0-P2 findings, no edits.
  apple-platform-developer     Apple system surfaces - App Intents/Siri, widgets, Live Activities, background tasks, tvOS/visionOS focus.
  codex-swift-reviewer         Cross-vendor Swift review on Codex Sol - concurrency and modern SwiftUI. Use on Claude-built Swift changes.
  swift-developer              Swift/SwiftUI app features on Apple platforms - views, SwiftData, Swift 6 concurrency. Needs macOS + Xcode.
  swift-qa-engineer            Swift Testing suites, XCTest migration, simulator runs and bug repros for Apple apps. Needs macOS + Xcode.
  swift-reviewer               Read-only Swift review - concurrency safety, modern SwiftUI API, performance, Keychain/crypto. Never edits.
```

Without it (`/tmp/swift-demo-plain.rEYz`, empty project):

```
orchestrator briefing bytes: 15518
Apple roster lines: 0
```

The 7 lines cost the orchestrator 1,152 bytes on an Apple project and 0
elsewhere. §1 estimated about 700 bytes for 6 lines; the padded name column
and the longer descriptions make each line about 165 bytes.

Expected-skill description bytes per teammate (the worker briefing names
each with its description; measured from each `SKILL.md` `description:`):

| teammate | skills | bytes |
|---|---|---|
| `swift-developer` | 9 | 2,150 |
| `apple-platform-developer` | 7 | 2,854 |
| `swift-reviewer` | 7 | 1,669 |
| `swift-qa-engineer` | 6 | 996 |
| `apple-accessibility-auditor` | 3 | 734 (+ 1 name) |
| `app-release-preparer` | 8 | 2,028 |
| `codex-swift-reviewer` | 2 | 325 |

## Test changes

- `SKIP_NEW_TEAMMATES` in `tests/baseline_oracles.rs` and
  `tests/skills_catalog.rs`: the 7 names (sorted). No oracle or golden
  changed.
- `roster/validation.rs`: validation arm gets `swift-reviewer`,
  `swift-qa-engineer`, `apple-accessibility-auditor`,
  `codex-swift-reviewer`. Claude count 35 to 41 (the Codex seat does not
  count).
- `crates/horch/src/cmd/doctor.rs` (outside the plan's list, changed because
  the gate failed at its root): `xcode_is_not_checked_unless_an_offered_teammate_needs_it`
  assumed no built-in teammate requires Xcode. It now runs the built-in
  roster with a `Cargo.toml` project (no check) and a `Package.swift`
  project (1 problem). `xcode_first_launch_pending_is_reported` matched
  "needed by swift-developer"; the line now lists the built-in Apple seats
  first, so it checks "(needed by " and "swift-developer" apart.

## Gotchas and follow-ups

- After the rebase onto `723e958` (D19 merged), the gate runs
  `cargo clippy --workspace --all-targets -- -D warnings`, and it passes.
- MobileBuildMCP sends error telemetry to Sentry by default
  (`MOBILEBUILDMCP_SENTRY_DISABLED=true` turns it off), and asc has
  `ASC_TELEMETRY_DISABLED`. I left both at the upstream default. Decide
  fleet-wide.
- The asc deny list matches 5.9.1. Check it again with `asc --help` after an
  upgrade.
- Mid-pattern `*` support in Claude Code deny rules is not verified (see
  above).

## Settled after merge (orchestrator, 2026-10-04)

- Mid-pattern `*`: verified. Claude Code's permissions page ("Wildcard
  patterns") says `*` stands in for any text at any position, with examples
  `Bash(git * main)` and `Bash(* --version)`. The 2 mid-pattern deny rules
  in `app-release-preparer` match.
- Telemetry: off fleet-wide for these tools. The 5 teammates with
  mobilebuildmcp set `MOBILEBUILDMCP_SENTRY_DISABLED: "true"` (2.7.1
  `build/utils/sentry.js:95` compares with the string `"true"`).
  `app-release-preparer` sets `ASC_TELEMETRY_DISABLED: "1"` (asc 5.9.1
  `internal/telemetry/state.go:397`, `envTruthy`).
