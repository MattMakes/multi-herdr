# Plan 05: implement context watch and early compaction

Design (read it first, in full): `ai_docs/designs/context-compaction.md`.
This plan cites it as "design §N". The design holds every harness fact,
format, number and message text. Do not re-research them.

## Goal

Add `horch context` and `horch compact`, the data file
`teammates/_base/compaction.md`, and prose in both fleet bases, so that the
orchestrator watches the context size of every session (itself included) and
compacts a session after it writes `ai_docs/handoffs/<role>-whats-next.md`.
Also, `horch note` warns a worker when its own context is over its threshold
(design §5.4).

## Architecture in one paragraph

`horch-core/src/context.rs` reads the current context of one session from its
transcript tail (Claude, Codex, pi, Prime) or SQLite (OpenCode), and computes
window, native trigger and threshold. `horch/src/cmd/context.rs` lists the
orchestrator (found through herdr), every working ledger record, and any
`--session`. `horch/src/cmd/compact.rs` sends the request message, or starts a
detached job that waits for idle, types the per-harness compact line, waits
for the transcript marker, waits for idle, and types the resume message.
Every typed text is in `_base/compaction.md`; Rust only fills placeholders.
`horch/src/cmd/messaging.rs` `note` records the note, then builds the same row
for the worker's own record and prints the `warning` message when the row is
over and the record is not already asked (design §5.4 rule A).

## Rules for every unit

- NEVER read, print, set, export or depend on `ANTHROPIC_API_KEY`. Any
  `claude` command is `env -u ANTHROPIC_API_KEY claude ...`.
- Do not start subagents or background agents.
- The working tree is shared. Do not run `git checkout`, `git switch`,
  `git stash` or `git reset`. Edit only the files your unit owns.
- Build and test with your own target dir:
  `export CARGO_TARGET_DIR=/tmp/<your-role>-target`. The shared `target/` has
  a stale build-script path.
- Every unit ends with both gates green:
  - G1: `cargo test --workspace 2>&1 | grep -E '^test result:'` - every line
    `ok`, `0 failed`. Baseline at `a4dcee2`: 340 passed.
  - G2: `cargo build -p horch && HORCH_TEAMMATES_DIR=$PWD/teammates $CARGO_TARGET_DIR/debug/horch teammates --check`
    prints `roster ok: 25 teammates, 20 offered to the orchestrator`.
- Golden-file procedure: NEVER edit or regenerate a file in
  `crates/horch-core/tests/golden/`. A prose change adds a named, commented
  "sanctioned" block to `crates/horch-core/tests/golden_prompts.rs` that
  reproduces the new text exactly (design §8.5). Only unit U5 owns that test.
- Report with `horch tell orchestrator "[<role>] DONE: ..."` in STE. List
  the files you changed, the G1 pass count, and anything outside your scope.

## Units, ownership and order

| unit | goal | suggested teammate | owns | depends on | parallel group |
|---|---|---|---|---|---|
| U1 | context readers, limits, threshold, fixtures | `opus` | `crates/horch-core/src/context.rs` (new), `crates/horch-core/src/lib.rs`, `crates/horch-core/Cargo.toml`, `crates/horch-core/tests/fixtures/context/**` (new) | - | A |
| U2 | herdr pane status, ledger role lookup and events | `sonnet` | `crates/horch-core/src/herdr.rs`, `crates/horch-core/src/ledger.rs` | - | A |
| U3 | `compact_at` field, `Base.messages`, `_base/compaction.md`, message renderer | `sonnet` | `crates/horch-core/src/teammates.rs`, `crates/horch-core/src/prompts.rs`, `teammates/_template.md`, `teammates/_base/compaction.md` (new), `teammates/README.md` | - | A |
| U5 | prose, execpolicy, handoff skill, golden test | `sonnet` (or `codex-terra`) | `teammates/_base/fleet-worker.md`, `teammates/_base/fleet-orchestrator.md`, `teammates/_base/codex-orchestrator-execpolicy.md`, `skills/handoff/SKILL.md`, `crates/horch-core/tests/golden_prompts.rs` | - | A |
| U4 | `horch context` and `horch compact` CLI and job; context warning in `horch note` | `opus` | `crates/horch/src/main.rs`, `crates/horch/src/cmd/mod.rs`, `crates/horch/src/cmd/context.rs` (new), `crates/horch/src/cmd/compact.rs` (new), `crates/horch/src/cmd/messaging.rs` | U1, U2, U3 | B |
| U6 | docs | `sonnet` | `README.md`, `docs/phase-skills.md` | U4, U5 | C |
| U7 | cloud-tier validation | `sonnet` | `ai_docs/reports/context-compaction/07-validation.md` (new) | U1-U6 | D |

