# S1 swift-core: core Swift and SwiftUI skills

Unit `swift-core`, branch `ds/swift-core`, teammate opus-42.
Plan: `ai_docs/plans/domain-skills/s1-swift-core.md`. Specification:
`ai_docs/reports/swift-fleet-skills-2026-10.md` §4.1, §5, §7.3.

## Result

7 skills are bundled. `target/debug/horch skills` lists all 7. Each skill
directory has its upstream MIT `LICENSE` next to `SKILL.md`. Copied files:
SKILL.md, `references/*.md` and LICENSE only. No `agents/`, `assets/`,
`.claude-plugin/`, `gemini-extension.json` or dotfiles.

| skill | verdict applied | `vendored` | SKILL.md bytes | dir bytes | files |
|---|---|---|---|---|---|
| observability | vendor (verbatim) | true | 20138 | 154439 | 15 |
| swift-concurrency-pro | adapt | false | 5881 | 59526 | 15 |
| swift-format-style | adapt | false | 4399 | 46314 | 8 |
| swift-testing-pro | vendor + optional line (see decisions) | false | 4634 | 41421 | 7 |
| swiftdata-pro | adapt, light | false | 4452 | 21101 | 9 |
| swiftui-liquid-glass | adapt | false | 3763 | 13293 | 3 |
| swiftui-pro | adapt, light | false | 4261 | 29018 | 11 |

Dir bytes include LICENSE. Every adapted skill fits the 12 KB / 160 KB budget.

Commits:
- `Skills: Add observability and swift-testing-pro`
- `Skills: Add swiftui-pro and swiftui-liquid-glass`
- `Skills: Add swift-concurrency-pro and swiftdata-pro`
- `Skills: Add swift-format-style`
- `Reports: Add swift-core report` (this file)

## Gate

`HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate` fails only on the 3
V0 items in `crates/horch-core/tests/skills_catalog.rs`:
- `skills_bundled_text_only`: `observability/LICENSE` (V0 allows `LICENSE`).
- `skills_bundled_size_budget`: `observability/SKILL.md: 20138 bytes` (V0 exempts `vendored: true`).
- `skl_01_bundled_catalog_versions_and_digests`: the new repositories are not in `DESIGN_SOURCE_PINS` (V0 replaces the list with a shape check).

Each of these tests stops at its first panic. So I simulated the 3 V0 rules
in an uncommitted copy of `skills_catalog.rs` (allow `*/LICENSE`, exempt
`observability`, accept a github URL + 40-hex revision + licence). With
that copy, 10 of 10 catalog tests pass. I then restored the file.

`cmp_05_e2e_candidates_in_dataset_workspace` (`crates/horch-e2e/tests/dataset.rs:529`)
failed once in the full gate (1 `candidate.completed` event, expected 2).
It passed when run alone. It does not read skills. I record it as a load
flake: 9 units ran gates at the same time.

## Edits per skill

Line numbers are in our copy. "upstream:N" is the upstream line.

### observability (vendor)
- No edits. `diff -r` against upstream shows only the added LICENSE and the dropped `agents/` dir.
- `references/production-monitoring.md:67` names `github.com/open-telemetry/opentelemetry-swift`. It is a library, not a skill repository, so it stays.

### swiftui-pro (adapt, light)
- Source: the top-level `swiftui-pro/`, not the nested `swiftui-pro/skills/swiftui-pro/` (that variant uses `${CLAUDE_SKILL_DIR}`).
- `SKILL.md:32`: "without asking first" → message to the orchestrator.
- `references/hygiene.md:8`: "Offer to translate new keys" → tell the orchestrator; translate only if the task asks.
- `references/views.md:8`: "place view logic into view models" → §5 answer: follow the codebase; in a new codebase MV with `@Observable` and `@Query`; a view model only for a test seam. Link → `swift-testing-pro`.
- `references/data.md:37`: link → `swiftdata-pro`.
- `references/swift.md:56`: link → `swift-concurrency-pro`.

### swift-concurrency-pro (adapt)
- **Checked:** `references/testing.md:35,37,40`. Upstream said `.serialized` on a suite serializes only parameterized tests. Now: on a test function it affects only parameterized cases; on a suite it serializes every test function and sub-suite. Evidence: Apple's `ParallelizationTrait` documentation (the type behind `.serialized`) says: "When you add this trait to a test suite, that suite runs its contained test functions (including their cases, when parameterized) and sub-suites serially instead of in parallel." It also says the trait "has no effect when you apply it to a non-parameterized test function" (https://developer.apple.com/documentation/testing/parallelizationtrait). The vendored `swift-testing-pro/references/async-tests.md:10` says the same.
- `references/new-features.md:78-79` (added): caller-actor `nonisolated` async needs `NonisolatedNonsendingByDefault`, which `SWIFT_APPROACHABLE_CONCURRENCY = YES` turns on. Points to `references/build-settings.md`.
- `references/build-settings.md` (new): AvdLee `skills/swift-concurrency/SKILL.md:16-24` settings table plus its Xcode 26 note (`:26`). The "ask the developer to confirm" line (`:28`) becomes a message to the orchestrator. Own provenance source (AvdLee repo, pinned revision, sha256).
- `SKILL.md:33`: read build settings first, with the new reference. `SKILL.md:37`: ask gate → message. `SKILL.md:125`: reference entry for `build-settings.md`.
- `references/testing.md:218`: link → `swift-testing-pro`. `references/actors.md:103`: link → `swiftui-pro`.
- §5 `@concurrent`: `new-features.md:85-102` and `interop.md:53-71` already say CPU-heavy work only. No edit needed.

