# Export and platforms

Target engine: Godot 4.7. How an export is assembled, how packs, patches and feature tags really behave, and what each target platform demands that the others do not.

## Use this shelf when

- You design build variants (demo, server, store editions) or a CI export.
- You plan patches, DLC or mods delivered as extra packs.
- A game must run in a browser, on a phone, or on a signed desktop build.
- You need a platform API (JavaScript, Java/Kotlin, permissions) from script.
- Download size or first-run load time is a goal.

## How the engine works here

An export is a template binary (the engine without the editor) plus a pack. The pack holds `project.binary`, imported files from `.godot/imported/`, scenes converted to binary (`editor/export/convert_text_resources_to_binary`) and `.remap` files that point the original path at the converted file. Files and folders whose name starts with a dot are never packed. Non-resource files (`.json`, `.csv`, `.txt`) are packed only when an include filter names them.

Packs form a virtual `res://`. A pack loaded later wins for any path it shares with an earlier one, unless `replace_files` is `false`. That rule gives patches, DLC and mods. PCK is uncompressed and fast to read; ZIP is smaller, slower, and editable with OS tools.

Feature tags are plain strings. `OS.has_feature()` reads them, project settings can be suffixed with them (`setting.tag`), and `.gdextension` files use them to pick a library. Built-in tags are fixed at build time, except texture compression tags, `web_<os>` tags and `movie`.

Two files hold the preset data: `export_presets.cfg` (commit it) and `.godot/export_credentials.cfg` (passwords, keystore data, the encryption key; never commit it). In CI, supply secrets through the documented environment variables (`GODOT_SCRIPT_ENCRYPTION_KEY` and the per-platform keystore and signing variables), which override the preset during export.

## Advanced techniques

### Feature-tag overrides that are read correctly

`ProjectSettings.get_setting()` ignores tag overrides. Use `get_setting_with_override()` for your own settings, or an override such as `game/quality.mobile` is silently unused. `get_setting_with_override_and_custom_features()` evaluates a setting for tags you pass.

```gdscript
var quality: int = int(ProjectSettings.get_setting_with_override(&"game/quality/level"))
```

Traps: custom tags from a preset exist only in the exported build (and one-click deploy), not when you press Play in the editor, and not inside `EditorExportPlugin` scripts. `OS.has_feature("mobile")` is `false` in a Web build on a phone; test `web_android` or `web_ios` there.

### Patch packs and delta encoding

A patch pack (since 4.4) holds only files that differ from the base packs listed in the preset's Patching tab, and can also record file removals. CLI: `--export-patch <preset> <path>` with `--patches <list>`. List earlier patches as base packs so each new patch stays small.

Delta encoding (since 4.6) stores only the changed bytes of a file. It pays most for large assets with small edits, such as a translation file that gains a language. It adds load time for each patched resource, and that cost stacks with every later patch of the same file. Rules from the docs:

- Keep the compression level at 19 or below. Negative levels trade size for decode speed.
- Turn compression off for resources you expect to patch (import options, the Compress Binary Resources editor setting, GDScript export mode), then delete `.godot/imported/`.
- The base packs listed must be the exact files the player has, in load order. A re-export of an old version can differ by a few bytes and break the delta.

### Load packs before anything is preloaded

`preload()` resolves when a script loads, so a pack loaded from a menu cannot replace scenes that the menu already pulled in. Load packs in the `_init()` of an autoload.

```gdscript
extends Node

func _init() -> void:
	var dir: String = OS.get_executable_path().get_base_dir()
	var ok: bool = ProjectSettings.load_resource_pack(dir.path_join("patch1.pck"))
	if not ok:
		push_warning("patch1.pck was not loaded")
```

`DirAccess` does not show files added this way. For mods, pass `false` as the second argument, or keep mods in their own folder, so a mod cannot replace core files by path.

### Treat packs as code

A pack can carry scripts, so loading one runs someone's code. The docs name three risks: a hostile mod, a pack swapped on disk, a compromised patch server. Sign your own patches and verify them with `Crypto` and a public key in the main pack. There is no DRM for paid packs. The team's statement on the GodLoader malware makes the same point: a pack is a program run by the runtime next to it, and trust comes from its source.

### Pack encryption, and what it does not do

Encryption (AES-256) needs export templates you compiled with the same key (`SCRIPT_AES256_ENCRYPTION_KEY` at build time). Official templates cannot read encrypted packs. Nothing is encrypted until you add include filters in the Encryption tab. The key sits inside the binary, so this stops casual ripping only. Build steps are in `gdextension-and-engine-builds`.

### Dedicated server export

The "Export as dedicated server" resource mode replaces textures, materials and meshes with placeholders that keep only size data, adds the `dedicated_server` tag and forces headless mode. Per file you choose Strip Visuals, Keep (the server reads pixels, such as a collision map) or Remove. Remove breaks any scene that still references the file; use `load()` from script for those. Audio has no placeholder yet. Enable `application/run/flush_stdout_on_print` when a service manager collects the log.

