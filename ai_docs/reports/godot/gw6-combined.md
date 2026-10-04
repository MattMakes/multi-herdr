# GW6 godot-combined: presentation and diagnosis

Unit GW6 (worker opus-79). Seven combined skills: the GodotPrompter v1.14.0
base, renamed with `scripts/godot/rename.py`, plus own-text references written
after reading the gd-agentic-skills counterparts (operator decision 1: no
gd-agentic text or code copied).

| Skill | GodotPrompter base | gd-agentic consulted | New references |
|---|---|---|---|
| `godot-ui` | godot-ui | godot-ui-theming, godot-ui-rich-text, godot-ui-containers | 3 |
| `godot-tween-animation` | tween-animation | godot-tweening | 1 |
| `godot-shader-basics` | shader-basics | godot-shaders-basics | 2 |
| `godot-particles-vfx` | particles-vfx | godot-particles | 2 |
| `godot-audio-system` | audio-system | godot-audio-systems | 3 |
| `godot-debugging` | godot-debugging | godot-debugging-profiling (debugging half) | 3 |
| `godot-optimization` | godot-optimization | godot-performance-optimization, godot-debugging-profiling (profiling half) | 2 |

SKILL.md edits per skill: the `description:` is extended (all under 1024
bytes, none holds `": "`), and a "Fleet additions" list at the end names
each new reference. The upstream body is otherwise unchanged except for the
API fixes listed below.

## Method and evidence

- Engine: Godot 4.7.2.stable.official.ed1daf0bf, headless only. Own
  `--doctool` dump and scratch project in `.worktrees/_scratch/godot-opus-79/`.
- Every API name in own text was looked up in the dump before writing.
- Behavior claims were run headless where possible (section "Measured").
- Checks on the final tree, all 7 skill directories:
  - `scripts/godot/api_check.py`: 0 unknown names (after the 2 fixes below).
  - `scripts/godot/gdscript_blocks_check.py`: every own block parses as a
    whole script (67 own blocks across 16 files, 0 fail). 60 upstream blocks
    fail as fragments (report only, see "Upstream findings").
- Shader code (`glsl` blocks) cannot be compiled headless (the dummy
  renderer does not compile shaders). The shader blocks were written against
  the 4.7 shading-language reference and reviewed by hand; they are the one
  part not machine-checked.

## Measured on 4.7.2 (headless)

- `RichTextLabel`: replacing `[` with `[lb]` shows the tag text literally;
  `add_text()` never parses BBCode; a `RichTextEffect` with `var bbcode :=
  "bob"` installed with `install_effect()` is recognized (`get_parsed_text()`
  drops the tag), an unregistered tag stays as text.
- `Tween.new()` prints "Tween can't be created directly" and is invalid; an
  infinite `set_loops()` tween whose only step takes 0 s prints "Infinite loop
  detected"; `tween_subtween()` with a `Curve.sample_baked` custom interpolator
  runs to the end value.
- `GPUParticles3D.restart()` sets `emitting` to true; `finished` fires on a
  one-shot and leaves `emitting` false.
- Orphans: one `Node.new()` never added gives
  `OBJECT_ORPHAN_NODE_COUNT` = 1 and one id from `Node.get_orphan_node_ids()`.
- A custom `Logger` receives `push_error()` with 1 `ScriptBacktrace`.
- `print()` inside `Logger._log_message()` does not recurse: the engine prints
  "While attempting to print an error, another error was printed" and does not
  pass the inner message to loggers.
- `push_error()` in a headless `--script` run exits 0; a runtime script error
  in a scene run with `--quit-after` exits 0. The Logger-based smoke runner in
  `headless-and-in-game-diagnostics.md` exits 1 on the same scene.
- `print_debug()` prints `At: res://...:3:_init()` on a debug run.
- `MultiMesh.use_colors` set while `instance_count` is 4 fails with "Instance
  count must be 0 to toggle whether colors are used".
- The `WorkerThreadPool.add_group_task()` height-map block gives the same
  values as a sequential run (16,384 samples).
- Doctool defaults used in `spatial-acoustics.md`:
  `AudioStreamPlayer3D.attenuation_model` = 0 (`ATTENUATION_INVERSE_DISTANCE`),
  `max_distance` = 0, `unit_size` = 10, `area_mask` = 0,
  `doppler_tracking` = 0 (disabled).
- `godot --help` on 4.7.2: `--disable-vsync`, `--max-fps`, `--fixed-fps`,
  `--gpu-profile`, `--print-fps` are available in release builds;
  `--benchmark` and `--benchmark-file` are editor-only.

## Per skill: gap list

### godot-ui

