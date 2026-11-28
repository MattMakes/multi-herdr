# Godot + Rust worktree timing (GRS-15)

Date: 2026-11-27. Ticket: `docs/tickets/godot-rust-support/13-worktree-cache.md`.

## Decision

Decision: no seeding. Worktree creation does not copy `.godot/imported/`, `uid_cache.bin` or `global_script_class_cache.cfg`.

Seeding `.godot/imported/` saves less than 30 percent of the cold import wall time in every measured profile. The best seeded result saves -3 percent, so seeded imports are as slow as or slower than cold imports. Even a fully warm import, the upper bound for any cache, saves at most 17 percent. A cold `godot --headless --import` costs about 2 s, and Godot startup and the file scan take most of it. The ticket threshold is a saving above 30 percent, so `create_worktree` and `add_worktree` stay unchanged.

The cold cost in a new worktree is Cargo: 18.34 s for `cargo check` and 24.98 s for `cargo test`. Seeding the Godot cache cannot reduce this cost. Every worktree keeps its own `target/`. Sharing `CARGO_TARGET_DIR` stays forbidden.

## Host

- Apple M3 Ultra, 32 cores, macOS 26.5.1
- Godot 4.7.2.stable.official.ed1daf0bf (`/opt/homebrew/bin/godot`)
- cargo 1.99.1 and rustc 1.99.1 (Homebrew), crates already in the local registry cache

## Sample project

The sample project is a git repository with 2 parts:

- `game/`: a Godot project with `project.godot`, `main.gd` (`class_name Main`) and `sample.gdextension`
- `rust/`: a `cdylib` crate that depends on `godot = "0.5.5"`, with one `GodotClass` and one unit test

The repository commits each `*.import` sidecar file and ignores `.godot/` and `rust/target/`. Godot recommends this layout. The import was timed with 3 asset profiles, one git tag each:

| Profile | Assets | Texture import |
|---|---|---|
| `lossless` | 120 PNG 512x512 + 20 WAV 1 s (92 MB) | `compress/mode=0` |
| `vram` | the same 140 assets | `compress/mode=2` (S3TC) |
| `big` | `vram` + 30 PNG 2048x2048 (453 MB) | `compress/mode=2` (S3TC) |

## Method

Each run makes new worktrees from the base checkout with `git worktree add --detach <path> <tag>`. The base checkout has a complete `.godot/` from its own import. The script records wall time for each command with `time.perf_counter()`. Each measurement has 3 runs.

- Cargo cold: the first `cargo check` and the first `cargo test` in a new worktree. Each command has its own empty target directory, `CARGO_TARGET_DIR=<worktree>/rust/target-check` and `<worktree>/rust/target-test`.
- Cargo warm: the same command again, in the same worktree and target directory.
- Import cold: `godot --headless --path game --import` in a new worktree without `.godot/`.
- Import warm: the same command again, in the same worktree.
- Import seeded: a second new worktree. The script copies `game/.godot/imported/`, `uid_cache.bin` and `global_script_class_cache.cfg` from the base checkout, then runs the import. The copy time is recorded separately.

Commands:

```sh
git worktree add -q --detach <wt> <tag>
cd <wt>/rust && CARGO_TARGET_DIR=<wt>/rust/target-check cargo check
cd <wt>/rust && CARGO_TARGET_DIR=<wt>/rust/target-test cargo test
cd <wt> && godot --headless --path game --import
```

A cold import log shows `(Re)Importing Assets (140 steps)`, or 170 steps for `big`. A warm import log shows no import steps. The cold `big` worktree holds 30 `big_*.s3tc.ctex` files of 2 MB each, so the cold import really compressed the textures.

## Cargo results (seconds)

| Run | check cold | check warm | test cold | test warm |
|---|---|---|---|---|
| 1 | 18.46 | 0.04 | 26.75 | 0.04 |
| 2 | 17.48 | 0.04 | 24.99 | 0.05 |
| 3 | 19.09 | 0.04 | 23.21 | 0.06 |
| mean | 18.34 | 0.04 | 24.98 | 0.05 |

## Godot import results (seconds)

| Profile | Run | cold | warm | seed copy | seeded |
|---|---|---|---|---|---|
| lossless | 1 | 1.97 | 2.67 | 0.18 | 2.34 |
| lossless | 2 | 1.97 | 1.92 | 0.13 | 1.95 |
| lossless | 3 | 2.14 | 1.74 | 0.08 | 2.17 |
| vram | 1 | 1.76 | 1.76 | 0.05 | 2.15 |
| vram | 2 | 1.95 | 1.94 | 0.07 | 1.76 |
| vram | 3 | 2.14 | 1.95 | 0.06 | 1.93 |
| big | 1 | 2.15 | 1.76 | 0.09 | 2.54 |
| big | 2 | 1.93 | 1.77 | 0.09 | 2.78 |
| big | 3 | 2.16 | 1.68 | 0.09 | 2.36 |

## Against the 30 percent rule

The seeded saving is (cold - seeded - copy) / cold. The warm bound is (cold - warm) / cold.

| Profile | cold mean | warm mean | seeded mean + copy | seeded saving | warm bound |
|---|---|---|---|---|---|
| lossless | 2.03 | 2.11 | 2.28 | -13 % | -4 % |
| vram | 1.95 | 1.88 | 2.01 | -3 % | 3 % |
| big | 2.08 | 1.74 | 2.65 | -27 % | 17 % |

No profile passes the threshold of a saving above 30 percent. Result: no seeding.

## Notes

- Without committed `*.import` files, a seeded worktree still reimports every asset, because Godot writes new sidecar files. A project without committed sidecar files gets no gain from seeding.
- The first full import in a new checkout can exit with code 134 after it writes all imported files. The sample `.gdextension` names a library that the new worktree has not built yet. Later imports exit 0.
