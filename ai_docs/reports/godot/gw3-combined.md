# GW3 godot-combined: architecture (opus-76)

7 combined skills. Each one is a GodotPrompter v1.14.0 skill, renamed by
`scripts/godot/rename.py`, plus `references/` in own text. The text was
written after reading the gd-agentic-skills skill named for it (revision
`4c4d0ff`). No sentence, table or code was copied from gd-agentic-skills.
Each fact I added was checked on Godot 4.7.2.stable.official.ed1daf0bf: by
a headless test run (scratch projects under
`.worktrees/_scratch/godot-opus-76/exp/`), by the `--doctool` dump, or both.

## Summary

| Skill | Consulted (gd-agentic) | Own references | Own blocks | api_check (whole skill) |
| --- | --- | --- | --- | --- |
| `godot-project-setup` | project-foundations | `naming-and-layout.md`, `settings-and-metadata.md` | 3 parse | 0 unknown |
| `godot-scene-organization` | scene-management | `scene-loading.md`, `scene-transitions.md` | 8 parse | 0 unknown |
| `godot-event-bus` | signal-architecture | `connection-lifetime.md`, `inspecting-connections.md` | 9 parse | 0 unknown |
| `godot-dependency-injection` | autoload-architecture | `autoload-lifecycle.md` | 5 parse | 0 unknown |
| `godot-component-system` | composition, composition-apps | `orchestrator-components.md`, `ui-tool-composition.md` | 14 parse | 0 unknown |
| `godot-resource-pattern` | resource-data-patterns | `resource-lifetime.md`, `reactive-resources.md` | 9 parse | 0 unknown |
| `godot-gdscript-advanced` | gdscript-mastery | `typing-and-warnings.md`, `collections-and-callables.md` | 9 parse | 0 unknown |

The block check (`scripts/godot/gdscript_blocks_check.py`) parses every
block of my own text as a whole script. Most of my blocks also ran in a
headless test, not only parsed. The SKILL.md edits are only these: the
`description:` is extended (each is under 560 bytes) and a "Fleet additions"
list is added at the end. The exception is the fact and API fixes listed below.

## Measured on 4.7.2: facts that changed the text

These results come from my test runs. Several contradict gd-agentic-skills
or common belief.

1. **Lambda connections do not leak when their creator is freed.** A
   capturing lambda connected by object A is removed when A is freed
   (`get_connections().size()` 1 then 0, no error on the next emit). The
   real risk is a lambda that captures a different object B that is freed
   first: the call prints `Lambda capture at index 0 was freed. Passed
   "null" instead.` gd-agentic says that capturing lambdas always need a
   manual disconnect. That claim is wrong on 4.7.2 and was dropped.
2. **`CONNECT_REFERENCE_COUNTED`** counts only flagged connects. Two flagged
   connects need 2 disconnects. A plain connect plus a flagged one goes
   with 1 disconnect. A second plain connect returns `ERR_INVALID_PARAMETER`
   (31). Two separate `_x.unbind(1)` callables compare equal.
3. **Autoload start-up order:** all `_init()` calls run first (not in the
   tree). Then all autoloads and the main scene enter the tree. Then
   `_ready()` runs in list order, and the main scene comes last. In the
   first autoload's `_ready()`, `current_scene` is already set. gd-agentic
   says that `current_scene` is not reliable in an autoload `_ready()`. That
   is wrong for the start-up case and was dropped.
4. **`SceneTree._initialize()` in a `--script` run:** `root` is not inside
   the tree yet. A node added there gets no `_enter_tree()`, a body has no
   physics space, and no autoload `_ready()` has run. After
   `await process_frame` everything works. Every headless check in my
   references awaits a frame first.
5. **`change_scene_to_packed()`:** the old scene leaves the tree at once,
   `current_scene` is `null` until the next frame, and a coroutine in the
   old scene that awaits a frame never resumes. My first test hung for
   exactly this reason.
6. **`Engine.register_singleton()` with a `RefCounted`** prints `RefCounted
   singleton '...' will be disallowed soon; raw pointer will dangle`.
   gd-agentic recommends RefCounted services there. The reference uses an
   `Object` subclass that is freed by hand, and it ran with no warning.
