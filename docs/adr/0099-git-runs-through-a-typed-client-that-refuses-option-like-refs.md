# Git runs through a typed client that refuses option-like refs

`crates/horch-core/src/vcs/git.rs` is a typed git client (phase B2).
`GitClient` is the seam: the messaging commands call it, and a test can put
a fake behind it. `GitCli` is the real one. Every process it starts goes
through `horch_marketplace::git::GitRunner`, which removes the API key and
the repo-locating `GIT_*` variables and turns hooks off.

## Consequences

- Every argument that names a ref or a revision is refused when it starts
  with `-`, so a label or a branch name can never become a git option.
