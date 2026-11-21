# Assets and import pipeline

Target engine: Godot 4.7. How source files become engine resources, which import options change memory, quality and load time, and how to automate the pipeline.

## Use this shelf when

- A build is too big or uses too much video memory, or you pick a texture compression mode.
- You set up a 3D model pipeline (glTF, `.blend`, FBX) with LODs, collision and shared animations.
- You want import rules applied by code, not by hand in the Import dock.
- CI must import a fresh clone before a test run or an export.
- You load models from outside the PCK at runtime (mods, user content).

## How the engine works here

- Import is an editor job. For each source file the editor writes a `<file>.import` text file (importer name, options, UID, output paths) next to the source, and the converted resource into `res://.godot/imported/` as `<name>-<hash>.<ext>` plus an `.md5` file. A changed source checksum or a missing cache file triggers a reimport.
- Commit the `.import` files. Do not commit `.godot/`. A fresh clone has no cache, so a game run with no prior import fails to load imported assets.
- Exported builds contain only the imported form. `ResourceLoader` maps `res://a.png` to the cached file. `FileAccess` on an imported asset works in the editor and breaks after export. The reverse also holds: `ResourceLoader` cannot open a file that was not imported.
- VRAM formats depend on the target. Desktop uses S3TC (BC1 opaque, BC3 with alpha, RGTC for normal maps), or BPTC (BC7, BC6H for HDR) with **High Quality**. Mobile and web use ETC2, or ASTC with **High Quality**. The editor always imports the host format; `rendering/textures/vram_compression/import_etc2_astc` and `.../import_s3tc_bptc` add the other family. Changing them does not touch already imported textures: delete `.godot/imported/` to apply.
- A glTF file is parsed into a `GLTFState`, then `GLTFDocument` generates nodes. In the editor a second stage (`ResourceImporterScene`) applies dock options, name suffixes, LODs, UV2, the post-import script, and saves a `.scn`. Runtime loading has only stage one.
- `.blend` import calls an installed Blender to export glTF, so only what Blender's glTF exporter writes (extensions included) can arrive. FBX uses the built-in ufbx importer since 4.3; files first added before 4.3 keep FBX2glTF until you switch them.
- Resources are referenced by UID with the path as fallback. Imported files keep the UID in `.import`, scenes and resources in their header. Since 4.4 scripts and shaders get a `<file>.uid` next to them. UID files are editor data; export moves the mapping into the UID database.

## Advanced techniques

### Pick the compression mode from measured memory

| Mode | Disk | Video memory | Use |
| --- | --- | --- | --- |
| Lossless | small | full | 2D, UI, pixel art (default) |
| Lossy | very small | full (same as Lossless) | large 2D art where slight artifacts are fine |
| VRAM Compressed | small | about 4x to 6x less | 3D textures |
| Basis Universal | very small | same as VRAM Compressed | 3D where download size matters most |

An RGBA8 texture with mipmaps costs 5.33 MiB at 1024, 21.33 MiB at 2048 and 85.33 MiB at 4096 uncompressed, one quarter of that VRAM compressed. No alpha saves 25 percent, no mipmaps another 25 percent. Lossy only shrinks the file. VRAM compression also lowers sampling bandwidth, which helps integrated and mobile GPUs. Keep pixel art Lossless in 3D.

### Know what makes VRAM import fast or slow

Since 4.4 the BC formats are compressed on the GPU (Betsy, `rendering/textures/vram_compression/compress_with_gpu`). The 4.4 dev 1 post measured 8K HDR panoramas with mipmaps at 92 to 99 s before and under 0.5 s after. Since 4.6, RGB sources are converted to RGBA on the GPU, which the 4.6 release page reports as up to two times faster for 3D textures. The GPU path needs a Vulkan, Direct3D 12 or Metal device and covers BC1, BC3, BC4, BC5 and BC6 only: ETC2, ASTC and BC7 still compress on the CPU, and so does a CI runner with no GPU.

### Control Detect 3D instead of disabling it

