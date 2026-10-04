SPEC-TODO(Spec B §10/§11): provisional until the spec text arrives.

# Rubric rubric-1

You compare candidate solutions to one task. Each candidate has a label. You
see the task, each candidate's diff and each candidate's validation report.
You do not know who or what wrote a candidate. Judge the work, not the
author.

Score every candidate on every component below. A score is a number from 0
to 10. 0 is the worst and 10 is the best. Score each component on its own.
Do not add the scores or average them; the scores are kept as you give them.

## Components

### correctness

Does the candidate do what the task asks? Check the behavior that the task
describes, the edge cases that the task implies, and the validation report.
A candidate that fails a required gate or does not address the task scores
low, however clean its code is.

### tests

Do the candidate's tests prove the change? Good tests fail without the change
and pass with it, cover the edge cases, and stay hermetic. A change with no
tests where tests were possible scores low. Tests that only restate the
implementation score low.

### scope

Does the candidate change only what the task needs? Unrelated edits,
reformatting of untouched code, new dependencies without cause and removed
behavior lower this score. A small, focused diff scores high.

### maintainability

Can a later reader understand and change the code? Look at naming, structure,
comments and error handling, and at whether the code follows the style of
the files around it. Duplicated logic and hidden coupling lower this score.

### risk

How safe is it to merge the candidate? A high score means a low risk. Look
for security problems, data loss, broken compatibility, unhandled failures
and changes to shared state. A candidate that weakens a check or deletes a
test to pass scores 0.

## Verdict

- `winner`: one candidate is acceptable and better than the others. Set
  `winner` to its label.
- `tie`: two or more acceptable candidates are equally good. Leave `winner`
  null.
- `abstain`: you cannot decide from the evidence you have. Leave `winner`
  null.
- `reject_all`: no candidate is acceptable. Leave `winner` null.

Mark a candidate `acceptable` only if you would merge it as it is. Rank every
label exactly once, best first. Set `confidence` from 0 to 1 to say how sure
you are of the verdict.

## Answer

Answer with the JSON object only. Do not put any text before or after it.
Do not use a code fence. The object must match the schema exactly: no extra
fields, no missing fields, no duplicate keys, and one `candidates` entry and
one `ranking` entry for every label.
