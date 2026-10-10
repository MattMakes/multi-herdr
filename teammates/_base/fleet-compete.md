---
name: fleet-compete
description: >
  The competition rules for the fleet orchestrator. horch appends this block
  to the orchestrator's briefing only when the fleet starts with
  `horch fleet --compete` (the `herdr-fleet` launcher). `{compete_rounds}` and
  `{compete_budget_usd}` come from `--compete-rounds` and
  `--compete-budget-usd`. Spec: docs/specs/fleet-dataset.md section 7.
---
== Competition mode ==
This fleet runs in competition mode. For some units you run a competition
round instead of one worker. 2 teammate configurations do the same unit, each
in its own git worktree. A blind judge picks the winner. The winner lands on
the working branch. Each round is a data point for model selection.
The real work comes first. Every unit still lands, with or without a round.

Choose a unit for a round only when all of these are true:
- The unit owns its files. No live unit edits them.
- The unit needs no uncommitted work. The candidates start from the last commit.
- A build or test gate decides the unit. The gate runs in each candidate worktree.
- The unit is implementation work. It is not research, design, review, a live pane check, or work that needs the network or a credential.
- This fleet session has started fewer than {compete_rounds} rounds.
In every other case, spawn the unit as usual. Prefer a unit whose gate is fast.

Run a round:
1. Write the plan file as usual. Commit it with its exact path:
   git commit -m "Plan: <unit>" -- <plan path>
2. Choose the baseline. The baseline is the teammate that you would spawn for the unit.
3. Run this command. Use a short slug for the unit.
   multi-herdr-dataset run --plan <plan path> --baseline <teammate> --candidates 2 --budget-usd {compete_budget_usd} --allow-dirty --promote-to compete/<slug> --detach
4. The command returns at once and prints "round <id> started". The round runs in its own herdr workspace. Go on with other work.
5. If the command exits with code 4, preflight refused the round. Read the reason. Spawn the unit as usual.
Warning: do not set --budget-usd above {compete_budget_usd}. Do not use the working branch as --promote-to.
Do not poll a round. Its report comes to your terminal. `multi-herdr-dataset status` shows every round.

Land the result:
- The round reports with 1 line that starts with "[compete-". Treat it as a worker DONE.
- If the line names "Promoted: compete/<slug>@<sha>", run this command in the project checkout:
  git cherry-pick HEAD..compete/<slug>
  Then run the checks of the plan. Verify the result like any DONE.
- If the cherry-pick stops on a conflict, run `git cherry-pick --abort`. Then spawn a Claude worker with a plan that applies the diff of compete/<slug> by hand.
- If your sandbox refuses a git write, spawn a Claude worker to run the git step.
- If the line names "Winner: none", spawn the unit as usual with the baseline teammate. The round stays in the dataset.
- If the line names a winner and "Promoted: none", spawn the unit as usual with the baseline teammate. Tell the operator the round id and the Reason.
- If the line says the round is STOPPED, the round stopped on an error. Spawn the unit as usual. Tell the operator the resume command that the Reason names. Do not run that command yourself: it does not return until the round ends.

Records:
- The judge labels the candidates. Do not run `horch verdict` on a candidate record.
- Run `horch verdict` on each worker that you check, as usual. This includes a worker that applies a round's diff by hand.
- Recorded cost and outcome facts inform. Do not choose teammates, efforts or units from them.
