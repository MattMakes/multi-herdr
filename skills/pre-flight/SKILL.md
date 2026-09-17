---
name: pre-flight
description: Use before executing a plan to verify wiring, contracts, preservation, configuration, and prerequisites against source.
---

# Pre Flight

Verify the plan against the actual repository before dependent implementation begins.

1. Read the complete plan, design, acceptance checks, and relevant original implementation. Confirm paths, tools, and dependencies exist or are explicitly created by a task.
2. Review these categories, recording evidence and marking inapplicable categories with a reason:
   - Wiring: interface or exported API to implementation, construction/registration, entry point, and consumer.
   - Preservation: validation, error handling, retries, concurrency, transactions, and side effects survive unless a change is authorized.
   - Contracts: methods, routes, schemas, required fields, serialization, status codes, CLI behavior, and authentication remain compatible or have an explicit migration.
   - Configuration: precedence, feature flags, middleware, startup/health behavior, and duplicate settings are accounted for.
   - Domain values: meaningful defaults, thresholds, cache TTLs, indicators, and retry counts are preserved or deliberately changed.
   - Credentials: runtime source, access method, expiration/rotation, certificates, and proxy requirements remain supported; never copy credential values into artifacts.
3. Check that local startup and representative integration checks are specified for services. For rewrites, distinguish runtime parity evidence from static inference. Missing external access is an evidence gap, not proof of failure.
4. Verify every requirement maps to an executable check with an expected result. Flag checks that only assert a file exists when runtime wiring is what matters.
5. Write the assigned report or `ai_docs/reports/YYYY-MM-DD-pre-flight-report.md` with PASS/WARN/FAIL per category, precise source/plan references, and the correction needed for each gap.

Conclude CLEAR or HOLD for the affected tasks. Correct routine plan gaps within your assigned ownership and existing authorization. Send unresolved contract decisions to the orchestrator and continue independent work; do not impose an interactive human approval step.

## Worker coordination

Follow the assigned brief and repository instructions. Load only the skill content relevant to the current task. For a material ambiguity or missing prerequisite, use `horch tell orchestrator` with a concise question, evidence, and the affected task. Continue independent work and block only the dependent action. If messaging is unavailable, record the blocker in the worker result. Do not wait on an interactive human prompt or add routine approval checkpoints when the work is already authorized.
