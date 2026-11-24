---
name: rust-unsafe-review
description: Use when Rust code adds, changes or reviews an `unsafe` block, an `unsafe fn`, an `unsafe impl` (Send, Sync), raw pointers, `transmute`, `MaybeUninit`, FFI or a GDExtension (gdext) boundary. Covers `// SAFETY:` comment contracts, pointer provenance and aliasing, FFI and engine-boundary invariants, and running `cargo +nightly miri test` when the toolchain has it.
---

# Rust unsafe review

`unsafe` does not switch off the borrow checker. It lets the code make promises that the compiler cannot check. Each promise must be written down beside the code that depends on it, and each promise must be true on every path that reaches it. Review `unsafe` code as a list of promises, then try to break each one.

Check library behaviour (for example `std::ptr`, `std::mem`, `godot` crate APIs) with the docs tool you have. Do not recall an API contract from memory.

## When to use

- A diff adds or changes `unsafe { }`, `unsafe fn`, `unsafe trait`, `unsafe impl`, `extern "C"`, `#[no_mangle]`, `#[repr(...)]` on an FFI type, or a raw pointer type.
- A diff calls `transmute`, `from_raw_parts`, `get_unchecked`, `assume_init`, `set_len`, `Box::from_raw`, `Arc::from_raw` or `ManuallyDrop::drop`.
- A safe function changes and an `unsafe` block elsewhere in the module relies on its behaviour. Soundness is a property of the whole module, not of one block.
- Rust code crosses into a C library, a Godot engine object through gdext, or any other runtime that owns memory.

## Workflow

1. **List every `unsafe` site in scope.** Search the changed crate for `unsafe`, `extern`, `transmute` and `from_raw`. Include unchanged sites in the same module, because a safe change can break them. Check: you have a list of sites with file and line.

2. **Require a contract at each site.**
   - Each `unsafe` block has a `// SAFETY:` comment directly above it. The comment names each precondition of the called API and says why it holds here.
   - Each `unsafe fn` and `unsafe trait` has a `# Safety` section in its doc comment. That section lists what the caller must guarantee.
   - Each `unsafe impl Send` or `unsafe impl Sync` says why the type is safe to move or share across threads.
   - A comment that says only "this is safe" or repeats the code is not a contract. Ask for the precondition list.
   - If the repository has a comment-lint rule, the `// SAFETY:` comment is a decision record and stays. Check: every site has a contract with named preconditions.

3. **Check each precondition against the code.** For each promise, find the line that makes it true. Read the paths that reach the block, including error paths and early returns. Use the checklist below. Check: each promise points to a line, a type invariant or a caller contract.

4. **Shrink the unsafe surface.** Prefer a safe API when one exists (`slice::split_at_mut`, `Cell`, `bytemuck`-style checked casts if the repository already uses them). Keep each `unsafe` block as small as the operation that needs it. Wrap the operation in a safe function that checks its inputs, and make fields that hold an invariant private. Check: no public safe function lets a caller break an invariant.

5. **Run Miri when the toolchain has it.** Run `rustup toolchain list` and look for a nightly with the `miri` component.
   - When it exists, run `cargo +nightly miri test` on the crate or on the tests that reach the `unsafe` code (`cargo +nightly miri test -p <crate> <filter>`).
   - Miri cannot run FFI calls or most engine calls. Put the pointer logic behind a function that tests can call without the foreign side, and run Miri on that.
   - When nightly or Miri is not installed, do not install a toolchain without permission. Say in the report that Miri did not run and why.
   Check: you have Miri output, or a stated reason that it did not run.

6. **Add a test for each invariant that can fail.** Test the boundary: empty input, maximum length, alignment, a null pointer from the foreign side, a drop after a panic. Check: the tests pass under `cargo test` and, when available, under Miri.

7. **Report.** For a review, give each finding with file, line, the broken promise and a concrete input or call sequence that breaks it. Mark a finding as unsound only when you can name that input. Otherwise mark it as a missing contract.

## Checklist: pointers and memory

