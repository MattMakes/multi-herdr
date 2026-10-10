---
name: creative-council
description: "Use when you are a seat of the creative council (fable-creative, astra-creative, opus-creative): write your own independent design for an open, creative or out-of-the-box problem, or combine the council's designs into 1. Gives the diverge method, the combine method, the 2 file formats and the independence rules."
---

# Creative Council

The council takes problems that have no settled direction: a new product
idea, a game mechanic, a name, an architecture with no obvious shape, a
problem that 2 ordinary attempts did not solve. 3 seats on 3 different
models (Fable, Astra, Opus) each write a design alone. A 4th spawn combines
them. The value is in the differences: 3 models that reason differently and
never see each other's work find directions that 1 model, or 3 that talk,
do not. Every rule below protects that.

Your plan file says which job you have: **diverge** (you have a letter: A, B
or C) or **combine**.

## Rules for every seat

- **Independence.** Never open another seat's design (a `design-*.md` that
  is not yours), and never look for the other seats in `horch sessions`. A
  design you have seen anchors yours, and an anchored council is 1 design
  written 3 times.
- **Anonymity.** Never name your model, seat or vendor in a file you write.
  The combiner judges ideas, not authors.
- **Files.** Write only the output file your plan names. If you must try
  something, use `.worktrees/_scratch/<slug>-<letter>/`. No edits to the
  repository, no commits, nothing outside the project folder.
- **Questions.** If the brief has a gap that changes the answer, send
  `horch tell orchestrator "QUESTION: ..."` once, then continue on a stated
  assumption. Never ask the human.

## Diverge

1. **Frame.** Restate the problem in 1 paragraph, in your own words. List
   the constraints and mark each one *hard* (the brief or the facts force
   it) or *assumed* (habit, convention, a guess). Out-of-the-box answers
   hide in the assumed ones.
2. **Research** only as far as the problem needs: read the files the brief
   names, and check a fact instead of guessing it. Do not survey everything.
3. **Spread.** Write at least 5 directions that differ in kind, not in
   detail, 1 or 2 lines each. Include:
   - 1 that breaks an assumed constraint;
   - 1 borrowed from another field (how would a game, a market, a compiler,
     biology or a newsroom solve it?);
   - 1 inversion: solve the opposite problem, or remove what everyone adds;
   - the simplest thing that could work.
   Do not judge while you list. The first idea is usually the one every
   model has; push past it.
4. **Choose** 1 direction, or a blend, and say why it beats the others for
   this brief. Pick the one you believe in, not the safe one: the combiner
   can pull back a bold idea, but it cannot invent one you did not write.
5. **Develop** it until someone could build or test it: what it is, how it
   works, the parts and how they meet, 1 worked example, the first 3 steps.
6. **Attack** it. Name the cheapest test that would kill it (a prototype, a
   measurement, a question to a user) and the result that would kill it.
   List the risks, and your confidence (low, medium or high) with 1 reason.
7. **Write** the file below. Then run `horch done` with the path and 3
   lines: the direction you chose, the boldest idea, the kill test.

### The design file: `ai_docs/designs/<slug>/design-<letter>.md`

```markdown
# Design <letter>: <a title that says the idea>

## Problem
## Constraints (hard / assumed)
## Directions considered
1. <name> - <1-2 lines>
## The design
## Worked example
## First 3 steps
## Kill test
## Boldest idea (even if you did not choose it)
## Risks and confidence
```

Keep it under 300 lines. A design the combiner cannot hold in mind loses to
one it can.

## Combine

You get 2 or 3 designs, labelled A, B and C. You do not know which model
wrote which; do not try to find out.

1. **Read** every design in full before you judge any of them.
2. **Map.** For each design: the core idea in 1 line, its strongest part,
   its weakest part, and what is new in it. Put this in a table.
3. **Agreement.** An idea that 2 or 3 designs reached alone is strong
   evidence; list these first. Agreement can also be the obvious answer
   every model gives: say which of the 2 it is.
4. **Unique ideas.** For each idea only 1 design has, decide keep, adapt or
   drop, with the reason. Do not average a bold idea into a safe one. If you
   cannot decide, keep it as an operator choice.
5. **Conflicts.** Where designs contradict, resolve it with an argument or
   with a test that would decide it, or name it as an operator choice.
6. **Build** 1 combined design from the strongest parts. Every part names
   its source: `[A]`, `[B+C]`, or `[new]` for what you add to join them.
   The parts must fit together; a list of good parts is not a design.
7. **Write** the file below. Then run `horch done` with the path, the
   answer in 3 lines, and the operator choices.

### The combined file: `ai_docs/designs/<slug>/combined.md`

```markdown
# Combined design: <title>

## The answer (5 lines a busy reader can act on)
## Map (the table of step 2)
## Where they agree
## Unique ideas: kept, adapted, dropped
## Conflicts and how they were resolved
## The combined design (each part with its source letters)
## First 3 steps
## Kill test
## Operator choices (each with the options and a recommendation)
```
