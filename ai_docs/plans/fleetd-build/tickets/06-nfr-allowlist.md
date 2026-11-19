# Allow the workspace crate horch-platform in the dependency tests

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo test -p horch-core --test nfr -- nfr_05_no_new_runtime_crates nfr_06_dependency_allowlist`
**Own:** `crates/horch-core/tests/nfr.rs`, `scripts/pl6/`

## Goal

The tests `nfr_05_no_new_runtime_crates` and `nfr_06_dependency_allowlist` of
`crates/horch-core/tests/nfr.rs` pass on the run branch. Today they fail: Phase P of fleetd added
the leaf crate `crates/horch-platform` as a dependency of the other workspace crates
(`docs/specs/fleetd/design.md`, Migration plan, Phase P), and the lists of these 2 tests do not
know it.

## Steps

1. Run the Check. See the 2 tests fail with "gained 'horch-platform'".
2. Add `horch-platform` to each list for each crate that depends on it now. If a test also names
   a crate that `horch-platform` brought as a new direct dependency of a workspace crate, add it.
   Add no other entry. Keep the rule of each test.
3. `scripts/pl6/` records the result of each test before the move of Phase P (requirement PL-6).
   Read its scripts and comments. If its baseline records these 2 tests as `ok`, it stays correct
   after this fix: change nothing there. If it records them as `failed`, correct the baseline for
   these 2 tests only, by the method that the script documents.
4. Run the Check. Commit.

## Constraints

- Before the commit, run `git remote -v` and `git config user.email`. The remote must be
  `git@git-alt:MattMakes/multi-herdr.git` and the email `m4ttplu5@gmail.com`. If not, send BLOCKED.
- No co-author line. Never write the em dash character. NEVER use `ANTHROPIC_API_KEY`.
- Do not use a shared `CARGO_TARGET_DIR`. Never install a development build.
- If the GitHub SSH connection times out at `horch ledger finish`, run it again, 3 times maximum.

## Done when

The Check passes.
