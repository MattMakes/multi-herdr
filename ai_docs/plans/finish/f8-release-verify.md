# F8 release-verify: the sandboxed release teammate can archive, and the docs agree

Single-branch work: follow `ai_docs/plans/finish/01-single-branch.md`.

## GOAL

1. Proof that `xcodebuild archive` works inside the exact sandbox horch
   writes for `app-release-preparer` (or the sandbox is fixed so it does),
   without code signing: a tiny app project under
   `/Users/mascott/projects/multi-herdr/.worktrees/_scratch/release-probe/`
   (a minimal iOS or macOS SwiftUI app; write the `.xcodeproj` by hand or
   with a tool already installed; never install new tools without asking),
   archived with `CODE_SIGNING_ALLOWED=NO`. Record the commands, the
   sandbox settings used (from horch's overlay for this teammate) and the
   result. Test the `/private/var/folders` `allowWrite` entry the same way.
2. Every description of `app-release-preparer` says the agent never
   uploads: `teammates/README.md` (~line 214), `docs/skills-and-teams.md`
   (~line 217), and the `asc-xcode-build` skill (adapted skill: edit only the
   upload part, record it in its provenance `adaptation`).

## CONTEXT

- Read `ai_docs/reports/finish/release-sandbox.md` (S6): design, probes,
  "Gotchas". The second probe (archive) was refused by the auto-mode
  classifier; run the archive as an ordinary build command in the scratch
  project, not as a sandbox-escape probe, and never read or touch real
  credentials or the login keychain.
- Run the sandboxed command the way the pane would: through Claude Code
  with horch's settings overlay for this teammate (`claude -p` with
  `--settings <file>`, `env -u ANTHROPIC_API_KEY`), or explain a faithful
  alternative.
- If archive needs a path the sandbox denies, add the narrowest
  `allowWrite`/`allowRead` entry to the teammate, with the reason.

## FILES

own: `teammates/app-release-preparer.md` (sandbox entries only),
`teammates/README.md` (that line), `docs/skills-and-teams.md` (that
paragraph), `skills/asc-xcode-build/` and its provenance entry,
`ai_docs/reports/finish/release-verify.md`.

## STEPS

1. Probe project + archive in the sandbox; fix if needed. 2. Docs and skill.
3. Targeted checks (`horch teammates --check`, skills tests); COMMITTED.
