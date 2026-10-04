# Swift developers for the fleet: skills and personas

Date: 2026-10-03. Research only. Nothing in `teammates/`, `skills/` or
`crates/` changed.

Sources read:
- the index, swift-agent-skills
  at `a6d22db72162`;
- all 32 repositories it links, cloned at the revisions in
  [Appendix A](#appendix-a-pinned-upstream-revisions);
- Apple's Xcode 27 skills, read from a third-party re-export for reference
  only.

Every SKILL.md was read, and the references were skimmed. Four upstream errors
that this report turns into fix instructions were checked again against the
source (marked **checked**).

## 1. Recommendation

Add **six teammates**: four builders and reviewers, one auditor and one
release preparer. Back them with **about 27 vendored skills**, attached by
name rather than by phase. Swift work also needs three things from the
operator and horch:

- a macOS host with Xcode;
- XcodeBuildMCP on the panes that build;
- a small horch change so the operator's own Xcode 27 Apple skills can be
  attached. Those skills cannot be redistributed.

| teammate | agent / model / effort | phase | the job it is picked for |
|---|---|---|---|
| `swift-developer` | claude / opus / medium | implementation | SwiftUI app features, SwiftData, Swift 6 concurrency |
| `apple-platform-developer` | claude / opus / medium | implementation | App Intents and Siri, widgets and Live Activities, background tasks, tvOS/visionOS focus |
| `swift-reviewer` | claude / opus / high | validation | Read-only Swift review: concurrency, modern API, performance, Keychain/crypto |
| `swift-qa-engineer` | claude / sonnet / high | validation | Swift Testing, XCTest migration, simulator runs, bug repros |
| `apple-accessibility-auditor` | claude / sonnet / high | validation | VoiceOver, Dynamic Type, Voice Control, UI copy. Reports, never edits |
| `app-release-preparer` | claude / sonnet / medium | implementation | Archive, internal TestFlight upload, review-readiness audit, release notes. Never submits |

Effort follows the role, as in
[model-guide-2026-09.md](model-guide-2026-09.md): builders run at medium,
reviewers and QA at high. The orchestrator pays about one roster line per
teammate, roughly 110 bytes each, so six teammates cost it about 700 bytes.

**Optional seventh:** `codex-swift-reviewer` (codex / gpt-5.6-sol / high),
carrying `swiftui-pro` and `swift-concurrency-pro`. It is the cross-vendor
counterpart to `swift-reviewer`. It needs its own file because
`horch spawn` has no `--skill` flag, so the existing `codex-reviewer` cannot
pick up Swift skills per spawn.

## 2. What decided fit

A skill can be vendored only if it survives the fleet's rules:
- **No subagents.** `README.md`, "No fleet pane spawns subagents".
- **No human at the pane.** An interactive gate must become a message to the
  orchestrator.
- **Self-contained and harness-neutral.** `skills/README.md`.
- **Valid name and description.** `name` must equal the directory name, and
  the description must be at most 1024 characters
  (`crates/horch-core/src/skills.rs` `catalog()`).

On top of those rules, three facts shaped this report.

1. **Copies.** Apple's Xcode 27 skills belong to Apple and stay outside the
   bundle; teammates load them as operator skills.
2. **Not in the phase catalogs.** Swift skills must not go into
   `phase_skills()`, or every Python or Rust worker spawned with
   `--phase implementation` would carry SwiftUI descriptions. Attach them by
   name with `skills:`, the same way `orchestrate` is attached.
3. **Size.** The current bundled skills are about 350-400 words each, and
   `skills/` totals 416 KB. The shortlist below is about 1.86 MB of markdown,
   all compiled into `horch`. Bodies and references load on demand, so the
   cost a pane pays every turn is descriptions only. For `swift-developer`
   that is the 468-byte implementation catalog plus about 1.75 KB of Swift
   descriptions, roughly 550 tokens.

## 3. The two sources

**The community index** links 35 skills. Two thirds of them were updated in
the last two months. The `*-pro` skills are short, terse rule lists,
human-written and accurate, and form the core. One
platform skills are the freshest and densest: they cover the iOS 27 APIs from
WWDC26. One repository mixes useful SwiftUI skills
with four that fan out to subagents.

**Apple's Xcode 27 skills** came out after most of that list. Xcode 27
shipped on 2026-09-14 with seven first-party skills, which the operator can
export with `xcrun agent skills export <dir>`:

| skill | words | notes |
|---|---|---|
| `swiftui-whats-new-27` | 10.2k | The only source on the iOS 27 SwiftUI APIs (the `@State` macro, `reorderable()`, document protocols). Every community skill targets iOS 26 / Swift 6.2 |
| `swiftui-specialist` | 21.8k | Overlaps `swiftui-pro`. Authoritative, but large |
| `test-modernizer` | 1.3k | Overlaps `swift-testing-pro` |
| `uikit-app-modernization` | 12.4k | Only for UIKit codebases |
| `audit-xcode-security-settings` | 8.8k | Prefers Xcode's own MCP tools (`XcodeRead`, `XcodeGrep`) |
| `c-bounds-safety` | 19.4k | Only for C code |
| `device-interaction` | 1.1k | **Unusable here.** Its body says "This is a SUBAGENT skill. Invoke it via the Agent tool", and it drives `DeviceInteraction*` tools that only Xcode's MCP bridge provides |

These skills cannot be vendored. They are exported on the operator's own Mac
and versioned with Xcode, so horch needs a way to attach a skill from an
operator directory; see section 7.2. Start with `swiftui-whats-new-27` and
`test-modernizer`. Run `swiftui-specialist` against `swiftui-pro` and let
`horch cost`'s skills table decide which one to keep.

## 4. Skill verdicts

Key:
- **Vendor**: a verbatim copy plus provenance, as `skill-creator` is done.
- **Adapt**: a curated copy with the listed edits, as the existing bundles
  are done.
- **Reference**: read it, but don't ship it.
- **Skip**: not worth carrying.

### 4.1 Core Swift and SwiftUI

| skill (upstream) | verdict | edits | teammates |
|---|---|---|---|
| `swiftui-pro` | **Adapt, light** | Use the top-level copy, not `skills/swiftui-pro/`. That copy is an older Claude-plugin variant using `${CLAUDE_SKILL_DIR}`. Change "without asking first" (`SKILL.md:32`) and "Offer to translate" (`hygiene.md:8`) to messages to the orchestrator. Replace the GitHub links at `data.md:37` and `swift.md:56` with vendored skill names. Make `views.md:8` ("place view logic into view models") neutral, per section 5 | developer, reviewer |
| `swift-concurrency-pro` | **Adapt** | **Fix `testing.md:35` (checked):** it says `.serialized` on a suite only serializes parameterized tests, but on a suite it serializes every test inside. In `new-features.md`, say that caller-actor `nonisolated` async needs `NonisolatedNonsendingByDefault` / Approachable Concurrency to be on. Add build-settings table (`SKILL.md:16-24`) as a reference. Turn the ask gate into a message | developer, platform, reviewer |
| `swiftdata-pro` | **Adapt, light** | Turn the ask gate into a message and repoint the external links. Add `migrations-and-history.md` and `core-data-adoption.md` as extra references, because swiftdata-pro has no migrations, history or `@ModelActor` coverage. Strip `#Unique` advice, which has no CloudKit caveat | developer |
| `swift-testing-pro` | **Vendor** (top-level copy) | Optional: one line on headless runs (`swift test --filter`, `xcodebuild test -only-testing:`), and a note that exit tests don't run on iOS | developer, qa |
| `swift-format-style` | **Adapt** | **Fix `anti-patterns.md:80-88` (checked):** a fixed wire format with `calendar: .current` breaks for users on non-Gregorian calendars; use `.iso8601` or a POSIX locale with a Gregorian calendar. Soften "Never use legacy Formatter" to "for display". Narrow the description to formatting shown to the user | developer |
| `swiftui-liquid-glass` | **Adapt** | Fix the background-extension snippet, which never calls `.backgroundExtensionEffect()` (`liquid-glass.md:204-215`). Replace `foregroundColor` at `:252`. Verify `scrollExtensionMode` and delete it if it doesn't exist. Delete the "always seek guides" line at `:5` | developer |
| `observability` | **Vendor** | none | developer, platform |
| `swiftui-view-refactor` | **Reference** | Fold its member-ordering and stable-view-tree rules into the house style (section 5) instead of carrying a second "review SwiftUI" skill | - |
| `swiftui-ui-patterns` | **Reference** | Its snippets use APIs that `swiftui-pro` flags (`tabItem`, `cornerRadius`, `DispatchQueue.main.asyncAfter`). It also tells the agent to edit its own `references/` | - |
| `swift-concurrency` | **Reference** | 30k words and 67 course links. It contradicts swift-concurrency-pro on `@concurrent` for I/O (`SKILL.md:96-101`, checked). Take only the settings table | - |
| `swift-concurrency-expert` | **Skip** | Wrong isolated-conformance syntax (`SKILL.md:71-74`), and a `Task.detached` "after" example. swift-concurrency-pro covers all of it | - |
| `swiftdata-expert-skill` | **Reference** | Two references go into `swiftdata-pro`, as above. Its example model doesn't compile and declares uniqueness twice | - |
| `swift-api-design-guidelines-skill` (Erikote04) | **Skip** | Restates swift.org. "Every declaration should have a documentation comment" produces noise in app code | - |
| `swift-architecture-skill` (efremidze) | **Reference** | Defaults to MVVM and never offers MV (`selection-guide.md:60-61`). Its 8 scripts only maintain that repository | - |
| `swiftui-design-principles` (arjitj2) | **Skip** | Hard-coded `.system(size:)` fonts break Dynamic Type. Its description triggers on any SwiftUI view | - |
| `core-data-expert` | **Reference**, or vendor if a Core Data app arrives | Before shipping it, fix the double continuation resume in the async `loadPersistentStores` bridge (`concurrency.md:129-142`) | - |

### 4.2 Apple system surfaces

| skill | verdict | edits | teammates |
|---|---|---|---|
| `app-intents` | **Vendor** | none. Covers the iOS 27 `LongRunningIntent` and `AppIntentsTesting` | platform |
| `widgets` | **Vendor** | Optionally trim the 720-character description | platform |
| `background-execution` | **Vendor** | none | platform |
| `swift-focusengine-pro` | **Vendor** | Rename the directory to the skill name. Move `version`/`author`/`tags` under `metadata` if a loader rejects them | platform |
| `macos-spm-app-packaging` | **Adapt**, only if the fleet ships macOS apps outside the App Store | `sign-and-notarize.sh:16` writes a .p8 from an env var to `/tmp`; use `--key "$ASC_PRIVATE_KEY_PATH"` instead. Gate the `git push` / `gh release create` steps | (developer) |
| `macos-menubar-tuist-app` | **Reference** | Only useful with Tuist | - |
| `figma-to-swiftui` | **Reference** | Route its gates to the orchestrator, and attach it only to a pane that has the Figma MCP server | - |

### 4.3 Review, testing, accessibility, security

| skill | verdict | edits | teammates |
|---|---|---|---|
| `swiftui-performance-audit` | **Adapt** | The intake says "ask the user to run Instruments" (`SKILL.md:16,28,56,85`). Change it to: the worker runs `xcrun xctrace record --template SwiftUI` itself, or messages the orchestrator. Use `Text(value, format:)` instead of the cached shared formatter (`code-smells.md:22`) | reviewer |
| `swift-security-expert` | **Adapt** | Cut the 5.2k-word body by about 40% (the tone, scope and self-review sections). Optionally drop `compliance-owasp-mapping`, `testing-security-code` and `migration-legacy-stores`. Accurate and specific: it binds biometrics to `SecAccessControl`, says add-or-update on `errSecDuplicateItem`, and gives availability for ML-KEM and ML-DSA | reviewer |
| `ios-code-audit` → `swift-code-audit` | **Adapt, heavily** | Its three parallel Explore agents (`SKILL.md:58-60`) become three sequential passes. Write the report to a path the orchestrator chooses, not `CODE_AUDIT.md` in the repository root. Drop the `swiftui-expert-skill` step. Keep these parts: compiler warnings are the ground truth, "verify every Critical claim", grouping findings by root cause, and its concurrency and API-modernity checklist | reviewer |
| `swiftdata-testing` | **Adapt**, or skip | **Fix `decimal-money-values.md:8-23` (checked):** a `Decimal` literal goes through `Double`, and `Decimal` division is not exact. Make the MVVM and mock-repository section conditional | qa |
| `ios-debugger-agent` → `ios-simulator-run` | **Adapt** into a short playbook over XcodeBuildMCP | Use neutral tool names, boot a simulator itself, and report failures to the orchestrator instead of "ask the user" (`:16`, `:49`) | qa, developer |
| `ios-simulator-skill` (conorluddy) | **Skip** for now | Needs `brew install idb` and `pip install pillow`. Starts detached recorders, and has `simctl erase`/`delete` scripts. XcodeBuildMCP covers the same ground | - |
| `swift-testing-expert`, `swift-testing` (bocato) | **Reference** / **Skip** | Swift-Concurrency-Agent-Skill adds only test-plan and tag conventions. Bocato is stale (`XCTestDynamicOverlay`, `record: true`), and its description fires on "unit tests"/"TDD", so it would take over from the fleet's `tdd` | - |
| `swiftui-` and `uikit-accessibility-auditor` | **Adapt**, path only | `skills/<name>/checklist.md` becomes `checklist.md`. Freshest of the five accessibility skills: it has a WWDC26 readiness section and a P0/P1/P2 output contract | a11y |
| `appkit-accessibility-auditor` | **Adapt**, only for macOS work | same path fix | (a11y) |
| `ios-accessibility` (dadederk) | **Reference** | Deep, but stops at iOS 17 and has developer-confirmation gates. Its Large Content Viewer and `accessibilityInputLabels` material is the only coverage of those | - |
| `swift-accessibility-skill` | **Reference, never vendor with this description** | "always use together… even when the user doesn't mention accessibility" means about 3.5k tokens on every view edit, silent fixes and a summary appended to every output. Copy its `nutrition-labels.md` and `performAccessibilityAudit` material into the auditor's references | - |
| `writing-for-interfaces` | **Adapt** | Remove `context: fork`, a Claude-only field that forks a subagent. Cut the 828-character description to about 250, without the "trigger whenever" wording. Replace the voice interview (`:54-74`) with: infer the voice from the codebase, else ask the orchestrator | a11y |
| Skills `bug-hunt-swarm`, `review-swarm`, `orchestrate-batch-refactor`, `review-and-simplify-changes` | **Skip** | Each one fans out to subagents | - |
| Skills `project-skill-audit`, `github`, `react-component-performance` | **Skip** | Codex-only, generic, or not Swift | - |

### 4.4 Shipping

| skill | verdict | edits | teammates |
|---|---|---|---|
| `asc-cli-usage`, `asc-id-resolver`, `asc-crash-triage`, `asc-xcode-build` | **Adapt, light** | Drop "in this repo" from the `cli-usage` description, and its web-auth and Apple Ads sections | release |
| `asc-submission-health` | **Adapt** | Keep the diagnosis half only. Each cancel, retry or submit step becomes "send the dry-run to the orchestrator" | release |
| `asc-metadata-sync` | **Adapt** | Dry-run only. `asc metadata push` without `--confirm` still writes live metadata (`SKILL.md:61`), so the "`--confirm` gates mutation" assumption fails here | release |
| `appstore-review` (3paws) | **Adapt, light** | Flatten the folded description and drop `trigger: manual` / `agents:` if the loader is strict. Report checks that need runtime as UNVERIFIED. Downgrade the style rules ("no force unwraps") from FAIL to WARN | release |
| `app-store-changelog` | **Adapt**, one line | "Ask for clarification" (`:31`) becomes a message. Its script is read-only `git log` | release |
| `asc-release-flow`, `asc-signing-setup` | **Reference** | Each submits, revokes or deletes | - |
| `asc-ppp-pricing`, `asc-apple-ads`, `asc-ad-hoc-distribution`, `asc-app-create-ui`, `asc-wall-submit`, `asc-revenuecat-catalog-sync`, `asc-shots-pipeline`, `asc-aso-audit`, `asc-build-lifecycle` | **Skip** | They change prices, spend on ads, publish outward or need extra MCP servers and tools. `build-lifecycle` expires builds, which cannot be undone | - |
| `app-store-aso` (timbroddin) | **Adapt**, optional | Its validator prompts on stdin with no arguments (`validate_metadata.py:145-147`) and hangs a pane. Strip the Krankie install. Mark the speculative ranking claims as heuristics | (release) |

## 5. House style the skills disagree on

The vendored skills contradict each other in five places. Settle each once,
in the Swift personas, so a worker never picks between two loaded skills on
its own. The adaptations in section 4 already apply these answers.

| question | upstream positions | fleet answer |
|---|---|---|
| View architecture | MV with no view models (Skills view-refactor, ui-patterns); view models (`swiftui-pro` `views.md:8`, `efremidze` default, `swiftdata-testing`) | **Follow the codebase. In a new codebase, MV** with `@Observable` models and `@Query` in views. Add view models only for logic that needs a test seam |
| `@concurrent` | Not for ordinary async I/O (swift-concurrency-pro); for a network call | **Only for CPU-bound work** that must leave the caller's actor |
| `.serialized` | Parameterized only, even on a suite (swift-concurrency-pro, wrong); the whole suite (Swift-Concurrency-Agent-Skill, Apple) | **On a suite it serializes every test in it** |
| Formatting | Cached shared `NumberFormatter` (performance-audit); `Text(value, format:)` (`swiftui-pro`, App-Intents-Agent-Skill) | **FormatStyle for display**; a fixed POSIX locale for wire formats |
| Sizes | Fixed fonts and radii (arjitj2); exact Figma values; Dynamic Type (`swiftui-pro`) | **Dynamic Type and semantic styles.** Use exact values only where a design spec gives them, and never for text size |

The persona bodies should also carry the rules from `SwiftAgents`,
rewritten in our own words:
- `@MainActor` on `@Observable` classes unless the project defaults to main
  actor isolation;
- no GCD; `Task.sleep(for:)`;
- `NavigationStack` with `navigationDestination`;
- no `AnyView` and no `UIScreen.main`;
- under CloudKit, no `@Attribute(.unique)`, and every property optional or
  with a default.

## 6. The teammates

Each sketch shows only what differs from `_template.md`. All six set:
- `base: fleet-worker`
- `agent: claude`
- `inherit_plugins: false`
- `disallowed_tools: [Agent]`
- `disabled_skills: [herdr-orchestrator, herdr-worker]`

All six also need a mandatory first move, which is a persona instruction
written per teammate: check the toolchain (`xcodebuild -version`,
`swift --version`, `xcrun simctl list devices available`) and report
`BLOCKED` if it is missing. The checks below assume the toolchain is there.
The roster cannot see the host, and a Linux pane can do no more than
server-side SwiftPM.

```yaml
# swift-developer.md
brief_description: Swift/SwiftUI app features on Apple platforms - views, SwiftData, Swift 6 concurrency. Needs macOS + Xcode.
phase: implementation
model: opus
effort: medium
permission_mode: auto
skills: [tdd, swiftui-pro, swift-concurrency-pro, swiftdata-pro, swift-testing-pro,
         swift-format-style, swiftui-liquid-glass, observability, ios-simulator-run]
mcp_servers:
  xcodebuildmcp: {"type":"stdio","command":"npx","args":["-y","xcodebuildmcp@latest","mcp"]}
  context7: {"type":"stdio","command":"npx","args":["-y","@upstash/context7-mcp"]}
```

```yaml
# apple-platform-developer.md
brief_description: Apple system surfaces - App Intents/Siri, widgets, Live Activities, background tasks, tvOS/visionOS focus.
phase: implementation
model: opus
effort: medium
permission_mode: auto
skills: [tdd, app-intents, widgets, background-execution, swift-focusengine-pro,
         swift-concurrency-pro, observability]
mcp_servers: {xcodebuildmcp: ..., context7: ...}   # as above
```

```yaml
# swift-reviewer.md - read-only, like architect-reviewer
brief_description: Read-only Swift review - concurrency safety, modern SwiftUI API, performance, Keychain/crypto. Never edits.
phase: validation
model: opus
effort: high
permission_mode: auto
disallowed_tools: [Agent, Edit, Write, NotebookEdit]
skills: [code-review, security-review, swiftui-pro, swift-concurrency-pro,
         swiftui-performance-audit, swift-security-expert, swift-code-audit]
mcp_servers: {xcodebuildmcp: ...}   # compiler warnings are swift-code-audit's ground truth
```

```yaml
# swift-qa-engineer.md
brief_description: Swift Testing suites, XCTest migration, simulator runs and bug repros for Apple apps. Needs macOS + Xcode.
phase: validation
model: sonnet
effort: high
permission_mode: auto
skills: [check, debug, tdd, swift-testing-pro, swiftdata-testing, ios-simulator-run]
mcp_servers: {xcodebuildmcp: ...}
```

```yaml
# apple-accessibility-auditor.md - read-only
brief_description: Apple accessibility and UI-copy audit - VoiceOver, Dynamic Type, Voice Control, labels. P0-P2 findings, no edits.
phase: validation
model: sonnet
effort: high
permission_mode: auto
disallowed_tools: [Agent, Edit, Write, NotebookEdit]
skills: [swiftui-accessibility-auditor, uikit-accessibility-auditor, writing-for-interfaces]
mcp_servers: {xcodebuildmcp: ...}   # describe_ui gives the live accessibility tree
```

```yaml
# app-release-preparer.md
brief_description: App Store release prep - archive, internal TestFlight upload, review-readiness audit, notes. Never submits.
phase: implementation
model: sonnet
effort: medium
permission_mode: auto
skills: [asc-cli-usage, asc-id-resolver, asc-xcode-build, asc-crash-triage,
         asc-submission-health, asc-metadata-sync, appstore-review, app-store-changelog]
# A second line of defence only. A Bash deny pattern can be bypassed with `sh -c`.
disallowed_tools: [Agent, "Bash(asc review *)", "Bash(asc publish *)", "Bash(asc submit *)",
                   "Bash(asc web *)", "Bash(asc pricing *)", "Bash(asc certificates *)"]
env: {ASC_PRIVATE_KEY_PATH: "~/.config/horch/asc/release-preparer.p8"}  # a path, never the key
mcp_servers: {}
```

**Release safety lives in the API key, not in the persona.** Give
`app-release-preparer` an App Store Connect key whose role cannot submit for
review or change prices, in a file the operator owns. Give it no `asc web`
session: that one needs interactive 2FA. Its persona ends every
outward-facing step by sending the dry-run plan to the orchestrator, and a
human runs the final `--confirm` command. Before shipping it, verify:
- which `asc` subcommand names the deny patterns should match, against the
  installed `asc --help`;
- that `~/` expands inside `env:`.

**Persona bodies** follow the existing style: identity, scope and standing
constraints, and no protocol. `swift-developer` carries the section 5 house
style. Each persona that lists `ios-simulator-run` says to look at the running
app before reporting `DONE:`, as `frontend-developer` does with its browser.

## 7. What horch and the operator need

### 7.1 The host and MCP servers
- **macOS with Xcode 26 or later.** Xcode 27 gives the iOS 27 SDK. Xcode's
  first run has to be done once (`sudo xcodebuild -runFirstLaunch`),
  otherwise `xcodebuild` stalls a pane at the licence prompt. `horch doctor`
  could check for `xcodebuild` when any `swift-*` or `apple-*` teammate is in
  the roster.
- **XcodeBuildMCP** (v2.7.0, 2026-07-23). It builds, tests, runs and drives the simulator,
  and works with Xcode 27's Device Hub. It runs without Xcode open, which is
  why it suits unattended panes.
- **Xcode's own MCP bridge** (`xcrun mcpbridge`, Xcode 26.3 and later). It
  needs Xcode running with the project open, a setting switched on, and an
  approval dialog accepted. That doesn't fit an unattended pane, so leave it
  out for now. Apple's `audit-xcode-security-settings` prefers its tools but
  has a filesystem fallback.

### 7.2 Attaching operator-local Apple skills
Bundled skills are compiled in, and Apple's cannot be.
- **Claude** can use `plugin_dirs` today if the exported directory is wrapped
  as a plugin: a `.claude-plugin/plugin.json` plus `skills/`. Then
  `plugin_skills` exposes only the named skills.
- **Codex, OpenCode and pi** have no equivalent.

Proposed field, which horch materializes into the launch bundle in place of
the compiled-in copies:

```yaml
operator_skills:
  dir: ~/.agents/skills          # where `xcrun agent skills export` put them
  names: [swiftui-whats-new-27, test-modernizer]
```

- `--check` would fail on a missing directory or name, the same way it
  handles `plugin_dirs`.
- `device-interaction` stays out: it is a subagent skill.

### 7.3 Provenance
`provenance.json` already allows a per-skill `source_repository` and
`source_revision`; `skill-creator` uses them. Add one entry per vendored
skill, using the revisions in Appendix A and the SHA-256 of each upstream
SKILL.md.

## 8. Rollout order

Each step is useful on its own.

1. **Vendor the eight skills that need little or no editing**: `swift-testing-pro`,
   `observability`, `app-intents`, `widgets`, `background-execution`,
   `swift-focusengine-pro`, and the two apple-accessibility-skills auditors with the path fix.
   Then make the light adaptations: `swiftui-pro`, `swift-concurrency-pro`
   (the two fixes and the settings table) and `swiftdata-pro`.
2. **Add `swift-developer` and `swift-reviewer`.** Run one real task on a Mac
   host. Read `horch cost`'s skills table to see which expected skills were
   actually loaded.
3. Add `swift-qa-engineer` with XcodeBuildMCP and `ios-simulator-run`, and
   `apple-platform-developer`.
4. Make the heavier adaptations (`swift-code-audit`, `swift-security-expert`,
   `swiftui-performance-audit`, `writing-for-interfaces`) and add
   `apple-accessibility-auditor`.
5. **Add `operator_skills`** and attach `swiftui-whats-new-27` to the
   developers.
6. **Add `app-release-preparer` last**, after the operator has created a
   restricted-role key.

## 9. Not verified

- No skill was run. Every verdict comes from reading, and the four "checked"
  items were checked against the upstream text and Apple's documented
  behaviour, not by compiling code.
- Upstream claims about iOS 27 APIs (App-Intents-Agent-Skill `widgets` `allowedExecutionTargets`,
  `app-intents` `AppIntentsTesting`) and `scrollExtensionMode`
  were not checked against the SDK.
- The Xcode 27 skills list and word counts come from a third-party re-export
  dated 2026-06-09, which is a beta. The operator's own export may differ.
- The XcodeBuildMCP launch arguments (`... mcp`) and its tool names should be
  confirmed against the installed version.
- Whether Claude's `disallowed_tools` Bash patterns accept the `asc *` forms
  above.

## Appendix A: pinned upstream revisions

| repository | revision | used for |
|---|---|---|
| swift-agent-skills | `a6d22db72162` | index |
| SwiftUI-Agent-Skill | `be297ff80ddd` | swiftui-pro |
| Swift-Concurrency-Agent-Skill | `bee3f69ba171` | swift-concurrency-pro |
| SwiftData-Agent-Skill | `922d989473a9` | swiftdata-pro |
| Swift-Testing-Agent-Skill | `2d6bba14a3c8` | swift-testing-pro |
| Swift-FormatStyle-Agent-Skill | `5cda783ac709` | swift-format-style |
| Observability-Agent-Skill | `4ab4f371b03b` | observability |
| App-Intents-Agent-Skill | `f754f08c5ef4` | app-intents |
| Widgets-Agent-Skill | `06567e152df0` | widgets |
| Background-Execution-Agent-Skill | `a29ddf879308` | background-execution |
| Swift-FocusEngine-Agent-Skill | `903d58d0ed03` | swift-focusengine-pro |
| Skills | `05ba982bfeb0` | swiftui-liquid-glass, swiftui-performance-audit, ios-debugger-agent, app-store-changelog |
| apple-accessibility-skills | `ab6f67852f5d` | swiftui-, uikit-, appkit-accessibility-auditor |
| swift-security-skill | `bda2e0ccee6a` | swift-security-expert |
| ios-code-audit | `34edb296a150` | swift-code-audit |
| ios-swiftdata-testing-agent-skill | `e8a98757b3e6` | swiftdata-testing |
| skills | `d6c72b885a19` | writing-for-interfaces |
| mobile-ai-skills | `6d3570eab02b` | appstore-review |
| app-store-connect-cli-skills | `9a093fa52177` | asc-* |
| swiftdata-agent-skill | `4819a5381fa5` | two references into swiftdata-pro |
| Swift-Concurrency-Agent-Skill | `d5770817d262` | settings table into swift-concurrency-pro |
| PasqualeVittoriosi/swift-accessibility-skill | `3791850af76f` | nutrition-label material for the auditor |
| dadederk/iOS-Accessibility-Agent-Skill | `dcc3a36ce1d0` | reference |

## Sources

- swift-agent-skills and every repository it links (Appendix A)
- [superagents-lab/xcode27-skills](https://github.com/superagents-lab/xcode27-skills): a re-export of Apple's Xcode 27 skills, read for reference only
- [getsentry/XcodeBuildMCP](https://github.com/getsentry/XcodeBuildMCP) and its [npm package](https://www.npmjs.com/package/xcodebuildmcp)
- Xcode 26.3 MCP bridge: [InfoQ](https://infoq.com/news/2026/02/xcode-26-3-agentic-coding), [rudrank.com](https://rudrank.com/exploring-xcode-using-mcp-tools-cursor-external-clients)
- Xcode 27 release and its skills: [spaceport.build](https://spaceport.build/blog/whats-new-xcode-27), [Livsy Code](https://livsycode.com/best-practices/the-xcode-27-agent-skills/)
