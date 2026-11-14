---
name: godot-library
description: >-
  The reference library of advanced and less-common Godot 4.7 techniques, one shelf per area,
  built from the whole official manual, the class reference and the engine team's release notes
  and articles. Use it
  before you design or review any Godot system that must be fast, large, repeatable or easy to
  tune: engine internals, GDScript and C# speed, threads, servers and RIDs, frame budgets,
  rendering, GI, 2D, shaders, particles, physics, navigation, animation, UI, input, audio,
  networking, saves, import, editor tools, export, XR, world streaming, deterministic
  simulation and design iteration tools. Also use it when your engine knowledge may be older
  than 4.7: the version ledger lists what changed in 4.4 to 4.7 and which old beliefs are now
  false. Each shelf says when a technique pays, what it costs and where it traps you.
---

# Godot Library

Target engine: **Godot 4.7** (4.7.2 is the latest stable release).

This skill is a library, not a procedure. The other `godot-*` skills tell you how to do a job
step by step. A shelf tells you what the engine really does in one area, which advanced or
uncommon techniques exist there, when each one pays off for the game or for the team, what it
costs and how it fails. Read a shelf before you design, so that the design starts from the
full set of options and not from the first one you remember.

## How to use it

1. Read [version-ledger](references/version-ledger.md) once per task. Models and blog posts
   carry advice from Godot 3 and early Godot 4 that 4.7 made false.
2. Find the shelf for your problem in the catalog below. Your seat's briefing names the shelves
   for your role; the catalog holds all of them, and you may read any shelf.
3. Read the whole shelf. Start with "Use this shelf when" and "How the engine works here";
   the techniques make sense only with that model.
4. Pick a technique by its "when it pays" line, not by how clever it is. Most games need the
   simple form. A technique that removes a cost you did not measure adds only risk.
5. Check each API name on the project's engine (`godot --doctool`, see `godot-build-verify`)
   and measure the gain (`godot-perf-measure`) before you report it.
6. Name the shelf and the technique in your report, so that a reviewer can check your source.

## Rules of the library

- **A shelf proposes; the project decides.** The project context file, the pinned engine
  version and the measured numbers of this project win over any shelf.
- **Version honesty.** A shelf targets 4.7 and marks what 4.4, 4.5, 4.6 or 4.7 added. On an
  older project, a marked feature may not exist; say so and choose another way.
- **Sourced claims.** Each shelf ends with its sources: manual pages, release pages and
  engine-team articles. A version tag ("since 4.5") is proven by the release page or by the
  class reference of the older versions. A claim marked "reported" has one source that the
  manual does not confirm: measure before you depend on it.
- **Advanced does not mean default.** Each technique names its cost. If the cost line
  describes your project, do not use the technique.
- **No shelf replaces a procedure skill.** Each shelf ends with "Related skills"; use those
  for the steps and the checks.

## Catalog

### Read first

| Shelf | Holds |
|---|---|
| [version-ledger](references/version-ledger.md) | what changed in 4.4 to 4.7, and beliefs that are now false |

### Engine and code

| Shelf | Holds |
|---|---|
| [engine-internals](references/engine-internals.md) | objects, servers, frame order, deferred calls, signals inside |
| [scene-architecture](references/scene-architecture.md) | scene and project structure at scale, autoload and node alternatives |
| [data-and-memory](references/data-and-memory.md) | containers, Resources, reference cycles, loading, leaks |
| [gdscript-performance](references/gdscript-performance.md) | what the VM makes fast, typing, exports, annotations |
| [csharp-dotnet](references/csharp-dotnet.md) | C# interop cost, collections, GC, platform limits |
| [threads-and-concurrency](references/threads-and-concurrency.md) | worker pool, thread groups, thread-safe APIs, ownership |
| [gdextension-and-engine-builds](references/gdextension-and-engine-builds.md) | native code, call-boundary cost, custom and small builds |

### Performance

| Shelf | Holds |
|---|---|
| [cpu-and-frame-budget](references/cpu-and-frame-budget.md) | frame budgets, update tiers, time-slicing, node cost |
| [servers-and-low-level](references/servers-and-low-level.md) | RIDs without nodes, MultiMesh, thousands of entities |
| [debugging-and-profiling-tools](references/debugging-and-profiling-tools.md) | profilers, custom monitors, debug flags, loggers |
| [worlds-and-streaming](references/worlds-and-streaming.md) | threaded loading, chunks, large coordinates, unloading |

### Rendering and look

| Shelf | Holds |
|---|---|
| [rendering-pipeline](references/rendering-pipeline.md) | renderers, drivers, pipeline compiles, compositor, scaling |
| [gpu-3d-optimization](references/gpu-3d-optimization.md) | culling, LOD, visibility ranges, shadows, VRAM |
| [lighting-and-gi](references/lighting-and-gi.md) | GI methods, lightmaps, probes, stylized light |
| [2d-rendering](references/2d-rendering.md) | batching, tile maps at scale, 2D lights, SDF, custom draw |
| [shaders-advanced](references/shaders-advanced.md) | uniform scopes, screen reads, compute, stencil, shader cost |
| [particles-and-vfx](references/particles-and-vfx.md) | GPU and CPU particles, sub-emitters, trails, warm-up |

