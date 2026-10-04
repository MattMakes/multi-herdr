# GW2: own Godot skills (project-context, build-verify, scene-files)

Unit GW2, worker opus-75, 2026-10-04. Plan:
`ai_docs/plans/godot/gw2-own-skills.md`.

## What

Three skills in own text, each proven on Godot 4.7.2.stable.official
(`ed1daf0bf`, macOS) in scratch projects under
`.worktrees/_scratch/godot-gw2/` (git-ignored):

| skill | files | SKILL.md |
|---|---|---|
| `godot-build-verify` | `SKILL.md`, `references/commands.md`, `references/parse-check.md` | 8.5 KB |
| `godot-project-context` | `SKILL.md`, `references/context-scan.md` | 8.4 KB |
| `godot-scene-files` | `SKILL.md`, `references/worked-example.md`, `references/fleet-additions.md` | 7.7 KB |

The bundles hold text only (`skills_bundled_text_only`), so each GDScript tool
(parse checker, context scan, scene check, read-back, UID setter, scene
builder) is a ```gdscript block in a reference. The skill tells the worker
to write it to `.godot/horch-home/` (git-ignored, never scanned) and run it.
The plan allowed `references/parse_check.gd`; the text-only test forbids
`.gd`, so the checker is in `references/parse-check.md`.

## Sources consulted

- `skills/ue-project-context`, `skills/ue-build-verify`: shape and fleet rules.
- gd-agentic-skills `godot-builder` at `4c4d0ff` (LGPL-3.0), SKILL.md only,
  read for facts: XDG isolation, `owner` on generated nodes, UIDs after a
  headless `ResourceSaver` save, `quit()` at the end of a `-s` script. Nothing
  was copied. Each fact was re-tested; 2 of 4 were wrong or incomplete on macOS
  4.7.2 (below).
- Godot 4.7.2 `--help`, the `--doctool` dump, and `doc/classes/ProjectSettings.xml`
  at tag `4.7.2-stable` (physics engine defaults, `dotnet/project/assembly_name`).
- GUT and gdUnit4 sources at the pinned tags (exit codes, `-rd`).
- GW10 (opus-83) fact 8, relayed by the orchestrator: since 4.6 a `.tscn` has
  `unique_id` per node and no `load_steps` (GH-106837, GH-103352). Written to
  `godot-scene-files/references/fleet-additions.md`.

## Test frameworks (pinned, scratch only)

| framework | tag | commit | support |
|---|---|---|---|
| GUT | `v9.7.1` | `aeb5d4f3f7f0a6c9b5e178876d6c99b791fda605` | README: 9.7.1 is the `godot_4_7` branch, Godot 4.7.x |
| gdUnit4 | `v6.2.1` | `08ffc7c65b61b1b2edd545616061a99973c13ce1` | README: 4.5 to 4.7.1; run here on 4.7.2 |

Shallow clones in `.worktrees/_scratch/godot-gw2/gut-src` and `gdunit-src`;
the addons were copied into the scratch projects only.

## The "to verify" items

| item | result on 4.7.2 macOS |
|---|---|
| `XDG_DATA_HOME`/`XDG_CONFIG_HOME`/`XDG_CACHE_HOME` per instance | **Ignored on macOS.** `--import` with all three set rewrote `~/Library/Application Support/Godot/editor_settings-4.7.tres` and made `app_userdata/Blank/`; the XDG dirs stayed empty. |
| where user data and editor settings go during `--import` | `$HOME/Library/Application Support/Godot/` (`editor_settings-4.7.tres`, `app_userdata/<name>/`, `export_templates/`, ...) and `$HOME/Library/Caches/Godot/` (`editor_doc_cache-4.7.res`). |
| isolation that works | **`HOME=<dir>`**: all of the above goes under `<dir>/Library/...`; nothing under the real home changed (checked with `find -newer`). The skills use `HOME="$PWD/.godot/horch-home"`. Linux XDG: not run (no Linux host). |
| `owner` on generated nodes | Confirmed: a child added without `owner` is silently left out of the saved `.tscn`. |
| `ResourceSaver` + `--import` writes UIDs | **Partly.** `--import` writes `<script>.gd.uid` sidecars. Neither a headless `ResourceSaver.save()` (before or after the import) nor `--import` writes `uid=` into a `.tscn` header or its `ext_resource` lines. `ResourceUID.create_id_for_path()` + `ResourceSaver.set_uid()` writes the header uid; documented as "A UID on demand". |
| `--quit-after` on the main scene | `--quit-after 120` runs `run/main_scene` about 2 s and exits 0. With `--scene res://x.tscn` it runs that scene. A runtime `SCRIPT ERROR` still exits 0. |
| `--check-only` | Prints `SCRIPT ERROR: Parse Error` and exits 0 (confirmed). |

## Runs (all on 4.7.2; logs in the scratch dir)

Scratch projects: `blank` (main scene, scripts), `gut` (GUT 9.7.1), `gdunit`
(gdUnit4 6.2.1), `ctx` (autoload, input action, addon `plugin.cfg` 1.4.2,
Jolt, `canvas_items` stretch, `mobile` renderer, export preset; settings
written by Godot's own `ProjectSettings.save()`), `csharp` (`.csproj`,
`"C#"` feature), `newer` (`config/features` 4.8), `scenes` and `we` (scene
files).

