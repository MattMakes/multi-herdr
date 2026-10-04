# F8 release-verify: xcodebuild archive inside the app-release-preparer sandbox

Plan: `ai_docs/plans/finish/f8-release-verify.md`. Worker: opus-68. Date: 2026-10-04.
Claude Code: 2.1.289. Host: macOS 26.5.1, Xcode 26.6 (17F113), SDKs iOS 26.5 and macOS 26.5.
Follows S6: `ai_docs/reports/finish/release-sandbox.md`.

## Result

- `xcodebuild archive` with `CODE_SIGNING_ALLOWED=NO` succeeds inside the exact
  sandbox horch writes for `app-release-preparer`, for a macOS app and for an
  iOS app (`generic/platform=iOS`). Both print `** ARCHIVE SUCCEEDED **`, exit 0.
- The `/private/var/folders` `allowWrite` entry is needed. Without it the same
  archive fails with exit 65.
- Xcode's default DerivedData (`~/Library/Developer/Xcode/DerivedData`) is
  read-only in the sandbox, so an archive without `-derivedDataPath` fails
  with exit 65. The persona already requires `-derivedDataPath build/DerivedData`.
- The sandbox needs no new entry. The teammate file changed only in a comment.

## How the probe ran

The probe project is at `.worktrees/_scratch/release-probe/` (git-ignored).

- `project.yml` (xcodegen 2.x, already installed), 2 targets with one source
  file `Sources/ProbeApp.swift` (a SwiftUI `App` with one `Text`):
  `Probe` (macOS 14.0) and `ProbeiOS` (iOS 17.0). Both set
  `GENERATE_INFOPLIST_FILE: YES` and `CODE_SIGNING_ALLOWED: NO`.
- `xcodegen generate` wrote `Probe.xcodeproj`.
- `overlay.json` is horch's sandbox overlay for the teammate. I built it from
  `horch teammates --json`: the teammate's `sandbox` block, plus the 4 keys of
  `SANDBOX_FORCED` (`crates/horch-core/src/harness/claude.rs`) and
  `permissions.blockReadsOutsideWorkingDirectories: true`. That is what
  `overlay_sandbox` does. The rest of the real launch overlay (plugin and
  skill switches, status line) does not touch the sandbox.
- Each run was a sandboxed Claude Code session, the way the pane runs:

  ```sh
  env -u ANTHROPIC_API_KEY claude -p --model sonnet \
    --settings "$(cat overlay.json)" \
    --allowedTools "Bash(./archive.sh *)" -- \
    "Run exactly this one Bash command and nothing else: ./archive.sh <tag> ..."
  ```

  `--allowedTools` only pre-approves the command in `-p` mode. It does not
  change the sandbox.
- `archive.sh` runs, in the project directory:

  ```sh
  xcodebuild archive -project Probe.xcodeproj -scheme "${SCHEME:-Probe}" \
    -configuration Release -destination "${DEST:-generic/platform=macOS}" \
    -archivePath "build/$tag/archive.xcarchive" \
    -derivedDataPath "build/$tag/DerivedData" \
    -clonedSourcePackagesDirPath "build/$tag/SourcePackages" \
    CODE_SIGNING_ALLOWED=NO
  ```

  `archive-ios.sh` sets `SCHEME=ProbeiOS DEST=generic/platform=iOS`.
- `overlay-no-varfolders.json` is `overlay.json` with `allowWrite: []`.

## The sandbox settings used

```json
{"sandbox": {
   "enabled": true, "failIfUnavailable": true, "allowUnsandboxedCommands": false,
   "filesystem": {
     "allowWrite": ["/private/var/folders"],
     "denyRead": ["~/.asc", "~/.config", "~/Library/Keychains",
                  "~/.appstoreconnect", "~/private_keys", "~/.private_keys"],
     "allowRead": ["~/.config/horch/asc", "~/Library/Developer",
                   "~/Library/Preferences", "~/Library/Caches",
                   "~/Library/MobileDevice/Provisioning Profiles"]},
   "network": {"allowedDomains": ["api.appstoreconnect.apple.com"],
               "strictAllowlist": true},
   "credentials": {"envVars": ["17 ASC_* variables, mode deny"]}},
 "permissions": {"blockReadsOutsideWorkingDirectories": true}}
```

## Runs

| # | Overlay | Command | Result |
|---|---|---|---|
| 1 | `overlay.json` | `./archive.sh with-varfolders` (macOS) | exit 0, `** ARCHIVE SUCCEEDED **` |
| 2 | `overlay.json` | `./check.sh`, then `./archive.sh with-varfolders-2` (macOS) | sandbox active (below); exit 0, `** ARCHIVE SUCCEEDED **` |
| 3 | `overlay-no-varfolders.json` | `./check.sh`, then `./archive.sh no-varfolders` (macOS) | sandbox active; exit 65, `** ARCHIVE FAILED **` |
| 4 | `overlay.json` | `./archive-ios.sh ios-with-varfolders` (iOS) | exit 0, `** ARCHIVE SUCCEEDED **` |
| 5 | `overlay.json` | archive without `-derivedDataPath` (macOS) | exit 65, `** ARCHIVE FAILED **` |