Group A units run in parallel. B, C and D run in that order. No file has 2
owners. U4 uses the public APIs of U1, U2 and U3 exactly as named below; if a
name must change, the owner reports it to the orchestrator before U4 starts.

Acceptance requirements (each unit lists the ones it serves):

| id | requirement | check |
|---|---|---|
| AR1 | Per-harness current-context formulas (design §3.2-§3.5) | C4, C6 |
| AR2 | Tail read with a growing window; marker search that ignores escaped needles (design §3.6) | C4 |
| AR3 | Threshold rule and limits table (design §4) | C4, C6, C7 |
| AR4 | `horch context` rows, states, text and JSON (design §3.7, §3.8) | C6, C7, L1-L3 |
| AR5 | `horch compact` request, job steps, job file, log (design §6.3, §6.4) | C5, L4-L9 |
| AR6 | Messages are data in `_base/compaction.md` (design §8.2) | U3 tests, C2 |
| AR7 | Worker and orchestrator prose, execpolicy, handoff path (design §8.3, §8.4, §6.1) | C3, C2 |
| AR8 | Ledger events (design §7) | U2 tests, C5, L4 |
| AR9 | Docs (design §8.6) | U6 check |
| AR10 | No `ANTHROPIC_API_KEY` use | C9 |
| AR11 | `horch note` warns its own worker when over the threshold: note first, `warning` message from data, rule A repeat control, `context-warned` event, silent on every error (design §5.4, §7, §8.2) | C10, U3 tests, L13 |

Check ids C0-C10 and L1-L13 are defined in design §9.

---

## U1: context readers, limits, threshold, fixtures

Serves AR1, AR2, AR3. Suggested teammate: `opus` (judgement on formats and
edge cases).

FILES
- own: `crates/horch-core/src/context.rs` (new), `crates/horch-core/src/lib.rs`,
  `crates/horch-core/Cargo.toml`, `crates/horch-core/tests/fixtures/context/**` (new).
- do not touch: `usage.rs` (reuse its pub finders and `Locations`, `Missing`
  only), every other file.

PUBLIC API (U4 depends on these names)