A texture first used in a 3D material is reimported with mipmaps and VRAM compression. For art that must stay sharp, set `detect_3d/compress_to` to **Disabled** before first use, or set the mode back to Lossless afterwards. Both keep the mipmaps.

### Normal maps, HDR and channel work at import

- `compress/normal_map` set to **Enable** stores RGTC (two channels). Quality is much better at the same size. The blue channel is gone, so custom shaders must rebuild Z.
- DirectX-style normal maps need `process/normal_map_invert_y`.
- `compress/hdr_compression`: HDR with alpha is not VRAM compressed by default (stored as RGBE9995). **Always** drops alpha. HDR VRAM compression needs **High Quality** (BPTC, ASTC).
- `process/channel_remap/*` (since 4.5) reorders or inverts channels on import, so a packed map from another engine needs no edited copy.
- `process/hdr_clamp_exposure` removes sparkles in reflections from very bright panoramas.

### Size limits without touching sources

`process/size_limit` scales the longer side down on import (cubic filter). Use it to stay under the 4096 limit common on mobile and web. Hard limits apply even at 0: 16383 px for Lossy, 16384 for Basis Universal, 32768 otherwise.

### Basis Universal tuning

`compress/uastc_level` (since 4.5) raises quality and encode time. `compress/rdo_quality_loss` of 0.01 or more shrinks files at much longer encode time. Keep `rendering/textures/basis_universal/zstd_supercompression` on, or files are as large as plain VRAM textures. Float images fall back to VRAM Compressed.

### Mesh options that change frame cost

- `meshes/generate_lods` (default on) builds LODs with meshoptimizer; selection uses the viewport `mesh_lod_threshold`. Turn it off for meshes never seen from afar to cut size and import time. **Normal Merge Angle** per mesh fixes shading breaks. Since 4.6 the simplifier prunes small separate parts, so multi-part meshes keep their shape at distance; reimport old models to get it.
- `meshes/create_shadow_meshes` welds vertices for cheaper shadow passes at equal quality.
- `meshes/light_baking` set to **Static Lightmaps** generates UV2; `meshes/lightmap_texel_size` trades bake time for detail.
- `meshes/ensure_tangents`: turn off when no material uses normal or height maps.
- `meshes/force_disable_compression`: enable when normals or UVs look blocky, or a mesh spans more than a few thousand meters.

### Name suffixes as a contract with artists

`-col`, `-convcol`, `-colonly`, `-convcolonly`, `-occ`, `-occonly`, `-navmesh`, `-rigid`, `-vehicle`, `-wheel`, `-noimp` on objects; `-alpha`, `-vcol` on materials; `loop` or `cycle` at the start or end of a clip name. `-`, `$` and `_` all work, case-insensitive. `nodes/use_node_type_suffixes = false` (since 4.4) keeps only the non-node suffixes; `nodes/use_name_suffixes = false` drops all, which matches runtime loading.

### Post-import script

Set `import_script/path` to a `@tool` script. It runs after the import options.

```gdscript
@tool
extends EditorScenePostImport

func _post_import(scene: Node) -> Object:
	for node: Node in scene.find_children("*", "MeshInstance3D", true, false):
		var mesh_instance := node as MeshInstance3D
		if String(mesh_instance.name).ends_with("_noshadow"):
			mesh_instance.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	return scene
```

Return the scene. `get_source_file()` gives the path, so one script can branch per folder. For rules on every model, register an `EditorScenePostImportPlugin` (`EditorPlugin.add_scene_post_import_plugin`): it adds its own dialog options and gets `_pre_process`, `_internal_process` per node or resource, and `_post_process`.

### glTF extensions that also work at runtime

A `GLTFDocumentExtension` registered with `GLTFDocument.register_gltf_document_extension` sees the JSON: `_import_preflight`, `_parse_node_extensions`, `_generate_scene_node`, `_import_node`, `_import_post`. Use it to turn glTF extras (Blender custom properties) into node metadata or typed nodes. The same code runs in exported games, unlike suffixes and post-import scripts. `godot-assets-pipeline` shows the `append_from_file` and `generate_scene` calls. Runtime loads skip the editor stage: no generated LODs, shadow meshes or UV2. `FBXDocument` and `FBXState` (since 4.3) follow the same shape.

