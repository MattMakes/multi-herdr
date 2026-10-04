# GW13 godot-integration: report

Plan: `ai_docs/plans/godot/gw13-integration.md`. Worker: opus-87. Date:
2026-10-04. Engine: Godot 4.7.2.stable.official.ed1daf0bf, headless only.
The catalog summary and every unproven claim of the wave are in
`ai_docs/reports/godot/wave-final.md`.

## Commits

| commit | what |
|---|---|
| e73369e | `scripts/godot/` (api_check, xref_check, csharp_blocks_check, known_classes.txt, tests), the gate step, 21 skills. It also took an uncommitted `rename.py` edit that is not GW13's (see Checks). |
| (the commit that adds this file) | this report and `wave-final.md` |

## 1. api_check.py: the 2 gaps

**Bare class names.** In GDScript a class name must exist after `extends`,
in a type annotation (`var x: Foo`, a `func` parameter, `for x: Foo in`),
after `->`, in `Foo.new()`, `is Foo`, `as Foo` and `Array[Foo]` /
`Dictionary[K, Foo]`. Strings and comments are cut first. A name is allowed
when the bundle defines it (`class_name`, inner `class`, `enum`, `const`
anywhere in the skill directory, or the file when it has no skill), or
when `scripts/godot/known_classes.txt` lists it for that skill. The list
has 2 sections: addon classes (Beehave, LimboAI, Dialogue Manager, gdUnit4,
GUT, Phantom Camera, Popochiu, Google Play Billing, the GDExtension example
class) and project classes that an example assumes (`Player`, `Bullet`,
`HealthComponent` in a skill that does not define it, ...). First run: 114
unknown names, all of them addon or example classes; 0 engine typos. The
list holds 49 names, each limited to the skills that use it.

**Deprecated members.** The plan expected `deprecated="..."` in the doctool
XML. Measured: the `--doctool` dump of the release binary has no
deprecation marks and no descriptions, and `--dump-extension-api-with-docs`
has descriptions but no deprecation field either. The marks are in the
editor's doc cache, `~/Library/Caches/Godot/editor_doc_cache-4.7.res` on
this Mac (a `Resource` whose `classes` metadata holds 1,076 class dicts
with `deprecated` keys: 272 methods, 159 constants, 30 properties, 3 theme
items, 2 signals and 10 classes). `api_check.py` now:

- reads `deprecated="..."` from any dump XML (the official doc XML has it;
  the test fixture uses it), and `deprecated.json` next to the dump;
- writes `deprecated.json` once from the editor doc cache with a headless
  script (`DOC_CACHE_SCRIPT`), when the cache exists (macOS, Linux and
  Windows cache paths); without it, it prints `deprecated: no data` and
  checks the rest;
- reports a deprecated `Class.member`, a bare deprecated class, and a call
  `.some_method(` on any value when every class with that method marks it
  deprecated (only names with an underscore; `.resolve(` on a project
  object is not an engine call). The known case
  `NavigationPolygon.make_polygons_from_outlines` is in the data.
- `--strict-own skills/provenance.json`: a deprecated name fails in own text
  and is report-only in upstream files. Unknown names fail everywhere.
- A comment line `# api-check: allow X` alone on a line allows `X` for the
  rest of the code block.

Result on the 64 skills: 0 unknown, 4 deprecated `TileMap` uses, all in the
`TileMap` to `TileMapLayer` converter of
`godot-version-migration/references/headless-tools.md`. That script reads
the deprecated node on purpose; the block now starts with
`# api-check: allow TileMap`.

## 2. xref_check.py

`scripts/godot/xref_check.py` checks every text file of every `godot-*`
skill: each `godot-<name>` word names a skill directory, unless it is a
teammate (`teammates/godot-<name>.md`), a project in `EXTERNAL` (godot-cpp,
godot-docs, godot-headers, godot-ios-plugins, godot-proposals, godot-rust,
godot-wfc, godot-xr-tools), or part of a URL, path or anchor. It also checks
that every relative markdown link names a file and every `#anchor` names a
heading (GitHub slugs). Result: 0 broken references in 389 files. The genre
references that GW8 wrote before their skills existed all resolve now.
Gate: `python3 scripts/godot/xref_check.py`, first in the Godot step.

## 3. csharp_blocks_check.py (orchestrator item 1)

Each ```csharp / ```cs block is compiled in a scratch `Godot.NET.Sdk/4.7.2`
project (`net8.0`, .NET SDK 10.0.101, packages from NuGet into
`.worktrees/_scratch/godot-csblockcheck/nuget`, dotnet home in the scratch
too). A block is compiled as a whole file (if it declares a type), and as
class members and as statements inside a class for each of 7 bases; it
passes when one form compiles. Measured: csc reports only the first stage
that has errors (syntax, then declarations, then method bodies), so the
build repeats without the forms that got errors until a pass is clean
(at most 8 passes; a form left unchecked fails). `--strict-own` as in the
GDScript check. Run time on the 64 skills: 24 s with a warm NuGet cache, so
it is in the gate. Unit tests: 4 (2 need dotnet).

