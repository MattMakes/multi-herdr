# Draw Call Optimization

Reference for `skills/godot-optimization/SKILL.md` — reducing per-frame draw calls via batching, atlases, materials, culling, and LOD.

> ← Back to [SKILL.md](../SKILL.md)

---

Every distinct mesh, sprite, or canvas item that cannot be batched with its neighbours costs one draw call. Reducing draw calls is one of the highest-leverage optimisations, especially on mobile.

### Automatic 2D Batching

Godot 4.7 batches compatible 2D work automatically in every renderer. No node enables it. Keep compatible sprites, glyphs and canvas primitives consecutive in draw order, and measure the visible draw calls.

`CanvasGroup` is not a batcher. It is a backbuffer-compositing node that draws its children as one visual object through a screen-reading shader, and it can add buffer and composite cost. Use it only when you need that compositing.

**This is a scene-order change, not a code change** — and therefore identical in GDScript and C#.

Constraints for batching to occur:
- Children must share the same texture (use a texture atlas).
- Children must use the same blend mode and shader (or none).
- No `CanvasItem.clip_children` or light occluder between them.

### Reducing Unique Materials

Each unique material combination breaks a batch. Keep the material count low:

```gdscript
# WRONG — creating a new material per instance duplicates draw calls
func _ready() -> void:
    var mat := StandardMaterial3D.new()
    mat.albedo_color = Color(randf(), randf(), randf())
    $MeshInstance3D.material_override = mat  # unique material = unique draw call

# RIGHT — one shared material; the shader declares `instance uniform vec4 tint;`
@export var shared_material: ShaderMaterial

func _ready() -> void:
    $MeshInstance3D.material_override = shared_material
    $MeshInstance3D.set_instance_shader_parameter(&"tint", Color(randf(), randf(), randf()))
```

Declare the varying value as an `instance uniform` in the shader and set it with `set_instance_shader_parameter()`. Every instance keeps the same material. Call `duplicate()` on the material only when an instance uniform cannot hold the difference, for example a different texture.

**C#:**

```csharp
// WRONG — creating a new material per instance duplicates draw calls
public override void _Ready()
{
    var mat = new StandardMaterial3D();
    mat.AlbedoColor = new Color(GD.Randf(), GD.Randf(), GD.Randf());
    GetNode<MeshInstance3D>("MeshInstance3D").MaterialOverride = mat; // unique draw call
}

// RIGHT — one shared material; the shader declares `instance uniform vec4 tint;`
[Export] public ShaderMaterial SharedMaterial { get; set; }

public override void _Ready()
{
    var mesh = GetNode<MeshInstance3D>("MeshInstance3D");
    mesh.MaterialOverride = SharedMaterial;
    mesh.SetInstanceShaderParameter("tint", new Color(GD.Randf(), GD.Randf(), GD.Randf()));
}
```

For 3D scenes, `GeometryInstance3D.gi_mode = GI_MODE_STATIC` opts the geometry into global illumination (baked or dynamic GI). It does not merge meshes. Merge compatible static geometry in the asset pipeline or author an HLOD mesh, and keep smaller pieces where culling matters.

### Texture Atlases

Pack multiple sprites into a single atlas texture so all sprites sharing that atlas batch into one draw call.

- In the editor: **Import > Sprite Frames** supports atlas import.
- For tilemaps, use a single TileSet with one atlas texture per tile layer.
- For UI, pack icons into a single `AtlasTexture` or use `StyleBoxTexture` regions.

### Visibility Culling

Stop processing and rendering objects that are off-screen.

```gdscript
# 2D — VisibleOnScreenNotifier2D pauses processing when the node leaves the viewport
extends Sprite2D

@onready var _vis: VisibleOnScreenNotifier2D = $VisibleOnScreenNotifier2D

func _ready() -> void:
    _vis.screen_entered.connect(_on_screen_entered)
    _vis.screen_exited.connect(_on_screen_exited)
    set_process(false)  # start paused; enable only when visible

func _on_screen_entered() -> void:
    set_process(true)

func _on_screen_exited() -> void:
    set_process(false)
```

