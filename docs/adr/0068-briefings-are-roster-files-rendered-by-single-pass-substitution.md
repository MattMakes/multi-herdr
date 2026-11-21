# Briefings are roster files rendered by single-pass substitution

`crates/horch-core/src/prompts.rs` holds no prompt prose. Every briefing
the fleet sends to an agent originates in a markdown file in the roster
under `teammates/`. The roster module reads those files and substitutes
placeholders. The only transformation is literal `{placeholder}`
substitution, done in a single pass so that substituted text is never
rescanned. There is no conditional rewriting, no paraphrasing, no
summarising and no assembling of sentences in Rust: a prompt that appears
in a pane is byte-for-byte what the `.md` file says, with `{name}` spans
replaced. To change what an agent is told, edit the file.

## Consequences

- The 1 piece of composition left is selection: which of the 3 closing
  paragraphs in `_base/fleet-worker.md` applies (fresh task, resume, or
  idle). All 3 are verbatim from the file.
- `prompts::render` emits a value containing `{...}` as-is, so a task
  description that mentions `{role}` cannot rewrite the briefing around
  it.
- An unrecognised placeholder is an error, never a silently empty string.
  A worker briefed with a hole in it misbehaves in a pane nobody is
  watching.
- `prompts::context_message` renders message `key` of base
  `context-messages` (CTX-11, CTX-23) as 1 line: each `\r` and `\n` in a
  value becomes a space, because a newline typed into a pane submits the
  input early. It fails when the base or the key is missing, or when the
  message names a placeholder that `vars` does not give.