7. **`Resource.duplicate(true)`** copies embedded sub-resources and shares
   external ones. `duplicate_deep()` with no argument also shares external
   ones. `duplicate_deep(Resource.DEEP_DUPLICATE_ALL)` copies both. The
   GW10 note from opus-83 (item 1) cites the 4.5 upgrade guide for the same
   fact.
8. **A custom class whose `_init()` needs an argument** fails to load from
   a `.tres` (`Method expected 1 argument(s), but called with 0`). The load
   returns a plain `Resource` with no script.
9. **GDScript warnings:** a level `1` warning prints nothing in headless
   runs (`load()` and `--check-only`). A level `2` warning is a parse error
   (`Warning treated as error`). These warnings are `2` by default:
   `onready_with_export`, `native_method_override`, `inference_on_variant`
   and `get_node_default_without_onready`. So gd-agentic's "`@onready`
   overwrites `@export`" is now a load error, not silent behavior.
10. **Erasing inside `for key in dict`** skips entries silently. In my test,
    3 keys left 2 keys. Iterating over `keys()` is safe.
11. **`override.cfg` in `user://`** works only with
    `application/config/project_settings_override="user://override.cfg"`.
    With that key set, a saved `run/max_fps=77` was active on the next start.
12. **`--check-only`** exits 0 on a parse error (confirms the wave report).

## Gap analysis per skill

### godot-project-setup

Added: naming table (files, nodes, signals, handlers), `%` unique names,
feature-folder layout, `.gdignore` for raw sources, typed-code warning keys
and their headless behavior, build metadata (`application/config/version`,
`OS.has_feature`, `Engine.get_version_info`), `override.cfg` with a `user://`
path.
Dropped: the gd-agentic EditorScript scaffolders, validators and
dependency auditor (editor-only, not usable headless); the threaded loader
and event bus (covered by `godot-scene-organization` and `godot-event-bus`);
node pooling and `WorkerThreadPool` (optimization and multithreading
skills); the custom `Logger` (debugging skill); "NEVER group by file type"
(the GodotPrompter base recommends a split layout, so this is an opinion,
not a fact); the per-version migration notes (operator decision 3).

### godot-scene-organization

Added: threaded loading (`load_threaded_*` with every status, failure and a
missing path), a loader autoload, early staging, measured scene-change
timing, `change_scene_to_node` and `unload_current_scene`, content packs, a
fade autoload, pause overlays, state across changes, `Node.reparent`,
the close-request save, and an orphan check (`OBJECT_ORPHAN_NODE_COUNT`,
`Node.print_orphan_nodes`).
Dropped: the manual transform-preserving reparent (`Node.reparent` keeps
the global transform by default on 4.7); pooling (optimization skill);
`get_tree().root.get_child(-1)` as the current scene (not needed, see fact 3);
`set_script()` at run time (rare, and a modding topic).

### godot-event-bus

Added: connection flags, double connects, bind/unbind/append-source
argument order, lambda lifetime (fact 1), retargeting, emit-then-free,
`await` on bus signals, a connection report, a headless emit recorder
without GUT, and a symptom table.
Dropped: gd-agentic's nested signal classes on the bus (inner `RefCounted`
classes with signals; they add indirection and no value); the editor
signal debugger (EditorScript); the gdUnit4-only testing note (the base
uses GUT, and my recorder needs no framework).

### godot-dependency-injection

Added: start-up order (fact 3), two-phase start, `static var` state,
engine singletons with an `Object` (fact 6), `Mutex` and deferred emits for
threads (tested with `WorkerThreadPool`, 8 tasks), paused process mode,
"never free an autoload", and a headless health check.
Dropped: "Godot hangs on circular autoload dependencies" (not proven; a
cycle in `_ready()` reads unready state instead, which the text covers);
"never modify a singleton's children in `_ready()`" (not proven); the
lazy-loaded singleton (it adds nodes to root behind the tree's back; the
two-phase start covers the need); the Mermaid diagram generator.

### godot-component-system

Added: the orchestrator rule, input/move/facing components, status effects
as child nodes (tested: tick, end, self-removal), a registry keyed by
`get_global_name()`, slot validation that survives release builds
(`assert` is debug-only), a headless isolation test, and UI/tool/dock
composition (logic/view split, focus ownership, a saveable group, tested).
Dropped: the hitbox/hurtbox/health components (the base has them); the
`Callable`-injected interaction component and the NodePath follower
(variations of what is shown); the "rock test" name (kept the idea as the
isolation test).

