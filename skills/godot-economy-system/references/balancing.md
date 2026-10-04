# Balancing an economy

## Faucets and sinks

A **faucet** puts currency into the game: loot, quest rewards, sales to
shops, idle income. A **sink** takes it out: shop purchases, repairs, fees,
taxes, upgrades, consumables, respec costs. When faucets beat sinks over
time, prices lose meaning (inflation). When sinks beat faucets, players feel
poor and stop spending.

Plan both before you tune numbers:

1. List every faucet and every sink with its `reason` id (the wallet's
   ledger uses the same ids).
2. Estimate income per minute of play at a few points in the game (first
   hour, mid game, end game).
3. Price the important purchases as minutes of play: "a sword costs about
   20 minutes of mid-game income".
4. Add a repeatable sink for the late game (upgrades with rising cost,
   cosmetics, a fee on trades), or late-game money piles up.

## Gold per minute

Measure, do not guess. The wallet ledger (`SKILL.md`) already records
`(currency, delta, reason, msec)`. Sum it over a play session:

```gdscript
class_name EconomyReport
extends RefCounted


## Income and spend per minute for one currency, split by reason.
static func per_minute(entries: Array[Dictionary], currency: StringName,
		session_msec: int) -> Dictionary:
	var minutes: float = maxf(session_msec / 60000.0, 0.001)
	var by_reason: Dictionary[StringName, float] = {}
	var income: int = 0
	var spent: int = 0
	for e: Dictionary in entries:
		if e["currency"] != currency:
			continue
		var delta: int = e["delta"]
		if delta > 0:
			income += delta
		else:
			spent -= delta
		by_reason[e["reason"]] = by_reason.get(e["reason"], 0.0) + delta / minutes
	return {
		"income_per_min": income / minutes,
		"spent_per_min": spent / minutes,
		"by_reason": by_reason,
	}
```

Run it at the end of a play-test session or on a debug key, and print the
result. Watch:

- **Net per minute** (income minus spend). It should stay small and positive
  in the mid game.
- **The largest faucet.** One reason that gives most of the income is a
  farm route players will find.
- **Balances at the cap.** Players who sit at the cap have nothing to buy.

For a headless balance check, simulate many sessions in a `SceneTree`
script with a seeded `RandomNumberGenerator`: roll the loot tables for N
kills, apply the planned purchases, and check that the balance after an hour
lands in the target range.

## Prices that scale

Upgrade costs usually grow per level. Two common curves:

- Linear: `base + step * level`. Easy to read; falls behind late income.
- Exponential: `roundi(base * pow(growth, level))`, with `growth` around
  1.07 to 1.15 for idle games. It outruns linear income, which keeps a sink
  open.

Compute costs from a formula in one place, and test the first 100 levels for
overflow and for a cost of 0.

## Very large numbers

GDScript `int` stops at 9,223,372,036,854,775,807; one more wraps to a large
negative number with no error. A `float` reaches about 1.8 × 10^308 but
keeps only about 15 to 16 significant digits, which is fine for idle-game
numbers that display as "1.23e45" and wrong for exact counts.

For an idle game:

- Store the big values as `float` and accept the rounding, or as a
  mantissa-and-exponent pair (`float` mantissa in [1, 10), `int` exponent).
  Write the add, compare and format functions once and test them.
- Keep exact `int` for anything a player trades or that must match exactly
  (premium currency, item counts).
- Guard each `int` grant against overflow: the wallet's cap does this when
  every currency has a cap.

## Telemetry in builds

In a debug build, print a line per ledger batch or write the ledger to a file
under `user://`. Do not write the file once per change; batch it (every N
entries or every N seconds). Strip or disable telemetry in release builds
unless the game has a consent flow for it.

## Checks

- With a seeded loot simulation of 1,000 kills, income per kill is within the
  design range.
- The cost formula for levels 0 to 100 is positive and increasing.
- No currency without a cap can reach the `int` limit in the simulated play
  time.