Result: 494 blocks; every own-text block compiles; 0 own failures. The
upstream failures are report-only: fragments that use a field or type from
another block (CS0103, CS0246, GD0102, GD0202), addon types, and 3 deliberate
"WRONG" examples.

The orchestrator's suspect `_animPlayer.SpeedScale = 2.0;`
(`godot-animation-system`) compiles: `AnimationPlayer.SpeedScale` is a
`double` in GodotSharp 4.7.2. No change.

## 4. Upstream API and fact fixes (standing rule)

Proved by the C# build (GodotSharp 4.7.2), the `--doctool` dump, the editor
doc cache descriptions, or a headless run. Each is in the skill's
provenance `adaptation`.

| skill | file | fix |
|---|---|---|
| godot-animation-system | `references/bone-constraints.md` | `AimModifier3D` / `CopyTransformModifier3D` have no `bone_name`, `target_bone_name`, `source_bone_name`, `copy_*` properties or angle limits. Rewritten to the per-setting API (`setting_count`, `set_apply_bone_name`, `set_reference_type`, `set_reference_bone_name`, `set_forward_axis`, `set_use_euler`, `set_primary_rotation_axis`, `set_copy_*`), GDScript and C#; angle limits point at `LookAtModifier3D`. Run headless: getters return the set values, no error. |
| godot-animation-system | `references/ik-recipes.md` | `FABRIK3D` has no `bone_chain`; sketches named `tip_bone`/`root_bone`. Now `set_root_bone_name`, `set_end_bone_name`, `set_target_node` per setting (run headless: 3 joints, target found). C# names `JacobianIK3D`, `TwoBoneIK3D` (not `...Ik3D`). |
| godot-3d-essentials | `references/environment-and-post.md` | AgX uses `tonemap_agx_white` (default 16.29) and `tonemap_agx_contrast` (default 1.25); `tonemap_contrast` does not exist. |
| godot-particles-vfx | `references/subemitters.md` | `ParticleProcessMaterial.sub_emitter_node` does not exist; the slot is `GPUParticles3D.sub_emitter` (NodePath on the parent node). Run headless. |
| godot-addon-development | `references/inspector-plugins.md` | C# override `_GenerateSmallPreview` -> `_GenerateSmallPreviewAutomatically` |
| godot-xr-development | `SKILL.md`, `references/grabbing-objects.md` | C# `UseXr` -> `UseXR`; C# `Generic6DOFJoint3D` -> `Generic6DofJoint3D` |
| godot-ai-navigation | `SKILL.md`, `references/steering-behaviors.md` | C# `PackedVector2Array` -> `Vector2[]`; `GD.RandRange` double cast to float |
| godot-math-essentials | `SKILL.md`, `references/random-numbers.md` | C# `Basis.IsOrthonormal()` does not exist (GDScript has it) -> `IsEqualApprox(Orthonormalized())`; `GD.Randi()` returns `uint`; C# `GD` has no `RandfRange`/`RandiRange` -> `GD.RandRange` |
| godot-debugging | `references/performance-debugging.md`, `systematic-method.md` | `Time.GetTicksUsec()`, `Engine.GetProcessFrames()` return `ulong` |
| godot-state-machine | `SKILL.md` | `GD.RandRange` double cast to float |
| godot-physics-system | `references/ragdoll-recipes.md` | `PhysicalBonesStartSimulation` takes `Godot.Collections.Array<StringName>` |
| godot-responsive-ui | `SKILL.md`, `references/mobile.md` | `GetVisibleRect().Size` is `Vector2`; `DisplayServer.ScreenOrientationEnum` -> `ScreenOrientation` |
| godot-csharp-signals | `references/awaiting-signals.md` | `SignalAwaiter.AsTask()` does not exist -> a 1-line async helper |
| godot-multiplayer-sync | `SKILL.md` | visibility filter `Callable.From<int, bool>` (it returns bool) |
| godot-gdscript-advanced | `SKILL.md` | static vars since 4.1, not 4.4 (CHANGELOG.md at `4.1-stable`: GH-76264). GW3's open item. |

`api_check.py` cannot see instance-member errors such as the bone
constraint and IK ones (`aim.bone_name = ...` on a typed local). The C#
build found them through the C# twins; the GDScript twins were fixed with
them.

## 5. Links (plan step 3)

- `godot-gameplay-loops` from the genre blueprints, where the loop is part
  of the genre's core loop: tower-defense and moba (waves), survival,
  sandbox, rts and idle-clicker (harvest; idle and offline gains), racing
  (time trial, 2 rows), platformer (revival, collection), metroidvania
  (collection, secrets). `SKILL.md` "Rules that hold in every genre" names
  the skill. Not linked: genres whose core loop has none of the 6 loops
  (roguelike death ends the run, it is not revival; action-rpg loot is not
  a collection set; and the other 16).
- `godot-gameplay-loops` -> `godot-genre-blueprints` (its "Other skills" table).
- `godot-component-system` -> `godot-combat-system` (Fleet additions): the
  combat bundle extends this skill's hurtbox (GW9). The other direction,
  and combat/economy/quest/loops among each other, already existed.
