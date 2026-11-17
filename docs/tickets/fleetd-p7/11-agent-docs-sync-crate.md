# 11: Widen herdr-docs-sync into the library agent-docs-sync: 1 source entry for each tool in docs-sources.toml, the sitemap and llms_txt kinds, and 1 folder for each version

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p agent-docs-sync --locked && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/agent-docs-sync/`, `crates/herdr-docs-sync/`, `Cargo.toml`, `Cargo.lock`

Spec: `docs/specs/fleetd/design.md` ("Docs sync and adapter conformance" (Sources; What one sync does, step 1); Migration plan, item 9 "Phase 7: docs sync and conformance, then exec mode").

## Goal

`crates/agent-docs-sync` fetches the listed pages of each tool of `docs-sources.toml` as markdown into `<out>/<tool>/<version>/`, with a manifest of page hashes.

## Requirements

- FR-8 (part): fetch the documentation of each CLI and record the installed version (step 1 of 1 sync).

## Context

- Read `docs/tickets/fleetd-p7/MAP.md` first: the sections "Open questions" and "Decisions that hold".
- Self-hosting danger: this phase changes the worker mode that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p6 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it (the text says "from ticket NN of fleetd-pN").
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`. No test uses the network: give each page through a local fixture. No ticket sends project text, a name or an email address to an outside service.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).
- Today: `crates/herdr-docs-sync` is a binary (`src/main.rs`, `sitemap.rs`, `page.rs`, `clean.rs`) that mirrors herdr docs from its sitemap into `herdr-docs/`, reports added, updated and removed pages, and prunes stale ones. It uses `ureq`.
- Choice (MAP.md Q4): `git mv crates/herdr-docs-sync crates/agent-docs-sync`, and make it a library with no binary. `horch docs sync` (ticket 22) is the command. The version is an input: the caller runs the version command through the launcher (SR-7), so this crate starts no process in this ticket.
- `docs-sources.toml` is `crates/agent-docs-sync/docs-sources.toml`, with the 4 entries of the design (herdr, claude, codex, agy) word for word, and is built into the crate with `include_str!`. The keys: `kind` (`sitemap` or `llms_txt`), `url`, `md_suffix`, `include`, `also_git`, `version_cmd`. An unknown key is an error that names it. Ticket 12 reads `also_git`; this ticket parses it.
- The network: production fetches with `ureq`. Each test gives the pages through a fetch port (`trait Fetch`) and a local fixture folder under `crates/agent-docs-sync/tests/fixtures/`. No test uses the network.
- `nfr_06_dependency_allowlist` (`crates/horch-core/tests/nfr.rs:214`) names `herdr-docs-sync` as a crate that may use an HTTP client, and `arch_scan.rs:720` names it too. Rename both entries (guard files, permitted).
- `scripts/pl6/baseline-macos.txt` is the record of Phase P. Do not change it.
- The repository folder `herdr-docs/` stays. Ticket 22 keeps a way to refresh it (`--out`).

## Steps

1. Write the tests first in `crates/agent-docs-sync/tests/sync.rs`: `sources_file_has_the_4_tools_of_the_design`, `unknown_source_key_is_an_error_that_names_it`, `llms_txt_source_fetches_only_the_included_pages_with_the_md_suffix`, `sitemap_source_keeps_the_herdr_behavior` (the old tests of `sitemap.rs` and `clean.rs` stay green), `pages_go_to_the_folder_of_the_version_with_a_manifest_of_hashes`, `a_page_that_fails_is_reported_and_the_others_are_written`. They fail before the code.
2. Rename the crate, add the library API (`sync(source, version, fetch, out) -> Report`), the llms_txt kind and the version folder.
3. Run the Check and the guard tests. Commit the exact files.

## Constraints

1. This fleet runs on the installed `horch` binary and on the live ledger
   `/Users/MASCOTT/.horch/ledger.sqlite3`. The code that you change is the code of that tool.
   Do not break the fleet that builds it:
   - Each test, and each `horch` or `fleetd` process that you build and start, uses temporary
     folders for `HORCH_DATA_DIR`, `HORCH_STATE_DIR` and `HORCH_LEDGER_DB`. Never point a
     development build at the live ledger or at the live state folders.
   - Never install a development build. Never run `horch self-update`, `cargo install` or a
     release script from a worktree.
   - Never start a development `fleetd`, watchdog or service unit on this machine outside a test
     that uses temporary folders. Never register a launchd job, a systemd unit or a scheduled task
     on this machine.
   - The only `horch` commands that you run on the live fleet are the worker commands of your own
     task (`horch ledger ...`, `horch tell orchestrator ...`).
2. NEVER use `ANTHROPIC_API_KEY`. Never read, print, set, export, pass or depend on it. Never
   write code, config, docs or teammate files that use it. Each `claude` run uses the claude.ai
   subscription login. Remove the variable from each `claude` command:
   `env -u ANTHROPIC_API_KEY claude ...`. If a task seems to need an API key, stop and ask.
3. Do not use a shared `CARGO_TARGET_DIR`. Build in the `target/` folder of your worktree. Run
   `cargo clean` in your worktree before `horch ledger finish` only if the disk has less than
   20 GB free (`df -h /`).
4. Before the first commit, run `git remote -v` and `git config user.email`. The remote must be
   `git@git-alt:MattMakes/multi-herdr.git` and the email must be `m4ttplu5@gmail.com`. If not,
   stop and send BLOCKED.
5. Commit messages have no co-author line. Never write the em dash character; use `-`. Never edit
   `CHANGELOG.md` or a generated file by hand.
6. Write the tests first and see them fail for the correct reason. Then write the code.
7. The source of truth is `docs/specs/fleetd/design.md` (FINAL). If the code of today and the
   design do not agree on a point that the ticket does not settle, stop and send QUESTION. Do not
   change `docs/specs/fleetd/` or `ddd/`.
8. Change only the files of `Own`. If the work needs another file, send QUESTION.
9. Prefer quality, simplicity and robustness to speed. No feature that the design does not name.
10. If the GitHub SSH connection times out at `horch ledger finish`, run it again, 3 times maximum.

## Done when

- The Check passes.
- `crates/herdr-docs-sync` no longer exists, and no file names it except the PL-6 baseline.
