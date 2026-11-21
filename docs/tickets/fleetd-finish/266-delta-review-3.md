# Delta review 3: the product changes of the last wave (project facts in START.md, Windows channel, Windows paths and file owners, the conformance hold, leftover trees)

**Persona:** primary `codex-sol`, fallback `opus-verify` · **Parallel:** yes
**Kind:** report
**Review:** none
**Check:** `test -s docs/specs/fleetd/reviews/delta-review-3-windows-facts-and-landings.md && grep -q '^## Findings' docs/specs/fleetd/reviews/delta-review-3-windows-facts-and-landings.md && grep -q '^## Verdict' docs/specs/fleetd/reviews/delta-review-3-windows-facts-and-landings.md && cargo fmt --all --check`
**Own:** `docs/specs/fleetd/reviews/delta-review-3-windows-facts-and-landings.md`

## Why

Done criterion 4 of the run: each change gets 1 review by a worker that did not write it. These tasks landed after delta review 2 and have no review: T-404, T-407, T-406, T-409, T-384, T-289, T-397, T-398 (and T-408 if it has landed when you start). Find each with `git log --oneline --grep '(T-<n>)' fleet/fleetd-finish` after `git fetch`. Method and report form: the same as `docs/specs/fleetd/reviews/delta-review-2-fixes-of-findings.md` (read it first): sections `## Scope`, `## Findings` (each with an id D266-F<n>, a severity Critical / Important / Minor, the file and line, a failure scenario, a proposed fix), `## Verdict`.

## Look hard at

1. T-404: project facts go into START.md as data. Can a fact close the fenced block or pass as an instruction (backtick runs, a line that starts with a heading, CR, very long lines, the bound of 100 facts and 2048 bytes)? Is the facts snapshot of the attempt immutable, and are the 2 renders (the pass and `horch spawn`) equal bytes when a fact is recorded between them? Does a worker see facts of another project?
2. T-407: the Windows command channel is the operator boundary (SR-8). Each server step for 1 client has a deadline; no client can block the accept loop; the peer check still runs before a request is read; each `unsafe` block is sound (handle and buffer lifetimes, cancel of overlapped I/O before the buffer is dropped).
3. T-406: the job result file and its folder are made owner-only on Windows; `horch_platform::path::canonicalize`: does the plain form change which file a path names (symlinks, UNC paths, long paths over 260 characters, a drive-relative path)? Is a path compare for a security check (a path inside a root) still done on 1 form?
4. T-409: where the product now prints forward slashes or LF, is that text read back as a path by any code on Windows?
5. T-384: the hold of dispatch for a profile with a "not proved" report: can a stale or forged report file release or hold a profile? T-289: can the removal of a released gate tree or a dropped scratch tree remove a folder that fleetd did not make?
6. The pass rule in each changed pass step: no SQL statement for each row; a pass never waits for a job.

## Rules

Read only: change no file except the report. Run no test that needs `scripts/dev-sandbox.sh`; you may run `cargo check` and read code. Never read, set or pass `ANTHROPIC_API_KEY`. Write 1 `horch ledger progress` line each 5 minutes. After the comment sweep run `horch ledger finish` again at once.

## Done when

The Check passes; each finding has its file, line and scenario; the verdict says if a Critical or Important finding is open.
