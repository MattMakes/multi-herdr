# GDExtension and engine builds

Target engine: Godot 4.7. When and how to go native: GDExtension libraries, custom C++ modules, and custom engine or export template builds (size, encryption, sanitizers).

## Use this shelf when

- A profiler shows a script hot spot and you weigh native code against a better design.
- You wrap a C or C++ library, or expose a typed native API to GDScript.
- You must ship an extension for several platforms, architectures or engine versions.
- You need a custom export template: smaller, encrypted, or with a module compiled in.
- You chase a memory or threading bug that only a sanitizer build will show.

## How the engine works here

GDExtension is three things: a C interface (`gdextension_interface.h`, described by `gdextension_interface.json` since 4.6), an API dump (`extension_api.json`: every class and method the engine exposes), and a `.gdextension` file that tells the engine which shared library to load. Language bindings generate code from the two JSON files. The official C++ bindings mirror the engine's internal API, so engine source is a usable reference.

An extension class is registered into `ClassDB` at an initialization level (core, servers, scene, editor). From then on it looks like an engine class: it can be a node type, a resource type, a base class for scripts. The native object you write is an extension instance that lives next to the engine `Object`; the two have different pointers.

Every call from script goes through the `Variant` call interface. Arguments are packed into `Variant` values and unpacked on the other side. That boundary, not the native code, is usually the cost that decides whether going native pays.

A custom module is the same kind of code compiled into the engine binary. It reaches internals that the extension interface does not expose, and it needs no extra library files, but every export template must be rebuilt with it.

The build system is SCons. One command line picks `platform`, `target` (`editor`, `template_debug`, `template_release`), `arch`, and options. Output names follow `godot.<platform>.<target>[.dev][.double].<arch>`.

## Advanced techniques

### Decide in this order: algorithm, servers, then native

Go native when the profiler shows time inside script logic that cannot be removed by a better data layout, by batching work into engine calls, or by calling the servers directly (see `servers-and-low-level` and `gdscript-performance`). A native loop that calls an engine method per element still pays the boundary per element. Native code pays when the whole inner loop, with its data, moves to the other side.

### Design the API around the boundary

Expose few, coarse methods: `step_all(delta)` or `query_many(points)`, not a method per entity. Pass bulk data as `Packed*Array`. Inside C++, a packed array keeps its data on the engine side, so each `operator[]` is a call into the engine; take `ptr()` for reading or `ptrw()` for writing once, then loop on the raw pointer. A packed array that arrives from script comes wrapped in a `Variant` and may be a copy, so do not count on in-place edits reaching the caller; return the result.

### Typed surface for GDScript

Bind methods with real argument and return types, register properties with hints, and register signals. The class then behaves like an engine class in typed GDScript, in autocomplete and in the inspector. Ship class documentation too: since 4.3, `godot --doctool ../ --gdextension-docs` writes XML stubs that the build embeds, for editor and debug builds only.

### Version targeting

An extension built for 4.x loads in later minors, not earlier ones (4.0 is the one exception; it does not load in 4.1+). So build against the lowest minor that has the API you need and ship one binary set. Set `compatibility_minimum` so older editors refuse it cleanly; `compatibility_maximum` (since 4.3) blocks newer ones when you know of a break. The interface file marks each function with the version that added it, and functions are not removed or changed incompatibly.

### Match precision and custom builds

An extension must match the float precision of the engine build. For a `precision=double` engine, or any custom build (`disable_3d=yes`, extra modules), dump the API from that exact binary and build against it:

```sh
godot --dump-extension-api
scons platform=linux custom_api_file=extension_api.json
```

### Library selection by feature tags

The `[libraries]` section maps tag sets such as `linux.release.arm64` to files. Entries are tested top to bottom and the first match wins, so put specific lines first (an `editor` line before the plain `release` line). Only the matching file is exported. Relative paths keep the addon movable. `[dependencies]` lists extra shared libraries to copy at export; on macOS they belong in `Contents/Frameworks`.

### Hot reload during development

`reloadable = true` makes the editor reload the library after each rebuild (C++ bindings, since 4.2; debug builds only). With CMake, turn on `GODOTCPP_USE_HOT_RELOAD`. Treat it as a development aid: leave it off in the shipped `.gdextension`. From script, `GDExtensionManager` exposes `load_extension()`, `reload_extension()` and `get_loaded_extensions()`, plus the `extensions_reloaded` signal. Since 4.7 the Project Settings window lists the project's extensions, which helps when one is loaded that nobody remembers adding.