### Web: choose threads on purpose

- No-threads build (default since 4.3): runs on any host, including portals and iframes. It also removed long-standing playback problems on macOS and iOS browsers.
- Threads build: needs a secure context plus `Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy: require-corp`. Without them the game does not start. Why it matters for design: an isolated page cannot call other sites, so ads and most third-party web APIs are gone, and many portals will not let you set headers at all. The PWA option installs a service worker that injects the headers when you cannot. GDExtension on Web has the same header requirement.
- Audio defaults to Sample playback on Web (`audio/general/default_playback_type.web`). Reason: without a mixing thread, stream mixing is tied to the frame rate, so a slow frame starves the buffer and audio crackles. A sample is handed to the browser's own audio engine and keeps playing through a frame drop. Cost: no `AudioEffect`, no reverb or doppler, no procedural audio, and only static streams (WAV, Ogg Vorbis, MP3) qualify. A stream is registered on first play; call `AudioServer.register_stream_as_sample()` while a level loads to avoid that hitch on weak devices. Switch a player to Stream when you need the missing features.
- Only the Compatibility renderer runs on Web (WebGL 2). C# projects cannot export to Web.

### Web: size and startup

Serve `.wasm` as `application/wasm`. Compress `.wasm` and `.pck` on the server; gzip takes the wasm to about a quarter of its size and Brotli does better. itch.io does not compress on the fly. For scale, the 4.3 release wasm was about 40 MB raw and 5 MB with Brotli (Web export report). A custom template without unused features shrinks it further. A custom HTML shell can preload files and pass arguments through the `Engine` JavaScript class. The whole pack is still downloaded before the game starts, so pack size is start time; split late content into packs you fetch later.

Since 4.5 the official Web templates use WebAssembly SIMD. The team's physics stress test ran about 1.5x to 2x faster on the CPU side; it does nothing for GPU limits. Browsers without SIMD cannot run these templates; build with `wasm_simd=no` if you must reach them.

### Web: browser rules that shape game code

Fullscreen and mouse capture only work inside an input event callback; polling `Input` in `_process()` is not enough.

```gdscript
extends Node

func _unhandled_input(event: InputEvent) -> void:
	var click := event as InputEventMouseButton
	if click != null and click.pressed:
		DisplayServer.window_set_mode(DisplayServer.WINDOW_MODE_FULLSCREEN)
```

A hidden tab stops `_process()` and `_physics_process()`, so network sessions time out. `user://` lives in IndexedDB; check `OS.is_userfs_persistent()`. Only HTTP, WebSocket client and WebRTC networking exist. Gamepads appear after the first button press.

### JavaScriptBridge beyond eval

`JavaScriptBridge.get_interface()` wraps a JS object, `create_object()` calls `new`, and `create_callback()` wraps a `Callable`. The callback must take exactly one `Array` argument, and you must keep the returned object in a member variable or it is freed. `download_buffer()` hands a save file to the player; `pwa_needs_update()` and `pwa_update()` drive PWA updates. Templates built with `javascript_eval=no` have no singleton: `eval()` returns `null`.

### Android without writing a plugin

Since 4.4, `JavaClassWrapper.wrap()` gives script access to Java classes, and the `AndroidRuntime` singleton (`Engine.get_singleton("AndroidRuntime")`) supplies the activity and application context. With a Gradle build, any `.jar` or `.aar` under `addons/` is included, so a small SDK call needs no Kotlin. A constructor is called as a method with the class name; inner classes use `Outer$Inner`. Real plugins use the v2 format (since 4.2): an AAR plus an `EditorExportPlugin`; the v1 `.gdap` format is deprecated. Since 4.7, `JavaClassWrapper.create_proxy()` implements a Java interface with a script object and `create_sam_callback()` wraps a `Callable`, so listener-style SDK APIs also work without Kotlin.

### Store SDKs and consoles: who maintains what

The engine does not bundle proprietary SDKs. The Foundation maintains the core store plugins (Google Play Billing, Google Play Games Services, StoreKit 2); other SDK integrations live in a community organisation with uneven upkeep, so check a plugin's state before a design depends on it. For consoles, the Foundation ships no ports: the platform code cannot be published as open source and needs vendor agreements. Ports come from third-party porting companies or from a custom port. Plan budget and time for that early, and keep platform calls behind one interface so a port touches one layer.

### Shader baker (since 4.5)

A per-preset option that stores compiled shader code in the pack. It cuts first-run load time, most on Direct3D 12 and Metal: the release notes report about 20x shorter load for the TPS demo on those drivers. It does not remove pipeline stutter. It works only for Forward+ and Mobile, only for drivers the exporting OS supports (Windows: Vulkan and D3D12; macOS: Vulkan and Metal; Linux and Android: Vulkan), and not in a `--headless` export. Plan the CI runner around that.

