# Profiling intake and collection checklist

## Intent

Use this checklist when code review alone cannot explain the SwiftUI performance issue and you need runtime evidence. Record it yourself; message the orchestrator only for what you cannot get.

## Establish first

- Exact symptom: CPU spike, dropped frames, memory growth, hangs, or excessive view updates.
- Exact interaction: scrolling, typing, initial load, navigation push/pop, animation, sheet presentation, or background refresh.
- Target device and OS version.
- Whether the issue was reproduced on a real device or only in Simulator.
- Build configuration: Debug or Release.
- Whether a baseline or before/after comparison already exists.

## Default capture

- Build the app in a Release build when possible.
- Record with the SwiftUI template: `xcrun xctrace record --template SwiftUI --device <name|UDID> --time-limit <n>s --output <path>.trace --launch -- <app>` (or `--attach <pid|name>`). It captures the SwiftUI lanes and Time Profiler together.
- Reproduce the exact problematic interaction only long enough to capture the issue.
- Read the trace with `xcrun xctrace export --input <path>.trace --toc`, then `--xpath` for the tables you need.

## Collect these artifacts

- Trace export or screenshots of the relevant SwiftUI lanes
- Time Profiler call tree screenshot or export
- Device/OS/build configuration
- A short note describing what action was happening at the time of the capture
- If memory is involved, the memory graph or Allocations data if available

## When to capture more

- Record a second capture if the first run mixes multiple interactions.
- Record a before/after pair when you try a fix.
- Message the orchestrator for a device capture if you have no device and the issue only appears off Simulator or scrolling smoothness matters.

## Common traps

- Debug builds can distort SwiftUI timing and allocation behavior.
- Simulator traces can miss device-only rendering or memory issues.
- Mixed interactions in one capture make attribution harder.
- Screenshots without the reproduction note are much harder to interpret.