### Startup order and engine lifecycle

Code at the core initialization level runs before most of the engine exists. Since 4.4 the `Engine`, `OS`, `ProjectSettings` and `Time` singletons are registered early enough to use there. For everything else, since 4.5 an extension can register main loop callbacks (`startup`, `shutdown`, `frame`) instead of guessing when singletons are ready; the release notes name unreliable singleton access as the reason they exist. The `frame` callback is also the cheap way to tick a native system once per frame without a node.

### Profile native code with a tracing profiler

Since 4.6 the engine can be built with Tracy, Perfetto or Instruments support. These show script functions, engine calls and your extension's code on one timeline, which the built-in profiler cannot. Use one to prove where the time goes before and after a port to native code (see `debugging-and-profiling-tools`). On Android, Perfetto is the default tracing tool for editor and template builds since 4.7.

### Shipping per platform

- Desktop: one library per OS, architecture and debug or release. CI is the usual way.
- Web: the default Web template has no extension support. Use the `dlink` template (`dlink_enabled=yes`), tick Extensions Support in the preset, and serve with cross-origin isolation headers. See `export-and-platforms`.
- Android: build for each ABI. An extension inside a v2 Android plugin sets `android_aar_plugin = true` so the AAR carries the libraries.
- iOS: the manual documents static `.a` or `.xcframework` files for iOS plugins; for an extension follow the binding's template (the manual does not spell this out).
- The on-device editors (Android, XR) run extensions only if Android binaries exist.

### Extension, module or fork

| Need | Choose |
|---|---|
| Game logic, a wrapped library, an addon to share | Extension |
| Access to internals the interface lacks | Module |
| No loose library files next to the game | Module |
| A change inside core, rendering or physics code | Fork (carry the patch) |

A module lives in `modules/<name>` with `SCsub`, `config.py` and `register_types.*`, or outside the tree with `custom_modules=<dirs>`. A custom module with the same folder name as a built-in one replaces it. `preregister_<name>_types()` runs before all other type registration when a module must hook in early. Vendor runtime modules package custom templates behind an `EditorExportPlugin` that overrides the preset's custom template.

### Smaller templates with a build profile

The Engine Compilation Configuration Editor (Project > Tools) detects the classes a project uses and saves a `.gdbuild` profile. Nothing shrinks until you compile templates with it:

```sh
scons platform=web target=template_release build_profile=game.gdbuild optimize=size lto=full
```

Since 4.5 detection also sets build options (not only classes) and counts classes used by the project's extensions when the binding reports them. It still misses classes used only in generated scripts or `Expression` strings, so test the result. Size levers from the docs, largest first:

| Lever | Note |
|---|---|
| Strip symbols / `debug_symbols=no` | 5x to 10x smaller than an unstripped build |
| `lto=full` | Needs 8 GB of free RAM to link |
| `optimize=size`, `size_extra` (since 4.5) | Slower code; Web already uses `size` |
| `module_text_server_adv_enabled=no` with `module_text_server_fb_enabled=yes` | Loses RTL and complex scripts; faster text |
| `disable_3d=yes` | About 15% smaller; templates only |
| `disable_advanced_gui=yes` | Removes `RichTextLabel`, `Tree`, `TextEdit`, `PopupMenu`, more |
| `module_jolt_enabled=no` or `module_godot_physics_3d_enabled=no` | Drop the unused 3D physics engine |

Put long option lists in `custom.py` at the source root.

### Pack encryption needs your own templates

Set `SCRIPT_AES256_ENCRYPTION_KEY` (64 hex digits from `openssl rand -hex 32`) in the shell before compiling templates, give the preset the same key, and add include filters. An MD5 mismatch error at startup means the template was built without the key.

### Sanitizer builds

`use_asan=yes` finds buffer overruns and use-after-free, `use_lsan=yes` leaks, `use_msan=yes` uninitialized reads, `use_tsan=yes` data races (about 10x slower and 8x the memory). Address, memory and thread sanitizers cannot be combined in one binary. Run the game or the extension's test project on a sanitizer build before blaming script.

## Niche but useful

