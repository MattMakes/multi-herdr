## Summary

Research ticket 02. Sources were read on 2026-11-11. Nothing was installed on this machine.

Q1, git itself:
- `rerere` replays only a resolution it recorded before. It removes no new conflict (F1).
- `ort` is the default strategy. It removes whitespace-only and rename conflicts with options. `-Xours`/`-Xtheirs` drop 1 side of each conflicting hunk (F2).
- `diff3` and `zdiff3` change only how a conflict is shown. They add the base text, which a resolver can use (F3).
- A custom merge driver is set in git config, not in the repo. The config is shared by all worktrees of 1 repository (F4).

Q2, structural merge tools. Only 2 tools merge Rust and are maintained:
- Mergiraf (GPLv3, v0.20.0, 2026-09-26). It merges Rust `.rs`. It treats module items, `impl` items, `use` lists, struct fields, enum variants and derive lists as unordered. It keeps a conflict for 2 insertions into 1 function body (F5, F6).
- weave (MIT or Apache-2.0, v0.7.1, 2026-11-02). It merges at the level of functions and other entities. It refuses ordered containers such as match arms. Its accuracy numbers are self-published only (F8).
- Spork and IntelliMerge are Java only. SemanticMerge is discontinued and has no Rust. Difftastic does not merge (F9 to F12).
- No published accuracy number exists for any structural tool on Rust. The Java numbers come from LastMerge and from Schesch et al. (F7, F13).
- On about 6,000 Java merges, git `ort` was correct on 46%, left 51% unhandled and was wrong on 3%. Spork was correct on 54% but wrong on 11% (F13).

Q3, Jujutsu: `jj` stores a conflict in a commit and rebases descendants automatically. A resolution squashed into the conflicted commit propagates to descendants. `jj` colocates with git, but it does not support git worktrees, `.gitattributes` or hooks. Git cannot represent a conflicted `jj` commit (F14, F15).

Q4, AI resolution:
- On Merge-Bench (2026, 7,938 hunks of at most 20 lines), the best model matched the developer text on 54.7% of Java hunks and 47.1% over all languages (F17).
- On ConflictBench, a 2026 LLM agent matched about 55% to 59% of true conflicts (F18).
- A clean merge can still be wrong. Git `ort` produced a clean merge that failed build or tests on 3% of merges. A structural tool raised that to 11% (Spork) or 50% (IntelliMerge) (F13, F19).
- CooperBench (2026): 2 agents that build 2 features in parallel and merge succeed about 30% to 50% less often than 1 agent that builds both features (F20).

Q5, agent systems: no system in the list documents a same-file merge by AST. The documented methods are:
- 1 worktree per agent with merge at the end (most systems).
- A merge queue that sends a conflict back to the author agent (Gas Town).
- 1 writer per file (Claude Code agent teams advice, MetaGPT) (F21).

Q6, combine at the file:
- Evidence for: CooperBench solo beats parallel (F20). Cognition and Claude Code docs advise 1 agent for same-file work (F22). Copilot Workspace planned changes per file (F23).
- Evidence against: agent success falls as the patch grows. It is below 10% for patches of more than 100 lines or 3 or more files. Instruction-following falls as the number of instructions grows (F24).
- Combined edits make tangled commits, which damage traceability unless each request gets its own commit (F25).

## Method and labels

Each finding has the label `verified` when I read the primary source in this session. It has `not verified` when I read only a search summary or a secondary source. Quotes are at most 2 lines.

## Q1. Git itself

F1. `git rerere` removes only conflicts that it saw before. It records the conflict and the hand resolution, and replays the resolution when the same conflict occurs again. It does not resolve a new conflict. It must be turned on with `rerere.enabled`. Evidence: https://git-scm.com/docs/git-rerere: "records the hand resolve when it is a new conflict, or reuses the earlier hand resolve when it is not." `verified`

F2. `ort` is the default strategy for a merge of 1 branch. It detects renames, builds a virtual base when there are several merge bases, and uses the histogram diff. Option `-Xignore-space-change` and related options remove conflicts that are whitespace only. Options `-Xours` and `-Xtheirs` resolve each conflicting hunk to 1 side, so the other side's change in that hunk is lost. Since git 2.50.0, `recursive` is a synonym for `ort`. Evidence: https://git-scm.com/docs/merge-strategies: "This is the default merge strategy when pulling or merging one branch." `verified`

