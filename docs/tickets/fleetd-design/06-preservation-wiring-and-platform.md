# 06: Preservation: agent CLI wiring, login sources and platform code that fleetd must keep

<!-- horch-ticket: 1 -->
**Persona:** primary `researcher`, fallback `opus` · **Parallel:** yes
**Kind:** report
**Review:** none
**Check:** `test -s docs/specs/fleetd/preservation-06-wiring-and-platform.md`
**Own:** `docs/specs/fleetd/preservation-06-wiring-and-platform.md`

## GOAL

The file `docs/specs/fleetd/preservation-06-wiring-and-platform.md` lists how `horch` wires each agent CLI and herdr today, where each login comes from at run time, and each place where the code differs by operating system.

## CONTEXT

- Read `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/preservation-context.md` first. It gives the situation and the rules for the findings file.
- Ticket 04 lists control plane behavior. Ticket 05 lists contracts and fixed values. Do not list them here.
- Decision D1 says that fleetd supports macOS, Linux and Windows, with all OS-specific code behind 1 platform layer that has 1 module per OS. Your platform table is the input for that layer.
- The harness adapters are in `crates/horch-core/src/harness/`. The herdr adapter is in `crates/horch-core/src/workspace/`. Process code is in `crates/horch-core/src/runtime/`, `procid.rs` and `exeid.rs`. File code is in `fsx.rs`.
- WARNING: the project rule in `CLAUDE.md` is absolute. Never read, print, set, export, pass or depend on `ANTHROPIC_API_KEY`. Every `claude` run uses the operator's claude.ai subscription login. Record how the code keeps this rule. Do not test it by reading the variable.

## STEPS

1. Run `git remote -v` and `git config user.email`. The remote must be `git@git-alt:MattMakes/multi-herdr.git`. The email must be `m4ttplu5@gmail.com`. If 1 is different, send `BLOCKED:` and end your turn.
2. Read the context file, the decisions file and the design. Read the design section "What herdr and the agent CLIs give us" and the section "Worker profiles and adapters" with care.
3. List the wiring map. For each harness in `crates/horch-core/src/harness/` (claude, codex, agy, opencode, pi, prime and each other one), give a row for each of these concerns: launch command and flags, model and effort, permission or sandbox mode, config home, skills, MCP servers, hooks, resume, the context reader, the usage or quota reader, the trust or approval prompt, and the update check. Columns: `Concern`, `Harness`, `Implementation` (`file:line`), `Design`.
4. List the herdr wiring: each herdr command or socket call that `horch` makes, where, and what it depends on in herdr's output. Note the herdr version that the code expects.
5. List the login sources. For each harness and each outside service, give: what needs a login, where the login comes from at run time (a subscription login in a config home, a keyring, a file, an environment variable name), if it expires, and how `horch` reports a sign-in error. Give names and paths only. Never print a secret or the value of a variable.
6. List the code that removes or blocks `ANTHROPIC_API_KEY` for `claude` runs. Give `file:line`. If no code does this, write that.
7. List the platform code. Run `grep -rn 'cfg(target_os\|cfg(unix)\|cfg(windows)\|cfg(not(' crates`. Group the sites by concern: process identity and liveness, signals and stop, process groups, file locks, file permissions, paths and home directories, symlinks, shell and quoting, sockets and pipes, service or daemon start, and other. For each concern give: the count of sites, the files, what macOS does, what Linux does, what Windows does, and the gaps (for example "Windows: not implemented").
8. List what the design assumes that 1 OS does not have. Search the design for `systemd`, `WatchdogSec`, `/proc`, `inotify`, `bubblewrap`, `SIGINT`, `SIGKILL`, `process group` and `launchd`. For each, give the design line and the state on macOS, Linux and Windows.
9. Write the `## Summary` section as the context file says.
10. Run `test -s docs/specs/fleetd/preservation-06-wiring-and-platform.md`. It must exit 0.

## CONSTRAINTS

- Never read, print or use `ANTHROPIC_API_KEY`. Never print a secret, a token or the value of an environment variable. Do not open a credential file. Its path is enough.
- Read only. Do not edit code. Do not install or update a tool.
- Do not edit a file other than the one in **Own**.
- Do not start a subagent or a nested agent CLI.

## DONE WHEN

- The check passes.
- Each of steps 3 to 8 has a table with at least 3 rows, or the sentence "I could not determine <X>".
- Each row has a location and, where the context file asks for it, a `Design` value.
- The file starts with `## Summary`.

## REPORT

Record 1 progress note after each of steps 3 to 8. Finish with `horch ledger finish`, then `horch ledger done`. In the DONE line, give the count of platform sites per concern, and the 3 most important gaps.
