# C# blocks

```csharp
int y = 2.0;
```

```csharp
public partial class Good : Node
{
    public override void _Ready() { GD.Print("ready"); }
}
```

```cs
[Export] public float Speed { get; set; } = 200.0f;

public override void _PhysicsProcess(double delta)
{
    Velocity = Velocity.MoveToward(Vector2.Zero, Speed * (float)delta);
    MoveAndSlide();
}
```

```csharp
using Godot;
using Godot.Collections;

var list = new Array<int> { 1, 2 };
```

<!-- csharp-check: skip -->
```csharp
this does not compile
```

```csharp
var n = new Node();
n.NoSuchMethod();
```