`check.sh` proves the sandbox was on in runs 2 and 3:

- `ls ~/Library/Keychains`: exit 1.
- `curl https://example.com`: exit 56, and Claude Code reported
  `deny network-outbound example.com:443 (host is not on the allow list)`.

The archive of run 1 holds `Products/Applications/Probe.app`. Its `Info.plist`
has `CFBundleIdentifier dev.horch.probe.Probe`, `CFBundleShortVersionString 1.0`,
`CFBundleVersion 1`, architectures `x86_64` and `arm64`, and an empty
`SigningIdentity`.

### Run 3: without `/private/var/folders`

Every save into `build/no-varfolders/DerivedData/Logs/*` failed, then the build
stopped:

```
IDELogStore: Failed to open Build log store: Error Domain=NSCocoaErrorDomain Code=513
  "You don't have permission to save the file "LogStoreManifest.plist" in the folder "Build"."
  NSUnderlyingError = "Error Domain=NSPOSIXErrorDomain Code=1 "Operation not permitted""
Couldn't create workspace arena folder '.../build/no-varfolders/DerivedData':
  Unable to write to info file ...
** ARCHIVE FAILED **
```

The target directory is inside the project, which is writable. Foundation's
atomic save stages the file in the per-user temp directory under
`/private/var/folders` and then moves it. So the entry is what lets the save
complete. The entry cannot be narrower in a shared teammate file: the per-user
part of the path (`/private/var/folders/<xx>/<hash>/`) differs on each Mac.

### Run 5: Xcode's default DerivedData

```
Error saving log: ... "Operation not permitted"
  NSFilePath=~/Library/Developer/Xcode/DerivedData/Probe-<hash>/Logs/Build/<uuid>.xcactivitylog
```

`~/Library/Developer` is in `allowRead`, not in `allowWrite`. This is correct:
the persona keeps all build output under `build/`. Do not widen it.

### Noise that is not a failure

Each sandboxed `xcodebuild` logs CoreSimulator errors (`Error opening log file
~/Library/Logs/CoreSimulator/...: Operation not permitted`, `simdiskimaged`
connection invalid, `Simulator device support disabled`). A device archive
does not use the simulator, and runs 1, 2 and 4 succeeded with these lines.
A simulator build or test inside this sandbox is not verified, and it will
likely fail. This teammate does not run the simulator.

## Not verified

- Code signing and export: `CODE_SIGNING_ALLOWED=NO` skips signing, so
  `xcodebuild -exportArchive` for App Store Connect was not run. S6 "Limits"
  still applies: the login keychain is outside the sandbox.
- `asc xcode archive` and `asc xcode export`: `asc` is not installed on this
  host, and the plan forbids new tools without asking. From the asc 5.9.1
  source: `asc xcode archive` passes no `-derivedDataPath` of its own, so it
  hits run 5 unless the caller adds
  `--xcodebuild-flag=-derivedDataPath --xcodebuild-flag=build/DerivedData`.
  `asc xcode build` without `--derived-data-path` uses
  `<user cache dir>/asc/xcode-build/...` (`internal/xcode/build.go:264-296`),
  which is `~/Library/Caches`, read-only in the sandbox.

## Docs and skill changes

- `teammates/README.md`: the Swift table row now says "writes the upload
  command for a human; never uploads or submits" (it said "internal TestFlight
  upload"). The paragraph under the table says it never uploads, and that a
  fallback would drop the sandbox too.
- `docs/skills-and-teams.md`: the `app-release-preparer` bullet names the
  sandbox and says it never uploads and never submits.
- `skills/asc-xcode-build/SKILL.md`, upload part only: the fleet rule says
  "Never upload, even when the assignment asks for it" (it allowed an upload
  that the assignment named). The `--wait` export, section "3. Upload or
  publish", the macOS `.pkg` upload and the description say the upload
  command is for a human.
- `skills/provenance.json`: the `adaptation` of `asc-xcode-build` records the
  new rule.
- `teammates/app-release-preparer.md`: a comment on `allowWrite` points to
  run 3. No sandbox value changed.

## Gaps outside this plan's FILES

- `skills/asc-xcode-build/SKILL.md` outside the upload part: the
  `asc xcode archive` examples write to `.asc/artifacts/` and pass no
  `-derivedDataPath` (run 5 fails without it), and the export examples and
  "Troubleshooting" pass `--xcodebuild-flag=-allowProvisioningUpdates`, which
  the teammate's deny list blocks. The persona overrides both, but the skill
  tells the agent to do them.
