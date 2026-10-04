---
name: swift-code-audit
description: Run a broad read-only audit of an iOS or macOS Swift codebase (bugs, dead code, duplication, Swift concurrency, deprecated APIs, security, performance, SwiftUI quality) and write one numbered report with file:line-cited findings. Use for a "code audit", "comprehensive review", "find tech debt" or "what should I clean up" across a Swift project.
---

# Swift Code Audit

## Operating rules

- **Read-only investigation, single deliverable.** No code changes. The output is one report at the path the orchestrator gives you. If the task names no path, message the orchestrator for one before you write. Never write `CODE_AUDIT.md` into the repository root on your own.
- **Every finding cites `path/to/file.swift:LINE` (or a line range).** "Throughout the codebase" is never acceptable for a Critical or High item.
- **Severity is assigned conservatively.** Critical means crash / data loss / memory corruption / security exposure. Don't inflate. See the severity guide below.
- **Verify every Critical claim before propagating.** A first pass will sometimes overstate severity. Open the cited file and confirm the bug is real. If you can't reproduce the claim by reading the lines, demote or drop.
- **Group by root cause, not by occurrence.** If one missing `@MainActor` annotation triggers seven warnings, that's one finding listing the seven sites, not seven findings.
- **`Dead/` (or any explicitly-archived directory) is excluded.** Check the project's agent docs (`AGENTS.md`, `CLAUDE.md`) and README for any "do not edit" directories before the passes.

## Workflow

### Step 1 — Scope the codebase

Quick measurements to brief the passes:

```bash
find . -name "*.swift" -not -path "./.git/*" -not -path "./Dead/*" -not -path "*/Pods/*" -not -path "*/.build/*" | xargs wc -l 2>/dev/null | tail -1
find . -name "*.swift" -not -path "./.git/*" -not -path "./Dead/*" 2>/dev/null | wc -l
find . -name "*.metal" 2>/dev/null | wc -l
```

Also identify the **hot-spot files** (largest LOC, central state):

```bash
find . -name "*.swift" -not -path "./.git/*" -not -path "./Dead/*" -exec wc -l {} \; | sort -rn | head -10
```

Read the project's agent docs and README if present. They usually flag central state files (e.g., `AppState.swift`), the rendering pipeline, and any intentionally-excluded directories.

### Step 2 — Capture compiler ground truth

Run a build and extract every warning. This becomes the canonical input for the concurrency / deprecation portions of the audit — you should never have to *guess* whether concurrency warnings exist.

