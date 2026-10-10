---
name: app-store-changelog
description: Create user-facing App Store release notes by collecting and summarizing all user-impacting changes since the last git tag (or a specified ref). Use when asked to generate a comprehensive release changelog, App Store "What's New" text, or release notes based on git history or tags.
---

# App Store Changelog

## Overview
Generate a comprehensive, user-facing changelog from git history since the last tag, then translate commits into clear App Store release notes.

## Workflow

### 1) Collect changes
- From the repo root, gather commits and touched files since the last tag (all commands are read-only):
  ```bash
  since="$(git describe --tags --abbrev=0 2>/dev/null || true)"
  range="${since:+$since..}HEAD"
  git log --reverse --date=short --pretty=format:'%h|%ad|%s' "$range"
  git log --reverse --name-only --pretty=format:'--- %h %s' "$range" | sed '/^$/d'
  ```
- If needed, set a specific tag or ref instead: `range="v1.2.3..HEAD"`.
- If no tags exist, `range` is `HEAD`, which is the full history.

### 2) Triage for user impact
- Scan commits and files to identify user-visible changes.
- Group changes by theme (New, Improved, Fixed) and deduplicate overlaps.
- Drop internal-only work (build scripts, refactors, dependency bumps, CI).

### 3) Draft App Store notes
- Write short, benefit-focused bullets for each user-facing change.
- Use clear verbs and plain language; avoid internal jargon.
- Prefer 5 to 10 bullets unless the user requests a different length.

### 4) Validate
- Ensure every bullet maps back to a real change in the range.
- Check for duplicates and overly technical wording.
- If any change is ambiguous or possibly internal-only, ask the orchestrator with `horch tell orchestrator`, state your assumption, and continue with it.

## Commit-to-Bullet Examples

The following shows how raw commits are translated into App Store bullets:

| Raw commit message | App Store bullet |
|---|---|
| `fix(auth): resolve token refresh race condition on iOS 17` | • Fixed a login issue that could leave some users unexpectedly signed out. |
| `feat(search): add voice input to search bar` | • Search your library hands-free with the new voice input option. |
| `perf(timeline): lazy-load images to reduce scroll jank` | • Scrolling through your timeline is now smoother and faster. |

Internal-only commits that are **dropped** (no user impact):
- `chore: upgrade fastlane to 2.219`
- `refactor(network): extract URLSession wrapper into module`
- `ci: add nightly build job`

## Example Output

```
What's New in Version 3.4

• Search your library hands-free with the new voice input option.
• Scrolling through your timeline is now smoother and faster.
• Fixed a login issue that could leave some users unexpectedly signed out.
• Added dark-mode support to the settings screen.
• Improved load times when opening large photo albums.
```

## Output Format
- Title (optional): "What's New" or product name + version.
- Bullet list only; one sentence per bullet.
- Stick to storefront limits if the user provides one.

## Resources
- `references/release-notes-guidelines.md`: Language, filtering, and QA rules for App Store notes.