```rust
pub struct Reading { pub tokens: Option<u64>, pub pending: bool, pub window: Option<u64>,
                     pub model: Option<String>, pub last_compaction: Option<Compaction> }
pub struct Compaction { pub at: String, pub trigger: Option<String>,
                        pub pre_tokens: Option<u64>, pub post_tokens: Option<u64> }
// both derive Debug, Clone, PartialEq, Serialize

pub enum Harness { Claude, Codex, Pi, Prime, OpenCode }      // FromStr from "claude" etc.
pub fn read_claude(text: &str) -> Reading;                    // text = a tail slice or a whole file
pub fn read_codex(text: &str) -> Reading;
pub fn read_pi(text: &str) -> Reading;                        // pi and Prime
pub fn read_opencode_rows(latest_json: &str, last_summary_json: &str) -> Result<Reading>;
pub fn read_session(loc: &usage::Locations, h: Harness, sid: &str)
    -> std::result::Result<(PathBuf, Reading), usage::Missing>;   // tail read + last_marker
pub fn opencode_db(home: &Path) -> PathBuf;                   // ${XDG_DATA_HOME:-home/.local/share}/opencode/opencode.db
pub fn transcript_len(h: Harness, path: &Path, sid: &str) -> Result<Offset>;  // bytes, or newest opencode msg id
pub enum Offset { Bytes(u64), MessageId(String) }
pub fn marker_after(h: Harness, path: &Path, sid: &str, after: &Offset) -> Result<Option<Compaction>>;
pub struct Limits { pub window: Option<u64>, pub native_trigger: Option<u64> }
pub struct ClaudeKnobs { pub auto_compact_window: Option<u64>, pub disable_1m: bool }
pub fn limits(h: Harness, model: Option<&str>, transcript_window: Option<u64>,
              claude: &ClaudeKnobs, codex_limit: Option<u64>) -> Limits;
pub fn claude_auto_compact_window(settings_overlay: Option<&str>, user_settings: Option<&str>,
              use_user_settings: bool, teammate_env: &BTreeMap<String, String>,
              process_env: Option<&str>) -> Option<u64>;      // design §4.2 order; values outside 100k..=1M ignored
pub fn codex_limit_from_args(args: &[String]) -> Option<u64>; // "-c", "model_auto_compact_token_limit=N"
pub const DEFAULT_THRESHOLD: u64 = 300_000;
pub fn threshold(base: u64, native_trigger: Option<u64>) -> u64;  // min(base, trigger*8/10)
```

STEPS
1. Add `memchr = "2"` to `crates/horch-core/Cargo.toml` `[dependencies]`.
   Add `pub mod context;` to `crates/horch-core/src/lib.rs`.
   Check: `cargo build -p horch-core` succeeds; `git diff Cargo.lock` shows no
   new package (memchr 2.8.3 is already locked).
2. Write the fixtures exactly as design §9.3 lists them, under
   `crates/horch-core/tests/fixtures/context/`. Use the same ids, file names,
   numbers and timestamps; check C6 depends on them.
   Check: every `.jsonl` line parses: `for f in $(find crates/horch-core/tests/fixtures/context -name '*.jsonl'); do jq -c . "$f" >/dev/null || echo BAD $f; done` prints nothing.
3. Write failing tests first (TDD), in `context.rs` `mod tests`, with the exact
   names in design §9.1 C4, plus `opencode_reads_a_real_database` marked
   `#[ignore]` (it needs `sqlite3`; it builds a temp database from
   `opencode/rows-post.json` with `sqlite3` and calls `read_session`).
   Check: `cargo test -p horch-core context::` compiles and the new tests fail.
4. Implement `read_claude`, `read_codex`, `read_pi` over a `&str` (parse
   lines from the end; skip lines that are not JSON), per design §3.2-§3.4.
   Include the Claude `iterations` rule and the pending rules.
   Check: the Claude, Codex, pi, Prime tests pass.
5. Implement `scan_tail` and `last_marker` (design §3.6). `read_session`
   uses `scan_tail` for the reading and `last_marker` for `last_compaction`
   when the tail window did not include one. Implement
   `tail_grows_past_a_five_mib_line` with a temp file: 1 usable Codex
   `token_count` line, then a 5 MiB `compacted` line, then nothing.
   Check: that test and `last_marker_ignores_escaped_needles` pass.
6. Implement `read_opencode_rows` and the shell-out `read_session` branch for
   OpenCode (`sqlite3 -readonly -json`), with the `^ses_[A-Za-z0-9]+$` guard
   (design §3.5). Missing `sqlite3` → `Err(Missing::NotRead)`.
   Check: the 2 OpenCode row tests pass. If `sqlite3` exists locally, also
   `cargo test -p horch-core context::opencode_reads_a_real_database -- --ignored`.
7. Implement `limits`, `LIMITS` table, `claude_auto_compact_window`,
   `codex_limit_from_args`, `threshold` (design §4.1, §4.2).
   Check: `threshold_caps_at_eight_tenths_of_native_trigger`,
   `limits_per_harness`, `claude_auto_compact_window_lookup_order` pass, and
   the table in design §4.3 is asserted row by row in `limits_per_harness`.
