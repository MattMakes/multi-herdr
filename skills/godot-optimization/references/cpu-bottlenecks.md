# CPU Bottlenecks (Script-Side Hot Paths)

Reference for `skills/godot-optimization/SKILL.md` — GDScript performance patterns: avoiding allocations, StringName, typed arrays, static typing, preload vs load.

> ← Back to [SKILL.md](../SKILL.md)

---

> The patterns in this section are GDScript-specific, but the underlying principles (avoid per-frame allocations, use efficient comparisons, prefer typed collections) apply equally to C#. Each subsection includes a C# equivalent where the translation is non-trivial.

### Avoid Allocations in _process

Creating new `Array`, `Dictionary` or `String` values inside `_process` or `_physics_process` allocates heap memory every frame. GDScript has no .NET garbage collector: these values are reference-counted and freed when the last reference goes. `Vector2`, `Vector3` and other small vectors are not allocations: they are stored inline in `Variant`. `Array`, `Dictionary`, `String` and managed C# objects allocate in different ways, so measure each one on its own terms.

```gdscript
# WRONG — allocates a new Array every frame
func _process(_delta: float) -> void:
    var nearby := get_tree().get_nodes_in_group("enemies")  # new Array each call
    for enemy in nearby:
        _check_aggro(enemy)

# RIGHT — cache the group query result, or use an Area2D overlap list
var _enemies: Array[Node] = []

func _ready() -> void:
    _enemies = get_tree().get_nodes_in_group("enemies")
    get_tree().node_added.connect(_on_node_added)
    get_tree().node_removed.connect(_on_node_removed)

func _on_node_added(node: Node) -> void:
    if node.is_in_group("enemies"):
        _enemies.append(node)

func _on_node_removed(node: Node) -> void:
    _enemies.erase(node)

func _process(_delta: float) -> void:
    for enemy in _enemies:  # no allocation
        _check_aggro(enemy)
```

**C#:**

```csharp
// WRONG — querying the group every frame allocates a new Godot.Collections.Array
public override void _Process(double delta)
{
    var nearby = GetTree().GetNodesInGroup("enemies"); // new array each call
    foreach (var enemy in nearby)
        CheckAggro(enemy);
}

// RIGHT — cache the list and maintain it via signals
private readonly List<Node> _enemies = new();

public override void _Ready()
{
    foreach (var node in GetTree().GetNodesInGroup("enemies"))
        _enemies.Add(node);
    GetTree().NodeAdded += OnNodeAdded;
    GetTree().NodeRemoved += OnNodeRemoved;
}

private void OnNodeAdded(Node node)
{
    if (node.IsInGroup("enemies"))
        _enemies.Add(node);
}

private void OnNodeRemoved(Node node)
{
    _enemies.Remove(node);
}

public override void _Process(double delta)
{
    foreach (var enemy in _enemies) // no allocation
        CheckAggro(enemy);
}
```

```gdscript
# Temporary vectors are inline values, not heap allocations and not a GC pause.
# Keep them local: a shared member vector adds mutable state for no measured gain.
extends Node2D

var _markers := PackedVector2Array()

func _process(_delta: float) -> void:
    _markers.resize(100)
    for i in range(100):
        var offset := Vector2(i * 10.0, 0.0)
        _markers[i] = position + offset
```

**C#:**

```csharp
// In C#, Vector2 is a struct (value type) — no heap allocation. Keep it local too.
public override void _Process(double delta)
{
    for (int i = 0; i < 100; i++)
    {
        var offset = new Vector2(i * 10.0f, 0.0f);
        DrawMarker(Position + offset);
    }
}
```

### Use StringName for Comparisons

`StringName` uses interned hashing; comparing two `StringName` values is an O(1) integer comparison. Comparing `String` values is O(n): the comparison scans both strings without building a new string. `StringName == StringName` is still cheaper because it compares interned pointers.

```gdscript
# WRONG — String comparison in a hot path
func _on_body_entered(body: Node) -> void:
    if body.name == "Player":  # String comparison
        _start_aggro()

# RIGHT — StringName literal (&"...") is interned at compile time
func _on_body_entered(body: Node) -> void:
    if body.name == &"Player":  # O(1) hash comparison
        _start_aggro()
```

**C#:**

```csharp
// StringName in C# — cache as a static readonly field for O(1) comparison
private static readonly StringName PlayerName = new("Player");

private void OnBodyEntered(Node body)
{
    if (body.Name == PlayerName) // interned comparison
        StartAggro();
}
```

```gdscript
# Cache StringName constants at the class level for repeated use
const ACTION_JUMP := &"jump"
const ACTION_FIRE := &"fire"
const GROUP_ENEMIES := &"enemies"

func _process(_delta: float) -> void:
    if Input.is_action_pressed(ACTION_JUMP):
        _jump()
    if Input.is_action_just_pressed(ACTION_FIRE):
        _fire()
```