## Niche but useful

- Android: AAB output and third-party SDKs need the Gradle build. With it, folders whose name starts with `_` are left out of the APK.
- Android crash reports: official templates have native debug symbols to download (since 4.5). The 2026 mobile report credits them, plus GPU driver workarounds, for two shipped games going from about 4% to under 1% crash rate.
- Android (4.6): the preset can mirror the device with `scrcpy` on run; the system file picker (Storage Access Framework) loads and saves single files without broad storage permissions.
- Android (4.7): picture-in-picture mode, and the splash screen is set in the preset.
- Windows icon and version metadata are written natively since 4.5; `rcedit` is not needed, so a Linux CI can do it.
- visionOS export exists since 4.5, as a windowed app only.
- `--export-pack` builds one pack for several binaries; a binary loads the `.pck` that has its own name.
- Export a pack with a `.zip` name to inspect what really ships.
- Windows and Linux `arm64` builds need no game changes.
- One-click deploy also covers desktop over SSH and wireless adb.
- `--display-server wayland` tests Wayland without changing the project (X11 is still the default; each falls back to the other).
- iOS: link the project folder into Xcode and set `godot_path` in the plist to skip re-export on every change.

## Traps

- ZIP main packs are not found automatically; start with `--main-pack game.zip`.
- CLI export paths are relative to the project, not to the shell folder.
- Windows pack embedding fails above about 3.89 GB for executable plus pack.
- A `.app` exported from Windows loses its executable bit; export a `.zip`.
- macOS without notarization is blocked by Gatekeeper; the default is an ad-hoc signature. Signing from Linux or Windows uses `rcodesign`.
- The Android keystore password and key password must be equal.
- The iOS simulator only supports Compatibility. iOS plugins are static libraries with a `.gdip` file under `res://ios/plugins`.
- Renaming exported Web files breaks loading.
- New projects default to `d3d12` on Windows since 4.6; older projects keep `vulkan` until you change `rendering/rendering_device/driver.windows`. Test both before release.
- Metal (4.4) runs on Apple Silicon only; Intel Macs fall back to Vulkan over MoltenVK.

## Version notes

- 4.3: single-thread Web export, Sample audio, PWA header injection.
- 4.4: patch packs with file removal (`PCKPacker.add_file_removal()`); `JavaClassWrapper` made usable; Metal driver; fallback to Compatibility when no modern driver exists (`rendering/rendering_device/fallback_to_opengl3`); export from the Android editor; Swappy frame pacing; Android sensor events off by default.
- 4.5: shader baker; Web SIMD; 16 KB pages (C# needs .NET 9); edge-to-edge; visionOS; native Windows metadata; `get_setting_with_override_and_custom_features()`.
- 4.6: delta-encoded patches; `d3d12` default for new Windows projects; Gradle builds on device through the build-environment companion app; Android template sources moved to `src/main/`.
- 4.7: `PCKPacker.add_file_from_buffer()`; Java interfaces from script; export templates download per platform and architecture; picture-in-picture on Android.

## Related skills

- `godot-export-pipeline` - preset files, CI workflow, versioning steps.
- `godot-release-and-store` - store editions and demo cuts.
- `godot-mobile-development` - app lifecycle, permissions, safe area.
- `godot-dedicated-server` - server architecture after the export.
- `godot-save-security` - what to do about tampering.

## Sources

- https://godotengine.org/releases/4.4/
- https://godotengine.org/releases/4.5/
- https://godotengine.org/releases/4.6/
- https://godotengine.org/releases/4.7/
- https://godotengine.org/article/progress-report-web-export-in-4-3/
- https://godotengine.org/article/upcoming-serious-web-performance-boost/
- https://godotengine.org/article/about-official-console-ports/
- https://godotengine.org/article/godot-sdk-integrations/
- https://godotengine.org/article/godot-mobile-update-apr-2026/
- https://godotengine.org/article/statement-on-godloader-malware-loader/
- https://godotengine.org/article/gabe-stable-release/
- https://docs.godotengine.org/en/stable/tutorials/export/exporting_projects.html
- https://docs.godotengine.org/en/stable/tutorials/export/feature_tags.html
- https://docs.godotengine.org/en/stable/tutorials/export/exporting_pcks.html
- https://docs.godotengine.org/en/stable/tutorials/export/exporting_for_web.html
- https://docs.godotengine.org/en/stable/tutorials/export/exporting_for_dedicated_servers.html
- https://docs.godotengine.org/en/stable/tutorials/export/exporting_for_android.html
- https://docs.godotengine.org/en/stable/tutorials/platform/web/javascript_bridge.html
- https://docs.godotengine.org/en/stable/tutorials/platform/android/javaclasswrapper_and_androidruntimeplugin.html
- https://docs.godotengine.org/en/stable/tutorials/performance/pipeline_compilations.html