F3. `merge.conflictStyle=diff3` adds the base text between the 2 sides. `zdiff3` does the same and moves lines that match on both sides out of the conflict region. Neither style changes whether a conflict occurs. The base text tells a resolver what each side changed. Evidence: https://git-scm.com/docs/git-merge: "zdiff3 ... is similar to diff3 but removes matching lines on the two sides from the conflict region". `verified`

F4. A custom merge driver is defined in git config as `merge.<name>.driver` with placeholders `%O %A %B %L %P`. A path selects it with `merge=<name>` in `.gitattributes`. Exit 0 means clean, a non-zero exit up to 128 means conflict, and above 128 means the merge fails. The built-in `union` driver keeps both sides without markers, in a random order. The repository config is shared by all worktrees unless `extensions.worktreeConfig` is on. So 1 driver setup serves every agent worktree of 1 clone. Evidence: https://git-scm.com/docs/gitattributes: "the definition of a merge driver is done in the .git/config file, not in the gitattributes file". Command `man git-worktree` (git 2.53.0): "By default, the repository config file is shared across all worktrees." `verified`

## Q2. Structural merge tools

| Tool | Languages | Rust | Git driver | License | Last release |
|---|---|---|---|---|---|
| Mergiraf | about 30 languages plus JSON, TOML, YAML and others | yes, `.rs` | yes | GPLv3 | v0.20.0, 2026-09-26 |
| weave | about 40 languages via tree-sitter | yes | yes | MIT or Apache-2.0 | v0.7.1, 2026-11-02 |
| Spork | Java | no | yes | MIT | v0.5.1, 2021-07-09 (last commit 2025-02-25) |
| IntelliMerge | Java | no | not documented | MIT | no release (last commit 2021-07-21) |
| SemanticMerge | C#, VB.NET, Java, C | no | no (standalone discontinued) | proprietary | not sold |
| Difftastic | many, diff only | diff only | no merge | MIT | 0.72.0, 2026-10-29 |

F5. Mergiraf supports Rust (`.rs`) and installs as a git merge driver. The driver line is `mergiraf merge --git %O %A %B -s %S -x %X -y %Y -p %P -l %L`, with `merge=mergiraf` in attributes. It runs a line merge first and returns it when it is clean. It runs the structured merge only on a file with a conflict. It keeps conflict markers for "suspicious cases". It resolves: changes to independent syntax elements, insertions into unordered parents, a format change against a content change, and a move against an edit. It does not resolve 2 insertions where order matters, such as 2 statements added to 1 block. It reports a duplicate key added on both sides as a conflict, where git would merge it. Evidence: https://mergiraf.org/languages.html ("Rust (*.rs)"), https://mergiraf.org/usage.html, https://mergiraf.org/conflicts.html, https://mergiraf.org/architecture.html, https://codeberg.org/mergiraf/mergiraf ("License: GPLv3"), https://codeberg.org/mergiraf/mergiraf/releases. `verified`

F6. The Mergiraf Rust profile marks 10 parent kinds as commutative: `source_file` and `declaration_list` (top-level, `mod` and `impl` items), `use_list`, `trait_bounds`, `enum_variant_list`, `field_declaration_list`, `field_initializer_list`, `function_modifiers`, `where_clause` and derive attribute lists. So 2 branches that each add a function to 1 module, 1 `impl` block or 1 `mod tests` block merge cleanly. Function bodies (`block`) and `match` arms are not in the list, so 2 insertions there stay a conflict. Evidence: https://codeberg.org/mergiraf/mergiraf/raw/branch/main/src/supported_langs.rs: `CommutativeParent::new("declaration_list", " {\n", "\n", "\n}\n")`. `verified`

F7. Mergiraf publishes no accuracy numbers. The only independent evaluation is LastMerge (Duarte, Borba, Cavalcanti, arXiv 2507.19687, 2025). It used 5,229 Java merge scenarios from 1,116 projects. Against Spork, Mergiraf had 290 added false positives and 62 added false negatives. Spork had 150 and 107. The 2 tools disagreed on conflicts in 12.2% of scenarios. The authors found "no evidence that generic structured merge significantly impacts merge accuracy". I could not determine any accuracy number for Mergiraf on Rust. Evidence: https://arxiv.org/html/2507.19687. `verified`

