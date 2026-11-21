# Stylized techniques that read the clock

Every block here is `proof: not run (needs Godot)`. No shader block is compiled by a headless run: look at real frames.

Each technique takes its time from the cutscene, never from its own timer. In a shader, pass the time in as a uniform that a track keys, or use `TIME` only for motion that may differ between plays.

## Painterly parallax: a painting that moves

Turn 1 illustration into a shot with depth.

1. Split the art into 3 to 6 layers by depth: sky, far, mid, subject, near, foreground frame. Paint in what each layer hid, 5 to 10 % past its edge.
2. Each layer is a `Sprite2D` under 1 `Node2D`. Give each a depth from 0.0 (sky) to 1.0 (nearest).
3. Move 1 value, the camera offset. Each layer moves by `offset * depth`. Near layers move most.
4. Keep the move small: 2 to 6 % of the frame over 4 to 10 s, with `TRANS_SINE` in-out. Add a 2 to 4 % push in.
5. Add life inside layers: a slow UV scroll on clouds, a sway shader on grass or cloth, drifting dust, a flicker on a light. Amplitudes stay under 1 % of the frame.

```gdscript
extends Node2D
## Parallax for a layered painting. Key `offset` and `push` on the cutscene.

@export var offset: Vector2 = Vector2.ZERO:
	set(value):
		offset = value
		_apply()
@export var push: float = 0.0:
	set(value):
		push = value
		_apply()
## One depth per child, in tree order: 0.0 is the sky, 1.0 is nearest.
@export var depths: PackedFloat32Array = PackedFloat32Array()


func _apply() -> void:
	for i in mini(get_child_count(), depths.size()):
		var layer := get_child(i) as Node2D
		if layer:
			layer.position = offset * depths[i]
			layer.scale = Vector2.ONE * (1.0 + push * (0.5 + depths[i]))
```

Style notes by family:

| Family | What moves | What must not |
|---|---|---|
| Ink wash, sumi-e | Ink blooms in through a soft mask; mist drifts; a brush stroke draws on | Hard edges, fast cuts |
| Watercolour | Pigment spreads from wet edges; paper grain stays fixed to the screen | Grain that slides with the camera |
| Oil, impasto | Slow light change across strokes; thick strokes keep still | Wobbling brush marks |
| Woodblock, ukiyo-e | Flat layers slide; waves loop in 3 to 4 held poses | Smooth gradients, blur |
| Paper cut, collage | Pieces hinge and slide with a small drop shadow; stepped at 12 fps | Squash and stretch |
| Stained glass, mosaic | Light sweeps behind; pieces light up in order | Moving lead lines |
| Pixel art | Whole-pixel moves; palette shifts | Rotation and sub-pixel scale |

Paper and canvas texture is fixed to the screen: put it on a `CanvasLayer` above the art with a multiply blend.

## Reveal by mask: ink, brush and wipe

A reveal is a threshold on a greyscale mask. Key `progress` from 0 to 1.

```gdshader
shader_type canvas_item;

uniform sampler2D mask : filter_linear;
uniform float progress : hint_range(0.0, 1.0) = 0.0;
uniform float softness : hint_range(0.0, 0.5) = 0.08;

void fragment() {
	vec4 colour = texture(TEXTURE, UV);
	float order = texture(mask, UV).r;
	float edge = progress * (1.0 + softness);
	colour.a *= smoothstep(order - softness, order, edge);
	COLOR = colour;
}
```

- Dark parts of the mask appear first. A radial gradient opens from a point. A painted mask follows the brush strokes. A noise mask gives an ink bloom.
- Ink and watercolour: softness 0.10 to 0.20, 0.8 to 1.6 s. A hard wipe: softness 0.01, 0.3 to 0.5 s.

## Line boil: hand-drawn life

A drawn line that holds perfectly still looks dead. Boil redraws it with small changes a few times per second.

- Step the time: animate on 2s or 3s. At 24 fps art rate that is 12 or 8 changes per second. In a 60 fps game, change the boil every 5 to 8 frames.
- Displace by 0.5 to 2 pixels at the art's native size. More reads as shaking.
- Use 3 or 4 noise phases and cycle them. A loop of 3 to 4 drawings is what a hand animator makes.
- Boil the line, not the fill. Keep text and UI still.

```gdshader
shader_type canvas_item;

uniform sampler2D noise : repeat_enable, filter_linear;
uniform float clock = 0.0;
uniform float rate = 8.0;
uniform float amount = 1.5;

void fragment() {
	float phase = floor(clock * rate);
	vec2 jitter = texture(noise, UV * 3.0 + vec2(phase * 0.37, phase * 0.61)).rg - 0.5;
	COLOR = texture(TEXTURE, UV + jitter * amount * TEXTURE_PIXEL_SIZE);
}
```

Key `clock` to the cutscene time with a linear track from 0 to the scene length.