Added:
- `theme-variations-and-runtime-styling.md`: type variations
  (`Theme.set_type_variation`, `theme_type_variation`), palette as a custom
  theme type, duplicate-before-change StyleBoxes, bulk overrides,
  theme-aware `_draw()` with `NOTIFICATION_THEME_CHANGED`, runtime root theme
  swap, `ThemeDB.get_project_theme()`, rules (no `set()` on theme items,
  focus style, `expand_margin` vs hit area).
- `rich-text-advanced.md`: escaping player text, push API, link routing and
  hover signals, custom `RichTextEffect` tags, reveal timing, log feeds
  (`scroll_following`, `remove_paragraph`, `threaded`), MSDF note.
- `container-layout-advanced.md`: stretch ratios, flow and aspect containers,
  width-driven grid columns, custom container (`NOTIFICATION_SORT_CHILDREN`,
  `fit_child_in_rect`), 3D preview in `SubViewportContainer`, scroll timing,
  virtual list, mouse filters, nesting depth.

Dropped:
- gd-agentic "default size flag is SHRINK_BEGIN": wrong; the default is
  `SIZE_FILL` (stated correctly in the reference).
- 4.7 `RichTextLabel.add_image()` units and `offset_transform_*`: already in
  the GodotPrompter text.
- Breakpoint-based responsive layouts, DPI scaling and RTL mirroring: covered
  by `godot-responsive-ui` and `godot-localization`.
- Typewriter with `[pause]` tags: dialogue concern, covered by
  `godot-dialogue-system`.
- Seasonal theme overlays (gd-agentic `godot-theme-easter` is excluded by the
  wave report); asset compression audit (belongs to export and assets skills).

### godot-tween-animation

Added (`tween-composition.md`): `tween_subtween()` timelines (4.4+),
`PropertyTweener.set_custom_interpolator()` with `Curve.sample_baked`,
`Tween.interpolate_value()`, `PathFollow2D.progress_ratio` paths,
`TWEEN_PROCESS_PHYSICS` with `reset_physics_interpolation()`, feel presets as
a `Resource`, the API's hard limits (all four limits measured above or in the
dump).

Dropped: kill-before-recreate, `set_ignore_time_scale`, staggered entry,
looping hover, relative recoil, text counter with `tween_method`: already in
GodotPrompter. Camera follow guidance kept to one sentence (camera skill).
Unproved claim not carried: "an unbound tree tween keeps running after its
node is freed" (rewritten as what `bind_node()` does: pause and process mode
follow the node, and the tween dies with it).

### godot-shader-basics

Added:
- `per-instance-and-global-uniforms.md`: `instance uniform` with
  `set_instance_shader_parameter()` (3D and `CanvasItem`), limits (no
  samplers, 16 per shader), `global uniform` with Project Settings and
  `RenderingServer.global_shader_parameter_set()`, `sampler2DArray` skins with
  `Texture2DArray.create_from_images()`, varyings, writing rules.
- `cutout-depth-and-world-space.md`: alpha scissor and hash for foliage and
  3D dissolve, depth-to-world reconstruction (Forward+/Mobile vs
  Compatibility), reversed-Z full-screen quad, triplanar, `shader_type fog`,
  first-use stutter and warm-up.

Dropped: first-shader tutorials and the built-in glossary (in GodotPrompter),
2D dissolve, outline, wave, toon, vignette (in GodotPrompter recipes),
`VisualShaderNodeCustom` and compute particle simulation (out of scope for
basics; compute is a GDExtension/RenderingDevice topic).

### godot-particles-vfx

Added:
- `particle-lifecycle-and-pooling.md`: one-shot lifecycle (measured), burst
  pool, `local_coords` for trails and teleports, no CPU collision events,
  `capture_aabb()` / `capture_rect()`, visibility-range LOD, `amount_ratio`
  versus `amount`, MultiMesh hand-off, preprocess cost.
- `particle-shaders-and-scale.md`: `shader_type particles` from a converted
  material, a full orbit shader, `emit_subparticle()` on collision, attractor
  and collider `cull_mask` against `layers`, camera-following
  `GPUParticlesCollisionHeightField3D`.

Dropped:
- gd-agentic "use CPUParticles2D with fract_delta because GPUParticles2D
  stutters on physics bodies": stated for 4.3; not proved on 4.7.2 (headless
  cannot show it). Not carried.
- `GPUParticles3D.emit_particle()` from script: exists in the dump, but its
  requirements could not be checked headless. Not carried.
- `amount_ratio` "does not save GPU memory": carried, reworded as what the
  property does (emits fewer without a restart; the buffer stays sized by
  `amount`). GodotPrompter's quality slider advice with `amount_ratio` stays.

### godot-audio-system