8. Implement `transcript_len` and `marker_after` (a marker line strictly after
   the offset; for OpenCode, a summary row with an id greater than the offset).
   Add tests `marker_after_sees_only_new_markers` for Claude and Codex.
   Check: passes.
9. Run G1 and G2.

DONE WHEN: all C4 test names exist and pass; G1 and G2 pass; `usage.rs` is
unchanged (`git diff --stat crates/horch-core/src/usage.rs` is empty).

---

## U2: herdr pane status, ledger role lookup and events

Serves AR5 (status), AR8. Suggested teammate: `sonnet` (small, specified).

FILES
- own: `crates/horch-core/src/herdr.rs`, `crates/horch-core/src/ledger.rs`.
- do not touch: every other file.

PUBLIC API (U4 depends on these names)

```rust
// herdr.rs
pub struct Pane { ..., #[serde(default)] pub agent_status: Option<String> }
impl Pane {
    pub fn is_idle(&self) -> bool;               // agent_status is "idle" or "done"
    pub fn agent_name(&self) -> Option<String>;  // agent_session.agent when agent_session is an object
}
// ledger.rs
impl Ledger {
    pub fn live_for_role(&self, role: &str) -> Result<Option<Record>>;   // newest updated_at with status "working"
    pub fn record_event(&self, key: &str, event: &str, text: &str) -> Result<()>;
}
```

STEPS
1. Write tests first in `herdr.rs` `mod tests`:
   `pane_parses_agent_status` (JSON `{"pane_id":"w1:p1","agent_status":"working"}`),
   `pane_is_idle_for_idle_and_done_only` (idle, done → true; working, blocked,
   unknown, missing → false), `pane_agent_name_reads_agent_session_agent`
   (object `{"agent":"claude","value":"x"}` → `claude`; string form → None).
   Check: they fail to compile or fail.
2. Add the field and the 2 methods. Check: the 3 tests and the existing
   `herdr.rs` tests pass.
3. Write tests first in `ledger.rs`: `live_for_role_picks_newest_working`
   (2 working records for one role, 1 done record; returns the newest working),
   `live_for_role_is_none_for_unknown_role`, `record_event_appends_history`
   (event name and text land in `history`, `updated_at` changes).
   Check: they fail.
4. Implement `live_for_role` and make `assign` use it, so the selection rule
   lives in 1 place. Implement `record_event` as a wrapper of the private
   `append_event`. Check: the new tests and every existing `ledger.rs` test
   pass (especially the `assign` tests).
5. Run G1 and G2.

DONE WHEN: the 6 new tests pass; existing tests pass; G1 and G2 pass.

---

## U3: `compact_at`, `Base.messages`, `_base/compaction.md`, renderer

Serves AR3 (config), AR6, AR11 (the `warning` message). Suggested teammate: `sonnet`.

FILES
- own: `crates/horch-core/src/teammates.rs`, `crates/horch-core/src/prompts.rs`,
  `teammates/_template.md`, `teammates/_base/compaction.md` (new),
  `teammates/README.md`.
- do not touch: `fleet-worker.md`, `fleet-orchestrator.md`,
  `codex-orchestrator-execpolicy.md`, `golden_prompts.rs` (U5 owns them),
  every other file.

PUBLIC API (U4 depends on these names)

```rust
// teammates.rs
pub struct Teammate { ..., #[serde(default)] pub compact_at: Option<u64> }
pub struct Base { ..., #[serde(default)] pub messages: BTreeMap<String, String> }
// Roster::base(name) already exists (teammates.rs:723); reuse it.
// prompts.rs
pub const COMPACTION_KEYS: [&str; 6] = ["request", "instructions", "resume", "reported", "failed", "warning"];
pub fn compaction_message(roster: &Roster, key: &str, vars: &BTreeMap<&str, &str>) -> Result<String>;
```