F8. weave (Ataraxy Labs) is a Rust tool. It merges at the level of entities (functions, classes, keys) through tree-sitter, and it supports Rust. It installs with `weave setup` or `git config merge.weave.driver "weave-driver %O %A %B %L %P"`. It resolves: different entities changed on each side, the same entity added identically on both sides, and different keys in 1 object. It keeps a conflict for: modify against delete, incompatible changes to 1 entity, 2 decorators on 1 function, and ordered containers such as statement sequences and match arms. It also supports `jj` as a merge tool. Its numbers are self-published: 31 of 31 correct on 31 hand-written scenarios, against 28 of 31 for Mergiraf and 17 of 31 for git. A replay of 4,971 file merges from 5 repositories (not Rust) gave 344 wins and 86 regressions against git. I found no independent evaluation. Evidence: https://github.com/Ataraxy-Labs/weave: "Resolves merge conflicts that Git can't by parsing code into functions, classes, and keys with tree-sitter". Release date from the GitHub releases API: `v0.7.1 2026-11-02T23:16:57Z`. `verified` for the features. `not verified` for the benchmark numbers.

F9. Spork is a structured merge tool for Java only. Its paper (IEEE TSE 2022) used 890 merge scenarios and reports formatting preservation better than JDime by an order of magnitude. Evidence: https://github.com/ASSERT-KTH/spork ("an AST based structured merge research tool for Java"), GitHub releases API: `v0.5.1 2021-07-09`. Paper numbers: https://arxiv.org/abs/2202.05329. `verified` for the tool. `not verified` for the paper numbers.

F10. IntelliMerge is a refactoring-aware merge tool for Java only. The open-source version has received no updates since 2020. Its paper reports 58.90% fewer conflicts than git, with 88.48% precision and 90.22% recall on its own metric. A replication (RefMerge, arXiv 2112.10370) came within 10% and argued that the metric does not measure correctness. Evidence: https://github.com/Symbolk/IntelliMerge ("no updates for the open source version"), https://dl.acm.org/doi/10.1145/3360596. `verified` for the tool. `not verified` for the numbers.

F11. SemanticMerge is no longer sold as a standalone product. Its technology ships inside Unity Version Control (formerly Plastic SCM) for C#, VB.NET, Java and C. It has no Rust support. Evidence: https://www.plasticscm.com/semanticmerge (search summary: "you cannot acquire new licenses or renew existing licenses"). `not verified`

F12. Difftastic diffs files by syntax. It does not merge. Evidence: https://github.com/Wilfred/difftastic: "Merging. AST merging is a hard problem that difftastic does not address." crates.io API: `0.72.0 2026-10-29`. `verified`

F13. Schesch, Featherman, Yang, Roberts and Ernst (ASE 2024) ran merge tools on about 6,000 Java merges where both parents pass their tests. "Incorrect" means a clean merge that fails to compile or fails the tests. Results:

| Tool | Correct | Unhandled | Incorrect |
|---|---|---|---|
| git `ort` | 46% | 51% | 3% |
| git `ort` ignore-space | 48% | 49% | 3% |
| Spork | 54% | 35% | 11% |
| IntelliMerge | 24% | 26% | 50% |
| IVn-ignorespace | 52% | 45% | 3% |

If an incorrect merge costs about 2 times an unhandled conflict, Spork becomes the worst tool except IntelliMerge. Evidence: https://arxiv.org/html/2410.09934: "Accounting for the cost of incorrect merges changes results, compared to previous experiments." `verified`

## Q3. Jujutsu (`jj`)

F14. `jj` records a conflict inside a commit as a logical structure, not as text markers. A rebase that meets a conflict succeeds, and `jj` rebases all descendants automatically. To resolve, a user runs `jj new <conflicted>`, edits the file or runs `jj resolve`, and then runs `jj squash` into the conflicted commit. Descendants are then rebased onto the resolved commit, and descendants that did not touch the file become conflict-free. Evidence: https://docs.jj-vcs.dev/latest/conflicts/: "Unlike most other VCSs, Jujutsu can record conflicted states in commits." https://docs.jj-vcs.dev/latest/tutorial/: "commit C automatically got rebased on top of the resolved B2, and that C is also resolved". `verified`

