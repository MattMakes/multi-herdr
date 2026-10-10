---
name: improve-codebase-architecture
description: Use when asked to find architectural friction or deepening opportunities in a codebase. Scans for shallow modules, writes a visual HTML report of candidates, then works through the chosen one with the orchestrator.
---

# Improve Codebase Architecture

Surface architectural friction and propose **deepening opportunities**: refactors that turn shallow modules into deep ones. The aim is testability and AI-navigability.

In the fleet: you are a worker, so follow 4 rules. Spawn no subagent: walk the code yourself. Never ask the human: send a `QUESTION:` to the orchestrator and wait for the answer. Never open a browser: write the report in the project and send its path. Run no grilling skill: ask 1 `QUESTION:` per open decision.

This command is _informed_ by the project's domain model and built on a shared design vocabulary:

- Read the skill `codebase-design` for the architecture vocabulary (**module**, **interface**, **depth**, **seam**, **adapter**, **leverage**, **locality**) and its principles (the deletion test, "the interface is the test surface", "one adapter = hypothetical seam, two = real"). Use these terms exactly in every suggestion, and don't drift into "component," "service," "API," or "boundary."
- The domain language in `GLOSSARY.md` gives names to good seams; ADRs in `docs/adr/` record decisions this command should not re-litigate.

## Process

### 1. Explore

**Scope before you scan: YAGNI.** Deepening a module pays off by making future changes to it easier, so put extra weight on the parts of the codebase that have recently changed. Decide *where* to look before you look:

- If the task named a direction (a module, a subsystem, a pain point), take it, and skip the inference below.
- Otherwise, walk back a good stretch of the commit history (`git log --oneline`) to find the codebase's hot spots, the files and areas that keep coming up, and let those paths pull your attention first. If the changes are scattered with no clear hot spot, widen the net.

Read the project's domain glossary (`GLOSSARY.md`) and any ADRs in the area you're touching first. If there is no `GLOSSARY.md`, use the terms of the specs (`docs/specs/` here) as the domain language. If there is no ADR folder, there is nothing to re-litigate.

Then walk the codebase yourself. For a scope of more than about 5000 lines or 3 subsystems, send 1 `QUESTION:` to the orchestrator that asks for more workers, 1 area per worker, and wait. Don't follow rigid heuristics; explore organically and note where you experience friction:

- Where does understanding one concept require bouncing between many small modules?
- Where are modules **shallow**, with an interface nearly as complex as the implementation?
- Where have pure functions been extracted just for testability, but the real bugs hide in how they're called (no **locality**)?
- Where do tightly-coupled modules leak across their seams?
- Which parts of the codebase are untested, or hard to test through their current interface?

Apply the **deletion test** to anything you suspect is shallow: would deleting it concentrate complexity, or just move it? A "yes, concentrates" is the signal you want.

Classify the dependencies of each candidate with the categories in `codebase-design/DEEPENING.md`; the card shows the category as a tag. Before you put a defect in a card, find a caller that reaches it. If none does, mark it latent.

### 2. Present candidates as an HTML report

Write a self-contained HTML file to `ai_docs/reports/architecture-<YYYY-MM-DD>-<slug>.html` in the project, so each run gets a fresh file. Use the path that your plan names. If it names none, use the output of `date +%F` for the date. Do not open it: you have no browser for the human. Send its path in the `QUESTION:` below; the orchestrator shows it to the operator.

The report uses **Tailwind via CDN** for layout and styling, and **Mermaid via CDN** for diagrams where a graph/flow/sequence reliably communicates the structure. Mix Mermaid with hand-crafted CSS/SVG visuals: use Mermaid when relationships are graph-shaped (call graphs, dependencies, sequences), and hand-built divs/SVG when you want something more editorial (mass diagrams, cross-sections, call-graph collapses). Each candidate gets a **before/after visualisation**. Be visual.

For each candidate, render a card with:

- **Files**: which files/modules are involved
- **Problem**: why the current architecture is causing friction
- **Solution**: what moves behind which seam, in 1 sentence. No types and no method lists; those come in step 3.
- **Benefits**: explained in terms of locality and leverage, and how tests would improve
- **Before / After diagram**: side-by-side, custom-drawn, illustrating the shallowness and the deepening
- **Recommendation strength**: one of `Strong`, `Worth exploring`, `Speculative`, rendered as a badge

End the report with a **Top recommendation** section: which candidate you'd tackle first and why.

**Use GLOSSARY.md vocabulary for the domain, and the `codebase-design` vocabulary for the architecture.** If `GLOSSARY.md` defines "Order," talk about "the Order intake module," not "the FooBarHandler," and not "the Order service."

**ADR conflicts**: if a candidate contradicts an existing ADR, only surface it when the friction is real enough to warrant revisiting the ADR. Mark it clearly in the card (e.g. a warning callout: _"contradicts ADR-0007, but worth reopening because…"_). Don't list every theoretical refactor an ADR forbids.

See [HTML-REPORT.md](HTML-REPORT.md) for the full HTML scaffold, diagram patterns, and styling guidance.

Do NOT propose interfaces yet. After the file is written, send 1 `QUESTION:` to the orchestrator with the report path and the candidates by name and strength: "Which of these would you like to explore?" Then wait.

### 3. Question loop

Once the orchestrator picks a candidate, walk the decision tree with it, 1 `QUESTION:` per open decision, and wait for each answer: constraints, dependencies, the shape of the deepened module, what sits behind the seam, what tests survive. Ask at most 3 `QUESTION:` lines unless the plan says otherwise. Each `QUESTION:` gives the code facts with file:line, 2 to 4 options and your recommendation. Put independent decisions in 1 question. After the last answer, append a section 'Decisions so far' to the report: the shape of the deepened module, what sits behind the seam, which tests survive. If an answer asks for a change outside your plan's files, ask before you make it.

Side effects happen inline as decisions crystallize; keep the domain model current as you go:

- **Naming a deepened module after a concept not in `GLOSSARY.md`?** If your plan gives you `GLOSSARY.md`, add the term. If not, list the term in the report under 'Glossary terms to add', and name it in your `horch done` summary.
- **Sharpening a fuzzy term during the conversation?** Apply the same rule to the sharpened term.
- **The orchestrator rejects the candidate for a load-bearing reason?** Send 1 `QUESTION:` to the orchestrator: "Do you want an ADR for this rejection, so that later reviews do not suggest it again?" Write the ADR only if the answer gives you the file. Only ask when the reason would actually be needed by a future explorer to avoid re-suggesting the same thing; skip ephemeral reasons ("not worth it right now") and self-evident ones.
- **Want to explore alternative interfaces for the deepened module?** Read the skill `codebase-design` and use the pattern in `codebase-design/DESIGN-IT-TWICE.md`.
