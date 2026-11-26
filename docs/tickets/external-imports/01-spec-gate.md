# 01: Spec gate with operator notes

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `cargo test -p horch-core --test golden_prompts --test skills_catalog && HORCH_TEAMMATES_DIR=teammates cargo run --quiet --bin horch -- teammates --check`
**Own:** `skills/orchestrate/SKILL.md`, `teammates/_base/fleet-orchestrator.md`, `teammates/spec-writer.md`, `teammates/ticket-writer.md`, `crates/horch-core/tests/golden_prompts.rs`, `crates/horch-core/tests/golden/`, `docs/specs/task-ledger.md`

Spec: `docs/specs/external-imports.md` (not yet committed; read it at this absolute path)

## Goal

The orchestrator has a formal review gate for each spec and each ticket map. The operator gives numbered notes in a file. The 2 writers revise from that file.

## Context

- You are a CLEAN worker. Do not open the plannotator repository, website or package. Do not search the web for it.
- The idea comes from the research report `ai_docs/reports/external-imports/plannotator.md`. Read only its sections "Comparison table", "Adoption options" (option O2) and "Open questions". That report is behavior-level text written in this project.
- The operator forbids third-party tools (D1). This ticket adds text rules only. Ticket 02 specifies a horch-native annotation tool. That tool writes the same notes file that this ticket defines.
- The orchestrator briefing text is `teammates/_base/fleet-orchestrator.md`. The section is "From a rough idea to tickets". `crates/horch-core/tests/golden_prompts.rs` pins that text.

## Steps

1. Define the notes file contract. Path: `ai_docs/plans/<slug>/review-<target>-<n>.md`, where `<target>` is `spec` or `map` and `<n>` is the review round from 1. Content: a header with the reviewed file path and its git blob sha, a line `Result: approved|annotated|dismissed`, then numbered notes `N<k>`. Each note has a kind (`change`, `question`, `delete`), a line range of the reviewed file, the quoted line start (at most 80 characters), and the operator text. Write the contract in `docs/specs/task-ledger.md` as a new subsection next to the idea-to-tickets text. Check: the subsection has 1 complete example file.
2. In `teammates/_base/fleet-orchestrator.md`, section "From a rough idea to tickets", add the gate to step 2 (spec) and step 4 (map). Rules: the orchestrator asks the operator to review the file. `approved` lets the next step start. `annotated` makes the orchestrator queue a revision task for the writer with the notes file path. `dismissed` is not an approval: the orchestrator asks again or stops. The orchestrator records each result in `decisions.md`. When the operator answers in chat, the orchestrator writes the notes file itself, in the operator's words. Keep each sentence in Simplified Technical English. Add at most 12 lines.
3. Put the same gate in `skills/orchestrate/SKILL.md` section 0, as 1 numbered item of at most 4 lines that points to the briefing section.
4. In `teammates/spec-writer.md` and `teammates/ticket-writer.md`, add a revision mode: when the brief names a notes file, the writer reads it, applies each note `N<k>`, and appends a section "Revision <n>" to its output that lists each `N<k>` with "applied", or "not applied: <reason>". The writer changes nothing that no note names.
5. Run `cargo test -p horch-core --test golden_prompts`. Update the golden data the way the test file says. Check: the test passes.
6. Run the full Check line of this ticket.

## Constraints

- Copy no text from plannotator. Do not use its name in the files you edit.
- Do not add a tool, a hook or a command.
- Do not edit a file that Own does not name.
- Never read, print or use `ANTHROPIC_API_KEY`.
- Add no code comments.

## Done when

- The Check passes.
- `grep -n "annotated" teammates/_base/fleet-orchestrator.md skills/orchestrate/SKILL.md docs/specs/task-ledger.md` prints at least 1 line for each file.
- `grep -ci plannotator` prints 0 for each file in Own.

## Report

Run `horch ledger finish`, then `horch ledger done`. The summary gives the notes file contract path and section, and each file changed.
