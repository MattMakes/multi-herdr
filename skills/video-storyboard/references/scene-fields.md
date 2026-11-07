# Optional scene fields

Use these fields in a `timeline.json` scene when the shot needs them. All
times are seconds from the scene start. All rectangles use normalized values
from `0.0` to `1.0`.

## `reframe`

`reframe` defines a push-in on a wide plate. It has `scale`, `focal_point`,
`from_sec` and `to_sec`.

```json
"reframe": {"scale": 1.5, "focal_point": [0.55, 0.42], "from_sec": 0.0, "to_sec": 1.2}
```

## `gauge_inset`

`gauge_inset` defines a proof inset. It has `source_rect`, `scale`,
`event_sec`, `lead_sec`, `tail_sec` and `anchor`. A stat-change caption needs
a `gauge_inset` from `0.3` s before the event to `1.2` s after the event.

```json
"gauge_inset": {"source_rect": [0.7, 0.05, 0.2, 0.12], "scale": 2.0, "event_sec": 1.1, "lead_sec": 0.3, "tail_sec": 1.2, "anchor": "top-right"}
```

## `plate_hold`

`plate_hold` holds 1 plate frame. It has `at_sec` and `duration_sec`.

```json
"plate_hold": {"at_sec": 2.0, "duration_sec": 0.5}
```

## `hud_crop`

`hud_crop` is a normalized region of product UI to crop or cover.

```json
"hud_crop": [0.72, 0.0, 0.28, 0.18]
```

## `static_hold`

Set `static_hold` to `true` and give `static_hold_reason` as a string. The
dead-time check skips this scene.

```json
"static_hold": true,
"static_hold_reason": "The end card must stay still for the final read."
```

## Caption and action timing

`caption_box` is an optional normalized `[x, y, w, h]` rectangle. Use it
when the storyboard fixes the caption position.

```json
"caption_box": [0.1, 0.86, 0.8, 0.08]
```

`locked_actions_sec` is an optional list of scene-relative seconds. Director
values come from `key_actions_sec` in the capture sidecar, minus the clip
in-point.

```json
"locked_actions_sec": [0.5, 1.5]
```
