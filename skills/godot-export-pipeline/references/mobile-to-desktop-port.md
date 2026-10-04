Adds the work order and checks for porting a mobile game to PC: input parity, UI density, a graphics quality ladder and the desktop behaviour players expect; read it when a touch-first project gets a Windows, macOS or Linux build.

# Porting a mobile game to desktop

> ← Back to [SKILL.md](../SKILL.md)

The opposite direction (desktop to mobile) is in
**godot-mobile-development**. The desktop runtime pieces this port needs
(window modes, settings file, safe quit, focus loss) are in
[desktop-runtime.md](desktop-runtime.md).

## Work order

1. **Input parity.** Every touch action gets a keyboard and mouse path
   through the Input Map. Gameplay code reads actions, so it does not
   change. Keep the touch path for touchscreen laptops.
2. **UI density.** Touch targets of 44 pt / 48 dp are too big for a mouse.
   Shrink the UI and use the free space; do not just scale the phone layout
   up.
3. **Desktop behaviour.** Window modes, a persisted settings file, a quit
   confirmation, pause on focus loss. See [desktop-runtime.md](desktop-runtime.md).
4. **Graphics ladder.** Phones ran a fixed low setting. PC players expect
   quality presets and an uncapped or monitor-matched frame rate.
5. **Store layer.** Mobile IAP and ads do not exist on PC. Replace them
   behind the same service interface (see the store guard in
   [desktop-runtime.md](desktop-runtime.md)), or remove them with a
   feature tag.

## Input parity table

| Mobile input | Desktop equivalent |
|---|---|
| Virtual joystick | WASD / arrows through the same move actions (`Input.get_vector`) |
| Tap on world | Left click; the same handler reads `InputEventMouseButton` |
| Drag to aim | Mouse position, or captured mouse look for 3D cameras |
| Pinch zoom | Mouse wheel (`MOUSE_BUTTON_WHEEL_UP` / `MOUSE_BUTTON_WHEEL_DOWN`) |
| Long press | Right click |
| On-screen buttons | Keys, plus a tooltip that names the key |
| Android Back | Escape opens the pause menu |

Hide the virtual joystick and on-screen buttons when the last input came
from a keyboard, mouse or gamepad, and show them again after a touch
event. Do not hide them only by feature tag: touchscreen laptops exist.

Hover is new on desktop. Buttons need a hover style, and items need
tooltips (`Control.tooltip_text`). Mobile UI was never tested for either.

## UI density

- Lower `Window.content_scale_factor`, or move to a desktop theme with
  smaller fonts and margins, behind the `pc` feature.
- Re-layout, do not only shrink: show the minimap, chat and hotbar that the
  phone layout hid.
- Test at 1280×720, 1920×1080, 2560×1440 and an ultrawide size with the
  `--resolution` flag (see **godot-responsive-ui**).

## Graphics quality ladder

One function applies a named preset, and the settings file stores the
name. Detect a starting preset from a short benchmark only if you also let
the player change it.

```gdscript
# graphics_quality.gd - autoload named GraphicsQuality
extends Node

enum Preset { LOW, MEDIUM, HIGH, ULTRA }

var current: Preset = Preset.HIGH


func apply(preset: Preset) -> void:
	current = preset
	var vp: Viewport = get_viewport()
	match preset:
		Preset.LOW:
			vp.msaa_3d = Viewport.MSAA_DISABLED
			vp.screen_space_aa = Viewport.SCREEN_SPACE_AA_DISABLED
			vp.scaling_3d_scale = 0.75
			vp.positional_shadow_atlas_size = 1024
			RenderingServer.directional_shadow_atlas_set_size(1024, true)
		Preset.MEDIUM:
			vp.msaa_3d = Viewport.MSAA_DISABLED
			vp.screen_space_aa = Viewport.SCREEN_SPACE_AA_FXAA
			vp.scaling_3d_scale = 1.0
			vp.positional_shadow_atlas_size = 2048
			RenderingServer.directional_shadow_atlas_set_size(2048, true)
		Preset.HIGH:
			vp.msaa_3d = Viewport.MSAA_2X
			vp.screen_space_aa = Viewport.SCREEN_SPACE_AA_DISABLED
			vp.scaling_3d_scale = 1.0
			vp.positional_shadow_atlas_size = 4096
			RenderingServer.directional_shadow_atlas_set_size(4096, true)
		Preset.ULTRA:
			vp.msaa_3d = Viewport.MSAA_4X
			vp.screen_space_aa = Viewport.SCREEN_SPACE_AA_DISABLED
			vp.scaling_3d_scale = 1.0
			vp.positional_shadow_atlas_size = 8192
			RenderingServer.directional_shadow_atlas_set_size(8192, true)
```

Effects that live on the `Environment` (SSAO, SSR, SDFGI, glow) belong in
the same function: look up the active `WorldEnvironment` and set its
`environment` properties per preset. The mobile build kept them off; the
desktop build can turn them on from MEDIUM up.

Also review:
- The project may target the **Mobile** renderer. Desktop can use
  Forward+ through the override setting
  `rendering/renderer/rendering_method` with a `.mobile` variant kept for
  phones. Changing the renderer changes the look: compare screenshots.
- `Engine.max_fps` was often set to 30 or 60 for battery. On desktop, use
  vsync, or match `DisplayServer.screen_get_refresh_rate()`.
- Camera far planes and LOD distances were tuned for phones. Raise them on
  the higher presets (`Viewport.mesh_lod_threshold` lower means more
  detail).

## Port checklist

- [ ] Every touch action has a keyboard and mouse path; rebinding works
- [ ] Touch controls hide after non-touch input and return after touch
- [ ] Hover styles and tooltips on interactive UI
- [ ] UI tested at four desktop resolutions and an ultrawide size
- [ ] Window modes, settings file and quit confirmation work
- [ ] Quality presets apply and persist; frame rate is not capped at 30
- [ ] IAP and ad code is replaced or removed in the PC build
- [ ] The release preset excludes mobile-only plugins and assets
