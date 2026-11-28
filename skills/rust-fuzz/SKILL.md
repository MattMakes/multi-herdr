---
name: rust-fuzz
description: Use when Rust code parses, decodes or transforms input that the code does not control (files, network data, save games, scripts, CLI arguments), when a function has a rule that must hold for every input (round trip, ordering, no panic), or when a review asks for more than example tests. Covers property tests with a deterministic PRNG or `proptest`, `cargo fuzz` and libFuzzer harness design, and shrinking each counterexample into a fixed regression unit test.
---

# Rust fuzz and property testing

An example test checks the inputs that you thought of. A property test and a fuzzer check the inputs that you did not. Both work the same way: state a rule that must hold for every input, generate many inputs, and stop at the first one that breaks the rule. The failing input is the useful output. Shrink it to the smallest case, then keep it forever as a plain unit test.

Use the tools that the repository already uses. Check their APIs (`proptest`, `quickcheck`, `arbitrary`, `libfuzzer-sys`, `cargo-fuzz`) with the docs tool you have, for the versions in `Cargo.lock`. Do not add a new test crate inside an unrelated task. When the repository has none, a small deterministic PRNG in the test module is enough for a property test.

## When to use

- A parser, decoder, deserializer, protocol handler or file loader.
- A pair of functions that should undo each other: encode and decode, serialize and parse, compress and decompress.
- A function with a rule that is easy to state but hard to test by example: "output is sorted", "length never grows", "never panics", "same result as the slow reference version".
- Code that has `unsafe`, index arithmetic or integer casts on input sizes.

## Workflow

1. **Write the property.** State it in one sentence before you write code. Common shapes:
   - **No panic.** For any bytes, `parse(bytes)` returns `Ok` or `Err` and does not panic, overflow or hang.
   - **Round trip.** For any valid value `v`, `decode(encode(v)) == v`.
   - **Oracle.** The fast version gives the same output as a simple, obviously correct version.
   - **Invariant.** After any sequence of operations, the structure's invariant holds (sorted, balanced, length matches count).
   - **Idempotence.** `normalize(normalize(x)) == normalize(x)`.
   Check: the property is one sentence and does not restate the implementation.

2. **Pick the tool.**
   - A property test runs inside `cargo test`, on every machine and in CI. Use it first.
   - `cargo fuzz` runs for minutes or hours with coverage feedback, on a nightly toolchain. Use it for parsers and `unsafe` code where a property test has not found anything and the input space is large.
   Check: you know which tool you use and why.

3. **Write the property test.**
   - With `proptest`, write a `proptest!` block with a strategy for the input. Build valid values from parts (`any::<u32>()`, `vec(..., 0..100)`, `prop_oneof!`) rather than filtering random values, because heavy filtering makes the test slow and weak.
   - Without a crate, write a small PRNG (for example xorshift or splitmix64) with a fixed seed, loop a few thousand times, and print the seed and the input on failure. A fixed seed makes every run the same, so a failure reproduces.
   - Keep the case count small enough that `cargo test` stays fast. A few hundred to a few thousand cases per property is usual.
   Check: the test passes on correct code and fails when you break the code on purpose.

4. **Write the fuzz harness when needed.**
   - Run `cargo fuzz init` in the crate, or add a target beside the existing ones in `fuzz/fuzz_targets/`. The `fuzz/` directory is its own crate and is not a workspace member.
   - A target is `fuzz_target!(|data: &[u8]| { ... })`. Call the code under test with the bytes. Do not `unwrap` the result. A returned `Err` is a correct answer.
   - For structured input, derive or implement `arbitrary::Arbitrary` on an input type and take that type in the closure. The fuzzer then reaches deeper code than random bytes do.
   - Add assertions for the property inside the target (round trip, oracle). A target that only checks "no panic" still finds bugs, but a property finds more.
   - Keep each run fast and free of side effects: no network, no writes outside a temp dir, no global state that carries between runs.
   - Run `cargo +nightly fuzz run <target> -- -max_total_time=<seconds>`. Put a few sample inputs in `fuzz/corpus/<target>/` to start it.
   - When nightly or `cargo-fuzz` is not installed, do not install them without permission. Say in the report that the fuzzer did not run and why.
   Check: the target builds and runs for the time you chose, or you have a stated reason that it did not run.

5. **Shrink each failure.**
   - `proptest` shrinks on its own and prints the smallest input it found. It also writes the case to a `proptest-regressions/` file. Commit that file so the case runs first next time.
   - For `cargo fuzz`, run `cargo +nightly fuzz tmin <target> <crash-file>` to shrink the input. Then `cargo +nightly fuzz fmt <target> <crash-file>` prints a structured input in readable form.
   - For a hand-written PRNG test, shrink by hand: remove parts of the input while the test still fails.
   Check: the failing input is as small as you can make it.

6. **Turn each failure into a unit test.** Write a plain `#[test]` with the shrunk input as a literal. Name it after the bug, not after the tool: `parse_rejects_length_past_end`, not `fuzz_crash_3`. Watch it fail, fix the code, watch it pass. This test runs on every machine without a fuzzer or a seed. Check: each found bug has a named unit test that failed before the fix.

7. **Report.** Give each property, the tool, the case count or run time, each failure found with its shrunk input, and the unit test that now covers it.

## Gotchas

- A property that copies the implementation's logic tests nothing. If the property and the code share a bug, the test passes. Use a different method: a reference version, a round trip, or a simple invariant.
- An `Arbitrary` or strategy that produces only valid input never tests the error paths. For a parser, fuzz raw bytes as well as valid values.
- A test that uses `rand::thread_rng()` or the time as a seed does not reproduce. Use a fixed seed, or print the seed on failure.
- Slow targets find fewer bugs. Remove logging and large allocations from the code path under the fuzzer, or put them behind a cfg.
- A timeout or out-of-memory report from the fuzzer is a real finding when the input size is small. It often means unbounded recursion or an allocation sized from untrusted data. Cap both and return an error.
- Do not commit the full fuzz corpus or crash artifacts unless the repository already does. Commit the unit test that holds the shrunk case.
