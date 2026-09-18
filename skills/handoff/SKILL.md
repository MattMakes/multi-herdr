---
name: handoff
description: Use when work must transfer to another worker or continue after context loss.
---

# Handoff

Preserve the state needed to continue the original objective without repeating investigation or losing user decisions.

1. Record the original request, subsequent steering, accepted scope, authorization, and constraints. Keep unresolved questions distinct from decisions already made.
2. Inventory completed work with concrete files, symbols, artifacts, and why changes were made. Identify what is committed, uncommitted, temporary, or owned by another worker; record branch/revision when available.
3. Record meaningful verification commands, results, the revision/state tested, and limitations. Keep failing, unavailable, and passing checks separate. Do not claim unrun checks as complete.
4. List remaining tasks in dependency order with exact paths, contracts to preserve, acceptance checks, and the next executable action. Identify task owners and active work so a successor does not overwrite concurrent changes.
5. Preserve failed approaches, observed errors, runtime discoveries, and hypotheses not yet tested. Include only failures that help the successor avoid repetition or understand a decision.
6. Record environment prerequisites and locations of configuration/credential sources without secret values. Link relevant design, plan, research, reports, and authoritative external sources.
7. Write the assigned handoff path or `ai_docs/handoffs/whats-next.md`. Re-read it against the actual workspace and ensure it tells the successor what to do first, what evidence to trust, and what remains uncertain.

Notify the orchestrator of the artifact and exact status. A handoff is a checkpoint, not proof the objective is complete and not a requirement for a human to restart a session. Continue authorized work when context and task ownership permit. A successor should recheck workspace state and relevant evidence before assuming recorded work is unchanged.

## Worker coordination

Follow the assigned brief and repository instructions. Load only the skill content relevant to the current task. For a material ambiguity or missing prerequisite, use `horch tell orchestrator` with a concise question, evidence, and the affected task. Continue independent work and block only the dependent action. If messaging is unavailable, record the blocker in the worker result. Do not wait on an interactive human prompt or add routine approval checkpoints when the work is already authorized.