Since 4.4 the importer reads `KHR_animation_pointer`: glTF animations can drive any mapped property (light color, camera FOV, material albedo or UV offset), not only transforms and blend shapes. Custom mappings between glTF JSON pointers and properties go in `_import_object_model_property` and `_export_object_model_property`.

### FBX through ufbx

ufbx applies unit conversion to the meshes, not to node scale, drops the extra `RootNode`, keeps the DCC material order, trims clips to the time range in the file header and computes bone rests from the bind pose, which helps retargeting. `animation/fps` only affects curve interpolation; clips keyed at 60 fps keep all keys. Blender writes FBX at 100x scale by default: export with **Apply Scalings: FBX Units Scale**. Do not switch an existing file from FBX2glTF to ufbx casually: the node hierarchy, rest poses and surface order change, which breaks inherited scenes, `NodePath`s in scripts and surface override materials. Skeleton-only FBX files that import as plain nodes need `nodes/import_as_skeleton_bones`.

### Shared animation sets

Import a glTF as **Animation Library** to get an `AnimationLibrary` and no scene. Add a `BoneMap` with `SkeletonProfileHumanoid` on the `Skeleton3D` in Advanced Import Settings. **Overwrite Axis** unifies rests, which makes clips portable. **Fix Silhouette** is for A-pose sources. **Normalize Position Tracks** stores hip height in `motion_scale` so strides fit other heights. **Unique Node** keeps track paths equal across characters. Enable **Remove Tracks** for libraries, not for scenes with animated props. When the DCC bone rests must survive, skip **Overwrite Axis** and retarget at runtime with a `RetargetModifier3D` node (since 4.4).

### Keep hand edits across reimports

A reimport rebuilds the whole scene. Safe routes: an inherited scene (add nodes, never remove); **Save to File** per mesh, material or animation (`.res` for big meshes; saved animations keep custom tracks); `materials/extract` (**Extract Once** keeps your edits). A renamed source material breaks the link. Since 4.5 the Import dock sets material extraction for many selected files at once, so several models can share one extracted material, and extracted resources are referenced by UID as well as path. A sub-resource inside an imported file cannot be referenced directly yet (an open editor team priority); extraction is the supported route.

### UIDs in the pipeline

Commit `.uid` files like `.import` files, and move or delete them together with their script or shader when you work outside the editor. A missing UID falls back to the path with a warning, which can silently pick the wrong file after two files swapped paths. `preload("uid://...")` works in scripts (copy the UID from the FileSystem dock menu; since 4.5, Ctrl-drag a resource into the script editor). After an upgrade from 4.3 re-save all scenes and resources once (**Project > Tools > Upgrade Project Files**) to avoid scattered diffs later.

### Headless import for CI

```sh
godot --headless --path . --import
```

`--import` opens the editor, waits for all imports and quits. `--export-debug` and `--export-pack` are documented to imply it. Caching `.godot/` between CI runs saves the reimport. See `godot-headless-runs` for the full procedure.

## Niche but useful

- **Import Defaults** tab in Project Settings: project-wide defaults per importer.
- `EditorFileSystem.reimport_files` after a tool edits `.import` files; never while an import runs.
- `.gdignore` (empty file) in a folder: no import, no export, hidden from the dock.
- **Keep File** / **Skip File** import types: ship a raw file, or exclude one.
- `.dds` keeps its own mipmaps: the way to ship hand-made mip levels.
- `DPITexture` for SVG (since 4.5): re-rasterized at runtime scale, no reimport.
- `AudioStreamWAV.load_from_file` (since 4.4) loads WAV at runtime, as Ogg and MP3 already could.
- Filter script on animation import (`@+*_Loop`, then `+`/`-` track lines) strips tracks.
- Animation **Slices**: cut one `default` timeline into named clips.
- `animation/import_rest_as_RESET` adds a `RESET` clip from bone rests; a T-pose reference file's `RESET` can serve as the silhouette template when retargeting.
- `nodes/root_script` attaches a script to the imported root; since 4.5 other nodes can get scripts in Advanced Import Settings.