**C#:**

```csharp
// 2D — VisibleOnScreenNotifier2D pauses processing when the node leaves the viewport
public partial class CulledSprite : Sprite2D
{
    private VisibleOnScreenNotifier2D _vis;

    public override void _Ready()
    {
        _vis = GetNode<VisibleOnScreenNotifier2D>("VisibleOnScreenNotifier2D");
        _vis.ScreenEntered += OnScreenEntered;
        _vis.ScreenExited += OnScreenExited;
        SetProcess(false); // start paused; enable only when visible
    }

    private void OnScreenEntered() => SetProcess(true);
    private void OnScreenExited() => SetProcess(false);
}
```

```gdscript
# 3D — VisibleOnScreenNotifier3D works identically
extends Node3D

@onready var _vis: VisibleOnScreenNotifier3D = $VisibleOnScreenNotifier3D

func _ready() -> void:
    _vis.screen_entered.connect(func(): set_process(true))
    _vis.screen_exited.connect(func(): set_process(false))
    set_process(false)
```

**C#:**

```csharp
// 3D — VisibleOnScreenNotifier3D works identically
public partial class CulledNode3D : Node3D
{
    public override void _Ready()
    {
        var vis = GetNode<VisibleOnScreenNotifier3D>("VisibleOnScreenNotifier3D");
        vis.ScreenEntered += () => SetProcess(true);
        vis.ScreenExited += () => SetProcess(false);
        SetProcess(false);
    }
}
```

You can also query visibility directly:

```gdscript
# Only update expensive logic when the node is visible on screen
func _process(_delta: float) -> void:
    if not $VisibleOnScreenNotifier2D.is_on_screen():
        return
    _run_expensive_animation_logic()
```

**C#:**

```csharp
// Only update expensive logic when the node is visible on screen
public override void _Process(double delta)
{
    if (!GetNode<VisibleOnScreenNotifier2D>("VisibleOnScreenNotifier2D").IsOnScreen())
        return;
    RunExpensiveAnimationLogic();
}
```

### LOD for 3D

Do not swap meshes from a per-frame script. Godot has 2 built-in mechanisms that need no script work:

- **Imported mesh LOD** lowers the detail of one mesh with screen-space size.
- **`GeometryInstance3D` visibility ranges** swap a detailed group for an HLOD mesh at distance. Set `visibility_range_end` on the detailed pieces and `visibility_range_begin` on the HLOD mesh. Set `visibility_parent` on each detailed piece to the HLOD node, so the pieces show only while the HLOD mesh is hidden by its range. The `*_margin` properties add hysteresis, so objects do not flicker at the boundary.

```gdscript
# Set once (usually in the editor); the renderer applies the ranges every frame
@onready var _detail: MeshInstance3D = $HighPolyMesh
@onready var _hlod: MeshInstance3D = $HlodMesh

func _ready() -> void:
    _detail.visibility_range_end = 50.0
    _detail.visibility_range_end_margin = 5.0
    _hlod.visibility_range_begin = 50.0
    _hlod.visibility_range_begin_margin = 5.0
    _detail.visibility_parent = _detail.get_path_to(_hlod)
```

**C#:**

```csharp
// Set once (usually in the editor); the renderer applies the ranges every frame
public override void _Ready()
{
    var detail = GetNode<MeshInstance3D>("HighPolyMesh");
    var hlod = GetNode<MeshInstance3D>("HlodMesh");
    detail.VisibilityRangeEnd = 50.0f;
    detail.VisibilityRangeEndMargin = 5.0f;
    hlod.VisibilityRangeBegin = 50.0f;
    hlod.VisibilityRangeBeginMargin = 5.0f;
    detail.VisibilityParent = detail.GetPathTo(hlod);
}
```

For automatic LOD: set `GeometryInstance3D.lod_bias` and enable **Rendering > Mesh LOD** in Project Settings. Godot 4 generates LOD levels automatically during import if **Import > Generate LODs** is enabled on the mesh asset.
