# Report: F2 trust-preflight

Unit: F2 `trust-preflight`. Single-branch work on `design-skills`. Worker: opus-55.

## Outcome

A dataset round no longer stalls at a harness trust dialog.

1. Preflight has a new check, PRE-14. It refuses a round (exit 4, before any
   worktree or model call) when a Claude or Codex candidate has not trusted
   the main repository root. The detail prints the one-time command.
2. The coordinator reads each young candidate pane. If the pane shows a trust
   dialog, the coordinator ends the candidate at the next look with
   `Failed(Cancelled{reason: "trust_dialog"})` and closes the pane. It does
   not wait for the deadline (default 3600 s).

The spec is the dataset design §3.2 (row PRE-14) and §4.11.3. §4.11.1 now
lists 14 checks, and PRE-13 is only "herdr reachable; horch found".

## Evidence (L1, opus-61, claude 2.1.289, codex-cli 0.160.0)

- Claude: `~/.claude.json`, `projects["<root>"].hasTrustDialogAccepted: true`.
  The key is the canonical main repository root (`/private/tmp/...`).
- Codex: `$CODEX_HOME/config.toml`, `[projects."<root>"]` with
  `trust_level = "trusted"`.
- A trusted parent folder does not trust a separate repository under it.
  Linked worktrees of a trusted repository do not prompt.
- I confirmed the dialog texts in the installed binaries with `strings`:
  - claude: "Quick safety check: Is this a project you created or one you
    trust?" and "Yes, I trust this folder". The capital letter changes
    between versions, so the match ignores case.
  - codex: "Trusting will apply to the repository root" and "Trust this
    folder? Codex can read, edit, and run files here".

## Other harnesses

| Harness | Trust step | What horch does |
|---|---|---|
| opencode | none (no trust text in the binary) | nothing |
| pi | none (no trust text in the bundle) | nothing |
| prime | none known | nothing |
| antigravity (agy) | yes: "Do you trust the contents of this project?", on the first launch in each directory (teammates/antigravity.md) | PRE-14 warns (trust unknown); the pane detection ends a stalled pane |

agy is not installed on this Mac, so I could not read its store.

## Files changed

- `crates/horch-core/src/competition/preflight.rs`: `TrustState`,
  `HarnessTrust`, `asks_for_trust`, `claude_trust`, `codex_trust` (a small
  line parser, no new crate), `trust_fix`, `pre_14_trust`. `PreflightPlan`
  has `trust_root` and `trust` in place of `trusted_parents`. PRE-13 has no
  trust part now.
- `crates/horch/src/dataset/preflight.rs`: `main_root` (linked worktree to
  main root), `harness_trust` (reads the stores, keeps only the verdict).
- `crates/horch-core/src/competition/observe.rs`: `TRUST_DIALOGS` (the
  strings in one place), `trust_dialog`, `normalize_screen`,
  `CANCELLED_TRUST`, `TRUST_DIALOG_WINDOW_S` = 300 s.
- `crates/horch-core/src/competition/coordinator.rs`: `observe` reads the
  pane of a live candidate younger than 300 s.
- `crates/horch-e2e/src/harness.rs`: `with_git` writes both trust entries
  into the sealed home (`trust_project`); `untrust_project` removes them.
- Tests: `crates/horch-core/tests/preflight.rs`,
  `crates/horch-core/tests/coordinator.rs`, `crates/horch-e2e/tests/dataset.rs`.
- Design: `ai_docs/designs/2026-10-02-dataset-competition-design.md`.

## Tests added

| Test | What it pins |
|---|---|
| `pre_14_claude_store` | trusted, parent-only, missing entry, canonical path, missing file (untrusted), broken JSON (unknown); no file content in the reason |
| `pre_14_codex_store` | basic and literal TOML strings, other tables, `untrusted`, empty and missing file |
| `pre_14_trusted_root_passes`, `pre_14_untrusted_root_refuses_with_the_fix` | the PRE-14 pass / fail / warn rule and the fix text |
| `pre_14_fix_commands_quote_the_root` | shell quoting; `agy` binary name |
| `pre_14_untrusted_repo_refuses_before_worktree_or_model` (e2e) | exit 4, PRE-14 failed, the fix printed, no worktree, no agent launch |
| `trust_dialog_matches_the_harness_dialogs` | the LA-7 screens match when wrapped and boxed; ordinary output does not |
| `pre_14_trust_dialog_pane_ends_the_candidate_at_once` | a pane at the dialog ends as `Cancelled{trust_dialog}` within the test's 50 ticks, not at the 1-hour deadline. It fails without the detection. |
| `main_root_of_a_linked_worktree_is_the_main_repository` | worktree `.git` file → main root; a submodule stays its own root |

## Decisions

1. A missing store file means "untrusted", so PRE-14 fails. A harness that
   never ran on this machine has not trusted anything. A store that horch
   cannot parse or read means "unknown", so PRE-14 only warns. A parser
   fault must not block the operator.
2. horch never writes a trust store. The operator accepts trust once, with
   the printed command.
3. The pane is read only during the first 300 s after the spawn. The dialog
   is the first screen. Later, an agent can print the same words, for
   example when it works on this repository.
4. The blocked candidate records `Cancelled{reason: "trust_dialog"}`. I used
   this failure kind because it already exists, so the event and execution
   formats do not change. The preflight report keeps `schema_version`
   "1.0.0": it has 1 more check row, and its fields do not change.

## Gotchas

- I ran `cargo fmt -p horch -p horch-core` once, early. Other workers had
  uncommitted edits in `evaluation/scheduler.rs`, `harness/claude.rs` and
  `harness/launch.rs`. If those files were not formatted, fmt changed them
  in the shared tree. I did not commit them. After that I used `rustfmt`
  on my own files only.
- `CLAUDE_CONFIG_DIR` is not supported. horch always reads
  `$HOME/.claude.json`. No part of horch reads `CLAUDE_CONFIG_DIR` today.

## Not done / follow-ups

- agy's trust store is unknown. When agy is installed, find its store and
  add a reader. Until then, PRE-14 warns for agy candidates, and an agy pane
  ends at once when it shows the dialog.
- `crates/horch/src/dataset/run.rs`: a plan with 0 candidates still exits 4
  instead of exit 3 (reported in T5).
