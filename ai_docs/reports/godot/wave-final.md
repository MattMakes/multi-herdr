# Godot wave: final report

The Godot wave (units GW0 to GW13, 2026-10-04) added 64 `godot-*` skills,
3 checks that run in the gate, 19 Godot teammates (GW12) and the
`requires: [godot]` / `skills_when` roster fields (GW11). Target engine:
Godot 4.7, proved on 4.7.2.stable.official.ed1daf0bf on macOS, headless only.
Per-unit detail is in `ai_docs/reports/godot/gw*.md`; the integration unit
is `gw13-integration.md`.

## The catalog: 64 skills

**Combined (36):** a copied upstream skill renamed by `rename.py`, plus
own-text references for the topics it lacked.
`vendored: true`.

| unit | skills |
|---|---|
| GW3 architecture | project-setup, scene-organization, event-bus, dependency-injection, component-system, resource-pattern, gdscript-advanced |
| GW4 gameplay | player-controller, state-machine, input-handling, physics-system, camera-system, ai-navigation, ability-system |
| GW5 systems and worlds | inventory-system, save-load, dialogue-system, procedural-generation, 2d-essentials, 3d-essentials, animation-system |
| GW6 presentation and diagnosis | ui, tween-animation, shader-basics, particles-vfx, audio-system, debugging, optimization |
| GW7 platforms, network, quality | export-pipeline, mobile-development, xr-development, multiplayer-basics, multiplayer-sync, dedicated-server, testing, code-review |

**Copied only (18, GW1):** renamed and not edited, except 2 adapted
(grill, brainstorming) and later API fixes: addon-development,
assets-pipeline, beehave, brainstorming, csharp-godot, csharp-signals,
dialogue-manager, gdextension, gdscript-patterns, grill, hud-system,
limboai, localization, math-essentials, multithreading, phantom-camera,
popochiu, responsive-ui.

**Own text (10):** build-verify, project-context, scene-files (GW2);
genre-blueprints with 27 genres and project templates (GW8);
gameplay-loops, combat-system, economy-system, quest-system (GW9);
version-migration, dimension-port (GW10).

Every skill name above has the prefix `godot-`.

## The checks and their final numbers

