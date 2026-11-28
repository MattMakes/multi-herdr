| Project files | Stack check |
|---|---|
| `Cargo.toml` (also with `project.godot`) | `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings` |
| `project.godot` | `bash skills/godot-build-verify/scripts/godot-check.sh` |
| `Package.swift` | `swift test` |

In a Godot+Rust project, also run `godot-check.sh` when a task changes scenes or scripts.

Godot check: `bash skills/godot-build-verify/scripts/godot-check.sh`, not bare `godot --headless`.
