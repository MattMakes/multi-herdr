---
name: godot-scene-files
description: "Use when you create or change a Godot 4 .tscn scene or .tres resource - editing it as text (properties, nodes, sub_resource and ext_resource ids, signal connections, instanced scenes) or building it with a headless SceneTree script and ResourceSaver - and must prove it with a headless load. Covers owner on generated nodes, uid:// and .uid sidecars, unique_id churn, and what never to edit (.godot/, .import, binary .res/.scn). Targets Godot 4.7; measured on 4.7.2."
---

# Godot Scene Files

`.tscn` and `.tres` files are text, and a fleet worker can edit them. But the
engine accepts some wrong edits without an error. This skill gives the rules,
the two ways to change a scene, and the check that proves the change. Every
fact below was measured on Godot 4.7.2. The worked example, with every file
and command, is in `references/worked-example.md`.

## Inputs

- `.agents/godot-project-context.md` (engine version, main scene). If it is
  missing, send `QUESTION:` and ask for `godot-tech-lead`. Exception: a new
  project has no context file yet. Use the engine version and the main scene
  that the brief names, and say so in your `DONE:` (`godot-build-verify`,
  "Inputs").
- The engine path and the `gd` helper from `godot-build-verify`,
  `references/commands.md` ("Find the engine", "Common setup"). Every Godot
  call is headless, with `HOME="$PWD/.godot/horch-home"`.

## The file format (4.7)

```text
[gd_scene format=3 uid="uid://dgpkaqj443icv"]

[ext_resource type="Script" path="res://levels/level.gd" id="1_vn7o3"]

[sub_resource type="RectangleShape2D" id="RectangleShape2D_jirdo"]
size = Vector2(32, 128)

[node name="Level1" type="Node2D" unique_id=1590365356]
script = ExtResource("1_vn7o3")

[node name="Shape" type="CollisionShape2D" parent="Wall" unique_id=559493353]
shape = SubResource("RectangleShape2D_jirdo")

[connection signal="timeout" from="Timer" to="." method="_on_timer_timeout"]
```

- Sections come in this order: header, `ext_resource`, `sub_resource`,
  `node`, `connection`. A `SubResource("x")` used before its
  `[sub_resource ... id="x"]` fails to load.
- `parent="."` is the root; `parent="Wall"` and `parent="Wall/Body"` are
  paths from the root. The root has no `parent`.
- An instanced scene is `[node name="Level" parent="." instance=ExtResource("1_lvl")]`.
  To override a property of one of its nodes, add
  `[node name="Spawn" parent="Level"]` with the property, and no `type`.
- Since 4.6, Godot writes a `unique_id` per node and no `load_steps` in the
  header (`references/fleet-additions.md`). Do not add `load_steps`.
- A `.tres` has a `[gd_resource type="..." format=3]` header and a
  `[resource]` section for its own properties; `ext_resource` and
  `sub_resource` work the same way.

## Rules

1. **Keep ids consistent.** Every `ExtResource("id")` and `SubResource("id")`
   names an id defined in the same file. A new id must be unique in the file;
   any string works (`RectangleShape2D_wall2`). A wrong id fails the load with
   `Parse Error`.
2. **Never type a `uid://`.** A missing `uid=` is valid: the scene loads and
   is referenced by `res://` path. Measured: neither a headless
   `ResourceSaver.save()` nor `--import` writes a `uid=` into a `.tscn`
   header or an `ext_resource` line. The editor writes them when it saves the
   scene (not run here: no GUI). If the task needs a stable UID now, let the
   engine make it (`references/worked-example.md`, "A UID on demand").
3. **Never invent a `unique_id=`.** Nodes you add by text need none; the scene
   loads without it. Godot 4.7 writes a new random `unique_id` on every node
   each time a script re-saves the scene, so a re-save changes every node
   line. For an existing scene, prefer a text edit: the diff stays small.
4. **Build by script for anything bigger than a few nodes,** and for every new
   scene. Set `owner` to the scene root on every node you add below the root,
   after `add_child`. Measured: a node without `owner` is silently left out of
   the saved file.
5. **Prove each changed scene with the scene check** below. Exit 0 is
   necessary, not enough: also grep its output.
6. **Never edit** `.godot/`, `*.import` files, binary `.res` or `.scn` files,
   or `.uid` sidecars. To change a binary resource, convert it in the editor
   (a human step in `DONE:`) or rebuild it by script as `.tres`.