### godot-resource-pattern

Added: the copy table (fact 7), `resource_local_to_scene`, cache modes
(measured `CACHE_MODE_IGNORE`), constructor defaults (fact 8), no nodes or
cycles in resources, Resource vs RefCounted vs Node, `emit_changed`, the
in-place collection trap, setter bounds, and a folder check with
`ResourceLoader.list_directory`.
Dropped: the resource pool (a pooling topic, rare for resources); the
EditorScript validator (replaced by the headless folder check); "circular
resource references may crash on load" (not proven; the text states the
proven part, reference cycles are never freed).

### godot-gdscript-advanced

Added: why types are faster, error-level warnings (fact 9), typed loop
variables, typed math helpers (each checked in `@GlobalScope`), `as`/`is`,
constructor defaults for scene scripts, `static var` and `@static_unload`,
member order, functional array methods (tested; there is no `Array.join`),
returned lambdas, erasing during iteration (fact 10), `get()` defaults and
pre-sized packed arrays.
Dropped: the gd-agentic 3.x-to-4.x syntax checklist (decision 3); the
EditorScript type checker, performance analyzer and signal validator; the
"static vars may never be freed (engine bug)" claim (not proven; the text
says to clear large statics by hand, which is safe in both cases).

## Upstream fixes (frozen fork, operator decision 4)

- `godot-gdscript-advanced/references/tool-script-recipes.md`:
  `Mesh.PRIMITIVE_LINE_LOOP` does not exist in 4.7.2 (api_check). It now
  uses `Mesh.PRIMITIVE_LINE_STRIP`, and the vertex loop runs to
  `segments + 1` to close the circle.
- Same file: `DebugDraw.draw_sphere` is a third-party addon. The 4.7.2 dump
  has no `DebugDraw` class. A comment line now says so.
- `godot-project-setup/SKILL.md` checklist: the CI item named
  `godot --headless --check-only`. It now says to use a `load()`-based
  parse check, because `--check-only` exits 0 on a parse error (fact 12).
- `godot-resource-pattern/SKILL.md` section 8 and
  `references/sharing-vs-unique.md` (GDScript and C# comments):
  `duplicate(true)` is no longer "deep" for external sub-resources (fact 7).
  The C# name `Resource.DeepDuplicateMode.All` follows the C# binding's
  naming rule. The dump confirms only the GDScript constant.

Each fix is listed in the skill's `provenance.json` adaptation.

## Upstream findings not fixed (not proven, or not a fact error)

- `godot-gdscript-advanced/SKILL.md` says "Static vars and methods (Godot
  4.4+)". I believe static variables came earlier (4.1). I cannot prove the
  version from the dump, so the text stays.
- `godot-gdscript-advanced/SKILL.md`: "integer vectors are 30-40% faster"
  and "`is_instance_valid()` burns ~1µs per call". These are not measured.
- `godot-project-setup/SKILL.md` checklist: the item about a "## GodotPrompter"
  section in `CLAUDE.md` is for a human workflow. It is not wrong, so it stays.
- Block-check failures in upstream text are fragments that use names from
  their context (`_jump()`, `health`, `EventBus`, `GutTest`, ...). Counts:
  project-setup 2, scene-organization 2, event-bus 10, dependency-injection 5,
  component-system 5, resource-pattern 7, gdscript-advanced 23. The gate
  reports them and does not fail on them. `NOTIFICATION_EDITOR_PRE_SAVE` in
  a gdscript-advanced fragment is a real `Node` constant. It fails only
  because the fragment has no `extends`.

## Commits

All commits used `.worktrees/godot-commit.sh`.

- `bb28987`: project-setup, scene-organization, event-bus.
- `49fd5a8`: dependency-injection, component-system.
- Batch 3 (this report's commit): resource-pattern, gdscript-advanced.

`skills_catalog` passed at each commit. Batch 2 was held twice by other
units' uncommitted work: a missing GW12 oracle file, then a GW12 edit in
`skills_catalog.rs`. The script restored the tree each time.