F15. `jj git init` in an existing git repository makes a colocated workspace, and `jj` and `git` commands work in it. The documented limits are: git worktrees are not supported (`jj workspace` is the replacement), `.gitattributes` is not supported, hooks are not supported, and submodules are not supported. A conflicted commit cannot be represented in git. `jj` stores it with a non-standard `jj:trees` header, and "Git tools will have trouble with revisions that contain conflicted files." `jj` does not run git merge drivers. It runs configured merge tools through `jj resolve`, and `mergiraf` is a built-in tool name. The latest `jj-cli` is 0.47.1 (2026-11-05, crates.io API). Evidence: https://docs.jj-vcs.dev/latest/git-compatibility/, https://docs.jj-vcs.dev/latest/config/. `verified`

## Q4. AI resolution of merge conflicts

F16. Earlier neural tools: MergeBERT (Microsoft, FSE 2022) reached 63% to 68% accuracy on Java, JavaScript, TypeScript and C#. "Correct" means a string match with the developer's resolution. DeepMerge (TSE 2022) resolved 37% of non-trivial JavaScript merges, and 78% of merges of at most 3 lines. DeepMerge can only select whole lines from the conflict. Evidence: https://arxiv.org/abs/2109.00084, https://arxiv.org/abs/2105.07569. `not verified`

F17. Merge-Bench (Schesch and Ernst, arXiv 2605.25890, May 2026) has 7,938 real conflict hunks from 1,439 repositories, each of at most 20 lines. It includes 716 Rust hunks from 147 repositories. Java results, as exact text match, normalized code match and give-up rate:

| Model | Exact | Code match | Gives up |
|---|---|---|---|
| Gemini 2.5 Pro | 54.7% | 62.5% | 3.4% |
| LLMergeJ 14B (trained) | 48.8% | 58.9% | 5.6% |
| o3 Pro | 46.1% | 54.3% | 10.6% |
| Claude Opus 4 | 44.4% | 51.2% | 21.2% |

Over all languages, Gemini 2.5 Pro scores 47.1% exact. A model that does not give up mostly fails by producing "different code". The text gives no Rust-specific number. The paper does not classify failures by conflict type. It does not check whether a resolution compiles. Evidence: https://arxiv.org/html/2605.25890v1. `verified`

F18. ConflictBench (Shen and Meng, JSS 2024) has 180 Java scenarios. 136 are true conflicts and 44 are false conflicts. A 2026 study (Shen, arXiv 2607.27674) ran LLM agents with a generate-validate-retry loop. The agents matched the developer's resolution on about 55% to 59% of true conflicts. The best classic tool, AutoMerge, scored 36.7%. The classic tools abstained on 20% to 90%, and the LLM did not abstain. The LLM judge "accepted 4 of the 5 resolutions that fail the deterministic structural check". Evidence: https://arxiv.org/abs/2607.27674. `verified` for 2607.27674. `not verified` for the ConflictBench counts.

F19. Merges that are textually clean but wrong in behavior:
- Git `ort` gives a clean merge that fails build or tests on 3% of merges. Spork gives 11% and IntelliMerge 50% (F13). `verified`
- Brun et al. (TSE 2013) found that conflicts are "frequent, persistent" and appear as build and test failures, not only as text overlap. Evidence: https://homes.cs.washington.edu/~mernst/pubs/vc-conflicts-tse2013-abstract.html. `verified`
- Tools that detect semantic conflicts with generated unit tests (SAM, Da Silva et al., JSS 2024) have a high rate of false negatives. Evidence: https://arxiv.org/pdf/2310.02395. `not verified`
- Mori and Hashimoto (arXiv 2607.07987) found incorrect results from existing merge tools, the git companion tool included, in 43,774 Java file merges. Evidence: https://arxiv.org/abs/2607.07987: "we found a number of incorrect results reported by existing tools". `verified`

I could not determine a published rate of clean-but-wrong merges for Rust, or for an LLM resolver measured by tests.

F20. CooperBench (arXiv 2601.13295, 2026) gives 2 agents 2 features on 1 repository. Each agent works in its own container. The 2 patches are merged with `git merge-file`, then a union merge, then a small learned resolver. In "Solo", 1 agent implements both features. Coop success was about 30% lower on average, and pooled retention was 0.59. GPT-5 and Claude Sonnet 4.5 reached about 25% in Coop, about half of Solo. 77.3% of tasks have conflicting ground-truth solutions. "Divergent architecture" (semantic loss after a clean merge) caused 29.7% of failures. "Work overlap" caused 33.2%. Evidence: https://arxiv.org/abs/2601.13295: "achieve on average 30% lower success rates when working together". https://arxiv.org/html/2601.13295. `verified`