7. **Commit the `.uid` sidecars.** `--import` writes `<script>.gd.uid` next to
   each new script. Commit it with the script, by path. A git-ignored scratch
   project has nothing to commit.
8. **Close the scene in the operator's editor first.** The editor rewrites an
   open scene on save. If the editor has the project open, report `BLOCKED:`
   (`godot-build-verify`, step 1).

## Change a scene by text

1. Read the whole file. Find the node by `name` and `parent`.
2. Edit the property line, or add the section. Keep the section order.
3. Run the scene check on the file.

## Change a scene by script

1. Write a `SceneTree` script that builds the tree, sets `owner` on every
   node, then calls `PackedScene.pack()` and `ResourceSaver.save()`. Put it in
   `.godot/horch-home/`, not in the project, unless the task asks to keep it.
   Do the work in `_initialize()`, not `_init()`: the autoloads do not exist
   in `_init()`. The `-s` script itself cannot name an autoload; use
   `root.get_node("<Name>")` (`godot-build-verify`, "Rules").
2. Set each script through a check that fails the build: a script that does
   not compile leaves the node with no script, and Godot goes on. Run it:
   `gd -s "$PWD/.godot/horch-home/build_level.gd"`. It must print its own
   success line and exit 0. Check the result of `pack()` and `save()`.
3. Run `gd --import` when the script added new scripts or assets, then commit
   the new `.uid` files.
4. Run the scene check on the file.

## Prove the change: the scene check

`references/worked-example.md` holds `scene_check.gd`. It loads each scene
given after `--`, instantiates it, counts the nodes and tests every signal
connection:

```bash
gd -s "$PWD/.godot/horch-home/scene_check.gd" -- res://levels/level_1.tscn > "$LOGS/scene.log" 2>&1; echo "exit=$?"
grep -E 'SCENE_CHECK|SCRIPT ERROR|^ERROR:|^WARNING:' "$LOGS/scene.log"
```

What each wrong edit does on 4.7.2:

| wrong edit | load | what shows it |
|---|---|---|
| wrong `ExtResource`/`SubResource` id, or a `sub_resource` after its use | fails | `SCENE_CHECK FAIL ... does not load`, `ERROR: Parse Error` |
| broken value syntax (`Vector2(64, 32`) | fails | the same |
| `ext_resource` path to a missing file | **loads** | `ERROR: ... referenced non-existent resource at: <path>` |
| `parent=` names a missing node | **loads** | `WARNING: Parent path './X' for node 'Y' has vanished` |
| `[connection]` names a missing method | **loads, runs, prints nothing** | only the check: `SCENE_CHECK FAIL ... connection Timer.timeout -> _on_timer_typo` |
| a misspelled property (`positon = ...`) | **loads, prints nothing** | read the value back: it stays at the default (worked example) |
| a build script in `_init()` sets a script that names an autoload | the scene **saves without that script** and the build prints its success line | `SCRIPT ERROR: Compile Error: Identifier not found`; the builder's check prints `BUILD_FAIL` and exits 1 (worked example, 1b) |

A property edit is proven when the value reads back: the worked example prints
`Spawn.position` from the instantiated scene.

Then run `godot-build-verify` (parse check, tests, smoke run) before `DONE:`.

## Review checklist

- [ ] No editor has this project open.
- [ ] Every new id is unique in its file; every reference names a defined id.
- [ ] No typed `uid://` and no typed `unique_id=`.
- [ ] Generated nodes have `owner` set to the scene root.
- [ ] No edit in `.godot/`, `.import`, `.uid`, `.res` or `.scn` files.
- [ ] Scene check exit 0, and 0 `ERROR:` or `WARNING:` lines for my scenes.
- [ ] Changed property values read back as intended.
- [ ] New `.uid` sidecars are in my commit.

## References

- `references/worked-example.md`: load before your first edit. A scene built
  by script, three text edits, an instanced scene override, the scene check
  source, the value read-back, and a UID on demand. All run on 4.7.2.
- `references/fleet-additions.md`: the 4.6 scene format change, with its
  sources.
- `godot-build-verify`: the engine lookup, the `gd` helper, the parse check
  and the test run.
- `godot-scene-organization`: how to split a game into scenes.
