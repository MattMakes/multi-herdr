---
name: godot-upgrade-tree-design
description: Design, price and judge an incremental game's upgrade tree, skill tree or meta-progression from bot and human playtests, not from guesses. It swaps the shipped tree for a dev-only hub-and-spoke diagnostic tree over every real upgradeable stat, lets bots with different buy policies play the real game headless, calibrates one price curve per chapter, measures each upgrade's worth by leave-one-out, and writes balance.json, the data each real tree is built from. It also covers strategy advocates, fun-band sweeps, a measured buy order, dead-node audits, spend gates, upgrade cues and human playtests. Use it whenever someone wants to balance, tune, price or playtest a tech tree, upgrade path, shop or currency economy, or asks which upgrades matter, what the best buy order is, whether prices or the buy order make players stall at a boss, why they have currency but nothing to buy, or whether an upgrade is felt. For a boss stall with no upgrade or economy cause, use godot-bot-balance-loop.
---

# Upgrade tree design: measure the tree with bots, judge it with people

Target engine: **Godot 4.7**. The bundled scripts are Python and use the standard library only.

An upgrade tree that is priced by hand drifts into walls, dead nodes and one dominant path, and nobody can say which
upgrade matters. This method replaces the shipped tree, in a dev-only mode, with the simplest tree that still touches
every real upgrade, and lets bots with different priorities play the real game. A report turns their traces into
`balance.json`, and the designer builds each real tree from it, then checks the result with human playtests. It works
because the diagnostic tree removes every choice except "which stat next", so the time a bot loses without a stat is
that stat's worth.

Terms: the **diag tree** (diagnostic tree) is the dev-only tree, and **diag mode** is the game running with it.
A **chapter** is a section of the game that ends with a major boss (a level, zone, biome or act). A **class** is
any character, vehicle or loadout with its own upgrades. A **policy** is a bot's buy rule.

The diag tree is 1 hub with 1 spoke per stat, caps that grow, and 1 pooled currency with 1 price curve.
Read `references/diag-tree-shape.md` for why each part of the shape is there.

## When to use

- A new or reworked upgrade tree, tech tree or meta-progression needs prices and an order from data.
- Players stall at a boss because of upgrade access, prices or the buy order, finish a chapter too fast, or report
  "currency but nothing to buy".
- You must pick global tuning (price base, caps, curves, spikes) against a stated fun target.
- You must learn which upgrades are dead, unfelt, or out of step with their text.
- Humans must play a build to answer design questions, and their play must be compared with the bots.

## When not to use

- The game has no bot that plays whole runs headless. Build that first (see `godot-bot-balance-loop`).
- The question is one fight's tuning at a fixed upgrade state. Use `godot-bot-balance-loop`.
- The numbers come from a scripted balance model and you only need the export and parity. Use `godot-balance-model-data`.
- The change adds or removes a mechanic. That is the owner's decision (see `references/design-research.md`), not a tuning task.

## Prerequisites

Check the 5 prerequisites in `references/prerequisites.md` first (stat pipeline, headless bot, determinism,
events, separate save). Report a missing one as a blocker.

## Method

Work in phases. Each phase ends with a check. The game side, the bot and the report can run in parallel once the
contract exists.

1. **Survey.** Read the stat pipeline, the tree data, the currencies, the unlock order and the bot. Record each
   stat's shipped per-level step, every game cap on a derived stat, the income per run of each currency per chapter,
   and the bot's wall seconds per run. Check: a design file holds these facts with their sources.
2. **Ask the owner the key decisions.** Offer a recommendation for each. The decisions set what the data means, so do
   not build before the answers.
   - The pool rule: 1:1 with the rarest spent first (default), per-chapter rates, or a rate table.
   - The hub scope: 1 hub shared by all classes, or 1 hub per class.
   - The price rule: 1 curve on the total level count, with a stage multiplier per unlocked class. Without it, the
     income jump at an unlock sells a hundred levels in 1 shop.
   - Specials: a 1-level unique verb is a special spoke outside the cap share; a boss reward is free at the kill.
3. **Write the contract.** Fill in `references/contract-template.md`: the data file, the runtime API, the versioned
   trace schema (`diag-trace/1`), the wall probe schema (`wall-probe/1`) and the policy file. Check: the game side,
   the bot and the report each name only fields the contract defines.
4. **Build the game side in a dev-only mode.** A deterministic generator writes the hub from the shipped tree data
   and keeps the `price` block. A runtime holds levels, the tier rule, closed spokes, the price, rarest-first spend,
   its own save and the trace writer. Add only 2 hooks to shared files: the stat pipeline reads diag levels in diag
   mode, and the shipped shop refuses buys in diag mode. Check: a test proves that diag mode off gives the shipped
   stats exactly, computed from live data, not from pinned numbers.
5. **Give the bot a policy flag and a sweep driver.** In the shop the bot buys by the policy until nothing is
   buyable. The driver plays N policies at once, each with its own save slot, out folder and trace. Only the shop
   changes; the bot keeps its own route and fight logic. Check: a smoke run of 2 policies for about 40 runs shows a
   boss rise and a boss die, and the logs have 0 script errors. A complete trace alone proves nothing.
6. **Calibrate the price per chapter.** Fit 1 value per chapter in chapter order: `base` from the first chapter, then
   each stage multiplier from the chapter where that class arrives. Each try resumes from the chapter-start snapshot
   and stops when the next chapter opens. Run 5 tries at once (for example the value x 1/9, 1/3, 1, 3, 9), then 4
   inside the bracket. Cut a try at a run or time cap, and mark it invalid on any script error line. Check: each
   chapter is OK (within about 15 % of its target minutes), FAST or WALL. WALL means a fight gates the pace, which is
   a finding for the designer, not a price to force. Sweep the growth factor 1 value at a time after that.