- `godot-inventory-system` <-> `godot-economy-system`: both declare
  `class_name LootTable` with different fields. Each reference now says so
  and tells the worker to keep one. Found by this unit, not by a report.

## 6. Report items: closed or why not

| report | item | result |
|---|---|---|
| GW0 | content units must fix 7 API errors | done by GW3-GW7; api_check 0 unknown |
| GW0 | `StringName.Empty` in C# | fixed by GW4; the C# build agrees |
| GW1 | `@export_file` / `@export_file_path` follow-up | present in `godot-gdscript-patterns/references/fleet-additions.md` |
| GW2 | Jolt default 4.4 vs 4.6 | fixed by GW4 |
| GW2 | logs of runs without `HOME` go to the real home | host fact, in wave-final.md |
| GW3 | static vars 4.4 | fixed (table above) |
| GW3 | "integer vectors 30-40% faster", "`is_instance_valid()` ~1 µs" | not measured; listed in wave-final.md |
| GW4 | `make_polygons_from_outlines` deprecated | now caught by api_check; no skill uses it |
| GW4 | `godot-combat-system` name in `progression-and-formulas.md` | resolves (xref_check) |
| GW5 | C# `SpeedScale = 2.0` | compiles; no change |
| GW5 | `randf()` "different platforms" pitfall | not tested across platforms; listed |
| GW5 | 5 measured facts | checked below |
| GW6 | `print_debug()` stripped in release | no release template; listed |
| GW7 | api_check misses bare classes | done (step 1) |
| GW7 | never await `frame_post_draw` under `--headless` | stated in `godot-testing`, `godot-dimension-port`, `godot-shader-basics`; `godot-build-verify` does not await it, so no change |
| GW7 | dedicated-server template claim | not proven; listed |
| GW8 | pointer to `godot-gameplay-loops` | done (section 5) |
| GW9 | headless proofs not in the gate | not done: the proofs need their scratch projects and an import (minutes, not seconds); moving them is a unit of its own. Listed. |
| GW10 | 8 proposals | all 8 are in the skills (GW1-GW5 and GW7 added them) |
| GW10 | 3 wrong rows in the official 3-to-4 guide | upstream doc bugs; no action here |
| GW11 | dataset coordinator gets no `skills_when` skills; slow `roster` test runs | Rust, outside this unit (do not touch `.rs`) |
| GW12 | `HORCH_PROJECT_DIR` gotcha; no oracle files for Godot seats | listed in wave-final.md; teammates are GW12's |

### GW5 measured facts (orchestrator item 2)

| fact | where the catalog uses it | state |
|---|---|---|
| JSON numbers load as floats | `godot-save-load/references/binary-saves.md`, `godot-dialogue-system/references/events-validation-state.md`, `godot-multiplayer-basics/references/local-testing-and-discovery.md`; the dedicated-server JSON readers cast with `int()` | correct; the upstream `json-saves.md` assigns parsed numbers without `int()` but makes no claim, and SKILL.md links the trap |
| `TileMapLayer.changed` is deferred | `godot-2d-essentials/references/tilemap-runtime.md` only | correct |
| CSG `bake_static_mesh()` is null in the frame of `add_child()` | `godot-3d-essentials/references/gridmap-and-csg.md` only | correct |
| nested state-machine condition path | `godot-animation-system/references/tree-layering-and-root-motion.md` only | correct |
| `load()` accepts unknown members on typed engine variables | re-measured here (`var b: Node2D; b.no_such_method()` loads, `can_instantiate()` true; an unknown name on `self` is a parse error) | was missing from `godot-build-verify/references/parse-check.md`; added |

## Checks at the final commit

Measured at commit e73369e on the 64 skills (389 files):

| check | result | time |
|---|---|---|
| `scripts/godot/xref_check.py` | 0 broken references | 0.3 s |
| `scripts/godot/api_check.py --strict-own` | 0 unknown names, 0 deprecated (4 `TileMap` uses allowed on purpose in the converter) | 3 s |
| `scripts/godot/gdscript_blocks_check.py --strict-own` | 1,053 blocks: 734 parse (604 whole, 57 with an added `extends`, 73 in a func), 319 fail, all upstream (report only); exit 0 | 8 s |
| `scripts/godot/csharp_blocks_check.py --strict-own` | 494 blocks: 264 compile (120 whole), 230 fail, all upstream (report only); exit 0 | 21 s (warm NuGet cache) |
| `python3 -m unittest discover -s scripts/godot/tests` | 31 tests; at e73369e the 8 `test_rename` tests error, because that commit swept in an unfinished `rename.py` edit from the licence-removal unit (G10), without its test changes. The other 23 pass. | 15 s |

## Gotchas

- zsh does not split an unquoted variable: `python3 x.py $L` with a
  newline-separated `$L` passes 1 argument. Use an array.
- macOS has no `timeout` command; a Godot script that errors before
  `quit()` hangs. Run Godot in the background with a kill after N seconds.
- `NUGET_PACKAGES` must be an absolute path.