### Simulation and play

| Shelf | Holds |
|---|---|
| [physics-advanced](references/physics-advanced.md) | Jolt, interpolation, direct space queries, shape cost |
| [navigation-advanced](references/navigation-advanced.md) | runtime bakes, chunks, avoidance, path queries, A* options |
| [math-and-procedural](references/math-and-procedural.md) | transforms, smoothing, seeded random, noise, runtime meshes |
| [determinism-and-simulation](references/determinism-and-simulation.md) | fixed steps, seeded streams, replays, sim and view split |
| [animation-advanced](references/animation-advanced.md) | mixer internals, skeleton modifiers, root motion, tweens |
| [input-advanced](references/input-advanced.md) | event order, buffering, low latency, rebinding, controllers |
| [audio-advanced](references/audio-advanced.md) | bus design, adaptive music streams, audio sync, polyphony |
| [networking-advanced](references/networking-advanced.md) | replication, channels, prediction, rollback, server builds |

### Interface and text

| Shelf | Holds |
|---|---|
| [ui-advanced](references/ui-advanced.md) | layout cost, themes at scale, fonts, focus, long lists |
| [i18n-and-accessibility](references/i18n-and-accessibility.md) | translation domains, plurals, RTL, screen readers |

### Pipeline and shipping

| Shelf | Holds |
|---|---|
| [assets-import](references/assets-import.md) | texture compression, mesh and animation import, import scripts |
| [io-and-saves](references/io-and-saves.md) | file formats, save safety, versioning, atomic writes |
| [editor-tools-plugins](references/editor-tools-plugins.md) | @tool rules, undo/redo, inspector, import and export plugins |
| [design-iteration-toolkit](references/design-iteration-toolkit.md) | live tuning, designer data, debug overlays, movie capture |
| [export-and-platforms](references/export-and-platforms.md) | presets, feature tags, PCK patches, Web and mobile limits |
| [xr](references/xr.md) | OpenXR, composition layers, foveation, hand tracking, comfort |
## Find a shelf by problem

| The problem | Start with | Then |
|---|---|---|
| The frame is slow and you do not know why | debugging-and-profiling-tools | cpu-and-frame-budget, gpu-3d-optimization |
| Thousands of similar things on screen | servers-and-low-level | 2d-rendering, particles-and-vfx |
| A stall the first time an effect shows | rendering-pipeline | shaders-advanced, worlds-and-streaming |
| A hitch on level load or on spawn | worlds-and-streaming | data-and-memory, threads-and-concurrency |
| Memory grows during play | data-and-memory | debugging-and-profiling-tools |
| Motion jitters or looks uneven | physics-advanced | rendering-pipeline, input-advanced |
| The same run gives different results | determinism-and-simulation | math-and-procedural |
| Online play desyncs or rubber-bands | networking-advanced | determinism-and-simulation |
| Many agents must find paths | navigation-advanced | threads-and-concurrency |
| A designer must tune values without a programmer | design-iteration-toolkit | editor-tools-plugins |
| A script is the hot spot | gdscript-performance | servers-and-low-level, gdextension-and-engine-builds |
| The build is too large or loads slowly | export-and-platforms | assets-import, gdextension-and-engine-builds |
| The game must run on Web or a phone | export-and-platforms | rendering-pipeline, threads-and-concurrency |
| The UI is slow, or breaks in another language | ui-advanced | i18n-and-accessibility |
| Old code or old advice may not fit 4.7 | version-ledger | the shelf of that area |

## What a shelf contains

Every shelf has the same parts, in this order:

- **Use this shelf when**: the situations it serves.
- **How the engine works here**: the internal model that explains the techniques.
- **Advanced techniques**: each with what it is, when it pays, how to do it with exact API
  names, and its cost and trap.
- **Niche but useful**: uncommon features with one reason each to use them.
- **Traps**: common mistakes and outdated beliefs.
- **Version notes**: what 4.4 to 4.7 changed in this area.
- **Related skills**: the procedure skills for the steps.
- **Sources**: the manual pages, release pages and articles behind the shelf.

The 4 cross-area shelves (version-ledger, design-iteration-toolkit,
determinism-and-simulation, worlds-and-streaming) adapt the middle parts to their subject.

## Keeping the library true

The library follows the latest stable Godot. When a new stable release ships, update
version-ledger first, then each shelf whose area the release notes name. The gate parses every
GDScript block and checks every engine name in a code block against the pinned engine version,
so a renamed API fails the build and not a worker's task.
