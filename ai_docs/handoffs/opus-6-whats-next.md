# T-2 handoff: storyboard fields and timeline checks

## State

Implementation is complete on branch
`fleet/video-lessons-T2-storyboard-fields-caption-hold-check-and`.
The implementation commit is `394bb061`.

## Changed files

- `skills/video-storyboard/SKILL.md`
- `skills/video-storyboard/references/fix-gates.md`
- `skills/video-storyboard/references/scene-fields.md`
- `skills/video-storyboard/scripts/check_timeline.py`

`SKILL.md` is 11,744 bytes. No new script was added. The existing
`check_timeline.py --self-test` suite has 58 passing tests.

## Implementation

- Defined `reframe`, `gauge_inset`, `plate_hold`, `hud_crop`, `static_hold`,
  `caption_box` and `locked_actions_sec`.
- Added shape validation for the optional scene fields.
- Added caption placement checks for 16:9 and 9:16 timelines.
- Added beat sync checks for cuts and locked actions.
- Added scaled menu-time checks and allowed `ui` and `menu` scene kinds.
- Kept the existing caption hold formula and added its required error details.
- Added the moved-beat DONE report format.

No beat time changed in this implementation.
No lesson from T-2 remains unapplied.

## Checks

The worker ran this command successfully after the final implementation:

```text
cargo test -p horch-core --test skills_catalog --test video_pipeline_wave && skills/video-storyboard/scripts/check_timeline.py --self-test
```

Results:

- `skills_catalog`: 33 passed, 0 failed.
- `video_pipeline_wave`: 12 passed, 0 failed.
- `check_timeline.py --self-test`: 58 passed, 0 failed.
- `git diff --check`: passed.
- `skills/video-storyboard/SKILL.md`: 11,744 bytes, below 12,288 bytes.

The Codex `horch ledger finish` check failed because its sandbox denied writes
to `/Users/mascott/.local/state/horch/tmp/opus-6`. The evidence shows 11
permission failures in `skills_catalog`; it shows no product-code failure.
A Claude worker must run `horch ledger finish`.
