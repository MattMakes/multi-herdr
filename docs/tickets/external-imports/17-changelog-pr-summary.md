# 17: Changelog line, PR summary, and the run's spec and tickets

<!-- horch-ticket: 1 -->
**Persona:** primary `sonnet-feature`, fallback `codex-terra` · **Parallel:** no
**Kind:** docs
**Review:** none
**Check:** `cargo test -p horch-core --test skills_catalog --test baseline_oracles`
**Own:** `CHANGELOG.md`, `docs/specs/external-imports.md`, `docs/tickets/external-imports/`

## Goal

PR #467 has a changelog entry, a summary at the top of its description, and the spec and tickets of the run.

## Context

- Run `external-imports`, branch `fleet/external-imports`, PR https://github.com/MattMakes/multi-herdr/pull/467. The operator approved a changelog line and a PR summary on 2026-11-25.
- Read `.claude/skills/push-pr/SKILL.md` for the changelog style and the release line. A merge with an entry under `## Unreleased` publishes a release.
- Facts for the text are in `ai_docs/plans/external-imports/decisions.md`. Read it. Read the PR body with `gh pr view 467 --json body -q .body`.
- The spec and the tickets are not committed. They are in the main checkout: `docs/specs/external-imports.md` and `docs/tickets/external-imports/`.

## Steps

1. Copy `docs/specs/external-imports.md` and each file of `docs/tickets/external-imports/` into the same relative paths of your worktree.
2. In the copies, replace the absolute path prefix of the main checkout with nothing, so that each path is repo-relative. Check: `grep -rn "/User[s]/" docs/specs/external-imports.md docs/tickets/external-imports/` prints nothing.
3. Add entries under `## Unreleased` in `CHANGELOG.md`, in the style of the v0.5.3 entries, for what a user of horch notices: (a) the operator review gate with a notes file for specs and ticket maps; (b) `design-system` generates palettes from seed colours with `palette.py`, derives contrast-safe roles and records a theme; (c) the 3 corrected web rules (native dialog, intrinsic-size animation, CSS scroll timelines before GSAP) and the precedence rule; (d) fixes: skill-intake fleet mode and controller spawn on the hc backend, and branch cleanup when worktree creation fails. Do not name plannotator, Astryx, Meta or Google.
4. Commit. Run `horch ledger finish`.
5. After finish passes, add a summary ABOVE the first `<!-- horch:task` marker of the PR body. Keep every existing line below it unchanged. Use `gh pr edit 467 --body-file <file>`. The summary has 4 parts: "Why" (the operator asked to bring 3 external projects in as own text, with no third-party tool); "What landed" (4 to 6 bullets); "Not in this PR" (the web guide import waits for a follow-up run after horch is reinstalled; the review tool `horch review` is a spec only; the sandbox broker is a design only in `docs/security/sandboxed-pane-horch-access.md`); "Release" (the line that push-pr asks for). Check: `gh pr view 467 --json body -q .body | head -30` shows the summary, and `gh pr view 467 --json body -q .body | grep -c "horch:task T-"` prints the same count as before your edit.

## Constraints

- Do not push by hand and do not merge. `horch ledger finish` does the push.
- Add no attribution line to a commit message or to the PR body.
- Do not put a person's name, an email address or a home folder path in a file or in the PR body.
- Never read, print or use `ANTHROPIC_API_KEY`.

## Done when

The Check passes, the PR body has the summary, and `git show --stat HEAD` lists `CHANGELOG.md`, the spec and the ticket files.

## Report

Run `horch ledger done`. The summary quotes the changelog entries.
