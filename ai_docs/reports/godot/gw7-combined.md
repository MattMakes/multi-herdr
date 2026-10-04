# GW7 godot-combined: platforms, network and quality

Unit plan: `ai_docs/plans/godot/gw7-combined.md`. Worker: opus-80.
Engine: Godot 4.7.2.stable.official.ed1daf0bf (`/Applications/Godot.app`), headless only.

## What

8 combined skills. Each is the GodotPrompter v1.14.0 skill, renamed by
`scripts/godot/rename.py`, plus own-text references under `references/`
written after reading the gd-agentic-skills sources for facts (own-text rule:
nothing copied). The SKILL.md edits are the extended `description:`, a
"Fleet additions" list at the end, and the upstream fixes listed below.

| Skill | Own references added | gd-agentic consulted |
|---|---|---|
| `godot-export-pipeline` | feature-tags-and-build-size, patch-packs-and-export-plugins, desktop-runtime, web-runtime, console-targets, mobile-to-desktop-port | export-builds, platform-desktop, platform-web, platform-console, adapt-mobile-to-desktop |
| `godot-mobile-development` | battery-and-background, desktop-to-mobile-port | platform-mobile, adapt-desktop-to-mobile |
| `godot-xr-development` | comfort-and-locomotion, session-and-performance | platform-vr |
| `godot-multiplayer-basics` | single-player-retrofit, local-testing-and-discovery, engine-version-notes | adapt-single-to-multiplayer, multiplayer-networking |
| `godot-multiplayer-sync` | rollback-netcode, interest-management | multiplayer-networking (rollback, interest culling), adapt-single-to-multiplayer |
| `godot-dedicated-server` | host-security, host-operations | server-architecture, multiplayer-networking |
| `godot-testing` | deterministic-tests, golden-state-tests, budgets-leaks-and-network-tests | testing-patterns |
| `godot-code-review` | never-lists, project-health-scoring | auditor (never-lists), analyst (scoring); the "Aurelius" and "Anara" voices are dropped |

21 own reference files in total.

## Commits

- `930679a` batch 1: godot-export-pipeline, godot-mobile-development, godot-xr-development.
- `b2c6fb8` batch 2: godot-multiplayer-basics, godot-multiplayer-sync, godot-dedicated-server.
- `46886bd` batch 3: godot-testing, godot-code-review, this report
  (with `.worktrees/godot-commit.sh`).
- Batch 4: the godot-testing runner fact fixes below and this report update
  (with `.worktrees/godot-commit.sh`); the sha is in the `horch done` summary.

Batches 1 and 2 were committed by hand under the lock, before the
`godot-commit.sh` rule. GW2's uncommitted README lines were in the working
tree then; each commit staged only this unit's README rows and provenance
entries (HEAD plus own lines, written to the index with `git update-index`).

## Checks

- `scripts/godot/api_check.py` on all 8 directories: 58 files, 0 unknown names.
- `scripts/godot/gdscript_blocks_check.py` on all 8 directories: 158 blocks,
  101 parse, 57 fail. All 57 failures are in upstream GodotPrompter files
  (report only under `--strict-own`). 0 failures in the 21 own files.
  One own block is marked `<!-- gdscript-check: skip -->`: a GUT test method
  in `golden-state-tests.md`, which needs the GUT addon to parse.
- Runtime tests (headless, scratch project `.worktrees/_scratch/godot-opus-80/proj`),
  beyond the parse check:

| Code | Test | Result |
|---|---|---|
| `console-targets.md` atomic save | sync and `WorkerThreadPool` write, then read | pass; no `.tmp` left |
| `local-testing-and-discovery.md` UDP relay | ENet server and client through the relay, 75 ms one way, 10 ms jitter | connected; RTT 172 ms |
| `local-testing-and-discovery.md` LAN discovery | 2 processes, broadcast and listen | found; JSON port is a float (noted in text) |
| `single-player-retrofit.md` offline peer claims | default peer, `is_server()`, unique id, sender id | `OfflineMultiplayerPeer`, true, 1; sender 1 via `rpc()`, 0 via direct call |
| `rollback-netcode.md` core | 2 sessions, inputs 4 ticks late, input delay 2, 200 random ticks | both equal the ground-truth run |
| `interest-management.md` spawn claim | ENet server and client, `MultiplayerSpawner` | hidden synchronizer: not spawned; `set_visibility_for(true)`: spawned; `(false)`: despawned; filter `false`: not spawned |
| `interest-management.md` grid | unit run with 2 synchronizers | visibility follows the peer and object cells |
| `host-security.md` auth gate | ENet, good and bad token | good connects; bad dropped, no `peer_connected` on the server |
| `host-security.md` rate limiter | burst then refill | 5 of 20 allowed, kick flag set, 5 after 0.5 s |
| `host-operations.md` 2 APIs in 1 process | RPC client branch to server branch | received with the client's peer id |
| `host-operations.md` health line | ENet server | JSON line printed |
| `engine-version-notes.md` `rpc_config` | `get_node_rpc_config()` after `rpc_config()` | returns the config |
| `deterministic-tests.md` frame helper | `RigidBody2D` falls onto a floor | `until()` returns true |
| `golden-state-tests.md` helper | missing, update, same, differ | messages as documented |
| `budgets-leaks-and-network-tests.md` | bench, orphan list, net pair RPC | all work |
| `project-health-scoring.md` audit | sample script with one planted issue of each kind | every issue found |
| `never-lists.md` lambda claim | node frees after connecting a lambda and a method | method connection removed, lambda kept and ran |

