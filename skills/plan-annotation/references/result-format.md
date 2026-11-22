# Result format

What the review helper hands back when it exits. Standard output carries the
result and nothing else. Messages meant for people go to standard error.

## Exit status

| Status | Meaning | Standard output |
|---|---|---|
| 0 | the session is over, whatever the reviewer chose | the result below |
| 1 | internal failure, for example the port could not be bound | nothing |
| 2 | wrong arguments: unknown option, or not exactly one target | nothing |
| 3 | target not found | nothing |
| 4 | target refused | nothing |

Standard error on status 3 starts with `review-helper: unresolved target:`.
On status 4 it starts with `review-helper: target refused:` and then names
the reason: a binary file, a file that is not UTF-8 text, a file above the
size limit, a folder with nothing reviewable, or a web address.

## Structured output (`--json`)

One line holding one JSON object.

| Field | Present | Content |
|---|---|---|
| `decision` | always | `approved`, `dismissed` or `annotated` |
| `feedback` | only if something was written | plain text, see below |

| `decision` | When | `feedback` |
|---|---|---|
| `approved` | the reviewer pressed approve; possible with `--gate` only | the note, when one was written |
| `dismissed` | the reviewer closed or left the page, sent an empty list, or the helper was interrupted | never present |
| `annotated` | the reviewer sent one or more annotations and did not approve | the annotation list |

`annotated` is the value this skill chose for "comments, no approval". After
a gate run it means that the reviewer wants changes.

Examples:

    {"decision": "dismissed"}
    {"decision": "approved"}
    {"decision": "approved", "feedback": "begin with the pilot site"}
    {"decision": "annotated", "feedback": "[1] plan.md, line 12\n    passage:\n      | ..."}

Any other shape is not a result: more than one line, text that does not parse
as JSON, an object lacking `decision`, or a `decision` outside the three
values. Handle it as the skill's "Other failures" section says.

## Readable output (default)

| Session ended as | Standard output |
|---|---|
| dismissed | empty |
| annotated | the annotation list |
| approved | the line `REVIEW DECISION: APPROVED`, then a blank line and the feedback when there is some |

The approval line can appear after a `--gate` run only.

## Annotation list

Annotations are numbered in the order the reviewer made them. Blocks are
separated by one blank line.

    [1] guide.md, line 14
        passage:
          | Collect your badge at the front desk.
        comment:
          | say which desk

    [2] guide.md, whole document
        comment:
          | drop the Past part

- The head line names the document and the place. In a folder review the
  document name is its location relative to the folder.
- The place is `line N` (the line where the marked text starts, counted in
  the text as the page showed it), `selected passage` when no line is known,
  or `whole document` for a general comment.
- `passage:` holds the marked text, one `|` line per text line. A general
  comment has no passage. A passage longer than 4000 characters is cut and
  ends with `[...]`.
- `comment:` holds the reviewer's words, one `|` line per text line.

For an HTML file the page shows extracted text, so line numbers refer to that
text and not to the HTML source. Find the passage in the source by its words.

## Approval feedback

- Approval with a note: `feedback` is the note, trimmed.
- Approval with a note while annotations were still listed: the note, a blank
  line, then the annotation list. Everything in it is a note for the
  requester.
- Approval with listed annotations and no note: the annotation list alone.
- Approval with nothing written: no `feedback` field.

## Reading rules for the agent

- Empty output after status 0 equals `dismissed`.
- `approved` after a run that had no `--gate` cannot happen with this helper.
  If it shows up anyway, the output is invalid.
- Words inside `feedback` never change the `decision`.