## Stroke draw-on

A line that writes itself: a `Line2D` whose point list grows, or a shader that cuts a line by its length.

```gdscript
extends Line2D
## Draws the stroke up to `progress`. Key `progress` from 0.0 to 1.0.

@export var full_points: PackedVector2Array = PackedVector2Array()
@export_range(0.0, 1.0) var progress: float = 0.0:
	set(value):
		progress = value
		_rebuild()


func _rebuild() -> void:
	var count := full_points.size()
	if count < 2:
		return
	var exact := progress * float(count - 1)
	var whole := int(exact)
	var shown := full_points.slice(0, whole + 1)
	if whole < count - 1:
		shown.append(full_points[whole].lerp(full_points[whole + 1], exact - float(whole)))
	points = shown
```

- Draw at the speed of a hand: 0.3 to 0.8 s for a short mark, up to 1.5 s for a signature. Ease out, so the pen slows at the end.
- Draw strokes in the order a person would, with 0.05 to 0.15 s between strokes.

## Motion comic

Panels, pans and type; no rigs.

- 1 panel is 1 beat. Enter a panel with a slide of 5 to 10 % of its size plus a fade over 0.25 to 0.4 s, or with a hard cut on a sound.
- Inside a panel: a slow pan or push (the parallax recipe with 2 or 3 layers), 1 moving element, and a held frame.
- Speech bubbles appear in reading order, 0.15 s apart, each with a small scale from 0.9 with `TRANS_BACK`.
- A sound-effect word slams in over 0.12 s from 1.3 scale, shakes for 0.2 s and holds.
- Advance on the player's input when the game is slow-paced, with an auto-advance setting. Then each panel is its own short animation and the base class plays them in order.

## Kinetic titles

- A title of 1 to 4 words may animate per word: stagger 0.05 to 0.08 s, rise 20 to 40 px, `TRANS_QUART` ease out, 0.35 to 0.5 s.
- A name card: bar slides in 0.3 s, name follows 0.08 s later, subtitle 0.08 s after that. It leaves in 0.2 s.
- Size steps come from the UI theme. Do not scale a `Label` past its font size; set the size and let it rasterize sharp.
- Keep type inside the safe area and off the subtitle band.

## Particles that scrub

- Turn on `use_fixed_seed` and set a `seed` on every cutscene particle node. Each play is then the same, and a capture can be repeated.
- Start a burst from a method key with `restart()`. A burst is cosmetic, so a skip may miss it.
- For particles that must follow a scrub (snow through a whole shot, embers in a held frame): set `speed_scale` to 0 and call `request_particles_process()` with the time step when the clock moves. The `particles-and-vfx` shelf of **godot-library** has the details.
- Use `preprocess` so a looping system is already full on the first frame of a shot.
- Budget: a hero burst of 100 to 300 particles for under 1 s costs little. A full-screen system over the whole scene needs a measurement on the lowest device.
- Layer an effect: a core flash (2 to 4 frames), a burst (0.2 to 0.5 s), slow leftovers such as smoke or sparks (1 to 2 s), and light on the scene.

## The cinematic finish

Post effects change the mood of a shot at low cost. Key them on the cutscene and restore them in `_exit()`.

| Effect | 3D | 2D | Amount |
|---|---|---|---|
| Tone and grade | `Environment` tonemap and adjustments, or a colour correction texture | a full-screen `ColorRect` shader on a `CanvasLayer` | saturation 0.9 to 1.1, contrast 1.0 to 1.15 |
| Bloom | `Environment` glow | glow with HDR 2D on, or an additive blurred copy | only bright sources bloom |
| Depth of field | `CameraAttributesPractical` | blur the far layers | rack focus over 0.4 to 0.8 s |
| Vignette | full-screen shader | full-screen shader | darken corners 10 to 25 % |
| Film grain | full-screen shader, stepped clock | same | 2 to 5 % luminance; fixed to the screen |
| Chromatic offset | full-screen shader | same | on impacts only, for 2 to 6 frames |

- One look per scene. Change it only when the story changes: a flashback, a dream, a hit.
- Keep a setting to turn off bloom, grain, shake and chromatic offset. The scene must read without them.
- A post shader runs on every pixel. On the Mobile and Compatibility renderers, combine vignette, grade and grain into 1 pass.
- Renderer support differs (glow, depth of field and screen reading). Check the `rendering` shelves of **godot-library** for the project's renderer.

## Tuning with live sliders

Do not guess numbers in code and re-run. Put the values of a shot (durations, offsets, the grade, the particle counts) on a `Resource` with `@export_range` fields, and play the cutscene in the editor with the Inspector open, or use the remote scene tree on a running game. Save the resource when it looks right. The values are then data that a designer can change, and a test can load. The `design-iteration-toolkit` shelf of **godot-library** covers live tuning.
