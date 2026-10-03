---
name: ui-redesign
description: Use when an existing website or app UI must look better without breaking it - auditing a current design, upgrading generic or dated screens, or extracting a design direction from a reference URL or screenshot - and the improvement must be proven with before/after screenshots.
---

# UI Redesign

Upgrade an existing interface in safe, reviewable steps, and prove each step with evidence. A redesign keeps what works (content, routes, behavior, brand, accessibility, analytics) and replaces what reads as generic or broken.

## When to use

- A user or plan asks to improve, modernize, restyle, or "de-slop" an existing site, page, or app screen.
- You must audit a current UI and produce a ranked fix list (read-only).
- You must extract a design direction from a reference URL or screenshot and apply it to an existing product.
- Not for: a greenfield build (use `ui-taste`), a new design system (use `design-system`), or a brand change (use `brand-identity`), where those skills are available.

## Inputs

- The target: repository paths or route list, and a way to run it locally (dev server command, static build) or a live URL.
- The mode, if stated: **preserve** (modernize inside the current brand) or **overhaul** (new visual language, same content and structure).
- Optional: a reference URL or screenshot to learn from, brand assets, a `design.md` or token file, known complaints.

## Workflow

1. **Classify the job.** Decide the mode (preserve or overhaul) and the scope (one page, or several pages that must share one system). If the mode is unclear and the answer changes the work, ask once: `horch tell orchestrator "[<role>] QUESTION: Preserve the brand or overhaul the visuals? I recommend <x> because <y>."` Continue the read-only steps while you wait.
   Done when: mode and scope are written in your notes.
2. **Capture the baseline.** Run the project. Take screenshots of every target page at the fixed widths, light and dark if both exist (`references/evidence.md`). Record the commit, the commands, and any console errors.
   Done when: a `before/` set exists and you can reproduce it with one command.
3. **Scan the code.** Record the framework, styling method, tokens, fonts, icon library, motion library, routes, and analytics hooks, each with `file:line`. List what must not change (`references/preservation.md`).
   Done when: the do-not-change list is written.
4. **Audit.** Score the current UI. If the `ui-taste` skill is available, use its rubric and anti-pattern searches. Otherwise use `references/audit-checklist.md`. Record each finding with severity, evidence, and a one-line fix.
   Done when: every finding has evidence (a measurement, a `file:line`, or a screenshot name) and a baseline score exists.
5. **Extract a direction (only if a reference was given).** Follow `references/extraction.md`: run the safety and refusal checks first, then extract structure, type roles, color anchors, and rhythm. Never copy pixels, copy, or images.
   Done when: the diagnosis is written and the parts you will adopt are named.
6. **Plan increments.** Order the fixes by impact and risk (`references/audit-checklist.md`, fix priority). Group them into increments that each touch one concern and can be reverted alone. List the files each increment will modify or create. Any deletion of a file, route, or component needs explicit approval first.
   Done when: the plan lists increments, files, and the check for each.
7. **Apply one increment at a time.** Make the change with the existing stack. After each increment: build, run the project's tests, re-render the affected pages, and compare with the baseline. Commit each increment separately if you are allowed to commit.
   Done when: the increment's check passes and nothing on the do-not-change list moved.
8. **Prove the improvement.** Take the `after/` set at the same widths and states. Re-run the audit and record the new score. Write the report with before/after pairs, the score change, what changed, and what you left alone and why (`references/evidence.md`).
   Done when: every claimed improvement has a before/after pair or a measurement, and the score did not drop on any axis.

For a read-only audit, stop after step 4 (or 5) and deliver the report. Do not edit files.

## Rules

- **Work inside the existing stack.** No framework, CSS-method, or component-library migration unless the plan says so. Check the dependency file before you add a library.
- **Do not break behavior.** Routes, URLs, anchor IDs, form field names and order, analytics events, auth, data fetching, and legal or consent text stay as they are unless the plan says otherwise.
- **Never delete silently.** No removal of files, routes, or component directories without explicit approval of a file-level list.
- **Preserve wins.** Do not regress focus states, alt text, keyboard paths, contrast, or performance. A redesign that lowers any measured score is not done.
- **Keep the content.** Visual modernization is not a copy rewrite. Fix only broken or clearly generated strings, and list each one in the report.
- **Small, reversible steps.** One concern per increment. If an increment fails its check, revert it and re-plan; do not stack fixes on a broken step.
- **Shared system for multi-page work.** Pages of one product share tokens, type, accent, and button style. Change the system once and apply it everywhere; do not restyle pages independently. Write the system to the project's design-system file if one exists; the `design-system` skill covers creating one.
- **Evidence over claims.** "Looks better" is not a result. Each claim has a screenshot pair or a measurement.
- **Complete output.** Deliver every page and state in scope. No stubs, no "the rest follows the same pattern".

## Review checklist

- [ ] Mode and scope recorded; question asked only if the answer changed the work.
- [ ] Baseline screenshots at all fixed widths, with the reproduction command.
- [ ] Do-not-change list written and still true at the end (routes, field names, analytics IDs, legal text).
- [ ] Every audit finding has evidence and a fix; the baseline score is recorded.
- [ ] Reference extraction (if any) passed the safety and refusal checks and copied no pixels, text, or images.
- [ ] Each increment was built, tested, and compared before the next one started.
- [ ] After screenshots at the same widths and states; the new score is recorded and no axis dropped.
- [ ] Report lists changes, kept items, deferred items, and checks that could not run.

## References

- `references/audit-checklist.md` - load for step 4 when `ui-taste` is not available, and for step 6 (fix priority).
- `references/preservation.md` - load for step 3 and before any increment that touches routes, forms, navigation, or shared styles.
- `references/extraction.md` - load for step 5, only when a reference URL or screenshot is given.
- `references/evidence.md` - load for steps 2 and 8: screenshot protocol, comparison, and the report template.
