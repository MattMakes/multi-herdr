# On Windows the codex fleet rules go in the shared rules file

Windows has no private codex home. Symlinks there need Developer Mode or an
elevated process, and a fleet that refused to start without one would be
worse than a shared rules file. So on Windows `Rules::Shared` in
`crates/horch-core/src/harness/codex.rs` stands for rules appended to the
shared rules file, and `install_rules` writes them where they always went.
The pane says so rather than implying an isolation it does not have.

## Consequences

- `ensure_rules` runs only on Windows. It appends any missing fleet
  `prefix_rule`s to codex's shared `~/.codex/rules/default.rules`. On every
  other platform the rules go into a private home, and this file is left as
  the user wrote it.
- `ensure_rules` is idempotent: a rule whose pattern is already present is
  left alone, so the user's own rules and formatting survive.
- Only allow rules go in the shared file. Every codex session reads it, so
  a forbidden worker rule would also refuse the command to the codex
  orchestrator and to the operator's own sessions.
- The 1 broad ledger rule that v0.4.0 appended is removed from the shared
  file, so the narrowed worker rules apply (#181).
- `read_rules_text` treats only a missing rules file as empty. A file that
  is not UTF-8 text is an error, because rewriting it from "" would drop
  the operator's own rules.
