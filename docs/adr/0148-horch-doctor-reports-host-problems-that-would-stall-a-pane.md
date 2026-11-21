# horch doctor reports host problems that would stall a pane

`horch doctor` (`crates/horch/src/cmd/doctor.rs`) checks the host tools a
teammate needs, so a worker in an unattended pane does not stall or report
`BLOCKED:` at its first step. Each problem is reported with its fix. The
precondition every launch recipe shares, `doctor::check`, does not validate
the roster: a roster warning must never stop a fleet from launching, and
roster problems are reported only by `horch doctor`.

## Consequences

- Claude loads any `herdr-*` entry in `<home>/.claude/skills` as an
  ambient skill whose text can name commands this horch lacks. The
  briefing horch compiles into each pane is canonical.
  `doctor::stale_skill_warnings` warns about each such entry and removes
  nothing (TRF-10).
- `doctor::git_lfs_problem` runs `git lfs version` with the fleet's git
  (`HORCH_GIT_BIN`, else `git` on PATH). A path is used as given. Without
  git-lfs in that git a new worktree of an Unreal project holds LFS
  pointer files instead of assets and `git lfs lock` fails.
- In an Unreal project (a `*.uproject` at the top)
  `doctor::lfs_rule_problem` requires `.gitattributes` to send `*.uasset`
  to LFS (`filter=lfs`). Otherwise the first asset commit puts binary bytes
  into git history permanently.
- The blender-artist needs a Blender that runs. The Blender Lab MCP server
  runs `BLENDER_PATH` when set, else `blender` on PATH. A launch sets
  `BLENDER_PATH` from the same lookup (`host_tool_bin`), which tries the
  macOS app bundle last, and `doctor::blender_problem` uses it too. The
  server's `*_for_cli` tools run on any Blender. Its live tools need
  Blender 5.1 (`BLENDER_LIVE_MIN`) and the add-on.
- A C# Godot project needs the Godot .NET engine and `dotnet`. The
  standard engine cannot build or run C#, and `godot-run.sh` never falls
  back to it. `doctor::godot_csharp_problems` looks at `GODOT_MONO_PATH`,
  then the macOS bundle when it exists, and reports 1 line per missing
  tool.
- `doctor::xcode_problem` requires `xcodebuild` on PATH and past its first
  launch, or the first build in a pane stalls at the licence prompt.
  `xcodebuild -checkFirstLaunchStatus` exits 0 when nothing is left to do.
  With only the Command Line Tools selected, the `/usr/bin/xcodebuild`
  shim fails and says so on stderr.
- An offered Apple teammate (`requires: xcode`) with `operator_skills:`
  gets them from `xcrun agent skills export --output-dir <dir>`
  (docs/skills-and-teams.md), which exists only from Xcode 27
  (`XCODE_EXPORT_MIN`). `doctor::xcode_export_check` checks that `--help`
  runs and names `--output-dir` on Xcode 27 or later. Below 27 it is only
  a note (U-54). It says nothing when no such teammate is offered or when
  `xcodebuild` is missing, which `xcode_problem` reports.