### swiftdata-pro (adapt, light)
- `SKILL.md:29`: ask gate → message.
- `SKILL.md:19-20` (added): review steps for migrations/history and Core Data adoption. `SKILL.md:105-106` (added): reference entries.
- `references/core-rules.md:5,14`: links → `swift-concurrency-pro`, `swiftui-pro`.
- `references/migrations-and-history.md` (vanab): line 50 is the only `#Unique` mention. I kept it and added the CloudKit caveat (no `#Unique` or `.unique` with CloudKit, see `cloudkit.md`). That agrees with Hudson `cloudkit.md:5` and §5.
- `references/core-data-adoption.md` (vanab): verbatim. Neither file mentions `#Unique` elsewhere.

### swift-testing-pro (vendor + optional line)
- `SKILL.md:28` (added): headless runs, `swift test --filter` and `xcodebuild test -only-testing:`.
- `SKILL.md:29` (added): exit tests do not run on iOS, tvOS, watchOS or visionOS. **Verified:** Apple's "Exit testing" article says "Exit tests are available on macOS, Linux, FreeBSD, OpenBSD, and Windows." (https://developer.apple.com/documentation/testing/exit-testing).
- `references/async-tests.md:3`: link → `swift-concurrency-pro`. `references/writing-better-tests.md:36`: link → `swiftui-pro`.
- `async-tests.md:8-12` on `.serialized` is correct (suite serializes everything; a single non-parameterized test is unaffected). No edit.

### swift-format-style (adapt)
- **Checked:** `references/anti-patterns.md:80-93` (upstream 80-91). The fixed-format example used `locale: .current, calendar: .current`. Now: `.iso8601.year().month().day()` first, then verbatim with `Locale(identifier: "en_US_POSIX")`, `.gmt`, `Calendar(identifier: .gregorian)`, with a comment on non-Gregorian users. Evidence for the API: `Date.ISO8601FormatStyle.year()` returns `Date.ISO8601FormatStyle` (https://developer.apple.com/documentation/foundation/date/iso8601formatstyle/year()).
- `references/date-styles.md:248-249` (added): one pitfall line, pin the calendar for fixed formats. The report names only `anti-patterns.md`, but `date-styles.md` has the same `.verbatim(..., calendar: .current)` pattern, and §5 requires "a fixed POSIX locale for wire formats". I chose a pitfall line, not a rewrite of the demo at `:200-221`, because those snippets show display output.
- `SKILL.md:27`: "**Never** use legacy Formatter" → "For display, do not use…", plus the wire-format rule.
- `SKILL.md:3`: description narrowed to values shown to the user.

### swiftui-liquid-glass (adapt)
- `references/liquid-glass.md:5`: "You should always seek guides…" deleted.
- `references/liquid-glass.md:206-217`: the background-extension snippet now calls `.backgroundExtensionEffect()` in the detail column, from Apple's example, with a note that it clips and should be used once. Evidence: https://developer.apple.com/documentation/swiftui/view/backgroundextensioneffect().
- **Verify `scrollExtensionMode`: does not exist.** `https://developer.apple.com/documentation/swiftui/view/scrollextensionmode(_:)` returns 404. Apple's "Landmarks: Extending horizontal scrolling under a sidebar or inspector" says: "When a scroll view touches the sidebar or inspector, the system automatically adjusts it to scroll under the sidebar or inspector" (https://developer.apple.com/documentation/swiftui/landmarks-extending-horizontal-scrolling-under-a-sidebar-or-inspector). I deleted the modifier and replaced `:219-231` with that behavior and Apple's `Spacer` inset pattern.
- `references/liquid-glass.md:257,261`: `foregroundColor` → `foregroundStyle`.
- `SKILL.md`: verbatim.

## Decisions

- **`swift-testing-pro` is `vendored: false`.** The plan says "vendor", but it also asks for the added line, so the copy is not verbatim. It fits the budget, so it needs no exemption. I also applied the fleet link rule to its 2 upstream-repo links. "vendored" stays an honest "verbatim" signal.
- **`observability` is `vendored: true`.** Its 20 KB SKILL.md is over the 12 KB budget and the copy is verbatim.
- **Provenance:** one entry per skill, appended after the existing entries, sorted by name among themselves. The existing list is not sorted, and `/tmp/dsmerge.sh` merges by name. Each entry lists LICENSE, SKILL.md and every reference copied or adapted, with sha256 of the upstream bytes and the PINS.txt revision.
- **README:** new "Swift and Apple skills" section at the end of `skills/README.md`, rows sorted. Other Swift units add their rows to the same table.
- Links to Apple documentation and to libraries stay. Only links to other upstream skill repositories became bundled skill names.

## Out of scope, noticed

- `swiftdata-pro` still has no `@ModelActor` coverage. The report (§4.1) gives that as a reason to add vanab references, but the 2 named files do not cover it. vanab `references/concurrency-and-actors.md` does. Follow-up: add it, or cover `@ModelActor` in the Swift persona.
- `swiftui-liquid-glass` uses `.font(.system(size:))` on SF Symbol images (`SKILL.md:73,77`, `liquid-glass.md:94-158,258,262`). §5 forbids fixed sizes for text only, so I left them.
- `swift-concurrency-pro/references/testing.md:90-171` uses `ViewModel` names in test examples. They show testing patterns, not architecture advice, so I left them.
- The P-Swift unit must attach these 7 skills by name. No teammate change is in this unit.
