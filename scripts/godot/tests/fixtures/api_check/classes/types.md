# Bare class names

```gdscript
extends Node
class_name LocalThing

var a: Node
var b: NoSuchClass
func f(x: Vector2, y: MissingType = null) -> Gone:
	var c := Node.new()
	var d := Phantom.new()
	if c is Ghost or c as LocalThing:
		pass
	var e: Array[Lost] = []
	var g: int = 0
	var h := "extends NotInAString"  # is NotInAComment
	var k: Spectre  # api-check: allow Spectre
	return null

class Inner extends Missing:
	pass
```