## Q5. Multi-agent coding systems: 2 agents on 1 file

F21. Documented methods:

| System | Method | Evidence | Label |
|---|---|---|---|
| Claude Code agent teams | 1 writer per file (advice). Task claims use a file lock, but file edits have no lock. | https://code.claude.com/docs/en/agent-teams: "Two teammates editing the same file leads to overwrites. Break the work so each teammate owns a different set of files." | verified |
| Claude Code worktrees | 1 worktree per agent. The user merges. | https://code.claude.com/docs/en/worktrees | verified |
| Cursor cloud agents | 1 VM and branch per agent, merge at the end by PR. Same-file conflicts are not documented. | https://cursor.com/docs/cloud-agent: "work on a separate branch, then push changes to your repo for handoff" | verified |
| Codex cloud tasks | 1 workspace per task, commit or PR. Conflicts are not documented. | https://learn.chatgpt.com/docs/cloud: "Each task has its own workspace" | verified |
| Devin | 1 VM per managed Devin, 1 PR each. A coordinator session "resolves conflicts", with no documented method. | https://docs.devin.ai/work-with-devin/advanced-capabilities | verified |
| OpenHands | Sub-agents share 1 workspace. No built-in conflict handling. The OpenHands blog advises dependency-aware sequencing, worktrees or file locks. | https://www.openhands.dev/blog/agentic-swarm: "several agents fight over the same files" | verified |
| Aider | 1 agent, 1 commit per edit. Parallel agents are not documented. | https://aider.chat/docs/git.html | verified |
| MetaGPT | 1 writer per file. One Engineer writes the files 1 at a time in a loop. | https://raw.githubusercontent.com/FoundationAgents/MetaGPT/main/metagpt/roles/engineer.py: `for todo in self.code_todos:` | verified |
| ChatDev | Sequential waterfall phases. 1 instructor and 1 assistant agent write the code in turn, so in effect 1 writer. | https://arxiv.org/abs/2307.07924 | not verified |
| Gas Town | 1 worktree per agent, plus a merge queue (Refinery, Bors-style, batches and bisects). On a conflict the Refinery sends `REWORK_REQUEST` with `Conflict-Files`. The author agent rebases and the Refinery retries. | https://github.com/steveyegge/gastown, https://raw.githubusercontent.com/steveyegge/gastown/main/docs/design/mail-protocol.md | verified |
| Conductor | 1 workspace and branch per task, merge at the end by PR. Conflicts are not documented. | https://www.conductor.build/docs/ | verified |
| Vibe Kanban | 1 worktree per task, rebase before merge. On a rebase conflict, the user can send generated instructions so the agent resolves it. The project is sunsetting. | https://vibekanban.com/docs/core-features/resolving-rebase-conflicts, https://github.com/BloopAI/vibe-kanban | not verified (conflict page); verified (README) |
| claude-squad | 1 worktree per session, push the branch. Conflicts are not documented. | https://github.com/smtg-ai/claude-squad: "Each task gets its own isolated git workspace, so no conflicts." | verified |
| Crystal | 1 worktree per session. Deprecated in February 2026 and replaced by Nimbalyst. Conflicts are not documented. | https://github.com/stravu/crystal | verified |

No system in the list documents a structural merge tool in its merge path. Only weave advertises use for agent fleets, and it is a tool, not a system (F8).

## Q6. Combine at the file

F22. Agent vendors advise 1 agent for work on the same file. Cognition: "Actions carry implicit decisions, and conflicting decisions carry bad results". It recommends "a single-threaded linear agent", whose limit is the context window (Walden Yan, 2025-06-12). Claude Code docs: "For sequential tasks, same-file edits, or work with many dependencies, a single session or subagents are more effective." Evidence: https://cognition.com/blog/dont-build-multi-agents, https://code.claude.com/docs/en/agent-teams. `verified`

