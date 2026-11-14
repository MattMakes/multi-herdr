# XR

Target engine: Godot 4.7. OpenXR and WebXR in depth: session states, frame timing,
render cost, sharp UI, passthrough, tracking data and the rules that keep players well.

## Use this shelf when

- You start a VR, AR or mixed reality game, or add an XR mode to a flat game.
- Frame time on a headset is over budget and you need the XR-only levers.
- UI or video looks blurry in the headset.
- You need hands, body, face, eye gaze, anchors or planes from the runtime.
- You design locomotion and must keep physical and virtual movement in step.

## How the engine works here

`XRServer` holds interfaces (`OpenXRInterface`, `WebXRInterface`, mobile VR) and
trackers. One interface is primary and owns output to the device. OpenXR takes over part
of the Vulkan setup, so it must be enabled before start (`xr/openxr/enabled`, plus
`xr/shaders/enabled`); your script only checks `is_initialized()` and sets
`Viewport.use_xr`. Most `xr/openxr/*` settings cannot change at run time.

`XROrigin3D` is the centre of the tracking space, not the player. The runtime moves
`XRCamera3D`, `XRController3D` and every `XRNode3D` relative to it each frame. All
tracked data (hands, anchors, render models) is local to the origin, so those nodes must
sit under it. One unit is one metre.

The runtime owns frame timing. V-Sync settings are ignored. The headset rate is 72 Hz at
least and up to 144 Hz, while physics defaults to 60 ticks: unmatched, objects visibly
stutter. The runtime may reproject late frames; that hides small misses but not physics
judder.

OpenXR hides raw input. You declare actions in an action map, bind them per interaction
profile, and the runtime picks the best profile for the device in use, falling back to
related profiles and at last to the too-simple "Simple controller". Bindings cannot be
changed at run time, and the map is separate from `InputMap`.

A session moves through states: `session_begun`, `session_visible` (rendered but no
input: system menu open, or headset off), `session_focussed`, `session_stopping`.

## Advanced techniques

### Start script that matches physics to the headset

```gdscript
extends Node3D

var xr: OpenXRInterface

func _ready() -> void:
	xr = XRServer.find_interface("OpenXR") as OpenXRInterface
	if xr == null or not xr.is_initialized():
		return
	DisplayServer.window_set_vsync_mode(DisplayServer.VSYNC_DISABLED)
	get_viewport().use_xr = true
	get_viewport().vrs_mode = Viewport.VRS_XR
	xr.session_begun.connect(_on_session_begun)

func _on_session_begun() -> void:
	var rate: float = xr.display_refresh_rate
	if rate > 0.0:
		Engine.physics_ticks_per_second = roundi(rate)
```

The refresh rate is only valid from `session_begun` on. `get_available_display_refresh_rates()`
lists what you may request. Pause the game in `session_visible` and resume in
`session_focussed`: a game that starts with the headset off stays in the visible state,
and without the pause it plays on with no input.

### Pick the reference space for the game type

| Space (`xr/openxr/reference_space`) | Origin | Use for |
|---|---|---|
| Local | At the head; recenter moves camera to origin | Cockpit and seated games |
| Stage (default) | Centre of the play area, fixed in the room | Room scale |
| Local Floor | Under the player, height kept | Standing in place, AR UI |

With Stage, handle `pose_recentered` yourself with `XRServer.center_on_hmd()`; stores
reject games that ignore recenter. With Local or Local Floor do not call it. Local Floor
plus stick locomotion lets a recenter teleport the player, even through walls; use Stage
and reset orientation only.

### Foveation and VRS

`xr/openxr/foveation_level` lowers resolution toward the edge of each eye. On Mobile and
Forward+ it needs `Viewport.vrs_mode = VRS_XR`; `OpenXRInterface.vrs_min_radius` and
`vrs_strength` tune it. `foveation_dynamic` lets the runtime vary the level with GPU
load, so set the level to high and let it float. Glow, depth of field and similar post
effects switch foveation off. Outside Android, MSAA 3D also disables it.
`render_target_size_multiplier` scales eye buffers but must be set before init.

