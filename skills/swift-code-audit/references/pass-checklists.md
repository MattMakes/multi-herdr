# Pass Checklists

Run the three passes yourself, one after another: A, then B, then C. Before each pass, fill in its project context from the placeholder table at the end of this file. Record the findings of a pass before you start the next one.

All three passes record findings in the same format so synthesis is mechanical:

```markdown
### <short title>
- Location: <path>:<line-range>
- What: <observed>
- Why: <impact>
- Action: <recommended>
- Severity: Critical | High | Medium | Low
```

> **Note on numbering.** Record pass findings *without* `N.M` subsection numbers. The synthesis step in `SKILL.md` Step 5 assigns them when it lays findings into the final report, and MUST add the `N.M` prefix to every heading. See the numbering rules in `report-template.md`.

---

## Pass A — Concurrency & API modernity

Scope: **Swift concurrency** and **API modernity** in the app at {PROJECT_PATH}.

Every finding MUST include: file path, line range, a one-sentence What, a one-sentence Why, and a one-sentence recommended Action. Do not propose code; point at problems. Be exhaustive but specific.

EXCLUDE these directories: {EXCLUDED_DIRS}

Project context:
- iOS deployment target: {IOS_TARGET} (e.g., 17+). macOS target: {MACOS_TARGET} if applicable.
- The compiler has emitted these warnings (canonical input for your concurrency audit):

{BUILD_WARNINGS}

Your tasks:

1. **Trace each compiler warning** to its surrounding code and explain root cause. Propose architectural fixes (mark a class `@MainActor`, make it an actor, introduce a Sendable snapshot type) — not code changes.

2. **Find concurrency anti-patterns the compiler may have missed**:
   - `DispatchQueue.main.async` inside async functions (should be `await MainActor.run` or `@MainActor` annotation)
   - `Task.detached` uses without cancellation handling
   - Completion-handler APIs (PhotoKit, AVFoundation, URLSession) where async equivalents exist
   - `withCheckedContinuation` wrappers that could be replaced by direct async overloads
   - `dispatchQueue.sync` / `.async` patterns that should be actors
   - `nonisolated(unsafe)` usages
   - Singletons accessed across actor boundaries

3. **Find iOS-target replacements for older APIs**:
   - PhotoKit / AVFoundation completion-handler methods with async variants
   - AVCaptureDevice.RotationCoordinator instead of orientation enums
   - `@Observable` migration completeness (any lingering `ObservableObject` / `@Published`)
   - URLSession async-await
   - `UIApplication.shared.connectedScenes` instead of `.windows`
   - Any `@available(iOS X.0, *)` guards where X is now below the deployment target

4. **Read these hot-spot files carefully** (not just grep):
{HOT_SPOT_FILES}

5. **Record findings**, one per issue, using the template above. Aim for completeness over brevity. If a single problem manifests at multiple call sites, group them with a list of locations under one finding.

---

## Pass B — Dead code, duplication, refactor candidates

Scope: **dead code, duplicated code, and refactor candidates** in the app at {PROJECT_PATH}.

Every finding MUST include: file path, line range, a one-sentence What, Why, and recommended Action. Be exhaustive but specific.

EXCLUDE these directories: {EXCLUDED_DIRS}

Project context:
- ~{LOC} LOC across {FILE_COUNT} Swift files, multi-target Xcode project. The codebase has been developed incrementally for several years.
- Known suspected stale files (verify and flag): {KNOWN_STALE_FILES}
- Known duplicated helpers (use as model for finding others): {KNOWN_DUPLICATES}

Your tasks:

1. **Find `#if false` blocks and commented-out alternates.** Use grep to locate `#if false` regions and contiguous `//`-commented Swift code blocks. For each: file:line, intent guess (obsolete vs intentional A/B), recommendation (delete vs leave with TODO).

2. **Find duplicated helpers across files.** Specifically look for:
   - Image encode/decode helpers (CIImage → JPEG/PNG, UIImage decode/orientation)
   - Thumbnail creation
   - Orientation conversion (CGImagePropertyOrientation ↔ UIImage.Orientation)
   - Palette / model serialization
   - File-path / Documents-dir construction
   - JSON encode/decode wrappers
   - Photo library album lookup / creation
   - Anything else appearing verbatim in 2+ files

3. **Find oversized files** (>500 lines). For each, propose a sensible split (e.g., "extract delegate methods into `Foo+AVCaptureDelegate.swift`").

4. **Find TODO/FIXME/HACK/XXX/`#warning` markers.** Each with file:line and a one-line summary of what's pending.

5. **Find stale files / unused code**:
   - Files with `_OLD`, `Old_`, `_v1`/`_v2`, `Legacy`, `Deprecated` in their names
   - Files where most/all content is `#if false`'d
   - Types/methods that aren't referenced anywhere (quick xref via grep)
   - Filename typos (e.g., `Extensinos.swift` instead of `Extensions.swift`)

6. **Find naming / organization inconsistencies.** Spelling mistakes in symbol names, files in the wrong folder, mixed file-organization conventions.

7. **Find `print()` calls** not behind `#if DEBUG`. Group by file with counts; flag any that wrap sensitive data.

8. **Find ad-hoc magic constants** that should be named (image dimensions, JPEG quality, retry counts, hardcoded URL strings, album names).

9. **Record findings** using the template. If a category has many instances, list top 5-10 specific examples plus a count of the rest.

---

## Pass C — Bugs, logic errors, security, performance

Scope: **bugs, logic errors, security, performance and SwiftUI quality** in the app at {PROJECT_PATH}.

