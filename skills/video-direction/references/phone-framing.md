# Phone framing, proof, hero, and brand intro

Read each planned shot at 360 px high. A viewer must understand the subject
and the claim without team context.

## Frame for a phone

- Keep the main subject 100 px to 120 px wide or larger at 1080p. Record
  the measured width in capture sidecar `pixel_facts.subject_width_px`.
  The capture sidecar gate and the critic check this value.
- The viewer must find the subject within 1 s after each cut. The critic
  checks this rule.
- When a plate is too wide, add `reframe` to the timeline scene. Use a
  `1.5` to `1.6` scale when that range gives a clear phone frame.
- Keep each important UI label at least 10 px high in a 360 px-high view.
  If it is smaller, use a `1.5` to `1.6` push-in on the important control.
  The critic checks the rendered label and push-in.
- Keep all menu and UI footage at 8 s or less in an 80 s film. Scale this
  limit in proportion to the target duration. `check_timeline.py` checks
  this limit.

The storyboard gate checks `reframe` values in `timeline.json`. The capture
sidecar gate checks measured pixels. The critic checks what the rendered
frame communicates to a viewer.

Use this scene field for a push-in on a wide plate:

```json
{"reframe": {"scale": 1.5, "focal_point": [0.5, 0.5], "from_sec": 0.0, "to_sec": 1.2}}
```

Stage a new capture take when the subject, action, or UI is weak. Change the
camera distance and product state for the shot. Do not use an edit to rescue
a weak take.

## Prove the hook and each claim

The first shot must prove the hook line in the first 2 s. Each short has its
own hook line and its own proof shot. Do not reuse a hook that its first shot
does not prove. The critic checks each hook.

Show each caption claim on screen during the caption. For a stat change, use
`gauge_inset` so the viewer can see the value change. Use the shared timeline
field exactly:

```json
{"gauge_inset": {"source_rect": [0.1, 0.1, 0.2, 0.2], "scale": 2.0, "event_sec": 1.1, "lead_sec": 0.3, "tail_sec": 1.2, "anchor": "top-right"}}
```

## Place the hero

Use 1 hero moment. Put it last in the content and on the largest music lift.
After the payoff, hold the hero for 2 s to 2.5 s. Put the CTA or end card
after this hold. The critic checks the hero order, music lift, and hold.

## Use the brand intro

Build the brand intro as its own small unit. Iterate it with the operator
before episode production starts.

- Keep the intro from 5 s to 6 s.
- Use the same intro in every long video in the series.
- Open each short on content. Show logos only at the end of a short.

The critic checks the intro duration, long-video consistency, and short open.

The long-video intro is a series rule. It can precede the content hook when
the operator approves that format. The first content shot must still prove
the content hook.
