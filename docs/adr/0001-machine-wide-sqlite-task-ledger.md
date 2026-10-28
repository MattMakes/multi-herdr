# One machine-wide SQLite task ledger at ~/.horch

The task ledger is 1 SQLite file at `~/.horch/ledger.sqlite3`
(`%USERPROFILE%\.horch\` on Windows) for every project, not a JSON file per
project under the XDG state dir like the session ledger. Many workers write
heartbeats and claims at once, and a claim must be atomic: a whole-file
JSON rewrite under a directory lock does not hold up to that, and SQLite's
`BEGIN IMMEDIATE` does. One fixed path on every OS lets an operator or an
agent find it without knowing XDG rules, and lets `ledger unfinished`
answer across projects.

## Consequences

- `rusqlite` (bundled) is the 1 SQLite crate, an exception to NFR-05 and
  NFR-09, which banned it. The operator approved SQLite for this ledger on
  2026-10-27; it is synchronous, so NFR-09's intent (no async runtime)
  holds.

- The file must be on a local disk; SQLite locking is unsafe on a network
  share.
- The JSON session ledger stays for its current readers (sessions, cost,
  telemetry) until a later expand-contract change moves them.
