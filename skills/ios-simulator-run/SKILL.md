---
name: ios-simulator-run
description: Build, run and inspect an iOS app on a simulator through the XcodeBuildMCP server, with xcrun simctl as the fallback. Use to run an iOS app, drive or inspect its UI, capture logs or console output, reproduce a bug or diagnose runtime behavior.
---

# iOS Simulator Run

A short playbook. Use the XcodeBuildMCP tools for simulator control, UI inspection and logs. The tool names below are the bare names from XcodeBuildMCP 2.x; your harness may show them with a server prefix, and versions rename tools. Match each step to the tool in the server's tool list that does that job. If the server is not connected, use the `xcrun simctl` commands given here, and say in your report that you did.

Follow this sequence unless the task asks for a narrower action.

## 1) Get a booted simulator

- List simulators (`list_sims`, or `xcrun simctl list devices available`). Use a simulator whose state is `Booted`.
- If none is booted, boot one yourself. Prefer the device the task names, else a recent iPhone on the newest installed runtime: `boot_sim`, or `xcrun simctl boot <UDID>` then `xcrun simctl bootstatus <UDID> -b` to wait.
- Never erase or delete a simulator.

## 2) Set session defaults

Set the session defaults (`session-set-defaults`) once:
- `projectPath` or `workspacePath` (whichever the repo uses)
- `scheme` for the app
- `simulatorId` from step 1
- Optional: `configuration: "Debug"`, `useLatestOS: true`

## 3) Build and run

- Build and run on the simulator (`build_run_sim`).
- **If the build fails**, read the error output. Fix it if it is in your task's scope, or retry once with `preferXcodebuild: true`. If it still fails, message the orchestrator with the first error and stop before any UI interaction.
- **After a successful build**, confirm the app launched with a UI description (`describe_ui`) or a screenshot (`screenshot`).
- If the app is already built and only a launch is needed, launch it (`launch_app_sim`, or `xcrun simctl launch <UDID> <bundle id>`).
- If the bundle id is unknown, get the app path (`get_sim_app_path`), then the bundle id (`get_app_bundle_id`).

## UI interaction

- **Describe the UI** (`describe_ui`) before every tap or swipe, and again after layout changes.
- **Tap** (`tap`): prefer `id` or `label`; use coordinates only if needed.
- **Type** (`type_text`) after focusing a field.
- **Gestures** (`gesture`) for scrolls and edge swipes.
- **Screenshot** (`screenshot`) for visual confirmation. Save the images you cite in your report.

## Logs and console output

- Start log capture (`start_sim_log_cap`) with the app bundle id, or `xcrun simctl spawn <UDID> log stream --predicate 'subsystem == "<bundle id>"'`.
- Stop it (`stop_sim_log_cap`) and summarize the important lines.
- For console output, set `captureConsole: true` and relaunch if required.

## Troubleshooting

- If the wrong app launches, confirm the scheme and bundle id.
- If UI elements are not hittable, describe the UI again after the layout settles.
- If you cannot get a simulator, a build or a launch to work, message the orchestrator with what you ran and the exact error. Do not guess at runtime behavior you did not observe.
