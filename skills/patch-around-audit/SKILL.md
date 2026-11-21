---
name: patch-around-audit
description: Use when asked to find code that keeps being patched around instead of fixed at its root - guards, special cases, flags and wrappers piled beside a comment, test or core block that nobody dared to change - or to fix such a site properly. Also use when a bug keeps coming back in the same function, when a comment or test defends behaviour that was meant to change, or when asked for a root-cause audit, a workaround audit or a stale-comment audit of any codebase, in any language.
---

# Patch-around audit

Find the places where a codebase was patched around instead of fixed, say
what the root cause is, and fix it at the root when the task says so.

## Why these sites exist

A comment that states intent ("must stay sorted", "legacy callers need
this", "do not change") reads as a constraint. So does a test that pins a
behaviour, and so does a core block that looks deliberate. An agent or a
hurried person who meets one while fixing a bug keeps it, and adds a guard,
a special case, a flag or a wrapper beside it. The next change does the
same. The result is an **anchor** that is wrong or out of date, with
**patches** layered around it, each one compensating for the anchor.

Two things follow. The anchor is often not marked as debt: no TODO, no
HACK, it looks like a rule. And you are subject to the same pull: while
you audit, the comments in front of you will read as constraints to you
too. Treat every claim in a comment as a statement to check, not a fact.

## Terms

- **Anchor**: the thing that was preserved. A comment, a docstring, a test
  or assertion, an instruction file line, or a core block of code.
- **Patch**: a later change that works around the anchor instead of
  changing it.
- **Site**: 1 anchor plus its patches. A site is the unit you report.
- **Contract**: what the code is supposed to do now. It comes from the
  task, a spec, a decision record, a ticket or the operator. It never
  comes from the anchor itself.

## Modes

- **Audit** (the default): find sites, judge them, write the report. Edit
  no code.
- **Fix**: the task names sites from a report, names a bug that keeps
  coming back, or says to fix what you find. Fix only sites whose
  contract is known. See "Fix a site".

If the task does not say, audit. When a fix task names 1 bug, do not
audit the repository first: build that 1 site (step 3), fix it, and list
any other site you passed on the way in 1 line each.

## 1. Set the scope

Use the paths or the diff the task gives. With none, take the whole
repository except vendored, generated and lock files. Note the tools the
repository has: test command, linter, and where specs and decisions live
(`docs/`, `specs/`, ADRs, tickets, `CHANGELOG`, instruction files such as
`CLAUDE.md` and `AGENTS.md`). You need them in step 3 to find the contract.

## 2. Collect candidates

Run 3 sweeps. Each is cheap and each finds sites the others miss. The
commands are in [references/history-commands.md](references/history-commands.md);
the code shapes are in [references/shapes.md](references/shapes.md).

1. **Comment sweep.** Comments, docstrings and test names that state a
   rule, a reason or a history: "must", "never", "always", "do not
   change", "keep", "legacy", "for backward compatibility", "workaround",
   "temporary", "for now", "special case", "for some reason",
   "previously", "used to". A comment that tells the history of a fix
   marks a place that was fixed more than once.
2. **History sweep.** Functions where commits keep adding lines around a
   core that does not change; files with many fix commits; a fix that
   follows a fix; a revert that follows a fix; a comment older than the
   code around it.
3. **Shape sweep.** Code that compensates: a value computed and then
   overridden, guards stacked before the real work, a flag that only
   skips a step, the same clean-up repeated at several call sites, a
   `safe_` / `_v2` / `_compat` twin, a caught error that is dropped, a
   test that asserts the workaround.

A place that 2 sweeps find is a strong candidate. In a large repository,
rank candidates by how many sweeps found them and how many commits touch
them, judge the top 20, and report how many you did not judge. A
repository with no history (a fresh copy, a squashed import) loses the
history sweep: say so in the report and lean on the other 2.

## 3. Build and judge each site

For each candidate, read the whole function and its callers, then its
history (`git log -L`). Write down:

- the anchor, quoted, and the claim it makes in your own words;
- each patch: the commit, the date, the lines, and what it compensates
  for;
- whether the claim is true of the code now. Check it: run the code, read
  the callers, write a 5-line probe. Mark each fact **checked** or
  **inferred**.

Then ask the 2 questions that separate a patched-around site from
ordinary code:

- **If you wrote this today, knowing everything the patches know, would
  it look like this?** If the honest answer is a much smaller function
  with no special cases, the patches are compensation.
- **If the anchor were corrected, which patches would become dead code?**
  Those are the patches of this site. A guard that would still be needed
  is not a patch; it is a rule of its own.

Give 1 verdict:

| Verdict | Meaning | What happens next |
|---|---|---|
| **live invariant** | The claim is true and the contract still wants it. The code around it is honest. | Leave it. List it only when a sweep ranked it high, so the reader knows it was checked. |
| **stale comment** | The code is right; the anchor describes something that is no longer there. | Correct or delete the anchor. Low risk. |
| **patched-around core** | The core is wrong or too narrow, and patches hide it. The contract is clear. | Root-cause fix. |
| **superseded intent** | The anchor defends behaviour that a later decision changed, and code is still bent around the old behaviour. | Root-cause fix, once the contract is confirmed. |

2 more outcomes are not verdicts:

- **Not a site**: validation at a trust boundary, a real edge case of the
  domain, or a workaround for a bug in someone else's code that still
  exists (check the version in use; when the upstream fix has shipped,
  the workaround is a stale anchor).
- **Intent unknown**: you can show the patching, but nothing you can read
  says whether the anchor's intent is still wanted. Do not guess. This
  becomes a question in the report.

You cannot decide intent. A decision that a behaviour should change
belongs to the operator; your part is to find the evidence that one was
made (a spec line, a ticket, a changelog entry, the task brief) and quote
it. Call a site superseded only with that evidence.

Do not manufacture sites. A codebase with 1 real site and a report that
says so is a good result; 10 weak findings teach the reader to ignore the
report.

## 4. Write the report

Write to the path the task names, or `ai_docs/reviews/patch-around-audit.md`;
if you cannot write files, give the report as your reply. The reader decides what to fix from the first screen, so the top holds the
counts and the questions; the evidence comes after. Use this structure:

```markdown
# Patch-around audit: <scope>

<N> sites. <K> ready to fix. <M> need a decision. <J> comment-only.
Candidates not judged: <count, and why>.

## Decisions needed
1. S3 `path:line` - <the question, answerable in one line>.
   If yes: <what stays>. If no: <what the fix removes>.

## Sites, worst first

### S1 `path:line` <function> - <verdict> (confidence: high|medium|low)
**Anchor** (<commit>, <date>):
> <the comment, test or block, quoted>
**It claims:** <one sentence>. **True now:** <yes | no | partly> (checked|inferred)
**Patches:**
| # | commit | date | where | compensates for |
|---|---|---|---|---|
**Root cause:** <one sentence: what is wrong at the anchor>
**Fix:** <what the core becomes; which patches are deleted; what the
comment becomes>
**Blast radius:** <callers, tests that pin old behaviour, other copies of
the claim>
**Cost of leaving it:** <the next bug this site will cause>

## Checked and left alone
<live invariants and not-a-site candidates a sweep ranked high, 1 line each>

## Method
<sweeps run, commands, what was unavailable>
```

Rank by consequence: number of patches, how recently the last one landed
(a site that is still growing outranks an old one), and blast radius.
Every site carries its evidence: a reader must be able to open the commit
and see the patch.

Send the orchestrator the "Decisions needed" list and the report path.
Keep working on what does not depend on the answers.

## Fix a site

Fix 1 site per commit, so each can be reviewed and reverted alone. Fix
only a site whose contract is known: a verdict of stale comment or
patched-around core, or a superseded or unknown site that the operator
has since answered.

1. **Write the contract down** in 1 to 3 sentences, with its source.
2. **Pin the wanted behaviour.** Write tests for what the contract says,
   including the cases each patch was added for. A test that pins the old
   behaviour is part of the site: change it with the code and say so.
3. **Rewrite the core** so it meets the contract directly. Work from the
   contract, not from the old code with pieces removed.
4. **Delete the patches, one at a time**, and run the tests after each.
   Tests that stay green show the patch was compensation. A test that
   goes red means the patch held a rule of its own: keep that rule, move
   it into the core, and note it.
5. **Correct the anchor.** The comment states what is true now and why,
   in the present tense, or it goes. No history in comments ("previously",
   "changed to fix"): history is what misled the last reader, and the
   commit message holds it.
6. **Hunt the copies.** Search for the same claim in other comments,
   docs, tests, instruction files and prompts. A copy left behind
   re-creates the site. An entry point that has the same gap and was
   never patched belongs to this site: fix it in the same commit.
7. **Verify.** Run the tests, and the linter when the repository has
   one. Report what you ran.
8. **Report the fix** in a few lines: the contract and its source, the
   root cause, what the core became, each patch removed (with the commit
   that added it), the anchor's new text, and what you ran.

The rule that created the site applies to your fix: when your change
contradicts a comment or a test, that comment or test is in scope for
the same change. If you find yourself adding a guard beside something
you believe is wrong, stop and fix the thing.

If a fix grows past the site (a public interface, a data format, many
callers), stop before the rewrite, report the size, and let the
orchestrator decide.

## Checklist

- [ ] Every site has a quoted anchor, a claim, and patches with commits.
- [ ] Each "true now" is marked checked or inferred.
- [ ] No verdict of superseded intent without quoted evidence of the
      decision; the rest are questions.
- [ ] The first screen of the report holds the counts and the questions.
- [ ] Audit mode: I edited no code. Fix mode: 1 site per commit, patches
      deleted, anchor corrected, copies searched.

## Worker coordination

Follow the assigned brief and repository instructions. For a missing
contract or a fix that outgrows its site, use `horch tell orchestrator`
with the question, the evidence and the site id. Continue with the sites
that do not depend on the answer. If messaging is unavailable, record the
question in the report. Do not wait on an interactive human prompt.
