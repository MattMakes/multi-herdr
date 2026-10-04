# Good

Prose mentions are not checked: OS.no_such_thing.

```gdscript
func _ready() -> void:
	var n := Node.new()
	n.process_mode = Node.PROCESS_MODE_INHERIT
	Node.ProcessMode
	Node.set_process_mode
	Node.connect
	Node.ready
	Node.script_changed
	Node.CONNECT_DEFERRED
	Node.ConnectFlags
	print(OS.get_name(), Vector2.ZERO, Key.KEY_SPACE, Variant.Type)
	$Player.no_such_member
	MyClass.anything
	Tween.TRANS_SINE
```

```
OS.untagged_block_is_not_checked
```

```csharp
GodotObject.IsInstanceValid(this);
var name = OS.GetName();
var z = Vector2.Zero;
if (Input.IsKeyPressed(Key.Space)) { }
var mode = Node.ProcessModeEnum.Inherit;
EmitSignal(Node.SignalName.Ready);
```
