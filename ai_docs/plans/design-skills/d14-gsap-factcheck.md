# D14 gsap-factcheck: verify motion-gsap API facts against gsap.com

Unit slug: `gsap-factcheck`. Branch: `ds/gsap-factcheck`.

## GOAL

Every API fact in `skills/motion-gsap/` (method names, option names and
defaults, plugin names, licensing statements) matches the official GSAP docs
at gsap.com, with special care for the Flip and Observer tables in
`references/plugins.md`, which D05 corrected from memory.

## CONTEXT

- Read first: `00-conventions.md`, `01-skill-authoring.md`,
  `ai_docs/reports/design-skills/skill-motion.md` (the corrections it made).
- Network is allowed for this unit: fetch gsap.com docs pages (Flip,
  Observer, ScrollTrigger, SplitText, ScrollSmoother, useGSAP, matchMedia,
  utils). Read only; install nothing.
- Keep the size budget. Keep the house style.

## FILES

own: `skills/motion-gsap/**`, `ai_docs/reports/design-skills/gsap-factcheck.md`.

## STEPS

0. Create the worktree (conventions §3).
1. Build a fact list from the skill (one line per claim with file:line).
2. Check each against the docs; record the URL per fact in the report.
3. Fix every wrong or outdated fact. Gate. Commit `Skills: Verify motion-gsap against the GSAP docs`.
4. Report: the fact table (claim, source URL, status). Follow conventions §7.
