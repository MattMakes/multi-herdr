# Live check: Apple tools

The MobileBuildMCP 2.7.1 server starts as the Swift teammates launch it, and
the tool and workflow names that the Swift skills cite exist on it; the Xcode
agent-skills export is checked where Xcode 27 is installed. Items U-21, U-54.

## How to run

```bash
scripts/live/apple.sh
```

It needs Xcode (`xcodebuild`, `swift`), `node`/`npx`, and network for the
first `npx` download of `mobilebuildmcp@2.7.1`. A missing tool is `SKIP`.
It starts no model session, so it costs no tokens.

The script reads the `mobilebuildmcp:` line of `teammates/swift-developer.md`
and starts the server with that command, those args and that env. Only
`MOBILEBUILDMCP_ENABLED_WORKFLOWS` changes from step to step. It speaks MCP
over stdio (`initialize`, then `tools/list` or 1 `tools/call`). It writes
under `.worktrees/_scratch/live-apple/` and deletes the package build
directory at the end. The server itself writes build logs under
`~/Library/Developer/MobileBuildMCP/workspaces/`.

Steps:

- `xcode-version`: `xcodebuild -version`.
- `mcp-tools`: the handshake and `tools/list` with
  `simulator,ui-automation,swift-package`.
- `mcp-skill-names`: every backticked snake_case name in
  `skills/ios-simulator-run`, `skills/swift-code-audit`,
  `teammates/swift-*.md` and `teammates/apple-*.md`, plus `tap`, `batch`,
  `gesture` and `screenshot`, is in that tool list.
- `mcp-workflows`: the default set equals `simulator`; `simulator` has
  `snapshot_ui` and `screenshot` and no `tap`; `ui-automation` has `tap`,
  `type_text`, `gesture` and `batch`; only `swift-package` has
  `swift_package_build`.
- `mcp-build`: `swift_package_build` builds a 1-file Swift package. This
  needs no signing identity. An app target on a simulator (`build_sim`)
  needs an Xcode project, so the script does not build one.
- `xcode-skills-export`: on Xcode 27 or later,
  `xcrun agent skills export --help` runs and names `--output-dir`.
  `horch doctor` runs the same check (`xcode_export_check` in
  `crates/horch/src/cmd/doctor.rs`) when an offered teammate has
  `requires: xcode` and `operator_skills:`. Below Xcode 27, doctor prints a
  `note:` line, not a warning.

## 2026-10-06

| Step | Claim it proves | Tool version | Result | Evidence |
|------|-----------------|--------------|--------|----------|
| xcode-version | The host has Xcode for the Swift teammates (`requires: xcode`). | Xcode 26.6 (17F113), Swift 6.3.3 | PASS | `Xcode 26.6 Build version 17F113` |
| mcp-tools | `npx -y mobilebuildmcp@2.7.1 mcp` with the teammate env starts and answers MCP (`teammates/swift-developer.md` `mcp_servers`). | mobilebuildmcp 2.7.1, node 24.21.0 | PASS | `serverInfo` `mobilebuildmcp 2.7.1`; 42 tools with the 3 workflows; about 3 s for each launch after the first download. |
| mcp-skill-names | The 15 tool names in `skills/ios-simulator-run/SKILL.md`, `skills/swift-code-audit/SKILL.md` and the teammates exist on 2.7.1. | 2.7.1 | PASS | All 15 are present: `batch`, `boot_sim`, `build_run_sim`, `build_sim`, `gesture`, `get_app_bundle_id`, `get_sim_app_path`, `launch_app_sim`, `list_sims`, `screenshot`, `session_set_defaults`, `snapshot_ui`, `swift_package_build`, `tap`, `type_text`. |
| mcp-workflows | `skills/ios-simulator-run/SKILL.md`: "Only `simulator` is on by default"; `skills/swift-code-audit/SKILL.md`: `swift_package_build` needs `swift-package`. | 2.7.1 | PASS, 1 claim fixed | Default = `simulator` = 24 tools. The skill said that without `ui-automation` there is no `snapshot_ui` or `screenshot`. That is false: `simulator` has both. `ui-automation` (18 tools) adds `tap`, `type_text`, `gesture`, `batch` and more. The skill line is fixed. |
| mcp-build | A MobileBuildMCP build tool builds without a signing identity. | 2.7.1, Swift 6.3.3 | PASS | `swift_package_build` returned `structuredContent.data.summary.status` = `SUCCEEDED` in 2.8 s. |
| xcode-skills-export | `xcrun agent skills export --output-dir <dir>` exists (`docs/skills-and-teams.md`, "Operator skills"). | Xcode 26.6 | SKIP | Needs Xcode 27 or later. `horch doctor` on a `Package.swift` project prints `note: xcode skills export (needed by apple-platform-developer, swift-developer): Xcode 26.6 has no ... so it is not verifiable here.` |
