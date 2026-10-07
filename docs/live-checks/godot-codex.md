# Live check: Godot in a Codex sandbox

This check proves which Godot 4.7.2 headless commands work in a macOS Codex
`workspace-write` pane. It compares the pane's normal environment with
workspace-local user directories, then applies the TLS certificate settings
needed to remove the remaining sandbox error. It also builds and tests one
small `Health` node in GDScript. Item 17 of the Godot wave.

## How to run

Run this command in a `codex-sol` pane:

```bash
scripts/live/godot-codex.sh
```

Set `GODOT_PATH` to select another Godot executable. Otherwise, the script
uses `/Applications/Godot.app/Contents/MacOS/Godot`. It needs Godot 4.7.2,
`perl`, a readable `/etc/ssl/cert.pem`, and write access to
`.worktrees/_scratch/godot-codex-trial/`. It starts no model session.

The script prints a `NOTE` when `CODEX_SANDBOX` is not set. Such a run checks
Godot but proves nothing about the Codex sandbox. It redirects mode A to a
second scratch home in that case, so an external run does not write the
operator's Godot settings. The comparison run exits 1 because modes A and B
intentionally reproduce failures. The final
`B-fixed-*` and `health-task` lines are the fallback acceptance result. Full
logs stay under `.worktrees/_scratch/godot-codex-trial/logs/`.

Modes:

- A uses the pane's normal environment.
- B sets `HOME`, `XDG_DATA_HOME`, `XDG_CONFIG_HOME`, and `XDG_CACHE_HOME`
  under `.worktrees/_scratch/godot-codex-trial/.home/`.
- B-fixed uses mode B and sets both Godot TLS certificate overrides to
  `/etc/ssl/cert.pem`.

## 2026-10-07

Host: macOS 26.5.1 (arm64), Codex CLI 0.160.0 with
`CODEX_SANDBOX=seatbelt`, Godot 4.7.2.stable.official.ed1daf0bf.
Another worker reported that it stopped shared Godot processes at about
12:43Z. This table uses the complete rerun that finished after that warning
at 12:45Z. The rerun produced the same result matrix as the first run.

Every command used `--headless`. A result is `FAIL` when Godot prints an
`ERROR` line, even when Godot exits 0.

| Step | Mode | Exit | Result | Evidence |
|------|------|------|--------|----------|
| `--version` | A | 0 | PASS | Printed `4.7.2.stable.official.ed1daf0bf`; no error. |
| `--import` | A | 0 | FAIL | E1, E2, E3, E4, E5, E6. Resources still imported. |
| parse check | A | 0 | FAIL | `PARSE_CHECK checked=2 failed=0`, then E1, E2, E7, E8, E3. |
| main scene, `--quit-after 60` | A | 0 | FAIL | Printed `GODOT_CODEX_MAIN_READY`, then E1, E2, E7, E8, E3. |
| `--doctool` | A | 0 | FAIL | Generated 1,076 XML files, then E1, E2, E7, E8, E9. |
| `--version` | B | 0 | PASS | Printed the expected version; no error. |
| `--import` | B | 0 | FAIL | E3 only. The workspace-local editor settings and import data were created. |
| parse check | B | 0 | FAIL | `PARSE_CHECK checked=2 failed=0`, then E3. |
| main scene, `--quit-after 60` | B | 0 | FAIL | Printed `GODOT_CODEX_MAIN_READY`, then E3. |
| `--doctool` | B | 0 | PASS | Generated 1,076 XML files; no error. |
| `--version` | B-fixed | 0 | PASS | Printed the expected version; no error. |
| `--import` | B-fixed | 0 | PASS | Import completed and `.godot/imported/` exists; no error. |
| parse check | B-fixed | 0 | PASS | `PARSE_CHECK checked=2 failed=0`; no error. |
| main scene, `--quit-after 60` | B-fixed | 0 | PASS | Printed `GODOT_CODEX_MAIN_READY`; no error. |
| `--doctool` | B-fixed | 0 | PASS | Generated 1,076 XML files; no error. |

The error identifiers preserve every `ERROR` line from the 15 commands.
Repeated identifiers mean Godot printed the same line in more than one log.

- E1: `ERROR: Could not create directory: '/Users/mascott/Library/Application Support/Godot/app_userdata/Godot Codex Trial'.`
- E2: `ERROR: Error attempting to create data dir: /Users/mascott/Library/Application Support/Godot/app_userdata/Godot Codex Trial.`
- E3: `ERROR: Condition "ret != noErr" is true. Returning: ""`
- E4: `ERROR: Could not open 'user://' directory: 'user://'.`
- E5: `ERROR: Cannot save file '/Users/mascott/Library/Application Support/Godot/editor_settings-4.7.tres'.`
- E6: `ERROR: Error saving editor settings to /Users/mascott/Library/Application Support/Godot/editor_settings-4.7.tres`
- E7: `ERROR: Could not create directory: 'user://logs'.`
- E8: `ERROR: Failed to open log file for writing: user://logs/godot.log`
- E9: `ERROR: Cannot remove file or directory: '/Users/mascott/Library/Caches/Godot/editor_doc_cache-4.7.res'.`

Godot reports E3 from `get_system_ca_certificates` in
`platform/macos/os_macos.mm`. Mode B redirects file writes but does not give
the sandbox access to the macOS certificate service. Godot 4.7.2 has no
`--user-data-dir` option in `--help`. `SSL_CERT_FILE=/etc/ssl/cert.pem` did
not remove E3. A process check with `ps -axo pid,args` was also denied with
`operation not permitted`; the pane did not start or kill an editor process.

## Health task

The trial creates `health.gd` with `class_name Health`, `max_health`,
`damage(n)`, `heal(n)`, and a `died` signal. The headless test checks damage,
healing, upper and lower clamps, death, single signal emission, and the
post-death state.

| Step | Mode | Exit | Result | Evidence |
|------|------|------|--------|----------|
| import | B-fixed | 0 | PASS | Godot generated `.uid` sidecars for both new scripts; no error. |
| project parse check | B-fixed | 0 | PASS | `PARSE_CHECK checked=4 failed=0`; no error. |
| `test_health.gd` | B-fixed | 0 | PASS | `HEALTH_TEST passed=7 failed=0`; no error. |

## Verdict

A Codex pane can be a Godot builder fallback on this Mac. The fallback must
use a wrapper that applies all of these settings:

```bash
trial="$PWD/.worktrees/_scratch/godot-codex-trial"
export HOME="$trial/.home/home"
export XDG_DATA_HOME="$trial/.home/xdg-data"
export XDG_CONFIG_HOME="$trial/.home/xdg-config"
export XDG_CACHE_HOME="$trial/.home/xdg-cache"
```

The project must supply this workspace-local `override.cfg`:

```ini
[network]

tls/certificate_bundle_override="/etc/ssl/cert.pem"
```

The wrapper must also set this property in
`$HOME/Library/Application Support/Godot/editor_settings-4.7.tres` before an
import:

```ini
network/tls/editor_tls_certificates = "/etc/ssl/cert.pem"
```

Every engine call needs `--headless`. Project commands also need
`--path "$PROJECT"`. Use `--import` before the parse check, run the checker
with `--script`, smoke-run with `--quit-after 60`, and give `--doctool` a
workspace-local output directory. Plain mode is not a usable fallback:
Godot exits 0 while it fails to create user data, logs, settings, and cache
files outside the workspace.

A `fallbacks:` entry for a Godot builder therefore needs a Codex harness plus
this environment-and-certificate wrapper. The fallback instructions must
also require log scanning because the failing Godot commands return exit 0.
