---
name: rust-async
description: Use when writing, changing or reviewing async Rust - Tokio or another executor, `async fn`, futures, `tokio::select!`, spawned tasks, channels, timeouts or shutdown. Covers ownership and borrows across `.await`, cancellation safety in `select!`, `Send` and `Sync` bounds on futures, blocking calls on an executor thread, and bounded channels with backpressure.
---

# Rust async

An async function is a state machine. Every `.await` is a point where the task can stop, wait, move to another thread, or be dropped and never resume. Most async bugs come from code that assumes it runs from top to bottom without a break. Read each `.await` as "anything can happen here, including the end of this task".

Use the executor and the libraries that the repository already uses. Check their behaviour (Tokio, `futures`, `async-channel` and others) with the docs tool you have, for the versions in `Cargo.lock`. Do not recall an API from memory.

## When to use

- New or changed `async fn`, `impl Future`, `tokio::spawn`, `spawn_blocking`, `JoinSet`, `select!`, `timeout`, channel or `Mutex` use.
- A hang, a stuck shutdown, lost messages, unbounded memory growth, or a "future is not `Send`" compiler error.
- A review of code that runs on an executor.

## Workflow

1. **Find the runtime model.** Read `Cargo.toml` features and the `main` or test attributes (`#[tokio::main]`, `flavor = "current_thread"`, `#[tokio::test]`). Note the number of worker threads, whether tasks must be `Send`, and how the program shuts down. Check: you can say which threads run the code you change.

2. **Mark every `.await` in the changed code.** For each one, answer the 4 questions below. Check: each `.await` has an answer for each question.
   - What state is live across it, and is that state valid if the task stops here forever?
   - Is a lock guard, a `RefCell` borrow or a non-`Send` value live across it?
   - Can this future be dropped here by a `select!`, a `timeout` or a task abort?
   - Can this wait forever? If so, what bounds it?

3. **Fix ownership across `.await`.** See the ownership section. Check: the crate builds with no new `Send` errors and no new `clippy::await_holding_lock` warnings.

4. **Make cancellation safe.** See the cancellation section. Check: for each `select!` branch, you can say what happens to partial work when another branch wins.

5. **Keep blocking work off executor threads.** See the blocking section. Check: no sync file, network, process or CPU-heavy loop runs inside an async task without `spawn_blocking` or a dedicated thread.

6. **Bound every queue and every wait.** See the backpressure section. Check: every channel has a capacity, and every external call has a timeout or a caller-owned cancellation.

7. **Test the concurrency paths.** Write tests that cancel mid-operation, close a channel early, fill a bounded channel, and shut down while work is in flight. With Tokio, use `#[tokio::test(start_paused = true)]` and `tokio::time::advance` for time-based logic, so tests are fast and deterministic. Check: tests pass when run 20 times in a loop (`for i in $(seq 20); do cargo test <filter> || break; done`).

## Ownership across `.await`

- A `std::sync::MutexGuard` held across `.await` blocks other tasks on the same lock while this task sleeps, and it makes the future `!Send`. Drop the guard before the `.await`: copy out what you need inside a block `{ let v = guard.clone(); }`, then await.
- Use an async mutex (`tokio::sync::Mutex`) only when the critical section itself must await. It is slower than a sync mutex for short sections.
- `Rc`, `RefCell` borrows and raw pointers live across `.await` make the future `!Send`. A `!Send` future cannot go to `tokio::spawn` on a multi-thread runtime. Use `Arc` and a `Send` lock, or keep the work on a `LocalSet` if the design needs a single thread.
- `tokio::spawn` needs `'static` futures. Move owned data (`Arc`, cloned values) into the task. Do not try to borrow from the parent stack. For borrowed data with a bounded lifetime, use a scope pattern that the repository already uses, or restructure so the task owns its inputs.
- When a compiler error says a future is not `Send`, read the note that names the value held across the `.await`. Fix that value's scope. Do not add `unsafe impl Send`.

## Cancellation safety

- A future is dropped at its current `.await` when `select!` picks another branch, when a `timeout` fires, when `JoinHandle::abort` runs, or when the task that owns it ends. Code after that `.await` does not run. `Drop` impls do run.
- In a `select!` loop, a branch future that is created again on each iteration loses its partial progress when another branch wins. A `read_exact`, `read_line` or a custom "read until frame complete" can lose bytes that way. Read the docs for each method; Tokio marks which methods are cancel safe.
- Fix a cancel-unsafe branch in one of these ways:
  - Create the future once before the loop, pin it (`tokio::pin!`), and poll `&mut fut` in each iteration.
  - Move the work into a spawned task and select on its `JoinHandle` or a channel.
  - Keep partial state in a buffer that lives outside the future (a framed codec does this).
- Put cleanup that must happen on cancel in a guard type with `Drop`, not after an `.await`.
- For shutdown, pass a cancellation signal (for example `tokio_util::sync::CancellationToken` or a `watch` channel) and let each task finish its current unit of work. Do not rely on `abort` for work that writes data.
- A detached `tokio::spawn` keeps running after the caller gives up. Keep the `JoinHandle` or use a `JoinSet` so errors and panics are seen and the task stops on shutdown.

## Blocking calls on an executor

- A sync call that waits (std `fs`, `std::net`, `std::process::Command::output`, a sync DB driver, `thread::sleep`, `Mutex::lock` under contention) or a CPU loop longer than a few hundred microseconds blocks the worker thread. Other tasks on that thread stop. On a current-thread runtime, the whole program stops.
- Move blocking I/O to `tokio::task::spawn_blocking` or to the async version of the API (`tokio::fs`, `tokio::process`, `tokio::time::sleep`).
- Move long CPU work to `spawn_blocking`, a dedicated thread, or a thread pool such as rayon if the repository uses it. Send results back through a `oneshot` channel.
- Do not call `block_on` or `Runtime::new` inside an async context. Tokio panics on nested runtimes, and other executors can deadlock.
- `spawn_blocking` tasks cannot be cancelled. Give them their own stop flag if they loop.

## Bounded channels and backpressure

- An unbounded channel turns a slow consumer into unbounded memory growth. Use a bounded channel (`mpsc::channel(n)`) and pick `n` from the expected burst size. Write the reason beside the number in the code review or PR.
- Decide what a full channel means: wait (`send().await`, which pushes backpressure to the producer), drop (`try_send` and count drops), or replace the oldest value (`watch` or `broadcast` for latest-value data). Make that choice explicit.
- A `broadcast` receiver that falls behind gets `RecvError::Lagged`. Handle it; do not treat it as a closed channel.
- A loop over `recv()` ends when all senders drop. A task that holds its own sender clone never sees the end. Check that shutdown drops every sender.
- Limit concurrency for fan-out work with a `Semaphore` or a `buffer_unordered(n)` stream, not with unbounded `spawn` in a loop.
- Put a `timeout` on every call to a network peer, a lock that other services hold, or a process. Pick the value from the caller's own deadline.

## Gotchas

- `select!` picks a random ready branch by default. Use `biased;` when the order matters (for example, check shutdown first).
- A `JoinHandle` returns `Err` on panic. Do not unwrap it in production code; log or propagate the panic as an error.
- `async` closures and `async` blocks capture by reference unless you write `async move`. In a spawned task, use `async move`.
- `tokio::time::sleep` in a hot loop with a tiny duration still yields at the timer resolution (about 1 ms). Use `interval` with `MissedTickBehavior` for periodic work.
- Tests that use real time are slow and flaky. Use paused time.
