# Cross-process locks are directories published with their owner

`DirLock` in `crates/horch-core/src/fsx.rs` is the 1 cross-process lock.
It generalizes the ledger's mkdir spinlock and the telemetry pid lock. The
holder records who it is in an `owner` file. The lock directory appears
together with that file by an atomic rename that does not replace an
existing lock (`publish`). A lock whose owner is dead, or that is older
than `stale_after`, is broken. Breaking is coordinated (`break_lock`), so
2 processes that see the same stale lock cannot remove each other's fresh
lock.

## Consequences

- `publish` writes the owner into a staging directory and renames that
  directory to the lock path, so a lock is never seen without its owner. A
  holder whose owner write is slow (an `F_FULLFSYNC` on macOS under I/O
  load can take seconds) is not taken for one that died before writing it.
- `rename_noreplace` fails when the target exists. A plain `rename`
  replaces an empty directory, which is the lock of an older binary
  between its `create_dir` and its owner write.
- `write_owner` writes the staged owner file without a temp file and
  without fsync. Nobody reads it before the rename, and a lock lives no
  longer than its holder, so after a crash every holder is dead.
- The owner file holds the host name of the holder. A pid is only checked
  for liveness on the same host, since a pid on another host (a shared
  filesystem) cannot be checked.
- The owner file holds a random nonce per acquisition, so a holder only
  ever refreshes or releases its own lock.
- `DirLockGuard::drop` removes the lock only if the owner file still holds
  its nonce. A holder that outlived `stale_after` may have had its lock
  broken and taken by another process, and that lock is not its to remove.
- Without coordination the slower of 2 breakers removes the lock the
  faster one has just taken in its place. So a breaker first takes the
  `BreakerGuard`, then `<name>.lock.break/`, and checks again that the
  lock is stale. A process that cannot take either waits and retries.
- `BreakerGuard` is an exclusive `flock` on the directory of the lock. The
  kernel drops it when the process dies, so unlike a breaker directory it
  is never left behind.
- `BreakerGuard::drop` unlocks the `flock` before closing the file. A
  child forked by another thread shares the open file until it execs, and
  a close alone leaves the `flock` held while any copy is open.
- A breaker killed inside its window leaves `<name>.lock.break/`. It is
  removed once older than `BREAKER_STALE_AFTER` (5 s), and only under the
  guard: 2 processes that removed it at once could remove a fresh breaker
  a third had just made, and 2 breakers would run again. The breaker
  directory stays so that binaries from before the guard are still
  excluded.
- `remove_lock` renames the lock directory before deleting it, so a
  process that takes the lock in the meantime is not removed with it.
- `pid_alive` only says whether a pid exists. A pid alone can name a later
  program, so `DirLock` and the telemetry collector lock decide liveness
  with `crate::procid::alive`, which also checks the process start time.
- `pid_alive` rejects pid 0 and values above `i32::MAX`. As `kill()`
  targets they name a process group or every process, not 1 process.
- On Windows `pid_alive` opens a process handle instead of running
  `tasklist`: no process spawn, and no text match that could find the pid
  inside another number.
