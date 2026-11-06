# 40: The pr step waits for GitHub's PR head instead of failing on a fresh push

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p horch-core -- finish pr trf_50`
**Own:** `crates/horch-core/src/taskledger/finish/pr.rs`

Spec: `docs/specs/trace-fixes.md` (goal: no silent stop, no wasted retries; conventions in section 8). Added by the orchestrator.
Waits for: nothing.

## Evidence

In run `trace-fixes`, the first `pr` step failed for T-4, T-5, T-8, T-10, T-11, T-12, T-19, T-34, T-35 and T-37 with
`PR head behind (PR #339 head <previous landed commit> does not contain <this commit>: the run branch is not pushed ...)`.
Each time the reported head was the commit landed just before, and `horch ledger finish` passed on the retry. Read
them with: `sqlite3 ~/.horch/ledger.sqlite3 "select at, text from events where kind='step_failed' and text like '%PR head behind%'"` (read only).
`verdict` in `crates/horch-core/src/taskledger/finish/pr.rs` (near line 193) trusts `gh pr view --json headRefOid`
read right after the land pushed. GitHub updates a PR's head a few seconds after a push, so the step reads the old head.
The message then blames a missing push, which is false.

## Requirements

- **TRF-50**: The pr step passes when the remote run branch holds the landed commit, even while GitHub's PR head lags. It fails "PR head behind" only when the remote branch does not hold the commit. Test names start with `trf_50_`.

## Steps

1. Read `step`, `sync_pr_body` and `verdict` in `pr.rs`, and find where land pushes the run branch.
2. When `view.head` does not hold `landed`, read the remote branch tip with `git ls-remote <remote> refs/heads/<run_branch>`.
   - If the remote tip is `landed` or holds it (`git merge-base --is-ancestor`, fetch first if the object is missing): re-read `gh pr view` up to 6 times, 5 s apart. Pass as soon as the PR head holds it. If GitHub still lags after 30 s, pass anyway with a NOTE in the outcome detail that names both heads, because the branch is correct and GitHub will catch up.
   - If the remote tip does not hold it: keep today's failure, but say what is true: the remote run branch `<tip>` does not hold `<landed>`.
3. Make the wait and the count injectable so tests run with no real sleep.
4. Tests with the fake `gh` and a real temp git remote:
   - `trf_50_lagging_pr_head_passes_when_the_remote_branch_holds_the_commit`;
   - `trf_50_pr_head_catches_up_on_a_later_read`;
   - `trf_50_unpushed_commit_still_fails_and_names_the_remote_tip`.

Warning: never run a branch-built horch against the shared ledger. Set `HORCH_LEDGER_DB` to a scratch file for any manual run.

## Done when

The check passes.

## Report

Write `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/40.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