STEPS
1. Write tests first:
   - `teammates.rs`: `compact_at_is_optional_and_checked` (absent → None;
     `compact_at: 40000` → check error; `compact_at: 250000` → no error;
     `compact_at: 2000000` → check error);
     `compaction_base_has_all_messages` (builtin roster has base `compaction`
     with the 6 keys).
   - `prompts.rs`: `compaction_messages_are_single_line`,
     `compaction_message_substitutes_placeholders` (request with role `r-1`,
     tokens `312857`, threshold `300000`, handoff
     `ai_docs/handoffs/r-1-whats-next.md` renders the exact expected string from
     design §8.2), `compaction_warning_substitutes_placeholders` (warning
     with role `sonnet-1`, tokens `311225`, threshold `300000`, handoff
     `ai_docs/handoffs/sonnet-1-whats-next.md` renders exactly the rendered
     line in design §8.2), `unknown_placeholder_in_compaction_message_fails`.
   Check: they fail.
2. Add `compact_at` to `Teammate` (after `phase` or near `env`, with a doc
   comment that cites design §4.1). Add the range check
   50,000..=1,000,000 to `Roster::check` (`teammates.rs:809`).
   Add `compact_at:` with a comment block to `teammates/_template.md`. The
   template is parsed as a teammate by tests; keep it valid. Leave the value
   empty (null) in the template.
   Check: the `compact_at` test passes; existing template tests pass.
3. Add `messages` to `Base`. Create `teammates/_base/compaction.md` with the
   exact frontmatter of design §8.2 (6 messages, `warning` included), and a short body that holds the
   per-harness table of design §6.2. `build.rs` compiles every `_base/*.md`
   into `BUILTIN_BASES`; no build change is needed.
   Check: `compaction_base_has_all_messages` passes.
4. Add a roster check: base `compaction` exists; every key in
   `COMPACTION_KEYS` is present and non-empty; no value has a newline; every
   placeholder is one of `{role} {tokens} {threshold} {handoff} {pre} {post}
   {step} {reason} {log}`. Implement `compaction_message` with the existing
   `prompts::render` (`prompts.rs:27`).
   Check: the prompts tests pass; G2 prints the unchanged roster line.
5. Document `compact_at` in `teammates/README.md` (1 short paragraph: the
   threshold rule of design §4.1, and that the 0.8 cap always applies).
6. Run G1 and G2.

DONE WHEN: the 6 new tests pass; G1 and G2 pass; G2 still counts 25
teammates (a `_base` file is not a teammate).

---

## U5: prose, execpolicy, handoff skill, golden test

Serves AR7, AR11 (the worker prose line). Suggested teammate: `sonnet` (or `codex-terra`). Mechanical, but
exact to the byte.

FILES
- own: `teammates/_base/fleet-worker.md`, `teammates/_base/fleet-orchestrator.md`,
  `teammates/_base/codex-orchestrator-execpolicy.md`, `skills/handoff/SKILL.md`,
  `crates/horch-core/tests/golden_prompts.rs`.
- do not touch: `crates/horch-core/tests/golden/*` (never), every other file.

STEPS
1. Insert the worker section of design §8.3 into `fleet-worker.md` between
   the `== Scope ==` block and `== Message style: Simplified Technical English ==`,
   followed by 1 blank line. Keep `{role}` as a placeholder. The section
   ends with the 2 lines that start `If horch note prints a line that starts
   "NOTE: Context warning"` (design §8.3, self-warning path).
   Check: `cargo test -p horch-core --test golden_prompts every_worker` FAILS
   with "drifted outside the five sanctioned blocks" (proves the test sees it).
2. In `golden_prompts.rs` `every_worker_briefing_differs_only_where_sanctioned`,
   add a commented sixth block `compaction`: the exact section text with
   `{role}` rendered as `r-1`, including the 2 `NOTE: Context warning`
   lines, ending with `\n\n`. Change the replacement to
   `format!("gotchas, current state.\n{done_summary}\n{scope}{compaction}{ste}")`.
   Update "five" to "six" in the comments and the assert message, and add 1
   line to the file-level doc comment.
   Check: `cargo test -p horch-core --test golden_prompts every_worker` passes.