- `production=yes` is `use_static_cpp=yes debug_symbols=no lto=auto`: portable Linux binaries and release-grade output in one flag.
- `dev_build=yes` (`-O0`, asserts, `.dev` suffix) is for engine debugging; `dev_mode=yes` is a different alias (warnings as errors plus unit tests).
- `javascript_eval=no` builds a Web template without `JavaScriptBridge`.
- `wasm_simd=no` builds a Web template for browsers without WebAssembly SIMD; official templates require SIMD since 4.5.
- LibGodot (since 4.6): the engine built as a library, driven through `GodotInstance` by a host application that owns startup and the main loop. Linux, Windows, macOS.
- Since 4.6 the API marks object parameters and return values that must not be null. Bindings for strict languages turn that into compile-time checks.
- `GDExtensionManager.load_extension_from_function()` (since 4.6) registers an extension from an init function, without a library file on disk.
- `threads=no` builds the single-thread Web template.
- Android templates: add `generate_android_binaries=yes` to the last architecture built.
- `[icons]` in `.gdextension` gives extension nodes a 16x16 SVG editor icon.
- MinGW builds run the GDScript VM faster than MSVC builds, per the Windows compile page.

## Traps

- Engine containers are not thread-safe; an extension that starts threads must lock or keep data thread-local, and must not touch the scene tree off the main thread (see `threads-and-concurrency`).
- C++ STL types work inside your code but cannot cross the binding.
- A module changes the engine: forgetting to rebuild one export template gives a runtime error only on that platform.
- Without `module_text_server_fb_enabled=yes` a build with the advanced text server disabled shows no text at all.
- CMake `Debug` (compiler symbols) is not `template_debug` (engine debug features).
- Outdated belief: "an extension must be rebuilt for every minor". Forward compatibility holds since 4.1.

## Version notes

- 4.4: core singletons usable at the core initialization level.
- 4.5: main loop callbacks; `optimize=size_extra`; build profiles detect build options and extension classes; the API dump records float precision. Up to 4.5 the C header shipped as a file with the engine.
- 4.6: the interface is defined in JSON (`--dump-gdextension-interface-json`) and the C header is generated from it (`--dump-gdextension-interface`); required parameters; LibGodot; tracing profiler builds.
- 4.7: extensions listed in Project Settings. Other bindings the manual lists, all community made and not all production ready: D, Go, Kotlin/JVM, Nim, Rust, Swift, Odin.

## Related skills

- `godot-gdextension` - project setup, class binding and entry point step by step.
- `godot-language-choice` - GDScript, C# or native per subsystem.
- `godot-perf-measure` - prove the hot spot before writing native code.
- `godot-export-pipeline` - custom template paths in presets and CI.
- `godot-addon-development` - packaging an extension as an addon.

## Sources

- https://godotengine.org/releases/4.5/
- https://godotengine.org/releases/4.6/
- https://godotengine.org/releases/4.7/
- https://godotengine.org/article/dev-snapshot-godot-4-4-dev-6/
- https://godotengine.org/article/dev-snapshot-godot-4-6-dev-2/
- https://godotengine.org/article/dev-snapshot-godot-4-5-dev-1/
- https://godotengine.org/article/upcoming-serious-web-performance-boost/
- https://docs.godotengine.org/en/stable/engine_details/engine_api/gdextension/what_is_gdextension.html
- https://docs.godotengine.org/en/stable/engine_details/engine_api/gdextension/gdextension_file.html
- https://docs.godotengine.org/en/stable/engine_details/engine_api/gdextension/gdextension_interface_json_file.html
- https://docs.godotengine.org/en/stable/tutorials/scripting/cpp/about_godot_cpp.html
- https://docs.godotengine.org/en/stable/tutorials/scripting/cpp/core_types.html
- https://docs.godotengine.org/en/stable/tutorials/scripting/cpp/gdextension_docs_system.html
- https://docs.godotengine.org/en/stable/tutorials/scripting/cpp/build_system/scons.html
- https://docs.godotengine.org/en/stable/engine_details/engine_api/custom_modules_in_cpp.html
- https://docs.godotengine.org/en/stable/engine_details/development/compiling/introduction_to_the_buildsystem.html
- https://docs.godotengine.org/en/stable/engine_details/development/compiling/optimizing_for_size.html
- https://docs.godotengine.org/en/stable/engine_details/development/compiling/compiling_with_script_encryption_key.html
- https://docs.godotengine.org/en/stable/engine_details/development/debugging/using_sanitizers.html
- https://docs.godotengine.org/en/stable/tutorials/editor/using_engine_compilation_configuration_editor.html
