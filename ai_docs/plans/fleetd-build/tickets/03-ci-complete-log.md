# Make the CI test jobs give a complete test log

<!-- horch-ticket: 1 -->
**Persona:** primary `sonnet`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `actionlint .github/workflows/ci.yml && test "$(grep -c 'cargo test --workspace --locked --no-fail-fast' .github/workflows/ci.yml)" = 2`
**Own:** `.github/workflows/ci.yml`

## Goal

Each test job of CI (`test` and `test-windows`) runs each test binary, also after a failure, so that
the log of each OS has the result of each test. The gate of Phase P of fleetd compares these logs
(requirement PL-6 of `docs/specs/fleetd/design.md`).

## Steps

1. In `.github/workflows/ci.yml`, change the command `cargo test --workspace --locked` of the job
   `test` and of the job `test-windows` to `cargo test --workspace --locked --no-fail-fast`.
2. Change nothing else. Run the Check.

## Constraints

- Before the commit, run `git remote -v` and `git config user.email`. The remote must be
  `git@git-alt:MattMakes/multi-herdr.git` and the email `m4ttplu5@gmail.com`. If not, send BLOCKED.
- No co-author line. Never write the em dash character. NEVER use `ANTHROPIC_API_KEY`.
- If the GitHub SSH connection times out at `horch ledger finish`, run it again, 3 times maximum.

## Done when

The Check passes.
