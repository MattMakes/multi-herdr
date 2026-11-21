# Each Prime launch gets a private agent dir of links

Prime reads its settings and `models.json` from
`$PRIME_AGENT_CODING_AGENT_DIR`, so each launch gets its own agent dir
(design `ai_docs/plans/wave2/w1/design.md` §6.9,
`crates/horch-core/src/harness/prime.rs`). Every entry of the operator's dir
is a link, and only `settings.json` and `models.json` are generated. The
fleet window goes in a `models.json` that holds the override and nothing
else, and only when the operator's own files set no window for the model's
provider.

## Consequences

- `auth.json` (`AUTH`) is always linked, also when the operator has none
  and the link dangles. A login in the pane then lands in the operator's
  file, not in a dir that is removed: Prime creates nothing in the launch
  dir and writes the login through the link (`link_auth`). Prime also
  writes a refreshed token through the link onto the operator's file.
- `is_launch_private` never links `settings.json` or `models.json`
  (generated), a `*.lock` (a linked lock dir this pane can never take) or a
  `*.tmp` (another process's write in progress).
- The agent dir is not needed after the pane and is removed then with
  `remove_dir_all`, which does not follow links, so the operator's live
  `auth.json` is safe. The sessions dir stays for a resume.
- The launch, scan and session dirs are created owner only (0700): the
  session files hold the whole conversation.