All in the `godot_skills` step of `scripts/phase-gate.sh`. Each Godot check
prints `skipped: no Godot` (the C# check `skipped: no dotnet`) and passes
on a host without the tool. `--strict-own skills/provenance.json` makes a
finding in a file copied from upstream report-only; own text is strict.

Measured at commit e73369e on the 64 skills (389 files):

| check | result | time |
|---|---|---|
| `scripts/godot/xref_check.py` | 0 broken references | 0.3 s |
| `scripts/godot/api_check.py --strict-own` | 0 unknown names, 0 deprecated (4 `TileMap` uses allowed on purpose in the converter) | 3 s |
| `scripts/godot/gdscript_blocks_check.py --strict-own` | 1,053 blocks: 734 parse (604 whole, 57 with an added `extends`, 73 in a func), 319 fail, all upstream (report only); exit 0 | 8 s |
| `scripts/godot/csharp_blocks_check.py --strict-own` | 494 blocks: 264 compile (120 whole), 230 fail, all upstream (report only); exit 0 | 21 s (warm NuGet cache) |
| `python3 -m unittest discover -s scripts/godot/tests` | 31 tests; at e73369e the 8 `test_rename` tests error, because that commit swept in an unfinished `rename.py` edit from the licence-removal unit (G10), without its test changes. The other 23 pass. | 15 s |

What the checks do not prove:

- api_check reads code blocks, not prose, and checks `Class.member` and
  bare class names. A member on a typed instance (`aim.bone_name = ...`)
  is not checked; the C# build finds the C# twin of such an error, and a
  GDScript-only block can still hold one.
- The GDScript and C# checks prove that a block parses or compiles, not
  that it does what the text says. Runtime behaviour is proved only where a
  unit ran it headless (each unit report lists its runs).
- Upstream fragments that use names from their context do not compile
  alone; they are reported, not failed.

## Claims not proved on this host

Each stays in the skill as written (or was not carried) because this Mac
cannot test it. The unit report named in brackets has the detail.

| claim or code | why not proved |
|---|---|
| Every shader (`glsl` / `gdshader` blocks) in 3d-essentials, shader-basics, particles-vfx [GW5, GW6] | headless Godot uses the dummy renderer and compiles no shader |
| GPU-dependent behaviour: CPUParticles2D vs GPUParticles2D stutter on physics bodies, `emit_particle()` requirements, MSAA and foveation conflict [GW6, GW7] | no rendering under `--headless`; not carried |
| UPnP port mapping [GW7] | needs a router |
| DTLS setup, the WebSocket TLS path, the matchmaker HTTP call [GW7] | no certificates or services on this host |
| XR code; `Engine.get_singleton("OpenXRSpatialAnchorCapability")` [GW7] | no headset; OpenXR singletons exist only with OpenXR enabled |
| Store SDK calls (Steam, Google Play Billing, iOS IAP), console behaviour [GW7, GW1] | no SDKs, NDA platforms |
| Release-template behaviour: `print_debug()` stripped in release, the orphan monitor value in release [GW6] | no export templates installed; only debug runs were measured |
| "A server export template strips rendering from the binary" (`godot-dedicated-server/SKILL.md`) [GW7] | official 4.x builds have no separate server template ("Export As Dedicated Server" strips resources); not tested, left as written |
| The gdUnit4 C# path (gdUnit4Net `dotnet test`), the gdUnit4 `-c` flag, GUT `-gjunit_xml_file` output [GW7] | taken from addon help text; not run |
| Running C# scenes and C# tests [GW2] | needs `Godot_mono.app`, not installed; `dotnet build` works (SDK 10.0.101, Godot.NET.Sdk 4.7.2) |
| Linux `XDG_*` isolation [GW2] | no Linux host |
| Editor detection with `ps` against a live editor [GW2] | the GUI is forbidden |
| "Integer vectors are 30-40% faster", "`is_instance_valid()` costs ~1 µs" (`godot-gdscript-advanced`) [GW3] | not measured |
| "Different results on different platforms: global `randf()`" (`godot-procedural-generation`) [GW5] | not tested across platforms |
| 2D root motion from `AnimationTree` [GW4] | not verified; not carried |
| "Emission above 1.0 needs HDR in Project Settings" [GW5] | no such 3D setting found; not carried |
| "Offset a 3D sound 0.1 units to avoid panning jitter" [GW6] | not provable headless; not carried |
| `MultiplayerSynchronizer` "supports only primitive types" [GW9] | not verified; not carried |
| 4.7 stretch defaults in project templates [GW8] | not verified; not carried |
| Game feel: hit-stop length, knockback, ghost smoothness [GW9] | needs a human play test |
| 3D loop nodes (`CollectiblePickup`, `HarvestNode`, `GhostTrack` playback) [GW9] | parse- and load-checked, not run in a scene |
| `model_to_sprite` bake with a `@tool` SubViewport [GW10] | cannot run under `--headless` (`frame_post_draw` never fires); the skill sends a `QUESTION:` for an operator run |
| Bone constraint and IK modifiers moving bones as described [GW13] | the API calls run and the getters agree; the posed result was not asserted |
| `api_check` deprecation data on another host [GW13] | comes from the editor doc cache; a host without one prints `deprecated: no data` |

GW9's headless proofs (`.worktrees/_scratch/godot-opus-82/proof/`) are not
in the gate: each needs its scratch project and an import. Moving them into
a test is a separate unit.

## Host facts (macOS, Godot 4.7.2)

- macOS Godot ignores `XDG_DATA_HOME`, `XDG_CONFIG_HOME` and
  `XDG_CACHE_HOME`. It writes editor settings, `app_userdata/<project>/`
  (with `logs/`) and export templates to
  `~/Library/Application Support/Godot/`, and the doc cache to
  `~/Library/Caches/Godot/` in the real home. Headless runs that do not set
  `HOME` leave logs in
  `~/Library/Application Support/Godot/app_userdata/[unnamed project]/logs`.
- `HOME=<dir>` isolates all of it under `<dir>/Library/...`. The skills use
  `HOME="$PWD/.godot/horch-home"`.
- `--check-only` prints `Parse Error` and exits 0. `load()` of a script
  with a parse error returns a non-null `GDScript` whose `can_instantiate()`
  is false.
- `load()` accepts an unknown member on a typed engine variable
  (`var b: Node2D; b.no_such_method()`); only an unknown name on `self` is
  a parse error.
- Under `--headless`: `OS.has_feature("headless")` is false,
  `DisplayServer.get_name()` is `"headless"`, `RenderingServer.frame_post_draw`
  is never emitted (an `await` hangs), a viewport image is `null`, and no
  shader compiles.
- A runtime `SCRIPT ERROR` and `push_error()` still exit 0; gate on the log.
- The `--doctool` dump of the release binary (1,076 class XML files) has no
  descriptions and no deprecation marks. `--dump-extension-api-with-docs`
  has descriptions and no deprecation field. The editor doc cache
  (`editor_doc_cache-4.7.res`) has both.
- `dotnet` 10.0.101 builds `Godot.NET.Sdk/4.7.2` projects for `net8.0`
  from NuGet without Godot_mono. `NUGET_PACKAGES` must be absolute.
- macOS has no `timeout` command. A Godot script that errors before
  `quit()` keeps running; start it in the background and kill it.
- One Godot import at a time per project directory.
- A fleet pane sets `HORCH_PROJECT_DIR` to this repo, so `cd` alone does not
  change what `horch doctor` sees (GW12).

## Open items outside the skills

- Rust (GW11): the dataset coordinator plans with a roster without project
  facts, so its candidates get no `skills_when` skills; 2 `roster` test runs
  took 285 s and 461 s once, cause not found.
- The `skills_catalog` oracle loop skips every Godot seat; no seat has an
  oracle file (GW12).