## Traps

- `mipmaps/limit` is not implemented; changing it does nothing.
- Blender materials default to backface culling off; Godot then draws back faces.
- Blend shapes need **Export Deformation Bones Only** in the Blender glTF exporter.
- Looping audio never emits `finished`. Ogg and MP3 have a loop start only, forward only.
- IMA ADPCM cannot seek and loops forward only. Quite OK Audio is the WAV default.
- `.blend` in a repo needs Blender on every machine, CI included.
- Outdated belief: "FBX needs FBX2glTF". Not since 4.3.

## Version notes

- 4.3: ufbx importer, `FBXDocument`, rest-as-RESET and import-as-skeleton options.
- 4.4: GPU texture compression; `.uid` files; `KHR_animation_pointer`; `RetargetModifier3D`; runtime WAV loading; a fresh clone imports without the old errors and editor restart; glTF blend shape and animation library names lose unsupported characters (`:`, `/`, `,`, `[`); `EditorSceneFormatImporter._get_import_flags` removed.
- 4.5: batch material extraction in the Import dock; channel remap; `DPITexture`; libjpeg-turbo for JPEG; fix for non-joint nodes in skeletons (old files keep old behavior until **Naming Version** is changed).
- 4.6: better LODs for multi-part meshes; faster RGB texture compression.
- 4.7: `GLTFDocument` `IMPORT_FLAG_*` flags for runtime loads; `EditorSceneFormatImporter` constants moved into `ImportFlags`; font import `hinting` default changed.

## Related skills

- `godot-assets-pipeline`: base walk-through of import settings per asset type.
- `godot-headless-runs`: import steps and cache hygiene for automated runs.
- Sibling shelves: `editor-tools-plugins`, `gpu-3d-optimization`, `animation-advanced`.

## Sources

- https://docs.godotengine.org/en/stable/tutorials/assets_pipeline/import_process.html (also `importing_translations`, `exporting_3d_scenes`, `escn_exporter` in that folder)
- https://docs.godotengine.org/en/stable/tutorials/assets_pipeline/importing_images.html
- https://docs.godotengine.org/en/stable/tutorials/assets_pipeline/importing_audio_samples.html
- https://docs.godotengine.org/en/stable/tutorials/assets_pipeline/importing_3d_scenes/available_formats.html
- https://docs.godotengine.org/en/stable/tutorials/assets_pipeline/importing_3d_scenes/import_configuration.html
- https://docs.godotengine.org/en/stable/tutorials/assets_pipeline/importing_3d_scenes/advanced_import_settings.html
- https://docs.godotengine.org/en/stable/tutorials/assets_pipeline/importing_3d_scenes/node_type_customization.html
- https://docs.godotengine.org/en/stable/tutorials/assets_pipeline/retargeting_3d_skeletons.html
- https://docs.godotengine.org/en/stable/tutorials/io/runtime_file_loading_and_saving.html
- https://docs.godotengine.org/en/stable/tutorials/editor/command_line_tutorial.html
- https://docs.godotengine.org/en/stable/tutorials/migrating/upgrading_to_godot_4.7.html (and the 4.4, 4.5, 4.6 pages)
- https://godotengine.org/releases/4.4/ , https://godotengine.org/releases/4.5/ , https://godotengine.org/releases/4.6/
- https://godotengine.org/article/introducing-the-improved-ufbx-importer-in-godot-4-3/
- https://godotengine.org/article/uid-changes-coming-to-godot-4-4/
- https://godotengine.org/article/dev-snapshot-godot-4-4-dev-1/
- https://godotengine.org/article/dev-snapshot-godot-4-5-beta-1/
- https://godotengine.org/priorities/
