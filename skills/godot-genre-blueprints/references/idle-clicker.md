# Idle / clicker

Incremental games: clicks and generators produce a currency that buys more
generators, with numbers that grow past what a float can show. Cookie
Clicker and Adventure Capitalist are the reference points.

## Core loop

Click → buy generators → earn while idle → buy upgrades → prestige (reset
for a permanent multiplier) → grow faster than the last run.

## Required systems

| system | what this genre needs | skill |
|---|---|---|
| Big numbers | mantissa and exponent, compare, add, multiply, format | this reference |
| Generators and upgrades | cost growth, output rate; data in Resources | `godot-resource-pattern`, `godot-economy-system` |
| Offline progress | real elapsed time between sessions | `godot-gameplay-loops` (harvest: idle and offline gains), `godot-save-load` |
| Prestige | separate run state from permanent state | `godot-save-load` |
| Number labels and panels | update on change, not every frame | `godot-ui`, `godot-hud-system` |
| Click feedback | pooled floating numbers, tweens | `godot-tween-animation` |
| Mobile battery | low processor mode, background behaviour | `godot-mobile-development` |

## Scene tree (4.7)

```text
Game (Node)
├── Economy (autoload; currency, generators, tick)
├── SaveClock (autoload; last-save unix time, offline payout)
├── Main (Control)
│   ├── CurrencyLabel (Label)
│   ├── ClickTarget (TextureButton)
│   ├── Generators (VBoxContainer; one row per generator Resource)
│   └── PrestigePanel (Control)
└── FloatingNumbers (Control; pooled Labels)
```

## Genre code

A float overflows to `inf` near 1e308, and idle games pass that. Store a
mantissa in [1, 10) and an integer exponent.

```gdscript
extends RefCounted

var m := 0.0   # mantissa, 1 <= m < 10, or 0
var e := 0     # base-10 exponent

func set_value(value: float) -> void:
	m = value
	e = 0
	normalize()

func normalize() -> void:
	if m == 0.0:
		e = 0
		return
	var shift := floori(log(absf(m)) / log(10.0))
	m /= pow(10.0, shift)
	e += shift

func add(o) -> void:
	if o.m == 0.0:
		return
	if m == 0.0 or o.e - e > 15:
		m = o.m
		e = o.e
		return
	if e - o.e > 15:
		return
	m += o.m * pow(10.0, o.e - e)
	normalize()

func format() -> String:
	if e < 3:
		return str(roundi(m * pow(10.0, e)))
	const SUFFIX := ["K", "M", "B", "T", "Qa", "Qi"]
	var group := e / 3
	if group <= SUFFIX.size():
		return "%.2f%s" % [m * pow(10.0, e % 3), SUFFIX[group - 1]]
	return "%.2fe%d" % [m, e]
```

Give the class a `class_name` (for example `BigNum`) and type the `add`
argument with it. Add `multiply` the same way: multiply mantissas, add
exponents, normalize. Generator cost is usually `base * growth^owned` with growth
1.07 to 1.15; compute it with the same big-number type.

## Pitfalls

- Plain floats for currency. They reach `inf` and the save breaks. Use the
  big-number type from the first commit.
- Income from `Timer` nodes. Accumulate `delta` in one economy tick so
  income stays exact when frames drop.
- Offline time from `Time.get_ticks_msec()`. It restarts with the app. Save
  `Time.get_unix_time_from_system()` and pay out the difference on load, with
  a cap.
- Labels set every frame. Update when the value changes, or at most about
  10 times per second.
- A prestige that feels like a loss. The first reset should make the next
  run clearly faster, often 2 to 5 times.
- Parsing saved numbers with `to_int()`. Save the mantissa and exponent as
  two fields.
- A full CPU on a static screen. Turn on `OS.low_processor_usage_mode`.
- New Label nodes per click. Pool the floating numbers.