3. Insert the orchestrator section of design §8.3 into
   `fleet-orchestrator.md` immediately before `== Protect your context ==`,
   followed by 1 blank line.
   Check: `the_orchestrator_briefing_differs_only_where_sanctioned` fails.
4. In that test, add a commented block `context_watch` (exact text plus the
   blank line) and insert it before `== Protect your context ==` in the
   expected string. Confirm with an `assert_eq!(....matches("== Protect your context ==").count(), 1)`
   that the anchor is unique at that point of the replacement chain. Update
   "ten" to "eleven" in the comments and the assert message.
   Check: the test passes, and
   `the_codex_orchestrator_differs_from_the_claude_one_only_in_its_tier_block` passes.
5. Add the 2 rules of design §8.4 to
   `teammates/_base/codex-orchestrator-execpolicy.md` `rules:`.
   Check: G2 passes (it fails if the briefing does not name `horch context`
   and `horch compact`). `execpolicy_blocks_are_unchanged` passes (it counts
   only the 3 worker rules).
6. Change step 7 of `skills/handoff/SKILL.md` to the text in design §6.1.
   Change nothing else in the skill.
   Check: `cargo test -p horch-core skills::` passes.
7. Run G1 and G2.

DONE WHEN: `cargo test -p horch-core --test golden_prompts` prints
`5 passed; 0 failed`; no file in `tests/golden/` changed
(`git status --short crates/horch-core/tests/golden` is empty); G1 and G2 pass.

---

## U4: `horch context`, `horch compact`, and the `horch note` warning

Serves AR4, AR5, AR8, AR11. Suggested teammate: `opus` (process control, state
machine, herdr I/O). Starts after U1, U2, U3 report DONE.

FILES
- own: `crates/horch/src/main.rs`, `crates/horch/src/cmd/mod.rs`,
  `crates/horch/src/cmd/context.rs` (new), `crates/horch/src/cmd/compact.rs` (new),
  `crates/horch/src/cmd/messaging.rs` (only `note` and a new `mod tests`;
  `tell`, `assign`, `inbox` and `done` do not change).
- do not touch: every `horch-core` file (ask the owner through the
  orchestrator if an API is missing), every prose file.

CLI (clap, in `enum Command`, `main.rs:32`)

```
horch context [--json] [--over] [--session <agent>:<id-or-path>]...
horch compact <role> [--request] [--force] [--no-handoff] [--foreground] [--timeout <secs>=1800]
```

STEPS
1. Add both variants and dispatch in `main.rs`; add `pub mod context; pub mod
   compact;` in `cmd/mod.rs`. Doc comments on the variants are the `--help`
   text; keep them to 2-3 lines each.
   Check: `$CARGO_TARGET_DIR/debug/horch context --help` and `compact --help`
   print the flags.
2. `cmd/context.rs`: build rows (design §3.7): orchestrator via
   `Mailbox::resolve` + `pane_for("orchestrator")` + `pane_get` +
   `agent_session_id()` + `agent_name()` (design §3.8; on any error, 1
   stderr line and no row); `Ledger::open()?.read()` (`ledger.rs:161`) filtered to working,
   newest per role; `--session` rows. For each row: resolve the teammate
   (`Roster::load()`; record `tier`, or `orchestrator` / `orchestrator-codex`),
   call `context::read_session`, `context::limits` with
   `claude_auto_compact_window(...)` (read `$HOME/.claude/settings.json` as
   text; `use_user_settings` = `setting_sources` is None or contains `user`)
   and `codex_limit_from_args(&teammate.args)`, then `context::threshold`
   with `base` = `compact_at`, else `HORCH_COMPACT_AT`, else
   `DEFAULT_THRESHOLD`. Compute the state (design §3.7 table; `requested`
   uses design §5.4 rule A; `compacting` uses the job file of step 4).
   Render text or JSON with the exact keys and order of design §3.7.
   Make 2 functions `pub(crate)` for step 7: the row builder for 1 ledger
   record (it takes the roster, the `usage::Locations` and the
   `HORCH_COMPACT_AT` value as parameters, and does not call herdr; the
   caller fills `pane` and `pane_status`), and `already_asked(history: &[HistoryEntry], last_compaction:
   Option<&Compaction>) -> bool` (rule A: parse both times with `chrono`;
   do not compare the strings).
   Tests first (`mod tests`): `state_order_matches_design`,
   `json_keys_are_in_design_order`, `over_filter_and_empty_sentence`,
   `extra_sessions_are_numbered_from_one`, `already_asked_follows_rule_a`
   (no events → false; `compact-requested` → true; `context-warned` → true;
   a newer `compacted` event → false; a newer `last_compaction.at` →
   false; a `note` event → false; ledger time `2026-09-20T10:05:00Z` against
   marker time `2026-09-20T10:05:00.000Z` compares as equal times).
   Check: tests pass; then run C6 and C7 from design §9.1 and match the
   expected output exactly.