Every finding MUST include: file path, line range, a one-sentence What, Why, and recommended Action. Be exhaustive but specific.

EXCLUDE these directories: {EXCLUDED_DIRS}

Project context:
- App type: {APP_TYPE} (e.g., camera app with dithering, productivity app, etc.)
- Targets: {TARGET_LIST}
- API: {API_LAYER_DESCRIPTION} (e.g., tRPC client talking to dev/prod endpoints)
- IAP: {IAP_DESCRIPTION} (e.g., StoreKit 2 / RevenueCat)
- Subsystems the task flags for extra scrutiny: {FLAGGED_SUBSYSTEMS}

Your tasks:

1. **Bugs and logic errors.** Read these hot-spot files closely (not just grep):

{HOT_SPOT_FILES}

   Look for:
   - **Force-unwraps (`!`)** on optionals — especially in init paths, `UIImage(data:)!`, `URL(string:)!`, `Bundle.main.url(forResource:)!`
   - **Force-try (`try!`)** and **force-cast (`as!`)**, particularly in static initializers
   - **`fatalError`** in non-debug code paths
   - **Unchecked array indexing** (e.g., `arr[0]`, `palette[i]` without bounds checks)
   - **Error-eating `catch { }`** blocks
   - **Missing auth handling** — `.limited` Photos auth, `.denied`, `.notDetermined`
   - **Retain cycles** in closures (`[weak self]` missing where it should be)
   - **Race conditions** — work that should be on main but isn't (or vice versa); fetch-after-create races
   - **Async cancellation** — long-running Tasks that should be cancellable
   - **Format / UTI mismatches** in PhotoKit content editing
   - **State-init paths** — `@State` declared on passed values (silently ignores updates)

2. **Security:**
   - Hardcoded secrets, API keys, tokens — grep for `Bearer`, `apiKey`, `secret`, `Authorization`, base64 blobs
   - Auth-token storage (Keychain vs UserDefaults vs in-memory)
   - Debug vs prod URL switching mechanism — is it gated by `#if DEBUG` only? (dangerous — release builds can ship to dev)
   - User data written to disk — sane location (Documents vs Caches vs Application Support)?
   - Debug log / zip exports — verify they don't include PII, device IDs, IAP receipts
   - Entitlements files per target

3. **Performance:**
   - **Per-frame allocations** in render pipelines (Metal/CoreImage filters that allocate buffers per `outputImage` call)
   - **`CIContext` lifecycle** — fresh `CIContext()` per render is expensive
   - **Repeated JPEG/PNG re-encoding** during slider drags / live editing
   - **Full image decode for metadata only** (`UIImage(data:)` to extract orientation when `CGImageSource` is cheaper)
   - **Synchronous PhotoKit / large-collection fetches** on main thread
   - **AVCapture session reconfiguration churn** on parameter changes
   - **Heavy work in SwiftUI view body** (formatters, sorts, decodes that should be hoisted out of `body`)

4. **SwiftUI / UI** (if SwiftUI is in use; findings go to report §8). Read the main view files and shared UI helpers:
   - `@State` / `@Bindable` / `@Observable` misuse
   - Modifier ordering bugs
   - Missing accessibility labels, Dynamic Type or dark-mode gaps
   - Missing `Equatable` on hot leaf views

5. **Record findings** using the template.

Severity guide:
- **Critical** = will likely cause crashes, data loss, or security exposure.
- **High** = real bug a user can hit; security issue (token leak, etc.); deprecated API blocking future builds.
- **Medium** = perf issue, refactor candidate, missing modernization.
- **Low** = code style, naming, single-occurrence cleanup.

If a single root cause manifests in many sites, group with a list of locations under one finding.

---

## Placeholder discovery (how to fill the templates)

| Placeholder | How to get it |
|---|---|
| `{PROJECT_PATH}` | Current working directory. |
| `{EXCLUDED_DIRS}` | Read the project's agent docs (`AGENTS.md`, `CLAUDE.md`) for "do not edit" / archive directories; common ones: `Dead/`, `Pods/`, `.build/`. |
| `{IOS_TARGET}` / `{MACOS_TARGET}` | `grep -h "IPHONEOS_DEPLOYMENT_TARGET\|MACOSX_DEPLOYMENT_TARGET" *.xcodeproj/project.pbxproj \| sort -u` |
| `{BUILD_WARNINGS}` | The deduplicated warnings from the XcodeBuildMCP build tool or `xcodebuild ... 2>&1 \| grep warning:`. |
| `{LOC}` / `{FILE_COUNT}` | `find . -name "*.swift" -not -path "./Dead/*" \| xargs wc -l \| tail -1` |
| `{KNOWN_STALE_FILES}` | `find . -name '*OLD*' -o -name '*_OLD*' -o -name 'Old_*'` plus a quick grep for `#if false` at file top. |
| `{KNOWN_DUPLICATES}` | Optional — leave blank on first run. If the codebase was audited before, seed Pass B with known dup paths. |
| `{HOT_SPOT_FILES}` | Top 10 from `find . -name '*.swift' -exec wc -l {} \; \| sort -rn \| head -10` plus any central state files named in the project docs. |
| `{APP_TYPE}` / `{TARGET_LIST}` / `{API_LAYER_DESCRIPTION}` / `{IAP_DESCRIPTION}` | Read the project's agent docs and README. |
| `{FLAGGED_SUBSYSTEMS}` | Whatever the task names for extra scrutiny. If unspecified, leave blank. |
