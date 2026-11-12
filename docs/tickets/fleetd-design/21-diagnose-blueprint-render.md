# 21: Diagnose why the blueprint architecture render fails

<!-- horch-ticket: 1 -->
**Persona:** primary `researcher`, fallback `opus` · **Parallel:** yes
**Kind:** report
**Review:** none
**Check:** `test -s docs/specs/fleetd/blueprint-render-failure.md`
**Own:** `docs/specs/fleetd/blueprint-render-failure.md`
**Do not touch:** `ddd/`, `crates/`

## GOAL

The file `docs/specs/fleetd/blueprint-render-failure.md` gives the proven cause of the failed architecture render, and the smallest change to the plugin that fixes it.

## CONTEXT

- The DDD workspace `ddd/` is on your branch. Its last step ran `node /Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/shared/bin/ddd.mjs export blueprint ddd --deliver`.
- That command rendered `ddd/diagrams/final-F1.html` to `final-F7.html`. It failed 2 times on the architecture render. `ddd/diagrams/final.html` does not exist.
- The exact error: `blueprint deliver architecture ddd/diagrams/final.architecture.json failed (exit 1)`, then `[delivery/receipt-invalid] Could not parse the successful artifact-check receipt: Unterminated string in JSON at position 65308 (line 186 column 100)`.
- Position 65308 is a little less than 65536 bytes (64 KiB). 1 possible cause is that the receipt is cut off when a child process writes more than 1 pipe buffer and exits. This is a guess. Prove it or disprove it.
- The plugin is installed at `/Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0`. The renderer is in `/Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/skills/blueprint`. The exporter is in `/Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/shared/bin/ddd.mjs`. The plugin belongs to the operator. Its source repository is `MattMakes/skill-marketplace`, commit `17be4c6`.

## STEPS

1. Run `git remote -v` and `git config user.email`. The remote must be `git@git-alt:MattMakes/multi-herdr.git`. The email must be `m4ttplu5@gmail.com`. If 1 is different, send `BLOCKED:` and end your turn.
2. Copy `ddd/` to a new folder under `/tmp`. Do each experiment on the copy. Never run a command that writes into `ddd/` of your worktree.
3. Run the export command on the copy. Record the exact output and the exit code.
4. Find the code that writes the receipt and the code that reads it. Give `file:line` for each.
5. Measure the size of the full receipt. Find where the text is cut. Name the mechanism with evidence, for example: a process exit before stdout is flushed, a buffer limit of a child process call, or a fixed read size.
6. Prove the cause: make the smallest change in a copy of the plugin under `/tmp`, run the export on the copy of `ddd/`, and show that `final.html` is written.
7. Write the findings file with these sections: `## Cause` (1 paragraph, with evidence), `## Reproduction` (the commands), `## Fix` (a unified diff against the plugin source at commit `17be4c6`), `## Proof` (the output of the fixed run), `## Not determined` (what you could not prove).
8. Run the **Check** command of this ticket. It must exit 0.

## CONSTRAINTS

- WARNING: do not edit a file under `/Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0`. That folder is the operator's installed plugin. Work on a copy under `/tmp`.
- Do not edit `ddd/`. Do not commit a render result.
- Do not open an issue or a pull request on any repository.
- Write in plain technical English. Do not use the em dash.
- Do not edit a file other than the one in **Own**.
- Do not start a subagent or a nested agent CLI.

## DONE WHEN

- The check passes.
- The cause has evidence, or the file says "I could not determine the cause" with what you ruled out.

## REPORT

Finish with `horch ledger finish`, then `horch ledger done`. In the DONE line, give the cause in 1 sentence, and say if the fix is proven.
