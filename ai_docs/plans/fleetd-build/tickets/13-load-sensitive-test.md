# No test of the workspace depends on the real load of the machine

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** light
**Check:** `cargo test -p horch --test w3_ledger_cli_pins && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo fmt --all --check`
**Own:** `crates/horch/tests/w3_ledger_cli_pins.rs`, other test files of `crates/horch/tests` and `crates/horch-e2e/tests` that have the same fault

## Goal

The test `spawn_checks_the_run_the_persona_and_the_replaced_worker` in
`crates/horch/tests/w3_ledger_cli_pins.rs` passes at each machine load, and no other test that
runs `horch spawn` reads the real load.

## Context

The worker of T-86 saw this test fail when the machine load was about 266 on 14 CPUs. The health
guard of `horch spawn` read the real load (`horch_core::runtime::machine`, from
`horch_platform::machine::load_average`) and held the spawn. At a normal load the test passes.
A test must not depend on the load of the machine.

## Steps

1. Reproduce first. Find how a test can give the machine facts to `horch spawn` today (a fault
   setting, an env source, a fake). Search the test helpers before you add something. Make the
   test fail with a high fake load, or show from the code why the real load reaches the guard.
2. Make the test give a fixed, low load through the existing test seam.
3. If no seam exists, do not add a production env variable on your own. Send QUESTION with 2
   options, then end your turn.
4. Search the other tests that run `horch spawn` or the health guard for the same fault. Fix
   each one the same way. Name each file in the done summary.
5. Run the Check.

## Constraints

- Before the commit, run `git remote -v` and `git config user.email`. The remote must be
  `git@git-alt:MattMakes/multi-herdr.git` and the email `m4ttplu5@gmail.com`. If not, send BLOCKED.
- No co-author line. Never write the em dash character. NEVER use `ANTHROPIC_API_KEY`.
- Do not use a shared `CARGO_TARGET_DIR`. Never install a development build. Each test uses
  temporary folders, never the live ledger.
- Do not make the guard weaker in production. Change only how a test gives the facts.
- If the GitHub SSH connection times out at `horch ledger finish`, run it again, 3 times maximum.

## Done when

The Check passes, and the done summary names the seam and each test file that changed.
