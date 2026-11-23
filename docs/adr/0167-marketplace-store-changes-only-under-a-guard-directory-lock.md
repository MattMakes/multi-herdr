# The marketplace store changes only under a guard directory lock

One install at a time changes the skill store of `crates/horch-marketplace`.
`Store::lock` (`crates/horch-marketplace/src/store.rs`) covers the span from
the last read of `marketplace.lock` to its write, so 2 installs cannot each
write from the same old lock. `Installer::install` and `Installer::rebuild`
materialize a skill version only while holding it, because 2 installs of
the same version directory, and the `marketplace.lock` read-modify-write,
must not interleave. The lock is a directory, `marketplace.lock.guard`,
made with an exclusive `create_dir` and removed when the guard drops. The
crate cannot reuse the `DirLock` of horch-core, because horch-core depends
on this crate, and it has no `libc` for `flock`.

## Consequences

- `Store::lock` waits up to `LOCK_WAIT` = 60 s, then fails with a
  `TimedOut` error that names the guard directory.
- A guard directory older than `STALE_AFTER` = 30 s has no live holder. 30 s
  is safe because the guarded span is only a rename and 1 small file write.
- `LOCK_WAIT` is longer than `STALE_AFTER`, so a guard left by a holder that
  died is broken before a waiter gives up.
- `Store::clear_stale` breaks a stale guard by first renaming it aside to a
  random name, then removing it. Of several concurrent waiters only 1
  rename succeeds, so only 1 breaks the lock.
