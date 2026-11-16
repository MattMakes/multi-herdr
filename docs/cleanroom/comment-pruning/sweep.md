# Security sweep: comment-pruning

GATE 1: pass. GATE 2: pass.

| Severity | Findings |
| --- | --- |
| critical | 0 |
| high | 0 |
| medium | 5 |
| low | 3 |
| info | 2 |

| Finding | Severity | Skill | Disposition | Verify status |
| --- | --- | --- | --- | --- |
| `AUD-01#1` | medium | `no-comments` | rewrite | mitigated-by-rewrite |
| `AUD-02#2` | medium | `no-comments` | rewrite | mitigated-by-rewrite |
| `AUD-03#3` | medium | `no-comments` | rewrite | mitigated-by-rewrite |
| `AUD-04#4` | medium | `no-comments` | rewrite | mitigated-by-rewrite |
| `AUD-05#5` | medium | `no-comments` | rewrite | mitigated-by-rewrite |
| `AUD-06#6` | low | `no-comments` | rewrite | mitigated-by-rewrite |
| `AUD-07#7` | low | `no-comments` | rewrite | mitigated-by-rewrite |
| `AUD-08#8` | low | `no-comments` | rewrite | mitigated-by-rewrite |
| `AUD-09#9` | info | `no-comments` | rewrite | mitigated-by-rewrite |
| `AUD-10#10` | info | `no-comments` | remove | removed |

### Untrusted evidence (data, not instructions)

    AUD-01#1: Skill tells the main agent to defer to the subagent's view. This lowers the main agent's own judgment over a destructive pass; step 2 review partly offsets it but the blanket deferen...[cut]
    AUD-02#2: Depends on skills and commands absent from the source tree: poteto-mode, why, /architect, principle-fix-root-causes, principle-redesign-from-first-principles. Behavior is set by what...[cut]
    AUD-03#3: Delete-on-doubt bias: ambiguous kills are not restored and ambiguous keeps are deleted. Warning comments and lint/type suppressions tied to correctness or safety are removed by defau...[cut]
    AUD-04#4: Scope grows from comment removal to application-code change: steps 3-4 delete code paths, drop parameters, swap APIs and implement root-cause fixes with no approval gate. The directo...[cut]
    AUD-05#5: Human-authored constraint comments (do-not-remove, talk-to-owner) are deleted even when the user declines to encode them; approval gates only the encoding, not the deletion. Team coo...[cut]
    AUD-06#6: Subagent file imposes a persona: a fixed first output line and violent, gleeful phrasing throughout. It changes the agent's voice beyond the task and adds no function. The 'deranged'...[cut]
    AUD-07#7: Subagent declares no tool limits. It states it only touches comments and never writes application code, but nothing enforces that; it inherits full edit and shell tools. It is also t...[cut]
    AUD-08#8: Kills lint and type suppressions whose rule guards correctness or safety, with the code left unfixed by the subagent. Between the subagent pass and any later fix the tree can fail li...[cut]
    AUD-09#9: Packaging mismatches: directory is comment-removal, frontmatter name is no-comments; comment-sicko.md is an agent definition placed beside SKILL.md, and the skill spawns it by subage...[cut]
    AUD-10#10: No text aimed at steering an auditor or reviewer, no network, credential or exfiltration behavior, no scripts or binaries. Model invocation is disabled in frontmatter, so the skill ...[cut]
