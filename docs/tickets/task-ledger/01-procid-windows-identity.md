# 01: Exact process identity on Windows

**What to build:** On Windows, a stored PID plus its start time names
exactly 1 process, as it already does on Linux and macOS. Today `procid`
has no start time on Windows, so `alive` means "the pid exists" and a
signal could reach a stranger that got a reused PID. After this ticket,
`start_time`, `alive`, `is_same` and `signal_same` work on Windows: the
start time is the process creation time, and a stop opens 1 process
handle, checks its creation time on that handle, then terminates through
the same handle.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

**Persona:** primary `backend-developer`, fallback `opus`

**Parallel:** yes

**Spec:** `docs/specs/task-ledger.md` section 7 and section 8 item 6

- [ ] `start_time` returns `Some` for a live process on Windows and `None`
      for an ended one.
- [ ] `signal_same` on Windows sends nothing and returns false when the
      creation time does not match; a test proves it with a child that has
      exited.
- [ ] TERM maps to a polite stop where Windows has one, and KILL to
      `TerminateProcess`; the mapping is written in the module docs.
- [ ] No behaviour change on Linux or macOS: their existing `procid` tests
      pass unchanged.
- [ ] The Windows code compiles in CI (`cargo check --target
      x86_64-pc-windows-msvc` or the repo's existing Windows job).
