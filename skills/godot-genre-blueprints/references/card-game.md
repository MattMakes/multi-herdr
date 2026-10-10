# Card game

Collectible card games and deckbuilders: the player draws cards, spends a
resource to play them, and their effects resolve against a board. Slay the
Spire, Hearthstone and Magic are the reference points.

## Core loop

Draw → evaluate the hand → play cards → resolve effects → discard and end
the turn. A deckbuilder adds an outer loop: win a fight → add, remove or
upgrade a card → next fight.

## Required systems

| system | what this genre needs | skill |
|---|---|---|
| Card definitions | one Resource per card: cost, type, effect list | `godot-resource-pattern` |
| Turn phases | draw, main, combat, end; whose turn | `godot-state-machine`, `godot-combat-system` (turn-based part) |
| Hand, drag and targeting | Control drag and drop, hover, focus | `godot-ui`, `godot-input-handling` |
| Card motion | draw, play and discard tweens | `godot-tween-animation` |
| Effect messages | "card played", "unit died" for triggers | `godot-event-bus` |
| Deck lists and runs | save decks, unlocks, run state | `godot-save-load` |
| Foil and glow | card shaders | `godot-shader-basics` |

## Scene tree (4.7)

```text
Match (Node)
├── Rules (Node; owns Deck, Hand, Discard as Arrays of CardData)
├── EffectStack (Node; the resolver below)
├── Board (Control)
│   ├── EnemyRow (HBoxContainer)
│   └── PlayerRow (HBoxContainer)
├── HandView (Control; places CardView children on a Curve2D arc)
├── Piles (Control; DrawPile, DiscardPile counters)
└── TargetArrow (Line2D or a Control with _draw())
```

`CardView` is a Control that shows one `CardData`. The views never own game
state; `Rules` does.

## Genre code

Card effects resolve last-in, first-out: a response played on top of a
spell resolves before the spell. Keep effects as `Callable`s on a stack and
let a reaction push more effects while the stack resolves.

```gdscript
extends Node

signal effect_resolved(source: StringName)

var _stack: Array[Dictionary] = []
var _resolving := false

func push(source: StringName, effect: Callable) -> void:
	_stack.push_back({"source": source, "effect": effect})
	if not _resolving:
		_resolve()

func _resolve() -> void:
	_resolving = true
	while not _stack.is_empty():
		var top: Dictionary = _stack.pop_back()
		# An effect may push reactions; they land on top and resolve first.
		(top["effect"] as Callable).call()
		effect_resolved.emit(top["source"])
	_resolving = false
```

For an arc hand, sample a `Curve2D` with `sample_baked(t * curve.get_baked_length())`
for each card, rotate the card by the curve tangent, and tween to the
target. A tween per card that is killed before a new one starts keeps fast
draws from fighting each other.

## Pitfalls

- Game logic inside the card UI script. The UI sends "play card 3 on target
  B" to `Rules`; `Rules` checks cost and legality and pushes effects.
- A resolve queue that pops from the front. Reactions must resolve first;
  pop from the back.
- An empty draw pile with no rule. Shuffle the discard pile into the deck,
  or apply the genre's fatigue rule; never soft-lock.
- Floats for cost, attack and health. Use `int`.
- Cards that jump between piles. Tween every move, about 0.15 to 0.3 s.
- A dragged card that slides under its neighbours. Raise it with
  `move_to_front()` or a higher `z_index` while dragging.
- Card Resources edited at runtime. A buff on one copy changes every copy.
  Duplicate a `CardData` when it enters a match, or keep runtime changes in a
  separate per-instance dictionary.
- A custom Resource with required `_init()` arguments. The inspector and
  `ResourceLoader` cannot create it. Give every argument a default.
- Removing cards from an Array inside a `for` loop over it. Iterate a copy,
  iterate in reverse, or use `filter()`.
- Shuffling with the global RNG. Seed a `RandomNumberGenerator` per match so
  replays and bug reports reproduce.
