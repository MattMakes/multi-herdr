# Gates: Phase-scoped skills

OWNS: skills/**, crates/**, teammates/**, README.md, docs/**, ai_docs/**
Scope: Portable native skill integration for five harnesses.

- [x] G1: Portable skills validate and resolve
  CHECK: cargo test -p horch-core skills::tests
  EXPECT: test result: ok
  EVIDENCE: exit=0; shell=/bin/sh; cwd=/Users/mascott/projects/multi-herdr; path=b459dde41ae9/39 entries; output=Running unittests src/lib.rs (target/debug/deps/horch_core-53efaae43afb2b55) | Running tests/golden_prompts.rs (target/debug/deps/golden_prompts-f9ece253ff3eb59a)
- [x] G2: Native adapter launch contracts pass
  CHECK: cargo test -p horch-core launch::tests
  EXPECT: test result: ok
  EVIDENCE: exit=0; shell=/bin/sh; cwd=/Users/mascott/projects/multi-herdr; path=b459dde41ae9/39 entries; output=Running unittests src/lib.rs (target/debug/deps/horch_core-53efaae43afb2b55) | Running tests/golden_prompts.rs (target/debug/deps/golden_prompts-f9ece253ff3eb59a)
- [x] G3: Phase data survives serialization and selection
  CHECK: cargo test -p horch-core phase
  EXPECT: test result: ok
  EVIDENCE: exit=0; shell=/bin/sh; cwd=/Users/mascott/projects/multi-herdr; path=b459dde41ae9/39 entries; output=Running unittests src/lib.rs (target/debug/deps/horch_core-53efaae43afb2b55) | Running tests/golden_prompts.rs (target/debug/deps/golden_prompts-f9ece253ff3eb59a)
- [x] G4: Workspace regressions pass
  CHECK: cargo test --workspace
  EXPECT: test result: ok
  EVIDENCE: exit=0; shell=/bin/sh; cwd=/Users/mascott/projects/multi-herdr; path=b459dde41ae9/39 entries; output=Running tests/golden_prompts.rs (target/debug/deps/golden_prompts-98f533d49fe7dcb0) | Doc-tests horch_core
