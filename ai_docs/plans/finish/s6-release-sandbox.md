# S6 release-sandbox: an OS-enforced boundary for app-release-preparer

Unit slug: `release-sandbox`. Single-branch work: read
`ai_docs/plans/finish/01-single-branch.md` first and follow it.

## GOAL

`app-release-preparer` cannot reach any App Store Connect credential except
the one the operator configured for it, cannot run an outward App Store
write, and this holds even when the pane runs `sh -c` or unsets its env.
Enforced by the OS sandbox, not by Bash deny patterns.

## CONTEXT

- Finding (codex-reviewer-2, HIGH): the deny list is string-matched, so
  `sh -c 'unset ASC_CONFIG_PATH ASC_BYPASS_KEYCHAIN; asc ...'` bypasses it;
  the pane runs as the operator's user, so asc can read another config
  (`~/.asc/`, repo `.asc/`) or the login keychain; the assigned key also
  allows live uploads. Evidence: `teammates/app-release-preparer.md:18,30-33,45-65,76-79`,
  `harness/claude.rs:123-140`, `harness/launch.rs:111-115`.
- Claude Code has a Bash sandbox (macOS Seatbelt, Linux bubblewrap) with
  filesystem and network rules: read `https://code.claude.com/docs/en/sandboxing`
  and the settings reference first, and cite them. Find how a launch passes
  sandbox settings (a `--settings` file horch writes, or the teammate's
  settings; read `harness/claude.rs` and how horch already writes per-launch
  settings).
- Required configuration for this teammate:
  - sandbox on, and the launch fails closed (refuses with a clear error) if
    the sandbox is unavailable on the host;
  - no escape hatch: unsandboxed command retry disabled
    (`allowUnsandboxedCommands: false` or the current name);
  - filesystem: write only the project dir and its temp/build dirs; read the
    operator's asc config file and its key file; deny read of `~/.asc`,
    `~/Library/Keychains`, `~/.config` except `~/.config/horch/asc/`, and
    other credential stores you find in asc's docs;
  - network: only the App Store Connect API hosts asc uses (find them in the
    asc 5.9.1 source; the operator's copy is in `/tmp/App-Store-Connect-CLI-5.9.1`
    or fetch the tag tarball) plus what `xcodebuild` needs for an archive
    (decide and justify; if archives need broad network, say so);
  - no upload by the agent: `asc` upload / `builds upload` / `xcrun altool` /
    `notarytool submit` are denied, and the persona prepares the exact
    upload command for a human (the human runs it). Update the persona and
    README Apple section accordingly.
- Generalize only as far as needed: if horch has no per-teammate sandbox
  field, add one (`sandbox:` in teammate frontmatter, Claude only, `--check`
  rejects it on harnesses that cannot honour it) with tests.
- Test: a launch-level test that the written settings contain the sandbox
  block; a check that a teammate with `sandbox:` on a non-Claude harness is
  rejected; and, if feasible without a real key, a manual proof in the
  report that `sh -c 'cat ~/.asc/...'` is denied inside the sandbox (use a
  dummy file you create, never a real credential).

- `README.md` (~lines 373-378, the App Store Connect key section) says the
  pane cannot use another user key. That is false today
  (`ASC_CONFIG_PATH`/`ASC_BYPASS_KEYCHAIN` only steer asc defaults). Rewrite
  it to state exactly what the sandbox enforces once this unit lands, and
  what still depends on the key's role.

## FILES

own: `teammates/app-release-preparer.md`, `crates/horch-core/src/harness/claude.rs`
and the settings writer, `roster/teammate.rs` + `roster/validation.rs` (the
new field), `README.md` Apple section, `teammates/_template.md` (document the
field), tests, `ai_docs/reports/finish/release-sandbox.md`.

## STEPS

1. Read the sandbox docs; write the design in the report first.
2. Field + settings + check, with tests. Commit (pathspec).
3. Teammate + persona + README. Commit.
4. Targeted checks; COMMITTED note.