godot-build-verify (the bash blocks of `commands.md` were extracted and run
as written):

| run | exit | note |
|---|---|---|
| engine lookup, project 4.7 | 0 | `have=4.7 want=4.7` |
| engine lookup, project 4.8 | 3 | `REFUSE: project wants Godot 4.8, engine is 4.7` |
| engine lookup, `GODOT_PATH=/nonexistent` | 2 | `NO_ENGINE` |
| `--import` (blank, gut, gdunit) | 0 | `.uid` sidecars written |
| parse check, 2 bad scripts (missing colon; type error + unknown function) | 1 | `PARSE_CHECK FAIL` for both, `checked=6 failed=2` |
| parse check, good project (incl. `@abstract` class, cross-file `class_name`) | 0 | `checked=4 failed=0` |
| parse check, gdunit project with a broken test script | 1 | names `res://terr/test_broken.gd` |
| GUT pass / fail | 0 / 1 | |
| GUT empty dir / broken test script / broken + good script | 0 / 0 / 0 | broken script skipped; `All tests passed!` |
| gdUnit4 pass / fail / orphan / no flag / broken script / no tests | 0 / 100 / 101 / 103 / 105 / 0 | the report's 0/1/2 codes are not the 6.2.1 codes |
| gdUnit4 `-rd <absolute path>` | 0 | made `Users/...` inside the project; `-rd res://.godot/horch-home/...` works |
| smoke `--quit-after 120`, null call in `_process` | 0 | `SCRIPT ERROR: Cannot call method 'queue_free' on a null value.` |
| `dotnet build` pass / compile error | 0 / 1 | .NET SDK 10.0.101, `Godot.NET.Sdk/4.7.2` from NuGet, `net8.0`; `NUGET_PACKAGES` kept in scratch |
| standard build runs a C# scene | 0 | `ERROR: No loader found for resource: res://Player.cs` |
| static code on load | - | the parse check runs static var initializers and `_static_init()`; documented |

C#: `dotnet` is installed (10.0.101), so the build step ran. Running C#
scenes or C# tests needs `Godot_mono.app`, which is not installed: that part
is "not run on this host" in `commands.md`.

godot-project-context: the scan block of `references/context-scan.md` was
extracted and run as written on all 6 projects (blank, gut, gdunit, csharp,
ctx, newer), exit 0 each; `project.godot` unchanged (md5). The example output
in the reference is the `ctx` run. A project without `addons/` printed an
`ERROR:` in the first draft; fixed with a `dir_exists_absolute` guard.

godot-scene-files: the worked example was replayed from the markdown in a
fresh project (`we`): build exit 0 `BUILD_OK`; text edits; scene check exit 0
(`nodes=7`, `nodes=8`); read-back `(96.0, 32.0)` and `(128.0, 16.0)`; smoke
prints `CONNECTED timeout reached`; a typo in `method=` gives exit 1; UID set
then `UID_KEEP`, diff shows only the header line. Wrong-edit table, measured:

| wrong edit | result |
|---|---|
| unknown `ExtResource` id, `sub_resource` after use, unknown `SubResource` id, broken `Vector2(` | load fails (`Parse Error`), exit 1 |
| `ext_resource` path to a missing file | loads; `ERROR: ... referenced non-existent resource` |
| `parent=` names a missing node | loads; `WARNING: Parent path ... has vanished` |
| `[connection]` with a missing method | loads, runs, prints nothing; `Callable.is_valid()` is false. The scene check tests it. |
| misspelled property `positon` | loads, prints nothing; value stays default. Read-back catches it. |

## Checks

- `scripts/godot/api_check.py` on the 3 skills: 7 files, 0 unknown names,
  exit 0.
- `scripts/godot/gdscript_blocks_check.py` on the 3 skills: 7 blocks, 7 parse
  whole, 0 fail, exit 0.
- Frontmatter parses as YAML; descriptions are quoted, 423 / 511 / 466 bytes.
- `cargo test -q -p horch-core --test skills_catalog`: see the commit.

## Dropped or changed against the wave report

- "gdUnit4 exit codes 0/1/2": wrong for 6.2.1; the measured table replaces it.
- "per-instance XDG_*": does not work on macOS; `HOME` replaces it.
- "the import writes the UID": only for script sidecars; scenes keep no uid
  until the editor saves them or `set_uid` runs.
- Editor detection: `ps` for a Godot process without `--headless` whose
  arguments name the project path. Not run against a live editor (GUI is
  forbidden); the `--path` argument form was seen in other workers' headless
  processes.

## Outside scope (not fixed)

- GW10 notes that `godot-physics-system` says Jolt is the 3D default since 4.4;
  the 4.7.2 class reference says "the default for projects created starting in
  Godot 4.6". `godot-project-context` states 4.6.
- `~/Library/Application Support/Godot/app_userdata/[unnamed project]/logs`
  gets logs from other workers' headless runs that do not set `HOME`.