F23. Prior art for planning by file: GitHub Copilot Workspace made a plan that listed each file to change, with a list of actions for each file. One implementation step then produced the file. The product was sunset on 2025-05-30. Evidence: https://github.blog/ai-and-ml/github-copilot/5-tips-and-tricks-when-using-github-copilot-workspace/: "Copilot Workspace figures out which files it must alter, and exactly what it must accomplish in each file". `verified` for the plan. `not verified` for the sunset date (githubnext.com did not render). I could not determine a published system that merges change requests from different features into 1 task on 1 file.

F24. Size limits of 1 combined edit:
- SWE-bench-Live (arXiv 2505.23419): "a single-file patch that changes fewer than five lines is solved almost one time in two (48%)". "Once the patch edits three or more files, or spans more than one hundred lines, the success rate falls below ten per-cent". `verified`
- SWE-rebench (arXiv 2505.20411): resolve rate 28.6% for 1 file, 20.6% for 2 files and 17.5% for 3 or more files. `not verified`
- A 2026 study (arXiv 2608.15579) found that patch size stops being significant once task hardness is controlled for. So size may be a proxy for hardness. `not verified`
- IFScale (arXiv 2507.11538): the best frontier models reach 68% accuracy at 500 simultaneous instructions. Errors are mostly omissions, with a bias toward earlier instructions. `verified` for 68%. `not verified` for the omission detail.

F25. Traceability: a commit that serves several requests is a tangled change. Herzig and Zeller (MSR 2013) found up to 15% of bug fixes tangled, and 16.6% of source files wrongly linked to bug reports. Herbold et al. estimate tangled changes at 32% to 50%. Aider makes 1 commit per edit (F21), which keeps 1 request per commit inside 1 session. I could not determine a study that measures the traceability of combined agent edits. Evidence: https://www.microsoft.com/en-us/research/wp-content/uploads/2013/01/msr2013-untangling.pdf, https://arxiv.org/pdf/2011.06244. `not verified`

## What the facts mean for options A to E

Option A, rebase and re-verify. Plain git leaves about half of conflicting Java merges unhandled. About 3% of its clean merges fail build or tests, so the re-run of checks after a clean rebase is needed (F13, F19). `rerere` helps only when the same conflict repeats, such as a branch rebased several times (F1). `zdiff3` gives the integrator agent the base text at no cost (F3). The integrator agent can expect about 45% to 60% agreement with a human resolution, and a correct-looking result can still be wrong (F17, F18). Gas Town and Vibe Kanban use this pattern. Gas Town sends the conflict back to the author agent and does not use a separate integrator (F21).

Option B, combine at the file. CooperBench shows that 1 agent given 2 features succeeds about 30% to 50% more often than 2 agents whose patches are merged (F20). Cognition, the Claude Code docs and MetaGPT support 1 writer per file (F21, F22). The limits are size and instruction count. Agent success falls below 10% for patches of more than 100 lines or 3 or more files, and instruction-following degrades as requests are added (F24). A combined edit is a tangled change unless the writer commits each request separately (F25). No published system combines requests from different features in this way (F23).

Option C, integration lane. Most systems in F21 merge at the end, and none documents a shared integration branch for agents. Gas Town's Refinery is a merge queue to `main` that batches, bisects and sends conflicts back (F21). The facts here do not measure the cost of E2E checks on a batch. Ticket 01 covers that.

Option D, structural merge. For Rust, Mergiraf and weave are the only maintained options (F5, F8, F9 to F12). They remove the most common parallel-agent conflict in Rust: 2 branches that add items to 1 module, `impl` block, `mod tests`, `use` list, struct or enum (F6, F8). They do not remove 2 insertions into 1 function body or 1 `match` (F5, F6, F8). There is no Rust accuracy data. In Java, a structural tool raised wrong clean merges from 3% to 11% (F7, F13). So test re-runs after a structural merge are needed. 1 driver config serves all worktrees of 1 clone (F4). `jj` does not use `.gitattributes` or git worktrees, so it does not fit the current worktree model (F15).

Option E, a mix. The facts support these layers: 1 writer for 1 file inside 1 idea's scope (F20, F22), task sizes kept small (F24), and 1 commit for each request (F25). Across ideas, the facts support a structural driver for Rust item-level additions (F6, F8), followed by a test re-run on every clean merge (F13, F19). A conflict that remains goes to an agent that gets `zdiff3` context (F3, F17). The facts do not show where the size limit for 1 combined task is. F24 gives 100 lines or 3 files as a point where agent success drops.