Facts measured for the text: under `--headless`, `OS.has_feature("headless")`
is false and `DisplayServer.get_name()` is `"headless"`;
`RenderingServer.frame_post_draw` is never emitted (an await hangs); the
viewport image is `null`.

Not tested (stated in the text): UPnP (needs a router), DTLS setup, the
WebSocket TLS path, the matchmaker HTTP call, XR code (no headset; OpenXR
singletons exist only when OpenXR is enabled), store SDK calls, console
behaviour.

## Upstream fixes (in place, recorded in provenance)

- `godot-xr-development/references/hand-tracking.md`: API fix
  `XRHandTracker.HAND_JOINT_INDEX_TIP` -> `HAND_JOINT_INDEX_FINGER_TIP`
  (GDScript and C#).
- `godot-xr-development/SKILL.md` section 9: API fix. `XRSpatialAnchor` is
  not a class in 4.7.2 (not in the dump, core or modules). The GDScript and
  C# examples, the bullet and the checklist line now use
  `OpenXRSpatialAnchorCapability.create_new_anchor()` (returns
  `OpenXRAnchorTracker`) and an `XRAnchor3D` node. Getting the capability
  with `Engine.get_singleton("OpenXRSpatialAnchorCapability")` follows the
  4.6+ docs and was not run: the singleton exists only with OpenXR enabled.
  `api_check.py` did not flag this (a bare class name); a scan of
  class-like names found it.
- `godot-dedicated-server/SKILL.md` feature tag table: fact fix (approved by
  the orchestrator). `--headless` sets no `headless` feature tag (test above).
- `godot-testing` runner facts: fact fixes so the skill agrees with
  **godot-build-verify** (`references/commands.md`). Evidence: GW2 ran
  gdUnit4 6.2.1 and GUT 9.7.1 on Godot 4.7.2
  (`ai_docs/reports/godot/gw2-own-skills.md`); option names come from the
  addon sources (`gut/cli/gut_cli.gd`, `GdUnitTestCIRunner.gd`).
  - `SKILL.md` table (line 21) and CI paragraph (line 67): `gdunit4_runner` and
    `--add-gdunit-test-runner` -> `bin/GdUnitCmdTool.gd -a <dir|file>
    --ignoreHeadlessMode`; "exits non-zero on failure" -> the real exit
    codes and a log check; `-gexit` added to the GUT command.
  - `references/running-tests.md`: `GdUnitRunner.gd -- --testsuites` and
    `GdUnit4CSharpApiLoader.cs` (neither is a runner in 6.2.1) ->
    `GdUnitCmdTool.gd`; `--report-dir ./reports` -> `-rd res://reports`;
    an exit-code table (0 pass, 0 no tests, 100 fail, 101 orphans, 103
    headless refusal, 105 broken script); GUT `-goutput_dir` (not a 9.7.1
    option) -> `-gjunit_xml_file`; `-gexit` on every GUT command; the GUT
    exit 0 on a broken script or no tests, and a CI step that greps the log;
    CI Godot 4.3.0 -> 4.7.2.
  - Not measured: the C# path (gdUnit4Net `dotnet test`), the `-c` flag
    and `-gjunit_xml_file` output (taken from the addon help text).

## Upstream findings, not fixed

- 57 upstream code blocks do not parse alone; they are fragments using
  project classes or the GUT and gdUnit4 base classes. Per skill:
  godot-testing 32, godot-code-review 9, godot-mobile-development 6,
  godot-multiplayer-basics 3, godot-multiplayer-sync 3,
  godot-dedicated-server 2, godot-xr-development 2.
- `godot-dedicated-server/SKILL.md` line 26 says a "server export template
  strips rendering entirely from the binary". Official 4.x builds have no
  separate server template; "Export As Dedicated Server" strips resources.
  Not proven by a test here, so left as it is.

## Gap analysis per skill: added, dropped and why

### godot-export-pipeline
Added: feature tags and custom-feature variants, `res://` read-only, export
filters and `.gdignore`, texture compression per target, a size report;
runtime patch packs and load order, `--export-patch` / `--patches`
(checked in `--help`), `EditorExportPlugin` hooks, a headless export-all
script; desktop window modes, settings file, safe quit, focus loss,
low-processor mode, store SDK guard; web renderer and threads, hosting,
`user://` in IndexedDB, `localStorage` without `eval`, tab visibility,
user gestures, PWA updates; console readiness; mobile-to-desktop port and
a quality ladder.
Dropped: gd-agentic's "never use FileAccess alone for web saves" (wrong:
`user://` persists in IndexedDB), the post-export hook's
`get_option("export_path")` (wrong use; text uses
`get_export_preset().get_export_path()`), console FSR and shader-binary
claims and RAM numbers (NDA, cannot verify), the Python engine stripper,
native shell and secondary-window helpers (low value), Steam upload and
notarization automation (a fleet worker does not publish; the text gives
the operator a plan).

