# Rust GDExtension with godot-rust (gdext)

`gdext` (the `godot` crate, MPL-2.0) is the community Rust binding for Godot 4.
It is not maintained by the Godot project. The official native binding is
`godot-cpp` (C++); see the parent [GDExtension](../SKILL.md) skill. Current
`gdext` releases support Godot 4.2 and later. Check the crate's release notes
for the engine versions that your `godot` version supports.

A typical layout keeps the crate next to the Godot project:

```text
my-game/
  godot/            project.godot, scenes, my_ext.gdextension
  rust/             Cargo.toml, src/lib.rs, target/
```

## Cargo.toml

The extension is a `cdylib` crate that depends on the `godot` crate. Pin the
version that matches your engine.

```toml
[package]
name = "my_ext"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
godot = "0.x.y"
```

The library file name comes from the crate name: `libmy_ext.so` on Linux,
`libmy_ext.dylib` on macOS and `my_ext.dll` on Windows.

## Entry point

gdext generates the C entry function. You declare 1 type and mark its
`ExtensionLibrary` impl with `#[gdextension]`:

```rust
use godot::prelude::*;

struct MyExtension;

#[gdextension]
unsafe impl ExtensionLibrary for MyExtension {}
```

## A class

`#[derive(GodotClass)]` registers the struct as an engine class.
`#[class(init, base = Sprite2D)]` sets the base class and generates the
constructor. `#[init(val = ...)]` sets a field default.

```rust
use godot::classes::{ISprite2D, Sprite2D};
use godot::prelude::*;

#[derive(GodotClass)]
#[class(init, base = Sprite2D)]
struct Player {
    base: Base<Sprite2D>,
    #[init(val = 100)]
    hitpoints: i32,
}

#[godot_api]
impl ISprite2D for Player {
    fn ready(&mut self) {
        godot_print!("Player ready");
    }
}

#[godot_api]
impl Player {
    #[func]
    fn take_damage(&mut self, damage: i32) {
        self.hitpoints -= damage;
    }

    #[signal]
    fn died();
}
```

The `I<Base>` trait (`ISprite2D`, `INode`, ...) holds the engine virtuals.
`#[func]` exposes a method to GDScript, and `#[signal]` declares a signal.

## The .gdextension file

The file lives in the Godot project and points at the built libraries. gdext
exports the entry symbol `gdext_rust_init`.

```ini
[configuration]
entry_symbol = "gdext_rust_init"
compatibility_minimum = 4.2
reloadable = true

[libraries]
linux.debug.x86_64 = "res://../rust/target/debug/libmy_ext.so"
linux.release.x86_64 = "res://../rust/target/release/libmy_ext.so"
macos.debug = "res://../rust/target/debug/libmy_ext.dylib"
macos.release = "res://../rust/target/release/libmy_ext.dylib"
windows.debug.x86_64 = "res://../rust/target/debug/my_ext.dll"
windows.release.x86_64 = "res://../rust/target/release/my_ext.dll"
```

- `res://../rust/target/debug/` is for a crate beside the project folder. When
  the crate is inside the project, use `res://rust/target/debug/`.
- A `debug` key loads in editor and debug runs. A `release` key loads in a
  release export. Give each platform both keys.
- `reloadable = true` (Godot 4.2 and later) lets the engine reload the
  library after a rebuild. A headless check starts a new process, so it
  does not depend on this setting.
- `compatibility_minimum` is the oldest engine that may load the library.
  Do not set it below the engine API that the `godot` crate was built for.

### Per-target paths

`cargo build --target <triple>` writes to `target/<triple>/debug/`, not
`target/debug/`. When you build with `--target`, point the keys at that
folder:

```ini
[libraries]
linux.debug.x86_64 = "res://../rust/target/x86_64-unknown-linux-gnu/debug/libmy_ext.so"
linux.debug.arm64 = "res://../rust/target/aarch64-unknown-linux-gnu/debug/libmy_ext.so"
windows.debug.x86_64 = "res://../rust/target/x86_64-pc-windows-msvc/debug/my_ext.dll"
macos.debug = "res://../rust/target/universal-apple-darwin/debug/libmy_ext.dylib"
```

`universal-apple-darwin` is not a Rust target. It is the folder where you put
the `lipo` output (see below).

## Builds per target

Build and lint 1 target at a time, from the crate folder:

```sh
cd rust
rustup target add x86_64-unknown-linux-gnu
cargo build --target x86_64-unknown-linux-gnu
cargo build --target x86_64-unknown-linux-gnu --release
cargo clippy --target x86_64-unknown-linux-gnu --all-targets -- -D warnings
cargo test
```

| Platform key | Rust target | Output file |
|---|---|---|
| `linux.*.x86_64` | `x86_64-unknown-linux-gnu` | `libmy_ext.so` |
| `linux.*.arm64` | `aarch64-unknown-linux-gnu` | `libmy_ext.so` |
| `windows.*.x86_64` | `x86_64-pc-windows-msvc` | `my_ext.dll` |
| `macos.*` | `aarch64-apple-darwin` + `x86_64-apple-darwin` | `libmy_ext.dylib` |

A macOS universal library joins the 2 Apple builds:

```sh
cargo build --target aarch64-apple-darwin
cargo build --target x86_64-apple-darwin
mkdir -p target/universal-apple-darwin/debug
lipo -create \
  target/aarch64-apple-darwin/debug/libmy_ext.dylib \
  target/x86_64-apple-darwin/debug/libmy_ext.dylib \
  -output target/universal-apple-darwin/debug/libmy_ext.dylib
```

- A target needs its linker. A Windows MSVC build needs the MSVC tools, and a
  Linux build from macOS needs a cross linker. If this machine cannot build a
  target, report that platform as a gap. Do not commit a library that you did
  not build.
- `cargo test` runs the Rust unit tests on the host. Code that calls engine
  APIs needs a running engine, so test it through Godot (next section).
- Do not commit `target/`.

## Headless verification

A crate that compiles can still fail to load: a wrong path in
`[libraries]`, a wrong entry symbol, or a `compatibility_minimum` above the
engine. Godot reports these on import and does not register the classes.

1. Build the debug library for the host target.
2. From the Godot project root, run `godot-check.sh` from
   `godot-build-verify`. It imports the project with `--headless` through
   `godot-run.sh`, parse-checks every script, runs GUT when the project has
   it, and exits 3 on `SCRIPT ERROR`, `Parse Error` or
   `Failed to load script`. A scene or script that uses a class the
   extension failed to register fails this check.
3. Run a smoke script that asserts each registered class exists and can be
   created:

```gdscript
extends SceneTree

const CLASSES: Array[String] = ["Player"]


func _init() -> void:
	for class_id in CLASSES:
		if not ClassDB.class_exists(class_id):
			push_error("GDExtension class not registered: %s" % class_id)
			quit(1)
			return
		var instance: Object = ClassDB.instantiate(class_id)
		if instance == null:
			push_error("GDExtension class not created: %s" % class_id)
			quit(1)
			return
		if instance is Node:
			instance.free()
	print("gdext smoke: %d classes" % CLASSES.size())
	quit(0)
```

Save it as `res://tests/gdext_smoke.gd` and run it with
`godot-run.sh -s res://tests/gdext_smoke.gd`. Read the log as well as the
exit code: an extension load error is printed as `ERROR:` and the engine can
still exit 0.

A `DONE:` for a Rust change names the target triples you built, the
`cargo clippy` and `cargo test` results, the `godot-check.sh` result, and the
smoke script result.