3. `cmd/compact.rs` request path: `--request` renders `request` with
   `compaction_message`, sends it with `Herdr::send_line`, records
   `compact-requested`. Refuse role `orchestrator`.
   Test: `request_is_refused_for_orchestrator`.
4. Job file and log (design §6.4): `<ledger::state_root()>/compact/<ws>-<role>.json`
   and `.log`; liveness by `libc::kill(pid, 0)`; stale-file cleanup.
   Test: `stale_job_file_is_ignored` (a file with pid 0x7fffffff).
5. The job (design §6.3 steps 1-11) as `fn run_job(io: &dyn PaneIo, probe:
   &dyn Probe, cfg: &JobConfig) -> Result<JobOutcome>`, with `PaneIo`
   (`status`, `send_line`) and `Probe` (`reading`, `offset`, `marker_after`)
   traits and poll intervals in `JobConfig`. The real impls wrap `Herdr` and
   `context`. Compact line per harness from design §6.2, instructions from
   `compaction_message(.., "instructions", ..)`.
   Tests first, with fakes and 1 ms polls, exactly the names in design §9.1
   C5.
   Check: `cargo test -p horch compact::` passes.
6. Default path: validation (design §6.3), handoff guard (file exists and
   mtime within 60 min; test `handoff_guard_refuses_missing_or_stale_file`),
   then spawn `current_exe() compact <role> --foreground <flags>` with
   `setsid`, stdin null, stdout and stderr to the log file (copy the pattern
   of `settle_after_close`, `crates/horch/src/cmd/tilecmd.rs:613-645`). Print
   `compaction of <role> scheduled; log <path>`.
   Check: unit tests pass. Live behavior is local-only (L4-L12).
7. `cmd/messaging.rs` `note` (design §5.4). Keep the ledger write first and
   its error path unchanged. Then call a private check
   `fn note_warning(ledger: &Ledger, record_id: &str, loc: &usage::Locations,
   roster: &Roster, env_compact_at: Option<&str>, env_session_id: Option<&str>)
   -> Result<Option<(String, u64, u64)>>` (line, tokens, threshold). It
   reads the record, resolves the session id (record, else env), builds the
   row with the step 2 row builder, applies the warn rule and
   `already_asked`, and renders `compaction_message(.., "warning", ..)`.
   `note` passes `Locations::from_env()`, `Roster::load()`,
   `HORCH_COMPACT_AT` and `HORCH_SESSION_ID`. On `Ok(Some(..))`, `note`
   calls `record_event(record_id, "context-warned", "tokens <n> threshold
   <t>")`, ignores its error, and prints the line with `output::println`.
   On `Err` or `Ok(None)`, `note` prints nothing, also not to stderr.
   Tests first (`mod tests` in `messaging.rs`, temp ledger with
   `Ledger::for_project`, `Locations` on the U1 fixture home):
   `note_warning_fires_over_threshold`,
   `note_warning_skips_below_threshold_and_pending`,
   `note_warning_skips_after_warned_or_requested`,
   `note_warning_fires_again_after_compaction`,
   `note_warning_is_silent_on_errors` (unknown record, no session id,
   agent `none`, missing transcript: each returns `Ok(None)` or `Err`;
   C10 part 2 proves at the CLI that `note` then prints nothing and exits 0;
   do not set process env vars in unit tests),
   `note_warning_text_comes_from_compaction_base` (the line equals
   `compaction_message(.., "warning", ..)` output; Rust adds no text).
   Check: `cargo test -p horch messaging::` and
   `cargo test -p horch already_asked` pass; then run C10 from design §9.1
   and match its 13 lines exactly.
