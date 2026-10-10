# Concurrency build settings

Read `Package.swift` or the `.pbxproj` before giving advice that depends on concurrency behavior. Check the Swift language mode, strict concurrency level, default isolation, and upcoming features every time, not only for migration work.

| Setting | SwiftPM (`Package.swift`) | Xcode (`.pbxproj`) |
|---|---|---|
| Language mode | `swiftLanguageVersions` or `-swift-version` (`// swift-tools-version:` is not a reliable proxy) | Swift Language Version |
| Strict concurrency | `.enableExperimentalFeature("StrictConcurrency=targeted")` | `SWIFT_STRICT_CONCURRENCY` |
| Default isolation | `.defaultIsolation(MainActor.self)` | `SWIFT_DEFAULT_ACTOR_ISOLATION` |
| Upcoming features | `.enableUpcomingFeature("NonisolatedNonsendingByDefault")` | `SWIFT_UPCOMING_FEATURE_*` |
| Approachable Concurrency | N/A (use individual upcoming features) | `SWIFT_APPROACHABLE_CONCURRENCY` |

New projects created in Xcode 26 often start with `SWIFT_DEFAULT_ACTOR_ISOLATION = MainActor` and `SWIFT_APPROACHABLE_CONCURRENCY = YES`. Treat these as likely defaults for a new project, not as confirmed settings.

If a setting is unknown and the advice depends on it, do not guess. Send the orchestrator a message that names the setting and the advice that depends on it.