### Renderer choice

The manual's general advice is Mobile for desktop VR and standalone headsets; Forward+
runs but is not tuned for XR. Its Android deploy page still advises Compatibility for
Android headsets "for the time being". Test both on the target device. WebXR is
Compatibility only and needs multiview support in the browser. Many post effects are not
stereo-aware.

### Composition layers for sharp UI and video

A `SubViewport` on a mesh is sampled twice (scene render, then lens distortion) and
blurs. `OpenXRCompositionLayerQuad`, `OpenXRCompositionLayerCylinder` and
`OpenXRCompositionLayerEquirect` hand the viewport to the compositor, which samples it
once after distortion. Place the node under `XROrigin3D`, set `layer_viewport`, and use
`intersects_ray()` to turn a controller ray into UV for fake mouse events; it returns
`Vector2(-1, -1)` on a miss.

- Layers draw over the scene, so a hand in front is covered. Set `enable_hole_punch` and
  a negative `sort_order`: the layer is drawn first and the scene on top.
- A layer is absent from a desktop spectator view. `is_natively_supported()` tells you
  if the runtime falls back to the blurry in-scene path.
- On Android, `use_android_surface` feeds a media player surface straight to the layer;
  `protected_content` allows DRM video.

### Full-screen effects without a screen read

A full-screen quad with `POSITION = vec4(VERTEX.xy, 1.0, 1.0)` is centred on the render
target, not on the lens: eye projections are asymmetric. Subdivide the quad and offset
the inner vertices by `PROJECTION_MATRIX * vec4(0.0, 0.0, 1.0, 1.0)` so a vignette is
centred in the headset. Never read the screen texture in such an effect: it forces a
full copy of the eye buffers and cancels the XR render savings.

### Passthrough and occlusion

Set `XRInterface.environment_blend_mode` to `XR_ENV_BLEND_MODE_ALPHA_BLEND` (check
`get_supported_environment_blend_modes()` first), set `Viewport.transparent_bg`, and give
the environment a transparent background colour with a colour ambient source, since the
sky no longer lights the scene. Since 4.3 this one property also drives vendor
passthrough. To let a real table hide virtual objects, draw a proxy with this material:

```gdshader
shader_type spatial;
render_mode blend_mix, depth_draw_opaque, cull_back, shadow_to_opacity;

void fragment() {
	ALBEDO = vec3(0.0);
}
```

It writes depth, is transparent where lit, and turns opaque where virtual shadows fall,
so virtual objects cast shadows on the real surface.

### Hands: tracker node, model, modifier

Per hand: an `XRNode3D` under the origin with `tracker` set to `/user/hand_tracker/left`
(or `right`), a rigged hand using the humanoid bone names, and an `XRHandModifier3D` on
its `Skeleton3D`. `BONE_UPDATE_FULL` resizes bones to the real hand and can crush badly
weighted meshes; `BONE_UPDATE_ROTATION_ONLY` keeps the model's proportions. Read
`XRHandTracker.hand_tracking_source` to know if data is optical or inferred from a
controller. The action map does not suit hand input unless the runtime supports the hand
interaction profile (pinch, grasp, poke), which is not yet everywhere.

### Room-scale body that follows the head

Putting the origin under a `CharacterBody3D` breaks: the player walks away from the body,
collisions test the wrong place, and stick turns swing the player around the room centre.
Each physics frame do three steps: move the body to the camera's floor position with
`move_and_slide()` and shift the origin back by what the body moved; rotate around the
player, not the origin; then apply stick movement. When the head passes a wall the body
cannot follow, fade to black after a few centimetres of gap instead of pushing the view.

### Spatial entities (since 4.6)