Added:
- `mixing-voices-and-ducking.md`: bus roles, sidechain ducking with
  `AudioEffectCompressor.sidechain`, priority voice pool with stealing,
  per-sound caps, `max_polyphony`, `AudioStreamRandomizer`,
  `AudioStreamPolyphonic`.
- `spatial-acoustics.md`: 3D defaults from the dump, raycast occlusion with
  `attenuation_filter_cutoff_hz`, Area3D reverb and bus-override zones and the
  4.7 `area_mask` default, Doppler on source and camera, surface footsteps,
  distance culling.
- `audio-clock-and-analysis.md`: latency-corrected heard time, beat signal,
  subtitle cues, beat-aligned switch, `AudioStreamGenerator`,
  `AudioEffectSpectrumAnalyzer`.

Dropped:
- gd-agentic "3D players default to no attenuation": wrong; the default is
  `ATTENUATION_INVERSE_DISTANCE` (dump).
- gd-agentic "`AudioStreamPlayer.area_mask` defaults to 0": `area_mask` exists
  only on the 2D and 3D players (dump); GodotPrompter already says this.
- "Offset a 3D sound 0.1 units from the listener to avoid panning jitter":
  not provable headless. Not carried.
- Crossfade music manager, settings sliders, `linear_to_db`: in GodotPrompter.

### godot-debugging

Added:
- `leaks-and-orphans.md`: orphan monitor and functions (measured), a
  repeat-and-compare leak check, usual causes, backtrace variables holding
  references.
- `logging-and-crash-context.md`: custom `Logger` (4.5+, measured), backtraces
  on demand, `assert()` versus release-safe checks, debug-only output, thread
  safety checks.
- `headless-and-in-game-diagnostics.md`: failing a headless run on errors
  (measured), custom monitors, in-game overlay, 2D/3D debug drawing,
  `EditorDebuggerPlugin` tab, `Engine.is_editor_hint()` guards.

Dropped:
- gd-agentic "printing inside a Logger causes infinite recursion and a crash":
  wrong on 4.7.2 (measured). The reference states what happens instead.
- gd-agentic "the orphan monitor returns 0 in release": not measured (no
  release template on this host). The reference states that the orphan tools
  are for debug builds, which matches the measured debug behavior; the
  release value is not claimed.
- Remote debug console over the network: a feature, not debugging advice.

### godot-optimization

Added:
- `measuring-performance.md`: debug profiling versus release timing, frame
  caps, CPU versus GPU (Visual Profiler, `--gpu-profile`, resolution test),
  `Viewport.get_render_info()`, frame-time percentiles, repeatable benchmark
  launch with user arguments, micro-benchmark rules.
- `scaling-techniques.md`: MultiMesh (with measured ordering rule),
  `RenderingServer` canvas items, direct ray queries, time-budgeted job
  queue, staggered agents, `WorkerThreadPool` group tasks (measured).

Dropped: object pooling and visibility notifiers (in GodotPrompter),
Compatibility-renderer shader warm-up (carried once, in
`godot-shader-basics` `cutout-depth-and-world-space.md`), VRAM compression
table (assets and export skills), navigation internals (navigation skill).

## Upstream findings

### API fixes in place (orchestrator decision; recorded in provenance)

`godot-optimization/references/memory-management.md`:
- `Performance.MEMORY_DYNAMIC` does not exist in 4.7.2 (dump has
  `MEMORY_STATIC`, `MEMORY_STATIC_MAX`, `MEMORY_MESSAGE_BUFFER_MAX`). Replaced
  with `Performance.MEMORY_STATIC_MAX` ("Peak RAM") in the GDScript block and
  `Performance.Monitor.MemoryStaticMax` in the C# block.
- `OS.gc()` does not exist. The block now shows dropping the last reference
  (reference counting frees the resource and removes it from the cache).

No fact errors proved in the other upstream text. Claims I could not prove:
- `godot-debugging/SKILL.md` line 35: "`print_debug()` ... stripped from
  release exports". A debug run prints it with the frame; the release
  behavior was not tested (no release template). Left as it is.

### Upstream blocks that do not parse alone (report only)

`gdscript_blocks_check.py` on the 7 directories: 60 upstream blocks fail,
all fragments (undeclared `mat`/`tween`, `$` outside a node, `preload()` of
project files that do not exist in the scratch project, two `_ready` in one
block). Per skill: audio 9, debugging 12, optimization 16, particles 12,
shader 0, tween 5, ui 6. Per the orchestrator decision, upstream blocks were
not annotated or rewritten. Full list:
`.worktrees/_scratch/godot-opus-79/upstream_blocks.txt` (git-ignored).

## Not done

- Shader blocks are not compiler-checked (headless limit, see above).
- `emit_particle()`, CPUParticles2D interpolation and release-build orphan
  behavior are not covered, for the reasons above.
