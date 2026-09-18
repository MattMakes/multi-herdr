# Brief: add a Simplified Technical English rule for agent-to-agent messages

## Goal
Every message one agent sends to another through `horch tell`, `horch assign`, `horch done`, and the `[<role>] ready / DONE: / BLOCKED:` lines must follow Simplified Technical English (STE, ASD-STE100 style). Add that rule to the base prompts so every teammate inherits it. Prompts are data in this repo: this task changes markdown under `teammates/` only. No Rust.

## Files
- `/Users/mascott/projects/multi-herdr/teammates/_base/fleet-worker.md` (all workers inherit this)
- `/Users/mascott/projects/multi-herdr/teammates/_base/fleet-orchestrator.md` (the Claude orchestrator)
- `/Users/mascott/projects/multi-herdr/teammates/_base/codex-execpolicy.md` and `codex-orchestrator-execpolicy.md`: read them; only add the rule there if they carry their own messaging instructions rather than inheriting the two files above. If they inherit, leave them.
- `/Users/mascott/projects/multi-herdr/teammates/README.md`: add a two-line note that agent-to-agent messages use STE and point at the base files.

## The rule to add (adapt wording to each file's voice, keep the substance exact)
Heading: `== Message style: Simplified Technical English ==` (match the `== ... ==` heading style already used in the base files).

- Write every `horch tell`, `horch assign`, `horch done` message, and every `[role]` line, in Simplified Technical English.
- One instruction or one fact per sentence. Procedural sentences have at most 20 words. Descriptive sentences have at most 25 words.
- Use the active voice and the present tense. Name the actor.
- Use one word for one thing. Do not use synonyms for variety.
- Do not use idioms, metaphors, or hedges such as "it seems", "sort of", "basically".
- Write paths, commands, flags, and identifiers exactly as they are. Put one per sentence when possible.
- Use a list for parallel items, one item per line. Do not nest lists.
- Start a report with the role tag and one keyword: `ready`, `DONE:`, `BLOCKED:`, `NOTE:`, or `QUESTION:`. Then one sentence with the outcome. Then the details.
- Write numbers as digits and state units. Give exact counts when you know them.
- Put a warning before the action it applies to.

Give ONE short example in each base file, no more than 4 lines, e.g.:
```
[sonnet-1] DONE: The report is at ai_docs/reports/x.md. I changed 2 files. Tests pass: 14 of 14. Nothing is uncommitted.
```

## Steps
1. Read both base files fully before editing. Find the section that describes `horch tell` / `horch done` / the DONE line. Insert the new section immediately after it.
2. Keep the `{persona}` and `{task_briefing}` placeholders untouched and in place. Do not change any other section.
3. Update `teammates/README.md` with the two-line note.
4. Verify: `horch teammates --check` exits 0 (run from the repo root). Then run `cargo test -p horch-core teammates` to confirm no test asserts on the exact base-prompt text; if one does, update only its expected text and say so in your DONE message.
5. Count the added lines per file for your report.

## Out of scope
No Rust changes except a test's expected text if step 4 requires it. No changes to individual teammate persona files. Do not reword existing sections.

## Done looks like
Both base files carry the section with the example, README has the note, `horch teammates --check` is green, `cargo test -p horch-core teammates` passes. Reply with `horch done` listing the files and added line counts. Write your DONE message itself in STE.