8. Run G1 and G2, then C6, C7 and C10.

DONE WHEN: all C5 test names, the step 2-4 tests and the step 7 tests pass;
C6 prints the 9 expected lines and the 2 `last_compaction` lines; C7 prints
`1`, the exact sentence, and `over`; C10 prints its 13 expected lines; G1
and G2 pass.

---

## U6: docs

Serves AR9. Suggested teammate: `sonnet`. Starts after U4 and U5.

FILES
- own: `README.md`, `docs/phase-skills.md`.
- do not touch: every other file.

STEPS
1. Add a `README.md` section "Context watch and early compaction": the 2
   commands with 1 example each, the threshold rule (design §4.1) and the
   resulting table (design §4.3), the per-harness compact table (design §6.2,
   short form), the handoff path, the fallback, and 1 sentence on the
   `horch note` context warning (design §5.4). Take the command text
   from `horch context --help` and `horch compact --help` of the built binary.
   Check: every command in the section runs with `--help` and every flag it
   names exists.
2. Add 1 line to `docs/phase-skills.md` under the table: `handoff` writes
   `ai_docs/handoffs/<role>-whats-next.md` by default.
   Check: `grep -n 'whats-next' docs/phase-skills.md` prints 1 line.
3. Run G1 and G2 (docs are not compiled, but a doc-test or docs-sync test
   may read them).

DONE WHEN: both files updated; G1 and G2 pass.

---

## U7: cloud-tier validation

Serves AR1-AR11. Suggested teammate: `sonnet`. Starts after U1-U6. This
unit changes no source file.

FILES
- own: `ai_docs/reports/context-compaction/07-validation.md` (new).
- do not touch: every other file. On a failure, report it; do not fix it.

STEPS
1. Run C0 to C10 from design §9.1, in order, exactly as written, with
   `CARGO_TARGET_DIR=/tmp/<your-role>-target`.
2. For each check, write to the report: the command, the exact output (or
   the relevant lines), and PASS, FAIL or SKIPPED with the reason. C8 is
   SKIPPED when `sqlite3` is missing.
3. Add the G1 total pass count and compare it with the baseline 340.
4. Copy the local-only checklist L1-L13 (design §9.2) to the end of the
   report as "Operator checklist - not run", with an empty result column.

DONE WHEN: the report exists; every C check has a result; the orchestrator
has the report path.

---

## Operator checklist (local-only, after U7)

Run design §9.2 L1-L13 on the Mac with herdr and the real harnesses. Record
each result in the U7 report's operator table. The items that close open
UNVERIFIED facts:
- L5, L6: Codex compaction through `send_line`, and the rejection retry.
- L7, L8, L12: the detached job survives the Codex sandbox and the Claude
  Bash tool; self-compaction of both orchestrator flavors.
- L9: herdr `agent_status` for pi, OpenCode, Prime; OpenCode bare `/compact`
  through the autocomplete.
- L10: Claude applies `settings.json` `env` over the process environment.
- L13: the `horch note` warning in a live Claude pane and inside the Codex
  sandbox, and the cost of the whole `horch note` call.

## Open decisions

None block implementation. These are recorded as design choices that the
operator may revisit:
- No new backstop levers in v1 (design §4.4). The operator's existing
  `CLAUDE_CODE_AUTO_COMPACT_WINDOW=500000` is the Claude backstop.
- The 0.8 headroom factor is a constant, not a setting.

## Completion criteria

- U1-U7 report DONE.
- G1 and G2 pass on the final tree.
- The U7 report shows PASS for C1-C7, C9 and C10, and PASS or SKIPPED for C8.
- The operator checklist exists in the U7 report.