**C#:**

```csharp
// Cache StringName constants as static readonly fields
private static readonly StringName ActionJump = new("jump");
private static readonly StringName ActionFire = new("fire");
private static readonly StringName GroupEnemies = new("enemies");

public override void _Process(double delta)
{
    if (Input.IsActionPressed(ActionJump))
        Jump();
    if (Input.IsActionJustPressed(ActionFire))
        Fire();
}
```

### Typed Arrays

Typed arrays (`Array[Node]`, `Array[int]`) do not skip type checks: every typed `Array` write calls the type validator. Typing gives safety and lets the compiler specialize nearby expressions that read the elements. Packed arrays give contiguous value storage, which is the layout advantage for value types.

```gdscript
# Untyped — no element type; reads stay dynamic
var untyped_bullets = []

# Typed — each write is validated; reads are typed for the compiler
var bullets: Array[Node] = []

# PackedArrays are the most efficient for value types — stored as contiguous memory
var positions: PackedVector2Array = PackedVector2Array()
var velocities: PackedFloat32Array = PackedFloat32Array()
```

**C#:**

```csharp
// C# is statically typed — use concrete generic collections for best performance.
// Avoid Godot.Collections.Array (untyped) in hot paths; prefer List<T> or arrays.

// Untyped Godot collection — boxing and type checks on every access
Godot.Collections.Array bullets = new();

// Typed .NET collection — no boxing, cache-friendly
List<Bullet> bullets = new();

// For value types, use plain arrays for maximum throughput (contiguous memory)
Vector2[] positions = new Vector2[256];
float[] velocities = new float[256];
```

### Static Typing Benefits

<!-- csharp-parity: n/a — C# is statically typed by definition; there is no untyped alternative to contrast against, so this optimisation does not exist there. -->

Static typing enables the GDScript VM to emit more efficient bytecode and catches errors at parse time rather than runtime. **This one is GDScript-only** — C# has no dynamic counterpart to trade away, so it starts where the "RIGHT" example below ends.

```gdscript
# Untyped — every operation goes through dynamic dispatch
func move(delta):
    velocity = direction * speed
    position += velocity * delta

# Typed — the compiler knows the exact types, generates faster bytecode
func move(delta: float) -> void:
    velocity = direction * speed
    position += velocity * delta
```

Type inference with `:=` is equivalent to explicit types — use whichever is more readable.

### preload vs load

`preload` resolves the resource path at **parse time** and embeds it into the script binary. `load` resolves at **runtime** and may trigger a disk read (or cache lookup).

```gdscript
# preload — resolved at compile time; safe to use at class scope
const BulletScene: PackedScene = preload("res://scenes/bullet.tscn")
const HitSound: AudioStream = preload("res://audio/hit.ogg")

# load — resolved at runtime; use for dynamic paths or optional resources
func _load_skin(skin_name: String) -> Texture2D:
    return load("res://skins/%s.png" % skin_name)

# WRONG — load() inside _process reads from disk (or cache) every frame
func _process(_delta: float) -> void:
    var tex = load("res://icon.svg")  # repeated runtime resolution
    $Sprite2D.texture = tex

# RIGHT — preload at class scope, assign once in _ready()
const IconTexture: Texture2D = preload("res://icon.svg")

func _ready() -> void:
    $Sprite2D.texture = IconTexture
```

**C#:**

```csharp
// C# has NO preload — it is a GDScript parse-time construct with no C# equivalent.
// GD.Load<T>() is always a runtime lookup, so the cost has to be moved by hand.

// BEST — [Export] the resource and assign it in the editor. Loaded with the scene,
// zero runtime lookups, and a designer can swap it without touching code.
[Export] private PackedScene _bulletScene;
[Export] private AudioStream _hitSound;

// GOOD — static readonly, resolved once on first use of the type instead of per call.
private static readonly Texture2D IconTexture = GD.Load<Texture2D>("res://icon.svg");

public override void _Ready()
{
    GetNode<Sprite2D>("Sprite2D").Texture = IconTexture;
}

// For dynamic paths, GD.Load is the right tool — just never in a per-frame method.
private Texture2D LoadSkin(string skinName) => GD.Load<Texture2D>($"res://skins/{skinName}.png");

// WRONG — a runtime resolution every single frame.
public override void _Process(double delta)
{
    GetNode<Sprite2D>("Sprite2D").Texture = GD.Load<Texture2D>("res://icon.svg");
}
```

> The engine caches loaded resources, so a repeated `GD.Load` is a cache lookup rather than a
> disk read — but it still costs a path resolution and a Variant round-trip every call. `[Export]`
> removes both.
