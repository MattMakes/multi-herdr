# Project notes are keyed by the common git directory

`project_notes::identity` in `crates/horch-core/src/project_notes.rs`
resolves a git linked worktree through its `gitdir` pointer and
`commondir` to the common repository directory. Project facts recorded
from any checkout of the same repository then share 1 store, keyed by the
SHA-256 of that path.
