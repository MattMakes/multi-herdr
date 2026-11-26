# 02: Spec for the horch review tool

<!-- horch-ticket: 1 -->
**Persona:** primary `spec-writer`, fallback `opus` · **Parallel:** yes
**Kind:** docs
**Review:** none
**Check:** none
**Own:** `docs/specs/operator-review.md`

Spec: `docs/specs/external-imports.md` (not yet committed; read it at this absolute path)

## Goal

Write the spec `docs/specs/operator-review.md` for a horch-native tool that lets the operator annotate a spec or a ticket map line by line in a herdr pane.

## Context

- The operator forbids third-party tools (D1) and wants a needed tool built in this repository (D2).
- You are a CLEAN worker. Do not open the plannotator repository, website or package.
- Read the research report `ai_docs/reports/external-imports/plannotator.md`: sections "Summary", "Comparison table" and "Open questions" only.
- The operator's idea, in the operator's words: "no third party apps ever. rebuild here if its needed. cleanroom when there is ownership."
- Ticket 01 of `docs/tickets/external-imports/` defines the notes file contract (path `ai_docs/plans/<slug>/review-<target>-<n>.md`, a `Result:` line, numbered notes `N<k>` with kind, line range, quoted line start and text). The tool writes exactly that file. Read `docs/tickets/external-imports/01-spec-gate.md` step 1.
- horch is a Rust workspace in `crates/`. Pane hosting is in `docs/specs/pane-host.md`.

## Steps

1. Specify a command `horch review <file> [--slug <slug>] [--target spec|map]`: a terminal UI in the Rust binary, with no browser, no network and no new service.
2. Specify the minimum feature set: show the file with line numbers, select a line range, add, edit and delete a note with a kind, show the notes list, finish with `approved`, `annotated` or `dismissed`, write the notes file, and send 1 mail line to the orchestrator with the notes file path.
3. Specify the test seams: the notes file writer as a pure function, the key handling as a state machine with no terminal, and 1 end-to-end test.
4. List each open question with a recommendation.

## Constraints

- Copy no text and no design detail that only the source can give. Describe behavior from the operator's need.
- Name which existing crates and dependencies the tool uses. Read `Cargo.toml` and `crates/` first. Propose a new dependency only with a reason.
- Do not edit a file other than `docs/specs/operator-review.md`.

## Done when

`docs/specs/operator-review.md` exists, has an "Open Questions" section, and names the notes file contract of ticket 01 as its output format.

## Report

Run `horch ledger finish`, then `horch ledger done`. The summary gives the spec path and the count of open questions.
