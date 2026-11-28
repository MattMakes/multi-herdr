---
name: rust-error-handling
description: Use when designing, changing or reviewing how Rust code reports failure - a new error type, a `Result` signature, `?` chains, `unwrap` or `expect`, `panic!`, `thiserror`, `anyhow`, or error messages shown to users or logs. Covers typed errors for libraries versus contextual errors for applications, keeping the cause chain (`{e:#}`), and no panics on untrusted input.
---

# Rust error handling

A Rust error has 2 readers. Code reads it to decide what to do next. A person reads it to find out what went wrong. A library serves the first reader with a typed error that callers can match. An application serves the second reader with a chain of context that says what it was doing at each level. Pick the right shape for each boundary, keep the chain intact, and never panic on input that you do not control.

Use the error crates that the repository already uses. Check their APIs (`thiserror`, `anyhow`, `eyre`, `snafu` and others) with the docs tool you have, for the versions in `Cargo.lock`. Do not add a new error crate inside an unrelated task.

## When to use

- A new public function that can fail, or a changed error type.
- A new `unwrap`, `expect`, `panic!`, `unreachable!`, index or slice that can go out of bounds, or integer arithmetic on external values.
- A log line or user message that loses the real cause ("failed to load config" with no reason).
- A review of a parser, a protocol handler, a CLI, or any code that reads files, network data or user input.

## Workflow

1. **Find the boundary type.** Decide whether the code is library code (a crate or module that other code calls and matches on) or application code (a binary, a command handler, a top-level task). Read 2 or 3 neighbouring functions to see the error type they return. Check: you can name the error type that your code should return.

2. **Design the error for its reader.** Use the library or application section below. Check: each variant or context string answers "what failed" and, where useful, "with which input".

3. **Propagate with `?` and add context at each layer that knows something new.** Do not log and return the same error; do one or the other. Check: a failure deep in the stack prints a chain that names each layer's operation.

4. **Remove panics on untrusted input.** Use the panic section. Check: `grep -nE 'unwrap\(|expect\(|panic!|\[[a-z_]+\]' ` over the changed files shows only cases with a stated invariant.

5. **Test the error paths.** For each variant or failure your change adds, write a test that triggers it and asserts the variant (`assert!(matches!(err, MyError::NotFound { .. }))`) or the message. Do not test by `unwrap()` on the happy path only. Check: each new failure has a test.

## Library code: typed errors

- Return a concrete error type, usually an enum with one variant per failure a caller may handle differently. Do not return `anyhow::Error` or `Box<dyn Error>` from a public library API: callers cannot match it.
- Implement `std::error::Error` and `Display` for it. `thiserror` generates both; a hand-written impl is fine when the crate avoids proc-macro dependencies.
- `Display` gives one short lowercase sentence about this level only, with no trailing period and without the cause's text. The cause goes in `source()`. With `thiserror`, put the inner error in a `#[source]` or `#[from]` field and do not also print it in `#[error("...")]`. Printing it twice duplicates the text in every chain.
- Add the data a caller needs to act: the path, the key, the byte offset. Keep large values out of the error.
- Use `#[from]` only when there is one obvious mapping from the inner error. When the same inner error type means different things in different calls (2 different files that can both fail to open), use separate variants and `map_err`.
- Mark a public enum `#[non_exhaustive]` when the crate is published and may add variants, so adding one is not a breaking change.
- Do not expose a dependency's error type in a public variant unless that dependency is already part of the public API. Wrap it, or keep it as a `source` behind an opaque variant.

## Application code: contextual errors

- In a binary or a top-level handler, a single dynamic error type with context is usually right: `anyhow::Result`, `eyre::Result`, or the type the repository already uses.
- Add context with what the code was doing and the key input: `.with_context(|| format!("reading config {}", path.display()))`. Use `with_context` with a closure when the message allocates, so the happy path pays nothing.
- Print the whole chain at the top level. With `anyhow`, `{:#}` prints `outer: middle: inner` on one line, and `{:?}` prints the chain on separate lines plus a backtrace when `RUST_BACKTRACE=1`. Plain `{}` prints only the outermost message, which drops the real cause. For a std error, walk `source()` in a loop.
- Return a non-zero exit code on error. Print the error once, at the top, not at every layer.
- When the application must branch on a specific failure (retry on a timeout, 404 on not found), keep that failure typed until the branch, or use `downcast_ref` on the dynamic error with a test that proves the downcast still matches.

## No panics on untrusted input

- Untrusted input is anything the code does not create: files, network data, environment variables, CLI arguments, config, database rows, data from another process or a game engine script.
- On those paths, replace:
  - `unwrap()` and `expect()` on parse results with `?` and a typed error or context.
  - `v[i]` and `&s[a..b]` with `get(i)` and `get(a..b)` plus an error.
  - `a + b`, `a * b` and `as` casts on external sizes with `checked_add`, `checked_mul` and `try_from`, plus an error.
  - `unreachable!()` on a value from outside with an error variant.
- `expect` is fine for an invariant that the code itself guarantees (a regex literal that compiles, a lock that is never poisoned by design). The message states the invariant: `expect("regex literal is valid")`. A message like `expect("failed")` adds nothing.
- `unwrap` in tests is fine. In examples and doc tests, prefer `?` with a `fn main() -> Result<...>` so readers copy the right pattern.
- A panic inside a spawned thread or task does not stop the program by default. It turns into a `JoinError` or a poisoned lock. Handle those, do not unwrap them.
- A panic across an FFI or GDExtension boundary is undefined behaviour or an engine crash. Return an error code or catch the panic at the boundary.

## Gotchas

- `Box<dyn Error>` without `+ Send + Sync + 'static` cannot cross threads or go into `anyhow`. Use the full bound in any shared alias.
- `?` converts through `From`. A broad `From` impl can turn many unrelated errors into one variant and hide the source. Check which impl `?` picks.
- `Option` has no error message. Convert with `.ok_or_else(|| ...)` or `.context("...")` before `?` so the failure says what was missing.
- `main` that returns `Result` prints the error with `Debug`. For a user-facing CLI, handle the error in `main` and print `{:#}` yourself.
- Do not compare error messages in code. Match on types or variants. Message text is for people and can change.
