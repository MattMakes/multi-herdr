# Code and comment sweeps adapt to project languages

`code-sweep` gives the first round-1 review slot to a domain reviewer when
the diff touches a Godot, Rust or Swift stack, and keeps every other slot
on the cross-vendor generic reviewer. `comment-sweep` never sends Godot
structured data files to `comment-remover`.

## Code-sweep pairs a domain reviewer with the cross-vendor reviewer

`codesweep::domain_reviewer` picks the first match in this order:

1. `godot-code-reviewer` for `.gd`, `.tscn`, `.tres`, `.gdextension` or
   `project.godot`.
2. `rust-reviewer` for `.rs` or `Cargo.toml`.
3. `swift-reviewer` for `.swift` or `Package.swift`.

`codesweep::slot_reviewers` gives that persona the first slot of round 1,
which is always the `b` (correctness) lens, with the generic reviewer as
its fallback. When the domain reviewer is in `reviewers_down`, the slot
stays on the generic reviewer. Rounds 2 and 3 do not change: they use the
cheaper generic reviewers.

A generic reviewer misses engine and language bugs: node lifecycle, main
thread rules, `unsafe` soundness and ownership. A domain reviewer on every
slot loses the second vendor, which catches the blind spots of the author's
own model. 1 domain slot plus cross-vendor slots keeps both.

Triage also reads the Godot and C# signals: `class_name `, `signal `,
`@export`, `public class `, `public struct `, `public interface ` and
`#[godot_api]` pick `g`. `.csproj`, `Directory.Packages.props`,
`project.godot`, `.gdextension` and `plugin.cfg` pick `d`. `.tscn` and
`.tres` pick `u`.

## Comment-sweep skips Godot data files and keeps safety and doc comments

`comments::NOT_CODE` holds `.tscn`, `.tres`, `.import`, `.uid`,
`.gdextension` and `project.godot`. The Godot editor writes and rewrites
these files. A `;` line in them is data or editor state, and a hand edit
can break the scene or the import cache.

The pruning pass keeps Rust `// SAFETY:` comments, because
`clippy::undocumented_unsafe_blocks` requires them. It keeps Rust `///` and
`//!` doc comments and GDScript `##` doc comments, because they state a
public contract and the Godot editor shows `##` as help text.

`scripts/comments/comment_lint.py` scans `.cs` files for `//` and `/* */`
comments, so the repository comment ban covers C# too.
