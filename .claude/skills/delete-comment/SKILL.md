---
name: delete-comment
description: Use when the operator types /delete-comment, or asks to delete, sweep, audit or categorise the code comments of this repository (all of it, or the paths they name). Sorts every comment into tautology, stale, excuse or decision record; deletes all of them; files a GitHub issue for each hack an excuse was covering; moves each decision into an ADR; proves that only comments changed; then lowers the comment-lint baseline so the count cannot rise again.
---

# /delete-comment - delete comments, keep what they knew

Code in this repository carries no comments
([ADR 0004](../../../docs/adr/0004-no-comments-in-code.md)). Old files still
hold some. `scripts/comments/baseline.json` records how many lines of them
each file may hold, and the gate fails when a file holds more. This skill
removes a batch of the old ones and lowers the baseline.

A comment is deleted in every case. The category only decides what else
happens, so that nothing a comment knew is lost by accident.

The tool is `scripts/comments/comment_lint.py` (stdlib Python). Its
subcommands are `check`, `list`, `strip`, `verify` and `estimate`.

## 1. Settle the scope

- The operator named paths: those are the scope.
- No paths: run `python3 scripts/comments/comment_lint.py estimate` and read
  `scripts/comments/baseline.json`. Propose the files with the highest
  counts, or one directory, and say how many comment lines that is. A scope
  of everything is fine when the operator asks for it.
- Start from a clean tree (`git status --short` prints nothing) on a branch
  that is not `main`. Step 5 compares against `HEAD`.

Out of scope, always: files that `skills/copied.json` lists as copied, files
git does not track, Markdown, and Python docstrings (a docstring is a string
that programs read through `__doc__`).

## 2. List the comments

```
python3 scripts/comments/comment_lint.py list <paths> > "$SCRATCH/comments.jsonl"
```

Each line is one comment: `file`, `line`, `lines`, `text`. The list leaves
out the comments a program reads, and `strip` leaves them in the file: a
shebang, a lint or type-checker directive, a `SAFETY:` note (clippy's
`undocumented_unsafe_blocks` demands it), a licence header, the `just --list`
line above a recipe, the version behind a pinned action, and a `///` that
clap prints as help text. Do not delete those by hand either.

Read each comment with the code under it. Adjacent lines of one comment are
one decision.

For more than about 500 comments, split the list by directory and give each
part to a separate reviewer with the categories below. A reviewer only
classifies; it edits nothing.

## 3. Give every comment exactly one category

| Category | It is | Besides deleting it |
|---|---|---|
| **Tautology** | Repeats what the name, the signature, the types, the next lines, the test's name or a cited spec ID (`TEL-05`, `ARC-29`: `docs/specs/` is the record) already say. Banners, labels and "what this does" narration. The default. | nothing |
| **Stale** | Talks about code, names, phases, tickets or history that is no longer true or no longer matters: "used to", "the old X", a removed symbol, a finished migration. When in doubt whether the symbol exists, grep. | nothing |
| **Excuse** | Lets a hack in this project's own code stand: TODO, FIXME, "for now", "workaround", "keep in sync with X", a swallowed error, a sleep, retry or timeout that only the comment justifies, a justified lint suppression, "do not touch", dead code kept in case. A code change would make the comment unnecessary. | file an issue (step 4a) |
| **Decision record** | An important, non-obvious decision, constraint or invariant whose reason is not in the code: why this way and not the obvious way, a fact about an outside program, OS or protocol, a security or data-safety rule, a lesson from an incident. Would a competent engineer plausibly break something without it? If it is only nice to know, it is a tautology. | move it into an ADR (step 4b) |

Tie-breaks:

- A workaround for an outside program that the project cannot change is a
  decision record. A workaround for the project's own shortcoming is an
  excuse.
- A comment that holds narration and 1 real decision is a decision record.
- A decision that `docs/adr/` or `docs/specs/` already records needs no new
  record: check before you write one.
- Be strict with both rare categories. But a real hack or a real decision
  that you file under tautology is lost when the comment goes.

Keep a tally per category. The report in step 8 needs it.

## 4. Act on the two rare categories before you delete

### 4a. Excuse: file an issue for the hack

1 issue per hack, not per comment: several comments that excuse the same
shortcut share 1 issue. Search the open issues first
(`gh api -X GET search/issues -f q='repo:<owner>/<repo> is:issue <symbol>'`)
and add to a match instead of filing a duplicate.

- Title, in the style of the tracker: `<path>: <symbol> <what is wrong>`,
  for example ``crates/horch-core/src/sweep.rs: `codex_slug` copies the
  role-slug rule of `build_private_home` ``.
- Label: `enhancement`.
- Body: what the code does today, why that is a hack, the fix you propose,
  and the deleted comment quoted in full with its `path:line`. The quote is
  the only copy once the comment is gone.

Filing an issue is outward-facing. The operator asked for it by invoking
this skill; file them, and list the numbers in the report.

Do not fix the hack in this change. This skill changes comments only.

### 4b. Decision record: move it into an ADR

- ADRs live in `docs/adr/NNNN-kebab-title.md`. Take the next free number.
- 1 decision per ADR. Comments that state the same decision (it often
  appears in the code and again in its test) share 1 ADR. Related
  constraints of one decision go under `## Consequences`.
- Follow the form of the existing ADRs: a title that states the decision, a
  paragraph that says what was decided and why, then `## Consequences`.
  Name the module and the symbol, since the code no longer points back.
- Keep every concrete fact from the comment: numbers, version numbers,
  dates, the outside program's behaviour, the incident.
- Write it in plain present tense. Do not write "the comment said".

## 5. Delete, and prove that only comments changed

```
python3 scripts/comments/comment_lint.py strip <paths>
python3 scripts/comments/comment_lint.py verify
cargo fmt --all
```

`verify` compares each changed file with `HEAD` after it removes the
comments from both sides. It must report `0 with a code change`. Run it
before `cargo fmt`: the formatter may join lines that a comment kept apart,
and `verify` would then report a code change.

Never trade a comment for a shorter comment, and never move its text into a
string, a `#[doc = "..."]` attribute or a docstring to get past the lint.

## 6. Repair what leaned on a comment

Run the gate's checks for what you touched:

- Rust: `cargo clippy --workspace --all-targets -- -D warnings`,
  `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace`,
  `cargo test --workspace`.
- Python: the unit tests next to the script, where they exist.
- Shell: `bash -n <file>`. A branch that held only a comment is now empty
  and needs `:`.

A test that read a comment's text (a doc test, a scan for a marker) fails
here. Fix the test or the code so that it no longer depends on prose.

## 7. Lower the baseline

```
python3 scripts/comments/comment_lint.py check --update-baseline
python3 scripts/comments/comment_lint.py check
```

`--update-baseline` only lowers counts and drops files that reached 0. It
never raises one. If `check` still fails, a comment was added somewhere:
delete it.

## 8. Report

Give the operator:

- the tally: tautology, stale, excuse, decision record;
- the issues filed (numbers and titles) and the ADRs written;
- comment lines before and after (`estimate`), and lines the diff removed
  (`git diff --shortstat`);
- what is left, and anything that fit no category.

Then commit. A sweep is not visible to a user of horch, so it gets no
`CHANGELOG.md` line. To open the pull request, use `/push-pr`.
