# Antigravity quota windows are scoped by model group

`POOL_GOOGLE` in `crates/horch-core/src/routing/quota.rs` is the
operator's Google account. Every Antigravity model draws on it, whoever
made the model (Gemini, Claude, GPT). Its windows are not pool-wide. Each
window limits only the models of its group, and `parse_agy_usage` scopes
it by its bucket id.

## Consequences

- `parse_agy_usage` reads `agy -p /usage --output-format json` from agy
  1.4.3. The field names are undocumented. Each group of
  `command.data.groups` lists `buckets` with a `window` (`weekly`, `5h`),
  a `remaining_fraction` 0-1 and a UTC `reset_time`.
- `gemini-argon-*` buckets scope to `argon`. Pro plans give Argon its own
  group.
- `gemini-*` buckets scope to `gemini`, and to `argon` too when no Argon
  group exists. Ultra plans count Argon against the Gemini buckets.
- `3p-*` buckets scope to the Claude and GPT models agy serves: `opus`,
  `sonnet`, `gpt-oss`.
- Any other bucket id is unscoped, so it limits every model. This is the
  conservative choice.
- `model_family` tests `argon` before `gemini`. `gemini-4-argon` is
  limited by its own Argon group, and matching `gemini` first would scope
  it to the wrong window.