Anchors, planes and markers arrive as trackers (`OpenXRAnchorTracker`,
`OpenXRPlaneTracker`, `OpenXRMarkerTracker`) that drive `XRAnchor3D` nodes. Enable
`xr/openxr/extensions/spatial_entity/enabled` and the capability settings; the
`enable_builtin_*` settings run discovery for you. Calls are asynchronous. To persist an
anchor: create it with `OpenXRSpatialAnchorCapability.create_new_anchor()`, wait for the
tracking state, call `persist_anchor()`, then store the UUID with your own data.
Unpersist before removing. A plane with a hole-punch material becomes an occluder.

## Niche but useful

- `OpenXRRenderModelManager` (4.5+) shows the runtime's own model of the device in use.
- Eye gaze: an `XRController3D` with `tracker` `/user/eyes_ext` and a pose action.
- `xr/openxr/extensions/frame_synthesis`: the runtime synthesises frames from motion
  vectors and depth, so the game can render at a lower rate.
- `xr/openxr/submit_depth_buffer` can improve reprojection, but disables stencil.
- Binding modifiers (`xr/openxr/binding_modifiers/*`) add dpad inputs and thresholds.
- Action set `priority` decides which set wins a shared input.
- `OpenXRInterface.is_user_present()` and `user_presence_changed` (4.7 per
  `godot-xr-development`) detect a removed headset.
- `XRBodyModifier3D` and `XRFaceModifier3D` apply body and face tracker data.
- `--xr-mode off` runs the project flat; turn `xr/openxr/startup_alert` off and branch
  on `is_initialized()` for a desktop fallback.
- Godot XR Tools addon: ready-made player body, movement, teleport and pickup scenes.
- The XR editor runs on Quest (Horizon OS v69+) and Android XR; early access.

## Traps

- Comfort: default to snap turn and teleport; offer smooth modes as options.
- Bind only profiles you have tested. Runtimes remap unknown devices to a bound profile.
- `aim`, `grip` and `palm` poses differ by controller; `palm` is not supported everywhere.
- A viewport used by a layer cannot use the "When Visible" update mode.
- Develop standing up early; seated testing hides room-scale faults.
- Android headsets need a Gradle build; one preset per vendor. Since 4.6 the vendors
  plugin is optional but still needed for vendor features and often for store release.
- WebXR starts as a flat page; a user click must start the session.
- The manual does not cover a headset simulator; treat vendor simulators as external.

## Version notes

- 4.5: render models; foveation on the Mobile renderer and a D3D12 OpenXR backend
  (both per `godot-xr-development`).
- 4.6: spatial entities; OpenXR 1.1 (`xr/openxr/target_api_version`); vendors plugin
  optional. `OpenXRExtensionWrapper` virtuals gained an `xr_version` parameter.
- 4.7: `xr/openxr/foveation_eye_tracked` and `foveation_with_subsampled_images`;
  `eye_visibility` on layers; `create_new_anchor()` gained an optional `next` parameter.

## Related skills

- `godot-xr-development` - setup steps, grab and UI recipes, Quest export checklist.
- `godot-mobile-development` - Android build and device constraints.
- `godot-optimization` - general frame-time work before the XR levers.

## Sources

- https://docs.godotengine.org/en/stable/tutorials/xr/setting_up_xr.html
- https://docs.godotengine.org/en/stable/tutorials/xr/openxr_settings.html
- https://docs.godotengine.org/en/stable/tutorials/xr/xr_action_map.html
- https://docs.godotengine.org/en/stable/tutorials/xr/xr_room_scale.html
- https://docs.godotengine.org/en/stable/tutorials/xr/openxr_composition_layers.html
- https://docs.godotengine.org/en/stable/tutorials/xr/xr_full_screen_effects.html
- https://docs.godotengine.org/en/stable/tutorials/xr/ar_passthrough.html
- https://docs.godotengine.org/en/stable/tutorials/xr/openxr_hand_tracking.html
- https://docs.godotengine.org/en/stable/tutorials/xr/openxr_spatial_entities.html
- https://docs.godotengine.org/en/stable/tutorials/xr/deploying_to_android.html
- https://docs.godotengine.org/en/stable/tutorials/xr/webxr_intro.html
- https://docs.godotengine.org/en/stable/tutorials/editor/using_the_xr_editor.html