7. **Run the sweep.** Size it from the smoke run: playthroughs x wall hours / parallel slots. The default set is a
   balanced baseline (all weights 1) on 3 seeds, 4 or 5 themes from the game's verbs, 1 leave-one-out policy per
   shared stat spoke, and 5 seeded random policies. Generate the set with `make_policies.py`
   (`references/script-make-policies.md`; copy the block into a file). Check: every trace exists, every log has 0
   script errors, and the baseline seed spread is known.
8. **Probe each wall.** A wall is a boss with more failed tries than the owner allows (for example 3 or more).
   Replay that try from its snapshot once per spoke with +1 and +3 levels (a probe, not a buy). Check: the baseline replay equals the original try. The spokes with
   the biggest health drop are the spike candidates for that point of the real tree.
9. **Test a weaker player.** Run the top 3 policies with a seeded skill draw that misses fight verbs. A tree that
   works only for a perfect bot fails real players.
10. **Report.** Run `report.py` (`references/script-report.md`; copy the block into a file) on the sweep
    root with `--json`, the wall files and the targets. It writes `report.md`, `report.html` and `balance.json`
    (schema in `references/balance-json.md`). If your traces name the group or the chapter field differently, add
    `--group-key` and `--chapter-key`. Check: the report has no WARNING about traces with different stop rules, or
    you report 1 sweep at a time.
11. **Read the numbers as a designer.** Read pace, spoke worth, order per class, freedom, spikes and rare currencies
    as `references/reading-the-numbers.md` says. Check: each reading is a `notes` line.
12. **Cull and re-fit.** Cull the hub to the unique upgrade types, so the real tree has no near-duplicates. Re-fit
    the price for the culled tree, because a smaller tree at the old price is far too easy. Check: the culled tree
    passes the chapter targets again.
13. **Propose the real trees.** Write 1 short proposal per class with options. Any change to the shipped tree or to
    gameplay is the owner's decision; do not apply it yourself.

## Folded methods

8 more methods are in `references/`; see References. A new or removed mechanic, verb, behaviour or cue is a
proposal for the owner, not a build.

## Pitfalls

The 5 most important rows. The full table is in `references/pitfalls.md`.

| Symptom | Cause | Fix |
| --- | --- | --- |
| Every theme bought the same first spokes | A `level / weight` score ties at level 0 | Score `(level + 1) / weight`; break a tie by the higher weight |
| No buy for hundreds of runs | Weight 0 on more than about 20 % of spokes; the cap share can never be met | Keep weights above 0, or report the deadlock as a finding of the cap rule |
| A spoke takes levels with no effect | A game cap holds the derived stat | Close a spoke when 1 more level changes no derived stat; count it as at the cap; log the closing level |
| About 100 levels sold in 1 shop | 1 price curve plus an income jump at a class unlock | A stage multiplier per unlocked class, fitted per chapter |
| A clean trace, but no boss ever rose | A tool script failed to compile; the trace keys still filled | The smoke check proves key events; a `SCRIPT ERROR` line makes a try invalid |

## Artifacts

Build the diag tree generator and runtime, 2 hooks with a diag-off test, a bot policy flag, the sweep and
calibration drivers, the policy files, the report and the wall finder. The full list is in `references/artifacts.md`.

## Related skills

- `godot-bot-balance-loop`: builds the headless bot, its determinism and the wall diagnosis that this skill drives.
- `godot-balance-model-data`: owns the scripted balance model, its export and its parity with the game.
- `godot-headless-runs`: tool script rules, save isolation and run cleanup for every bot run here.
- `godot-visual-verification`: windowed captures for the felt-effect checks and cue captures.
- `godot-perf-measure`: frame budgets, shader warm-up and hitch maps for upgrade cues.
- `godot-release-and-store`: the exports that a human playtest build uses.
- `godot-feature-seams-and-saves`: the data seams and dev-mode flags that keep diag mode out of the shipped game.
- `godot-fleet-game-dev`: long background waits, parallel units, and the ask-and-wait protocol for owner decisions.

## References

- `references/diag-tree-shape.md`: why the diag tree has its shape.
- `references/prerequisites.md`: the 5 prerequisites; read first.
- `references/contract-template.md`: the contract to fill in (step 3).
- `references/balance-json.md`: the `balance.json` and `wall.json` schemas.
- `references/reading-the-numbers.md`: how a designer reads the report (step 11).
- `references/script-report.md`: `report.py` (step 10); read it to rebuild the report.
- `references/script-test-report.md`: tests and fixtures of `report.py`.
- `references/script-make-policies.md`: `make_policies.py` (step 7).
- `references/strategy-advocates.md`: the best play style or buy order.
- `references/fun-band-sweep.md`: global tuning against a fun target.
- `references/measured-buy-order.md`: replays score each upgrade.
- `references/wing-audit.md`: dead, walled or unfelt nodes.
- `references/spend-gates.md`: currency but nothing to buy.
- `references/upgrade-cues.md`: stat-keyed cues on screen and in sound.
- `references/human-playtest.md`: people play a build.
- `references/design-research.md`: redesign requests; the owner's call.
- `references/pitfalls.md`: the full pitfalls table.
- `references/artifacts.md`: the full list of tools to build.