Use the MobileBuildMCP build tool if it is connected (`build_sim`; `swift_package_build` for a Swift package, which needs the server's `swift-package` workflow); it returns structured diagnostics. Otherwise use `xcodebuild`:

```bash
xcodebuild -project <Project>.xcodeproj -scheme "<Dev scheme>" -configuration Debug build 2>&1 \
  | grep -E "warning:" | sort -u
```

Deduplicate (multi-target compilation produces duplicates) and bucket by root cause. If the build is incremental (returned in <2s), it likely skipped most files. Run `clean build` to get a complete warning set. If the project does not build, message the orchestrator with the first error and audit without the warning list; say so in §11.

### Step 3 — Run the three passes, one after another

Do the passes yourself, in this order. Each pass has a focused checklist in `references/pass-checklists.md`. Finish one pass and write down its findings before you start the next.

- **Pass A — Concurrency & API modernity.** Input: the warnings captured in Step 2.
- **Pass B — Dead code, duplication, refactor candidates.** Input: the hot-spot list and any known-stale files from the project docs.
- **Pass C — Bugs, logic errors, security, performance, SwiftUI.** Input: the hot-spot list and any subsystems the task calls out (camera pipeline, IAP, API client, etc.).

Every pass records findings in the per-finding template (see below). Record specific file:line findings, not summaries.

### Step 4 — Verify the Critical findings

Before writing the report, **open the cited lines for every Critical-flagged finding** and confirm:

- The code matches what the pass recorded.
- The impact claim is real (e.g., if a finding says "memory corruption," is the buffer actually undersized? Trace the math.).
- The recommended fix is sensible.

This step has caught hallucinated severity in prior runs. Demote or drop items that don't pan out. **Never propagate a Critical you haven't personally verified.**

### Step 5 — Synthesize the report

Use the skeleton in `references/report-template.md`.

**Mandatory: the report must include section numbers in every heading.** Readers file follow-up tasks like "fix §5.4", and they cannot do that if headings are unnumbered.

The numbering rules:

- **Top-level sections are numbered `## 1.` through `## 12.`** — exactly as listed below, in this order, even if a section has no findings (write "_No findings._" under the heading rather than omitting the section).
- **Every finding is a numbered subsection** `### N.M <short title>` where `N` is the parent section number and `M` increments from 1. Example: `### 5.1 Force-unwrap on Bundle.main.url`.
- **Numbers are stable across edits.** If a finding is removed during revision, leave the number and write `_REMOVED: <reason>_` as the body — do not renumber the survivors.
- **Executive summary items** (§1) are an ordered list referencing the underlying numbered finding (e.g., "**[Critical] Force-unwrap on Bundle.main.url** — §5.1 — `path:line`").
- **Verification entries** (§12) reference findings by their subsection number, e.g., `- **§5.1** — open \`path\`, lines 42-47.`. Do not leave `<N.M>` placeholders from the template.

Top-level sections, in order:

1. **Executive summary** — 5-10 highest-impact findings, one line each, with severity tag and a §N.M back-reference.
2. **Quick wins** — ≤30-minute fixes (delete stale files, remove debug `print`s, fix unused-let warnings, add accessibility labels).
3. **Concurrency**
4. **API modernity** — deprecations, replacements available at the deployment target
5. **Bugs / logic errors**
6. **Security**
7. **Performance**
8. **SwiftUI / UI**
9. **Dead code / duplication / refactor**
10. **Cross-cutting recommendations** — patterns worth applying repo-wide
11. **What was NOT audited** — explicit out-of-scope list
12. **Verification** — for each Critical/High, the exact lines that prove the claim

Per-finding template:

```markdown
### N.M <short title>
- **Location:** `path/to/file.swift:LINE-LINE`
- **What:** <observed problem in one sentence>
- **Why:** <impact / why it matters>
- **Action:** <recommended fix; no code, reference patterns>
- **Severity:** Critical | High | Medium | Low
```

Aim for 50-80 distinct findings. Group similar occurrences under one heading if a category has many instances (list the top 5-10 specific examples plus a count of the rest).

## Severity guide

- **Critical** — Likely to cause crashes, data loss, memory corruption, security exposure, or shipping the wrong server URL to production. Open the cited line yourself before assigning this.
- **High** — Real bug a user can hit; compiler warning that will become an error in a future Swift mode; a deprecated API that's actively being removed; an architectural concurrency issue.
- **Medium** — Performance / quality issue; refactor candidate; missing modernization with no functional bug.
- **Low** — Naming, cosmetic, code style, single-occurrence cleanup.

## Quality bar (final check before delivering)

- [ ] Every top-level section is numbered `## 1.` through `## 12.` and every finding is `### N.M <title>`.
- [ ] Executive summary and Verification entries reference findings by §N.M; no `<N.M>` placeholders survived.
- [ ] Every Critical and High finding has an exact line range.
- [ ] Concurrency findings cross-reference the actual Step-2 warning list.
- [ ] Every Critical finding has been verified by opening the cited file.
- [ ] Findings are grouped by root cause (one annotation → many warnings = one finding).
- [ ] The "What was NOT audited" section is explicit.
- [ ] No code is included in the report itself — recommendations describe patterns, not implementations.
- [ ] Any single finding can become a separate task without re-explaining context.

When the report is written, message the orchestrator with its path, the finding counts per severity, and the top 3 items.

## What this skill does NOT cover

- **Algorithmic correctness of Metal kernels / domain-specific code.** Surface obvious issues only.
- **Build settings, scheme configuration, Xcode project structure.** Beyond what's visible in shared schemes.
- **Third-party dependency internals.** SPM / CocoaPods packages are treated as black boxes.
- **Test coverage assessment.** A quick scan of test targets is fine; deep test review is separate work.
- **Localization correctness.** The audit can note untranslated strings but not assess wording.
- **Performance profiling.** The audit identifies *potential* hot paths but doesn't record Instruments traces. For that, use the `swiftui-performance-audit` skill.

## References

- `references/pass-checklists.md` — The checklists for passes A, B and C, and how to fill their project context.
- `references/report-template.md` — Full skeleton for the report.
