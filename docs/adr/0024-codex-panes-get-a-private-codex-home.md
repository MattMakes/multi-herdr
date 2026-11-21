# Codex panes get a private CODEX_HOME that holds only their rules

Codex differs from Claude in 2 ways that matter to the fleet. It loads
execpolicy rules at startup, so the fleet commands must be allowed to run
outside its sandbox, or a worker can neither report back nor shut itself
down. And it mints its session id itself and reveals it only after the
process starts, so the ledger harvests it. Codex reads rules only from
`$CODEX_HOME/rules/*.rules`. No flag or config key points 1 launch at a
different set. The only lever is `CODEX_HOME` itself. So `Rules::install` in
`crates/horch-core/src/harness/codex.rs` gives each codex pane a private
codex home, assembled by `build_private_home`. It symlinks every entry of
the real codex home except `rules/`, and its `rules/` holds this launch's
rules and nothing else. Auth, config, skills, plugins and `sessions/`
resolve through the symlinks to the real files, so a rollout still lands
where the ledger's harvest looks for it. Appending to the shared
`rules/default.rules` instead would grant every codex session on the
machine whatever any fleet ever needed, forever.

## Consequences

- `CODEX_HOME` points at the private home on the child process only. horch
  itself keeps the real `CODEX_HOME`, so the ledger's harvest and session
  discovery still read the real sessions directory.
- `Rules::install` takes `home` (the user's home directory, the same value
  `sessions_dir` takes), `codex_home` (the inherited codex home, from
  `codex_home`, that the private one mirrors), `root` (where the private
  one is built) and `role`, which only names the directory.
- `root` lies outside every writable root of the sandbox. `Codex::prepare`
  builds the private home under the data root, not the state root, because
  every pane may write the state root. Review F13 of T-7 found the private
  homes under the state root: a sandboxed worker could rewrite the rules
  (and drop the `forbidden` blocks) of a pane horch launches later.
- The private home is rebuilt on every launch rather than cached, so an
  entry added to the real codex home since the last fleet is linked rather
  than missing.
- A pane killed outright never reaches `Rules::finish` and leaves its
  directory behind. The role is in the directory name, so the leftovers
  (symlinks and 1 rules file, harmless to delete) say which pane they came
  from.
- After the agent exits, `Rules::finish` and `Rules::cleanup` remove the
  private home, unless codex created something at its top level that was
  not there at launch. That would be state written to a directory nobody
  will look in again, so the directory is kept and named rather than
  silently deleted.
- `CODEX_CACHE_FILES` (`cloud-config-bundle-cache.json`, `.hook-trust`) are
  caches codex writes at the top of every home it runs in (#140). They hold
  nothing the real codex home needs, so they do not keep a private home
  from being removed.
- Cleanup removes the links, never what they point at. This is the
  load-bearing safety property, and a test guards it: a regression would
  delete the operator's real codex home, auth, sessions and all, when a
  codex pane exits. `remove_dir_all` is safe on the private home because
  every entry except `rules/` is a symlink, and `remove_dir_all` removes
  links rather than following them.
- `Rules::attach_skills` replaces only the private home's `skills` link and
  never follows it, because following it would write into the operator's
  skills directory. `clear_skills_link` refuses anything that is not a
  symlink. Codex discovers the skill files natively.
- `Codex::validate` rejects `inherit_plugins: false`. Codex skills are
  installed into the private home, and codex has no CLI switch to disable
  all other operator extensions.
- The worker rules narrow ledger access (#181). A codex worker reaches the
  task ledger outside its sandbox only through the subcommands a worker
  runs: `finish` lifts the sandbox. `add` and `reopen`, which take any
  `--check` command that would then run unsandboxed, are `forbidden`. A
  subcommand no rule names runs in the sandbox.
