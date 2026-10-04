# swift-platform report (unit S2)

Branch `ds/swift-platform`. Skills: `app-intents`, `widgets`, `background-execution`, `swift-focusengine-pro`.

## Verdict applied

All four skills: **Vendor** (report section 4.2). Each skill is a verbatim copy with `vendored: true`. The copy holds `SKILL.md`, `references/*.md` and the upstream `LICENSE`. The copy excludes `agents/openai.yaml`, plugin manifests, images and dotfiles. Every copied file is byte-identical to the pin (the sha256 of each file is in `skills/provenance.json`).

| skill | upstream pin | SKILL.md bytes | directory bytes | files |
|---|---|---|---|---|
| `app-intents` | n0an/App-Intents-Agent-Skill@f754f08 | 24151 | 269790 | 15 |
| `background-execution` | n0an/Background-Execution-Agent-Skill@a29ddf8 | 15833 | 115644 | 13 |
| `swift-focusengine-pro` | mhaviv/Swift-FocusEngine-Agent-Skill@903d58d | 11001 | 200750 | 16 |
| `widgets` | n0an/Widgets-Agent-Skill@06567e1 | 26585 | 190736 | 21 |

## Edits made

- `app-intents`, `widgets`, `background-execution`: none. Verbatim.
- `swift-focusengine-pro`: none to content. The directory name is the skill name (upstream uses the repository root). The horch frontmatter parser (`crates/horch-core/src/skills/catalog.rs:423`, `validate_skill_md`) reads only `name` and `description` and ignores other keys. It accepts `version`, `author` and `tags` unchanged, so nothing moved under `metadata`.
- Not edited: `widgets` description (720 characters; the report marks the trim optional, and the skill is verbatim). The description is under the 1024-byte limit.

## Fleet-rule scan (verbatim copies, not edited)

A grep for `subagent`, `context: fork`, `ask the user`, `${CLAUDE_`, `/Users/`, `Task tool` and `AskUserQuestion` finds 2 files with matches. All matches are advice about the app's own runtime UI, not an instruction to the worker:
- `skills/app-intents/references/siri-intelligence.md:106`: clarifying questions in an intent.
- `skills/app-intents/references/fundamentals.md:335-336`: `RequestDisambiguationError`, `ConfirmationRequiredError`.

No skill has `context: fork`, a subagent instruction, a machine path or a `${CLAUDE_*}` variable. No link to another upstream repository was found.

## Verify items (iOS 27 claims)

Evidence: Apple's DocC JSON endpoint, fetched 2026-10-03 (the HTML pages need JavaScript). Each item is documented, `introducedAt` 27.0, not beta-flagged.

| claim | where in skill | result | URL |
|---|---|---|---|
| `LongRunningIntent` exists, iOS 27 | `app-intents/SKILL.md:235`, `references/long-running-and-execution.md:11` | **Confirmed.** Protocol, "extend the background execution time of an app intent", iOS/iPadOS/macOS/tvOS/visionOS/watchOS 27.0 | https://developer.apple.com/documentation/appintents/longrunningintent |
| `AppIntentsTesting` framework, iOS 27 | `app-intents/SKILL.md:25,235`, `references/testing-intents.md:6,220` | **Confirmed.** Framework "App Intents Testing", all platforms 27.0 | https://developer.apple.com/documentation/appintentstesting |
| `allowedExecutionTargets`, iOS 27 | `widgets/SKILL.md:199,204`, `references/interactive-widgets.md:142-146` | **Confirmed.** `static var allowedExecutionTargets: IntentExecutionTargets` on `AppIntent`, 27.0. `AppIntent.ExecutionTargets` is a typealias of `IntentExecutionTargets`. `.main` and `.widgetKitExtension` exist (also `.appIntentsExtension`, `.default`) | https://developer.apple.com/documentation/appintents/appintent/allowedexecutiontargets and https://developer.apple.com/documentation/appintents/intentexecutiontargets |

Not verified: the behaviour text around these APIs (for example "the widget extension has a read-only view of the shared store"). The check covers existence, type names and availability only. No skill claim was edited or deleted. Provenance `notes` records the result.

## Skipped

`macos-spm-app-packaging`: skipped, as the plan says. The fleet does not ship macOS apps outside the App Store yet.

## Gate

The gate fails only in `crates/horch-core/tests/skills_catalog.rs` (3 of 10 tests). All other gate steps pass. The 3 failures:
- `skills_bundled_size_budget`: `app-intents/SKILL.md` is 24151 bytes, over 12 KB. V0 exempts vendored skills.
- `skills_bundled_text_only`: the extensionless `LICENSE` files are not `.md` or `.txt`. V0 accepts `LICENSE`.
- `skl_01_bundled_catalog_versions_and_digests`: the 4 new upstream pins (`n0an/*`, `mhaviv/*`) are not in the test's known-pin list. This item is not in the plan's V0 list. The test file is not in this unit's scope. V0 or the orchestrator must add the pins.

## Gotchas and follow-ups

- `provenance.json` entries are appended after the existing entries (the existing file is not sorted); the 4 new entries are sorted by name.
- The `vendored` and `notes` keys are unknown to the current parser; serde ignores them. V0 adds `vendored`.
- Out of scope, noticed: none.
