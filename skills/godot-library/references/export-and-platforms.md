# Export and platforms

Target engine: Godot 4.7. How an export is assembled, how packs, patches and feature tags
really behave, and what each target platform demands that the others do not.

## Use this shelf when

- You design build variants (demo, server, store editions) or a CI export.
- You plan patches, DLC or mods delivered as extra packs.
- A game must run in a browser, on a phone, or on a signed desktop build.
- You need a platform API (JavaScript, Java/Kotlin, permissions) from script.
- Download size or first-run load time is a goal.

## How the engine works here

An export is a template binary (the engine without the editor) plus a pack. The pack holds
`project.binary`, imported files from `.godot/imported/`, scenes converted to binary
(`editor/export/convert_text_resources_to_binary`) and `.remap` files that point the
original path at the converted file. Files and folders whose name starts with a dot are
never packed. Non-resource files (`.json`, `.csv`, `.txt`) are packed only when an include
filter names them.

Packs form a virtual `res://`. A pack loaded later wins for any path it shares with an
earlier one, unless `replace_files` is `false`. That rule gives patches, DLC and mods.
PCK is uncompressed and fast to read; ZIP is smaller, slower, and editable with OS tools.

Feature tags are plain strings. `OS.has_feature()` reads them, project settings can be
suffixed with them (`setting.tag`), and `.gdextension` files use them to pick a library.
Built-in tags are fixed at build time, except texture compression tags, `web_<os>` tags
and `movie`.

Two files hold the preset data: `export_presets.cfg` (commit it) and
`.godot/export_credentials.cfg` (passwords, keystore data, the encryption key; never
commit it). In CI, supply secrets through the documented environment variables
(`GODOT_SCRIPT_ENCRYPTION_KEY` and the per-platform keystore and signing variables), which
override the preset during export.

## Advanced techniques

### Feature-tag overrides that are read correctly

`ProjectSettings.get_setting()` ignores tag overrides. Use `get_setting_with_override()`
for your own settings, or an override such as `game/quality.mobile` is silently unused.
`get_setting_with_override_and_custom_features()` evaluates a setting for tags you pass.

```gdscript
var quality: int = int(ProjectSettings.get_setting_with_override(&"game/quality/level"))
```

Traps: custom tags from a preset exist only in the exported build (and one-click deploy),
not when you press Play in the editor, and not inside `EditorExportPlugin` scripts.
`OS.has_feature("mobile")` is `false` in a Web build on a phone; test `web_android` or
`web_ios` there.

### Patch packs and delta encoding

A patch pack holds only files that differ from the base packs listed in the preset's
Patching tab. CLI: `--export-patch <preset> <path>` with `--patches <list>`. List earlier
patches as base packs so each new patch stays small.

Delta encoding stores only the changed bytes of a file. It makes patches much smaller but
adds load time for each patched resource, and that cost stacks with every later patch of
the same file. Rules from the docs:

- Keep the compression level at 19 or below. Negative levels trade size for decode speed.
- Turn compression off for resources you expect to patch (import options, the Compress
  Binary Resources editor setting, GDScript export mode), then delete `.godot/imported/`.
- The base packs listed must be the exact files the player has, in load order. A
  re-export of an old version can differ by a few bytes and break the delta.

### Load packs before anything is preloaded

`preload()` resolves when a script loads, so a pack loaded from a menu cannot replace
scenes that the menu already pulled in. Load packs in the `_init()` of an autoload.

```gdscript
extends Node

func _init() -> void:
	var dir: String = OS.get_executable_path().get_base_dir()
	var ok: bool = ProjectSettings.load_resource_pack(dir.path_join("patch1.pck"))
	if not ok:
		push_warning("patch1.pck was not loaded")
```

`DirAccess` does not show files added this way. For mods, pass `false` as the second
argument, or keep mods in their own folder, so a mod cannot replace core files by path.

### Treat packs as code

A pack can carry scripts, so loading one runs someone's code. The docs name three risks:
a hostile mod, a pack swapped on disk, a compromised patch server. Sign your own patches
and verify them with `Crypto` and a public key in the main pack. There is no DRM for
paid packs.

### Pack encryption, and what it does not do

Encryption (AES-256) needs export templates you compiled with the same key
(`SCRIPT_AES256_ENCRYPTION_KEY` at build time). Official templates cannot read encrypted
packs. Nothing is encrypted until you add include filters in the Encryption tab. The key
sits inside the binary, so this stops casual ripping only. Build steps are in
`gdextension-and-engine-builds`.

### Dedicated server export

The "Export as dedicated server" resource mode replaces textures, materials and meshes
with placeholders that keep only size data, adds the `dedicated_server` tag and forces
headless mode. Per file you choose Strip Visuals, Keep (the server reads pixels, such as
a collision map) or Remove. Remove breaks any scene that still references the file; use
`load()` from script for those. Audio has no placeholder yet. Enable
`application/run/flush_stdout_on_print` when a service manager collects the log.

### Web: choose threads on purpose

- No-threads build (default since 4.3): runs on any host, including portals and iframes.
- Threads build: needs a secure context plus `Cross-Origin-Opener-Policy: same-origin`
  and `Cross-Origin-Embedder-Policy: require-corp`. Without them the game does not start.
  The PWA option installs a service worker that fakes the headers when you cannot set
  them. GDExtension on Web has the same header requirement.
- Audio defaults to Sample playback on Web (`audio/general/default_playback_type.web`):
  low latency without threads, but no `AudioEffect`, no reverb or doppler, no procedural
  audio. Switch a player or the setting to Stream when you need those, and accept latency.
- Only the Compatibility renderer runs on Web (WebGL 2). C# projects cannot export to Web.

### Web: size and startup

