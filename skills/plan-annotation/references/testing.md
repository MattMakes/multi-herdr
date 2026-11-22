# Testing the helper

The helper and its test live as two fenced blocks in
`script-review-helper.md`. Testing means: extract both blocks into one
temporary folder, then run the test file with `python3`.

## What you need

- `python3` 3.10 or newer, standard library only.
- `awk` for the extraction.
- A loopback interface. No other network is used.
- A writable temporary folder. The test creates its files with the `tempfile`
  module, which honours `TMPDIR`, and removes them when it ends.

No browser is needed. The test replaces the browser start with a stand-in
that records the page address.

## Steps

1. Create an empty temporary folder.
2. Extract the helper and the test into it with the two `awk` filters shown
   at the top of `script-review-helper.md`. The first writes
   `review_helper.py`, the second writes `test_review_helper.py`.
3. Run `python3 test_review_helper.py` in that folder. Add `-v` to see each
   test name.
4. Exit status 0 and a closing line `OK` mean that every test passed.

The test finds the helper next to itself. To test a helper stored elsewhere,
put its location into the environment variable `REVIEW_HELPER`.

## What the test covers

| Area | Checks |
|---|---|
| ordinary sessions | comments return with their passage; no approval anywhere in the page; an approve message is refused; a closed session prints nothing; `--json` changes the format only; a comment that asks for outside action returns as text and no file changes |
| gate sessions | dismissed, approved with a note, approved with no note, changes requested; the reviewed file stays byte for byte the same |
| targets | unknown target ends with status 3 and opens no page, also when several files resemble the wording; a web address is refused with no socket use; binary, legacy-encoded, oversized and empty-folder targets are refused; a name with shell punctuation arrives as one argument |
| folders | only reviewable documents are listed; dot entries, binary files and links that lead outside are absent and cannot be fetched |
| serving | loopback address only; requests lacking the key, with a wrong or doubled key, or with a foreign `Host` get status 403 and no text; malformed messages leave the session open; listening stops when the session ends |
| page safety | HTML is reduced to text; a strict content security policy is sent; markup inside a file name stays inert |
| endings | leaving the page counts as dismissed after the grace time; a reload keeps the session; interrupt and terminate signals yield dismissed |

The test exercises the helper. How the agent acts on a result is laid down in
the skill text and in `worked-examples.md`, and no script can check it.

## When a test fails

- "the helper recorded no page address": the child process did not start.
  Check the `python3` version.
- A failure in the folder test on a system without links: the link cases are
  skipped where links cannot be created, so look at the remaining assertions.
- Timeouts: the loopback interface may be blocked. The test needs to connect
  to `127.0.0.1` on a port the system assigns.
