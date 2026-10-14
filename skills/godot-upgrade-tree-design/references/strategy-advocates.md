# Strategy advocates and policy sweeps

This file belongs to the `godot-upgrade-tree-design` skill. The terms (diag tree, chapter, class, policy) are defined in its SKILL.md.

Use this when you must learn which play style or buy order is best, with each style pushed to its best. A fixed
balanced bot buys in data order, so its order is an input, not a result; advocates make the order a result.

1. Run a plain baseline policy first. Every strategy is compared with it.
2. Write 1 shared advocate plan and 1 shared measure tool (a playstyle report: chapter minutes, failed boss tries,
   tries at the hardest boss, stall runs). Then every advocate uses the same definitions.
3. Make the full-run bot read a policy file (weights per spoke, order kind, special buy timing). Prove that the bot
   without the policy flag gives the same output as before.
4. Define each strategy as weights or rules (for example damage first, economy first, generalist, survivor,
   measured value). Give each strategy 1 advocate (a person or an agent session) in its own working copy.
5. Each advocate writes variants with a small maker script and plays each on the same 3 seeds. Change 1 idea per
   variant. A gain inside the seed spread is no gain. Stop after 2 variants in a row that do not beat the best.
6. Play the best variant once at a weaker skill setting.
7. Each advocate writes a case file: the variants table, the best policy, the weaker-skill result, a proposed
   first-N-buys order, risks and dead ends. Verify each headline from the traces. Play each proposed order as an
   order file on 3 seeds.
8. Round 2: each advocate reads every other case and answers or concedes. A neutral writer then compares the cases,
   votes on the first nodes of each path, and lists the open questions for the owner.
9. Copy each best policy into a panel folder. The panel feeds the fun-band config sweep in `fun-band-sweep.md`.