Serve `.wasm` as `application/wasm`. Compress `.wasm` and `.pck` on the server; gzip takes
the wasm to about a quarter of its size and Brotli does better. itch.io does not compress
on the fly. A custom template without unused features shrinks the wasm further. A custom
HTML shell can preload files and pass arguments through the `Engine` JavaScript class.

### Web: browser rules that shape game code

Fullscreen and mouse capture only work inside an input event callback; polling `Input`
in `_process()` is not enough.

```gdscript
extends Node

func _unhandled_input(event: InputEvent) -> void:
	var click := event as InputEventMouseButton
	if click != null and click.pressed:
		DisplayServer.window_set_mode(DisplayServer.WINDOW_MODE_FULLSCREEN)
```

A hidden tab stops `_process()` and `_physics_process()`, so network sessions time out.
`user://` lives in IndexedDB; check `OS.is_userfs_persistent()`. Only HTTP, WebSocket
client and WebRTC networking exist. Gamepads appear after the first button press.

### JavaScriptBridge beyond eval

`JavaScriptBridge.get_interface()` wraps a JS object, `create_object()` calls `new`, and
`create_callback()` wraps a `Callable`. The callback must take exactly one `Array`
argument, and you must keep the returned object in a member variable or it is freed.
`download_buffer()` hands a save file to the player; `pwa_needs_update()` and
`pwa_update()` drive PWA updates. Templates built with `javascript_eval=no` have no
singleton: `eval()` returns `null`.

### Android without writing a plugin

Since 4.4, `JavaClassWrapper.wrap()` gives script access to Java classes, and the
`AndroidRuntime` singleton (`Engine.get_singleton("AndroidRuntime")`) supplies the
activity and application context. With a Gradle build, any `.jar` or `.aar` under
`addons/` is included, so a small SDK call needs no Kotlin. A constructor is called as a
method with the class name; inner classes use `Outer$Inner`. Real plugins use the v2
format (since 4.2): an AAR plus an `EditorExportPlugin`; the v1 `.gdap` format is
deprecated.

### Shader baker (since 4.5)

A per-preset option that stores compiled shader code in the pack. It cuts first-run load
time, most on Direct3D 12 and Metal. It does not remove pipeline stutter. It works only
for Forward+ and Mobile, only for drivers the exporting OS supports (Windows: Vulkan and
D3D12; macOS: Vulkan and Metal; Linux and Android: Vulkan), and not in a `--headless`
export. Plan the CI runner around that.

## Niche but useful

- Android: AAB output and third-party SDKs need the Gradle build. With it, folders whose
  name starts with `_` are left out of the APK.
- Android crash reports: official templates have native debug symbols to download.
- `--export-pack` builds one pack for several binaries; a binary loads the `.pck` that
  has its own name.
- Export a pack with a `.zip` name to inspect what really ships.
- Windows and Linux `arm64` builds need no game changes.
- One-click deploy also covers desktop over SSH and wireless adb.
- `--display-server wayland` tests Wayland without changing the project (X11 is still
  the default; each falls back to the other).
- iOS: link the project folder into Xcode and set `godot_path` in the plist to skip
  re-export on every change.
- `PCKPacker.add_file_removal()` records a deletion in a patch pack.

## Traps

- ZIP main packs are not found automatically; start with `--main-pack game.zip`.
- CLI export paths are relative to the project, not to the shell folder.
- Windows pack embedding fails above about 3.89 GB for executable plus pack.
- A `.app` exported from Windows loses its executable bit; export a `.zip`.
- macOS without notarization is blocked by Gatekeeper; the default is an ad-hoc
  signature. Signing from Linux or Windows uses `rcodesign`.
- The Android keystore password and key password must be equal.
- The iOS simulator only supports Compatibility. iOS plugins are static libraries with a
  `.gdip` file under `res://ios/plugins`.
- Renaming exported Web files breaks loading.
- Consoles: the manual only points to the website; no official console templates ship.

## Version notes

- 4.4: `JavaClassWrapper` and `AndroidRuntime` usable from script; Android sensor events
  off by default.
- 4.5: shader baker; `optimize=size_extra`; C# on Android needs .NET 9 (16 KB page size
  rule of the store).
- 4.6: new projects use `d3d12` on Windows; Gradle builds from the Android editor;
  Android template sources moved to `src/main/`.
- 4.7: the manual documents delta-encoded patches and the Wayland notes used here.

## Related skills

- `godot-export-pipeline` - preset files, CI workflow, versioning steps.
- `godot-release-and-store` - store editions and demo cuts.
- `godot-mobile-development` - app lifecycle, permissions, safe area.
- `godot-dedicated-server` - server architecture after the export.
- `godot-save-security` - what to do about tampering.

## Sources

- https://docs.godotengine.org/en/stable/tutorials/export/exporting_projects.html
- https://docs.godotengine.org/en/stable/tutorials/export/feature_tags.html
- https://docs.godotengine.org/en/stable/tutorials/export/exporting_pcks.html
- https://docs.godotengine.org/en/stable/tutorials/export/exporting_for_web.html
- https://docs.godotengine.org/en/stable/tutorials/export/exporting_for_dedicated_servers.html
- https://docs.godotengine.org/en/stable/tutorials/export/exporting_for_android.html
- https://docs.godotengine.org/en/stable/tutorials/export/exporting_for_macos.html
- https://docs.godotengine.org/en/stable/tutorials/platform/web/javascript_bridge.html
- https://docs.godotengine.org/en/stable/tutorials/platform/web/customizing_html5_shell.html
- https://docs.godotengine.org/en/stable/tutorials/platform/android/javaclasswrapper_and_androidruntimeplugin.html
- https://docs.godotengine.org/en/stable/tutorials/performance/pipeline_compilations.html
