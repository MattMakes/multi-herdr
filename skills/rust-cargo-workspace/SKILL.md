---
name: rust-cargo-workspace
description: Use when changing a Rust project's Cargo setup - adding or upgrading a dependency, adding a crate to a workspace, adding or changing a feature flag, changing `rust-version` (MSRV), or touching `Cargo.lock`. Covers `[workspace.dependencies]` inheritance, additive features, MSRV checks, lockfile discipline, and the gate `cargo clippy --workspace --all-targets -- -D warnings`.
---

# Rust Cargo workspace

Cargo files decide what every crate in the repository compiles against. A small change to one `Cargo.toml` can change the build of every other crate, through feature unification and the shared lockfile. Make each Cargo change on purpose, keep it small, and check the whole workspace after it.

Use the layout and the conventions that the repository already has. Check Cargo behaviour with `cargo help <command>` or the docs tool you have. Do not recall a flag from memory.

## When to use

- A new dependency, a version bump, or a removed dependency.
- A new crate in the workspace, or a crate that moves.
- A new or changed `[features]` entry, or a new `optional = true` dependency.
- A change to `rust-version`, `edition`, `rust-toolchain.toml` or `[workspace.lints]`.
- A diff that changes `Cargo.lock`.

## Workflow

1. **Read the workspace shape.** Read the root `Cargo.toml` (`[workspace]`, `members`, `exclude`, `resolver`, `[workspace.package]`, `[workspace.dependencies]`, `[workspace.lints]`), `rust-toolchain.toml` if it exists, and `.cargo/config.toml` if it exists. Run `cargo metadata --no-deps --format-version 1` to list members when the globs are not clear. Check: you can name each member crate and the resolver version.

2. **Make the change in the right file.** See the sections below. Check: `cargo check --workspace --all-targets` passes.

3. **Check features.** When the change touches features or optional dependencies, build each crate with no default features and with all features (see the features section). Check: both builds pass for each crate that you changed.

4. **Check MSRV.** When the workspace sets `rust-version`, build with that toolchain (see the MSRV section). Check: the MSRV build passes, or the `rust-version` bump is a stated part of the task.

5. **Run the gate.** Run:
   ```sh
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```
   Use the project's own gate command instead when it has one (a `just`, `make` or `xtask` target) and the commands above are inside it. Check: all 3 pass with no new warnings.

6. **Review the lockfile diff.** Run `git diff --stat Cargo.lock` and read the changed package names. Check: every changed package traces to your change.

## Workspace dependency inheritance

- Put each third-party dependency that 2 or more crates use in `[workspace.dependencies]` in the root `Cargo.toml`, with its version and default features. In each member, write `name = { workspace = true }`. A member can add `features = [...]` and `optional = true` on top; it cannot remove features that the workspace entry enables.
- Keep `default-features = false` at the workspace level when any member needs the dependency without defaults. Members that need the defaults add them back with `features`.
- Put internal crates in `[workspace.dependencies]` too, with `path = "crates/<name>"` and, if the crate is published, a `version`.
- Use `[workspace.package]` for shared `edition`, `rust-version`, `license` and `repository`, and `field.workspace = true` in members, when the repository already does this.
- Use `[workspace.lints]` with `[lints] workspace = true` in members for shared lint levels, when the repository already does this. Do not add a new lint level to the workspace inside an unrelated task.
- Do not add a dependency that duplicates one the workspace already has for the same job. Search `[workspace.dependencies]` and `cargo tree -e normal -i <crate>` first.

## Additive features

- Cargo unifies features. When any crate in the build enables feature `x` of a dependency, every crate in that build gets `x`. So a feature must only add code and API. It must never remove an item, change a type or change behaviour that another crate relies on.
- Do not write mutually exclusive features (`backend-a` and `backend-b` that break when both are on). If a choice must be exclusive, make it a runtime option or a separate crate, or make the code accept both with a clear priority.
- Name the optional dependency feature explicitly with `dep:` syntax (`serde = ["dep:serde", "other/serde"]`) so the crate does not expose an implicit feature with the dependency name.
- Keep `default` small. A library default that pulls a large dependency forces it on every user.
- Check the feature matrix for each changed crate:
  ```sh
  cargo check -p <crate> --no-default-features
  cargo check -p <crate> --all-features
  ```
  When the repository uses `cargo hack`, run `cargo hack check -p <crate> --each-feature` instead.
- Resolver 2 (edition 2021 and later default) keeps dev-dependency and build-dependency features apart from normal ones. A crate that builds in tests but not alone often depends on a feature that only a dev-dependency enables. The `--no-default-features` build finds this.

## MSRV

- `rust-version` in `Cargo.toml` is a promise. A new dependency or a new std API can break it without any error on your newer toolchain.
- Check it with the declared toolchain:
  ```sh
  rustup toolchain list
  cargo +<rust-version> check --workspace --all-targets
  ```
  When that toolchain is not installed, do not install it without permission. Say in the report that the MSRV build did not run. When the repository uses `cargo msrv` or a CI job for MSRV, use the same command.
- When a dependency raises its own MSRV above the workspace value, pin a lower compatible version or raise `rust-version` as a stated decision. Do not raise it silently.
- When `rust-toolchain.toml` pins a toolchain, every `cargo` command in this directory uses it. Run `rustup show active-toolchain` to see which one runs.

## Lockfile discipline

- An application or a workspace with binaries commits `Cargo.lock`. Keep it committed when it is already in git.
- Change the lockfile only as much as the task needs. To add or bump one dependency, edit `Cargo.toml` and run `cargo update -p <crate>` (or `--precise <version>`). Do not run a bare `cargo update` inside a feature task: it bumps every dependency and hides the real change.
- Never edit `Cargo.lock` by hand. Never resolve a lockfile merge conflict by hand: take one side, then run `cargo check` to let Cargo repair it.
- Use `--locked` in CI-style checks (`cargo test --workspace --locked`) to prove that the committed lockfile matches the manifests.
- When the lockfile diff adds a crate you did not expect, run `cargo tree -i <crate>` to find which dependency pulled it.
- New dependencies need a reason in the PR: what it does that the workspace cannot do now, its maintenance state, its licence (check with the repository's licence tool, such as `cargo deny`, if it has one), and its transitive size from `cargo tree -p <crate> --depth 1`.

## Clippy gate

- The workspace gate is `cargo clippy --workspace --all-targets -- -D warnings`. `--all-targets` includes tests, benches and examples, which a plain `cargo clippy` skips.
- Fix the warning. Do not add `#[allow(...)]` to pass the gate unless the lint is wrong for that line; when you add one, put the reason in the attribute (`#[allow(clippy::x, reason = "...")]` on a toolchain that supports it) or in the report.
- When the gate fails on code you did not touch, say so in the report and do not fix unrelated code inside the task.

## Gotchas

- `cargo build` in a member directory builds only that member's dependency graph and its feature set. Results can differ from a workspace build. Run checks from the root with `--workspace` or `-p`.
- A `path` dependency outside the workspace root is not a member. `cargo test --workspace` does not test it.
- `[patch.crates-io]` works only in the root `Cargo.toml`. A patch in a member is ignored with a warning.
- Build scripts (`build.rs`) run on the host. A new `build.rs` that calls an external tool must fail with a clear message when the tool is missing.
