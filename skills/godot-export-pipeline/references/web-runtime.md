Adds web build behaviour beyond the export preset: renderer and threads, hosting rules, save storage, tab visibility, user gestures and safe JavaScript calls; read it before shipping or debugging an HTML5 build.

# Web runtime: what the browser build must do

> ← Back to [SKILL.md](../SKILL.md)

## Renderer and threads

- The web export runs the **Compatibility** renderer (WebGL 2). The
  project setting `rendering/renderer/rendering_method.web` selects it even
  when the desktop build uses Forward+. Test the look in Compatibility
  before you promise a web build.
- The web preset has a **Thread Support** option (Godot 4.3+). With threads
  off, the page does not need cross-origin isolation, and it runs on hosts
  where you cannot set headers (itch.io without the SharedArrayBuffer
  option, many static hosts). With threads on, the server must send
  `Cross-Origin-Opener-Policy: same-origin` and
  `Cross-Origin-Embedder-Policy: require-corp`.
- C# projects cannot export to the web in Godot 4.

## Hosting checklist

1. Serve over HTTPS. Clipboard, some storage and other browser APIs need a
   secure context.
2. Keep every URL in a custom HTML shell relative, so the game also runs
   from a subfolder such as `/games/mygame/`.
3. Serve `.wasm` with the MIME type `application/wasm`, and enable
   compression for `.wasm` and `.pck`.
4. Keep the download small: the export filters in
   [feature-tags-and-build-size.md](feature-tags-and-build-size.md) apply,
   and every megabyte is load time before the first frame.

## Saves on the web

`user://` works on the web: the engine keeps it in the browser's
IndexedDB. The browser can still delete it (private windows, storage
pressure, the player clearing site data). So:

- Write saves to `user://` with the normal `FileAccess` or `ConfigFile`
  code. Do not build a second save path only for the web.
- Offer an export of the save file for anything the player cares about.
  `JavaScriptBridge.download_buffer(bytes, name, mime)` starts a browser
  download.
- If you need `localStorage`, call it through `JavaScriptBridge.get_interface`.
  Never build JavaScript source with string formatting and pass it to
  `JavaScriptBridge.eval`: a quote in the data breaks the script, and data
  from a player becomes code.

```gdscript
# web_storage.gd - small localStorage wrapper, safe for non-web builds
extends Node


func save_json(key: String, value: Variant) -> bool:
	if not OS.has_feature("web"):
		return false
	var storage: JavaScriptObject = JavaScriptBridge.get_interface("localStorage")
	if storage == null:
		return false
	storage.call("setItem", key, JSON.stringify(value))
	return storage.call("getItem", key) != null


func load_json(key: String) -> Variant:
	if not OS.has_feature("web"):
		return null
	var storage: JavaScriptObject = JavaScriptBridge.get_interface("localStorage")
	if storage == null:
		return null
	var raw: Variant = storage.call("getItem", key)
	return null if raw == null else JSON.parse_string(str(raw))


func offer_download(path: String) -> void:
	var bytes: PackedByteArray = FileAccess.get_file_as_bytes(path)
	if OS.has_feature("web") and not bytes.is_empty():
		JavaScriptBridge.download_buffer(bytes, path.get_file(), "application/octet-stream")
```

## Pause when the tab is hidden

Browsers throttle hidden tabs, and audio that keeps playing in a hidden tab
annoys players. Listen to the page's `visibilitychange` event through a
callback object. Keep the callback in a member variable: if it is freed,
JavaScript calls into nothing.

```gdscript
# tab_visibility.gd - autoload
extends Node

var _on_visibility_js: JavaScriptObject = null  # Must outlive the listener.


func _ready() -> void:
	if not OS.has_feature("web"):
		return
	_on_visibility_js = JavaScriptBridge.create_callback(_on_visibility_changed)
	var document: JavaScriptObject = JavaScriptBridge.get_interface("document")
	document.call("addEventListener", "visibilitychange", _on_visibility_js)


func _on_visibility_changed(_args: Array) -> void:
	var document: JavaScriptObject = JavaScriptBridge.get_interface("document")
	var hidden: bool = bool(document.get("hidden"))
	get_tree().paused = hidden
	AudioServer.set_bus_mute(AudioServer.get_bus_index(&"Master"), hidden)
```

## Fullscreen, mouse capture and audio need a user gesture

Browsers allow fullscreen, pointer lock and audio playback only in
response to a click or key press. Request them from an input handler, for
example a "Click to play" button, not from `_ready()`.

```gdscript
# click_to_play.gd - on a full-screen Button shown at start
extends Button


func _ready() -> void:
	pressed.connect(_on_pressed)


func _on_pressed() -> void:
	Input.mouse_mode = Input.MOUSE_MODE_CAPTURED
	if OS.has_feature("web"):
		DisplayServer.window_set_mode(DisplayServer.WINDOW_MODE_FULLSCREEN)
	hide()
```

## Opening links

`OS.shell_open(url)` opens a new browser tab on the web. Call it from a
click handler for the same gesture reason.

## Progressive web app updates

When the preset enables **Progressive Web App**, the page can update
itself. `JavaScriptBridge.pwa_update_available` fires when a new version
is cached; `JavaScriptBridge.pwa_needs_update()` and
`JavaScriptBridge.pwa_update()` apply it (the page reloads). Ask the player
first, and save before you call `pwa_update()`.

```gdscript
# pwa_updater.gd - autoload
extends Node

signal update_ready


func _ready() -> void:
	if OS.has_feature("web"):
		JavaScriptBridge.pwa_update_available.connect(_on_update_available)


func _on_update_available() -> void:
	update_ready.emit()  # A menu shows "Update now?" and calls apply_update().


func apply_update() -> void:
	if JavaScriptBridge.pwa_needs_update():
		JavaScriptBridge.pwa_update()
```
