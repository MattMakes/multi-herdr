---
name: rust-perf
description: Use when Rust code must get faster or use less memory, when a diff claims a speed-up, or when a review touches a hot path - allocation in loops, `clone`, `collect`, `String` building, iterator chains versus indexed loops, `Vec` capacity, or new `unsafe` added for speed. Covers measuring first with `cargo bench` and Criterion baselines, allocation profiling, borrowing instead of cloning, iterator versus index trade-offs, and avoiding premature `unsafe`.
---

# Rust performance

A performance change is a claim, and a claim needs a number. Measure the current code, find where the time or memory goes, change one thing, and measure again with the same input. Keep a change only when the number moves by more than the noise. Most slow Rust code allocates too much or copies too much. Fix that first. Reach for `unsafe` last, and only with a benchmark that shows the gain.

Use the benchmark and profiling tools that the repository already uses. Check their APIs (`criterion`, `divan`, `dhat`, `iai-callgrind` and others) with the docs tool you have, for the versions in `Cargo.lock`. Do not add a new benchmark crate inside an unrelated task.

## When to use

- A task says "make X faster", "reduce memory" or "fix a frame-time spike".
- A diff says it improves speed but has no measurement.
- A review finds `clone()`, `to_string()`, `to_vec()`, `collect()` or `format!` inside a loop or a per-frame function.
- A diff adds `unsafe`, `get_unchecked` or a manual loop and says it is for speed.

## Workflow

1. **State the goal as a number.** Name the operation, the input size and the target: "parse a 10 MB file in under 200 ms", "no allocation per frame in `update`". Check: the goal has a unit and an input.

2. **Build a repeatable measurement.**
   - Look for `benches/` and `[[bench]]` in `Cargo.toml`. Add the benchmark beside the existing ones.
   - With Criterion, run `cargo bench --bench <name> -- --save-baseline before` on the current code. After the change, run `cargo bench --bench <name> -- --baseline before`. Criterion prints the change and says whether it is above the noise.
   - Wrap inputs and results in `std::hint::black_box` so the optimizer cannot remove the work.
   - Use input that looks like production: the same sizes, the same shape. A benchmark on 10 items says little about 10 000.
   - When no benchmark crate exists, a `#[test] #[ignore]` timing test with `std::time::Instant` and many runs is acceptable for a rough number. Say so in the report.
   Check: 2 runs of the baseline on the same code give numbers within a few percent of each other.

3. **Find where the cost is.** Do not guess.
   - CPU time: run a sampling profiler on a release build with debug symbols (`[profile.release] debug = true` or `CARGO_PROFILE_RELEASE_DEBUG=true`). Use the profiler that the host has: `cargo flamegraph`, `perf`, `samply` or Instruments on macOS.
   - Allocations: count them. Use the allocation profiler that the repository has (`dhat` as a test-only global allocator, `heaptrack`, or Instruments Allocations). A counting global allocator in a test is a simple fallback that can assert "0 allocations in this call".
   Check: you can name the 1 to 3 functions that hold most of the cost.

4. **Change one thing.** Use the checklists below. Make the smallest change that removes the cost. Check: tests still pass.

5. **Measure again.** Run the same benchmark against the saved baseline. Keep the change only when the gain is above the noise. Revert a change that does not help, even if it "should" help. Check: you have a before number and an after number for the same input.

6. **Report.** Give the goal, the before and after numbers, the input, the machine and the profile (`--release`, LTO, target CPU). Say what you did not measure.

## Checklist: allocation and copies

- **Borrow instead of clone.** A function that only reads takes `&str`, `&[T]` or `&T`, not `String`, `Vec<T>` or `T`. A `clone()` to satisfy the borrow checker on a hot path is a sign that ownership is in the wrong place. Restructure before you accept the copy.
- **Take ownership when the callee keeps the value.** A function that stores a `String` takes `String` (or `impl Into<String>`). Taking `&str` and calling `to_string()` inside forces a copy even when the caller has an owned value to give.
- **`Cow` for mostly-borrowed data.** When most inputs pass through unchanged and a few need a change, `Cow<'_, str>` avoids the copy on the common path.
- **Reserve capacity.** When the final size is known or bounded, use `Vec::with_capacity`, `String::with_capacity` or `reserve`. A `Vec` that grows by push reallocates and copies on each doubling.
- **Reuse buffers.** In a loop or a per-frame function, keep a `Vec` or `String` outside the loop and `clear()` it each time. `clear` keeps the capacity.
- **Avoid `collect` between steps.** A chain that collects into a `Vec` only to iterate it again allocates for nothing. Pass the iterator on, or return `impl Iterator`.
- **`format!` and `to_string` in loops.** Use `write!` into a reused `String`. Do not build a string to compare or hash it when you can compare the parts.
- **Small values.** A type that is `Copy` and small is cheap to pass by value. A large struct passed by value is a `memcpy` on each call. Pass `&T` or `Box` it.
- **`Rc` and `Arc` clones.** These are cheap but not free. `Arc::clone` is an atomic increment. In a tight loop, borrow the inner value once.

## Checklist: loops and iterators

- Iterator chains (`iter().map().filter().sum()`) usually compile to the same machine code as an indexed loop. Do not rewrite one into the other for speed without a benchmark.
- Indexed access `v[i]` adds a bounds check. The optimizer removes it when it can prove `i < v.len()`. Iterating with `iter()`, `zip` or `chunks_exact` gives it that proof. Slicing once before the loop (`let v = &v[..n];`) also helps.
- `chunks_exact` and fixed-size arrays let the compiler use SIMD. A `while` loop with complex exit conditions often blocks it.
- Hashing: `HashMap` uses a DoS-resistant hasher by default. For keys that do not come from an attacker, a faster hasher can help. Use one only if the repository already depends on it, and only where the benchmark shows hashing in the profile.
- Look up once. `if map.contains_key(&k) { map.get(&k) }` hashes twice. Use `get`, or the `entry` API for insert-or-update.
- Sort with `sort_unstable` when the order of equal elements does not matter. It does not allocate.

## Checklist: build settings

- Measure only release builds. A debug build can be 10 to 100 times slower, and its profile points at the wrong functions.
- Check `[profile.release]` and `[profile.bench]` before you compare numbers: `lto`, `codegen-units`, `opt-level`, `panic`. Changing these changes every number. Do it as its own change with its own measurement.
- `-C target-cpu=native` makes a binary that may not run on other machines. Do not set it in a shared config without asking.

## Unsafe for speed

- Add `unsafe` (`get_unchecked`, `set_len`, raw pointer loops) only after the safe version is profiled, the bounds check or copy is shown to be the cost, and a safe rewrite (iterators, `chunks_exact`, `split_at`) does not remove it.
- The benchmark must show a gain above the noise. Keep the safe version in a test and assert that both give the same output.
- Follow `rust-unsafe-review` for the `// SAFETY:` contract and Miri.

## Gotchas

- A micro-benchmark that the optimizer can see through measures nothing. If a result is near 0 ns, the work was removed. Use `black_box`.
- Laptops change clock speed with heat and power. Plug in, close heavy apps, and compare runs from the same session. Report a large change only when it repeats.
- A faster average can hide a worse tail. For frame or request timing, report the worst percentile (p99 or max) as well as the mean.
- Allocation is often cheaper than you think and cache misses cost more. A `Vec<T>` of values beats a `Vec<Box<T>>` for iteration, because the data sits together in memory.
- Do not trade readability for a gain that the profile does not show. Unmeasured "optimizations" are review findings, not improvements.