- **Validity.** The pointer is non-null, aligned for `T`, and points to an initialized `T` for the full read or write. A pointer to a zero-sized type still needs to be non-null and aligned.
- **Provenance.** The pointer comes from a reference or an allocation that covers the whole range it reaches. Pointer arithmetic stays inside that allocation (one past the end is the limit). An integer cast back to a pointer does not carry provenance. Prefer `ptr.add`, `ptr.offset` and the strict-provenance methods (`with_addr`, `map_addr`) over `as usize` round trips.
- **Aliasing.** While a `&mut T` is live, no other reference or pointer use touches the same memory. While a `&T` is live, nothing writes to it, except through `UnsafeCell`. Making a `&mut` from a raw pointer while a `&` to the same data is live is undefined behaviour, even when the code does not write.
- **Lifetimes.** A reference built from a raw pointer does not outlive the allocation. A lifetime that the function signature invents (an unbounded `'a` from `&*ptr`) is a red flag. Tie it to an input lifetime.
- **Initialization.** `MaybeUninit::assume_init` and `Vec::set_len` run only after every element is written. A `bool`, `char`, enum or reference with an invalid bit pattern is undefined behaviour even when nobody reads it.
- **Drop and panic safety.** If code between two `unsafe` steps can panic, the value must be in a valid state at that point, or a guard must restore it. Check for double drop after `ptr::read` and for leaks after `mem::forget`.
- **`transmute`.** Both types have the same size and a defined layout (`#[repr(C)]`, `#[repr(transparent)]` or a primitive). Prefer `from_ne_bytes`, `to_bits`, `cast` or a pointer cast with a clear contract.
- **Send and Sync.** A type with a raw pointer, `Rc` or `Cell` is not `Send` or `Sync` by default for a reason. An `unsafe impl` needs a reason that covers every method, including `Drop`.

## Checklist: FFI and GDExtension boundaries

- **Layout.** Every type that crosses the boundary is `#[repr(C)]` or `#[repr(transparent)]`, or a primitive with a fixed size. Do not pass a Rust enum, `String`, `Vec` or trait object by value across `extern "C"`.
- **Ownership.** Each pointer that crosses the boundary has one owner, and the contract says which side frees it and with which allocator. Memory from `Box::into_raw` goes back through `Box::from_raw`, never through the foreign `free`.
- **Nulls and lengths.** Check a foreign pointer for null before you make a reference or slice from it. Check that a foreign length fits in `isize` and in the buffer.
- **Strings.** Use `CStr` and `CString` for C strings. Check that the foreign side keeps a string alive for as long as Rust borrows it.
- **Panics.** A panic must not unwind across an `extern "C"` function. Catch it with `std::panic::catch_unwind` at the boundary, or use an `extern "C-unwind"` ABI only where both sides support it.
- **Threads.** Calls from a foreign thread need the same `Send` and `Sync` reasoning as Rust threads. A callback that the foreign side can call from any thread must not touch thread-local or main-thread-only state.
- **GDExtension (gdext).** The engine owns many objects. A `Gd<T>` of a manually managed class can point to a freed object. Check `is_instance_valid()` before use where the engine or a script can free the object. Do not hold a `bind()` or `bind_mut()` guard across a call that can re-enter the same object (a signal emit, a virtual call, `call_deferred` that runs now). Engine API calls happen on the main thread unless the docs for that API say otherwise. Read the `godot` crate docs for the version in `Cargo.lock` for the rules of each type.

## Gotchas

- "It passes the tests" proves nothing about undefined behaviour. Optimized builds can change behaviour that debug builds hide. Run tests in `--release` as well as under Miri.
- A safe `pub fn` that can cause undefined behaviour for some input is unsound even if no current caller passes that input. Make it `unsafe fn` with a `# Safety` section, or make it check the input.
- `#[allow(unsafe_code)]` or `unsafe_op_in_unsafe_fn` warnings are signals. Do not silence them in a diff without a reason in the report.
- Do not add `unsafe` for speed without a benchmark that shows the gain. Report a safe alternative when one exists.