### godot-mobile-development
Added: background versus foreground cost, resume with the wall clock,
adaptive `scaling_3d_scale`, ANR and main-thread rules, sensor settings
off by default (dump defaults), haptic amplitude; the desktop-to-mobile
port (input inventory, targets, occlusion, schemes, back stack, on-screen
keyboard).
Dropped: "drop `Engine.max_fps` to 1 on pause" (wrong: the OS stops
rendering in the background), thermal monitoring (no thermal API in
4.7.2; `OS.get_thermal_state` does not exist), touch, joystick and safe
area code (in godot-input-handling and godot-responsive-ui), IAP
boilerplate (in the base IAP reference), the hidden-instance shader warm-up
(superseded by the Shader Baker; unverified).

### godot-xr-development
Added: comfort rules, snap turn around the head, teleport that lands the
head on the target, play-area check with `XRInterface.get_play_area()`,
a vignette driver, seated mode; OpenXR session signals, recenter,
refresh rate, foveation, render scale (set before `initialize()`), an
effects budget.
Dropped: `XRServer.get_reference_frame_bounds_2d` (does not exist),
"never below 90 FPS" (wrong: 72 Hz is a valid Quest rate), focus handling
through window focus notifications (replaced by OpenXR session signals),
physics hand and haptic sequencer (covered by the base grabbing reference
and `trigger_haptic_pulse`), the MSAA and foveation conflict claim
(unverified).

### godot-multiplayer-basics
Added: architecture choice, input and simulation split, offline peer path,
server-checked requests, late-join snapshot, stable player ids;
multi-instance runs, a working UDP latency relay, LAN discovery, threaded
UPnP; the GW10-verified engine-version notes.
Dropped: gd-agentic's latency simulator (a no-op stub), STUN/TURN advice
(no Godot API outside WebRTC), the signal-to-RPC bridge (style only), the
network profiling overlay idea.

### godot-multiplayer-sync
Added: rollback netcode with a tested core and desync checksums; interest
management with a grid and the tested spawn-by-visibility behaviour.
Dropped: bit packing and adaptive throttling (in the base bandwidth
reference), gd-agentic's grid (a stub).

### godot-dedicated-server
Added: SceneMultiplayer auth handshake, object decoding rules,
`server_relay = false`, per-peer rate limits, DTLS, kick with a reason,
limits; manual polling, 2 APIs in 1 process, WebSocket host, health line,
matchmaker hand-off, a pointer for RID-based simulation.
Dropped: the RenderingServer and PhysicsServer cookbook (godot-optimization
owns it).

### godot-testing
Added: test layers, frame and physics stepping, headless limits, seeding,
seeded fuzzing; golden state files; performance budgets, orphan checks,
multiplayer tests in one process.
Dropped: visual snapshot tests (headless draws nothing; the base skill
also says not to test pixels), gd-agentic `yield_frames` / `yield_seconds`
(not gdUnit4 4.x names; the text uses engine signals), the mock network
stub, the manual testing checklist (trivial).

### godot-code-review
Added: domain never-lists (security, signals and lifetimes, typing
settings, rendering, paths, threads, networking, saves, physics) and a
measured project-health review with an audit script and a weighted score.
Dropped: the personas and their voice, the 4.7 migration never-list (not
all claims hold in the dump: `RichTextLabel.width_in_percent` still
exists; version migration has its own unit), cyclomatic complexity and
asset md5 checks (low value), certificate tiers (replaced by a plain
score reading).

## Notes for other units

- `--headless` facts above affect godot-build-verify: never await
  `frame_post_draw` under `--headless`.
- `api_check.py` checks `Class.member` names; a bare class name that does
  not exist (`XRSpatialAnchor`) passes it.
