---
name: security-review
description: Use for an authorized security audit or focused review of trust boundaries, credentials, parsing, and dependencies.
---

# Security Review

Audit the assigned repository or diff and validate findings before declaring vulnerabilities. This portable workflow replaces upstream scripts and specialized agent fan-out with direct evidence-driven analysis.

1. Define scope, revision, languages, entry points, deployment assumptions, and assets. Map trust boundaries: external inputs, identities/roles, privileged operations, storage, network destinations, and agent/tool execution when applicable.
2. Inspect applicable areas: injection and unsafe parsing; authentication/session and object-level authorization; path traversal and file permissions; outbound request controls; cryptography and sensitive logging; configuration/secrets handling; dependency provenance and known advisories. For agent integrations, examine prompt injection paths, tool authority, untrusted skill content, and supply-chain execution.
3. Use available project-native scanners and lockfiles. Record exact commands and database/advisory freshness. Never expose secret values in reports or logs; record location and credential type with redacted evidence. Do not install scanners or upload repository content as an assumed audit prerequisite.
4. Deduplicate findings by vulnerable path and root cause. For each, trace attacker-controlled input to a reachable sensitive operation, inspect guards and deployment prerequisites, and actively test the false-positive explanation.
5. Assign CONFIRMED, DISMISSED, or UNVERIFIED status with evidence. State impact and likelihood separately from confidence. Where useful and authorized, use a harmless local fixture to demonstrate the failure; do not probe production or external targets merely to validate a code finding.
6. Write `ai_docs/reviews/security-review.md` or the assigned artifact: scope, trust-boundary summary, tools/coverage, confirmed findings ordered by severity, exact locations, prerequisites, evidence, remediation, and unverified limitations. Preserve detailed evidence in the artifact so later summaries do not change its meaning.

This audit does not authorize remediation, credential rotation, external issue filing, or production exploitation. Report material findings to the orchestrator. Missing scanner/network access limits coverage; continue the applicable source review and never equate an incomplete audit with a clean bill of health.

## Worker coordination

Follow the assigned brief and repository instructions. Load only the skill content relevant to the current task. For a material ambiguity or missing prerequisite, use `horch tell orchestrator` with a concise question, evidence, and the affected task. Continue independent work and block only the dependent action. If messaging is unavailable, record the blocker in the worker result. Do not wait on an interactive human prompt or add routine approval checkpoints when the work is already authorized.
