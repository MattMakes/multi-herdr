# Sample

A whole script:

```gdscript
class_name Health
extends Node

signal died

var value := 10

func hit(amount: int) -> void:
	value -= amount
	if value <= 0:
		died.emit()
```

A script that uses the class above:

```gdscript
extends Node

var health: Health

func _ready() -> void:
	health.hit(1)
```

The same class_name again (a later version of the example):

```gdscript
class_name Health
extends Node
```

A statement fragment on a 2D node:

```gdscript
z_index = 10
z_as_relative = true
```

A fragment with a function and no extends, indented with spaces:

```gd
func _process(delta: float) -> void:
    rotation += delta
```

A broken block (no colon after the signature; Godot reports its 4th line):

```gdscript
extends Node

func _ready() -> void
	pass
```

<!-- gdscript-check: skip -->

```gdscript
this is not GDScript
```

```python
this is not checked either
```
