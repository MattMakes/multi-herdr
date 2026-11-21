# The marketplace treats skill sources and lock entries as untrusted input

`crates/horch-marketplace` installs skills from git repositories and records
them in `marketplace.lock`. The repository, the requested revision, the
source URL and the lock file all come from outside, so each is validated
before it reaches git or the file system. A git skill is fetched by
`crates/horch-marketplace/src/source.rs` into a bare repository with hooks
off (`core.hooksPath=/dev/null`). The pinned commit's tree is checked out
with `read-tree` and `checkout-index`, so no worktree, no hook and no
remote config of the untrusted repository is ever involved.

## Consequences

- `source::check_listing` applies the skill tree rules (no symlink, no
  submodule, size limits) to the `git ls-tree` listing before anything is
  checked out. A rejected tree is never written to disk.
  `integrity::tree_digest` checks the written tree again.
- `model::is_safe_ref_name` accepts only ref names git accepts that cannot
  be read as a command-line option or a revision range: no leading `-`, no
  `~^:?*[\` bytes and no bad shapes. The requested revision is user input
  passed to git as an argument.
- `model::GitRevision::Tag` resolves to a tag first and falls back to a
  branch of the same name, because an `owner/repo@name` source does not say
  which kind of ref `name` is. `Branch("HEAD")` means the remote's default
  branch.
- `MarketplaceError::Credentials` is raised when a source URL carries
  userinfo or a token. Its text says which kind it found and never holds
  the URL, so the secret cannot reach a log.
- `Installer::entry_dir` checks the id (`SkillId::parse`) and the version
  (a clean relative path with no `/`) of every `marketplace.lock` entry
  before joining them into `skills/<id>/<version>/`. The lock file is input
  that could otherwise traverse out of the store.
- `catalog::BundledFile` paths are relative to the skill directory,
  `/`-separated, and checked as clean relative paths before anything is
  written. Files marked `executable` are written with mode 755 on Unix. The
  file mode is not part of the skill tree digest, so changing a file's
  executable bit does not change the digest.
