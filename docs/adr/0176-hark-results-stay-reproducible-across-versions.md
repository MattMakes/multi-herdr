# hark results stay reproducible across versions

A hark result that a user tuned or that a cache holds must come out the
same in a later version. Seeds, tie-breaking orders and recorded constants
stay fixed, and float rounding is guarded where it would move a bound.

## Consequences

- `KEY_7_FIRST_UNIFORM` in `skills/hark/engine/tests/recipes_ui/test_ui_keys.py`
  pins the first uniform draw of `typing.key_rng(7)`
  (0.7605958890389746, recorded once on numpy 2 and reviewed). The per-key
  seed formula of the typing recipe never changes, because a changed seed
  changes every typed run that users already tuned.
- In `hark/forge/bank/_build.py` `_candidates` the derive candidate rows
  are ordered by ascending rendered length, then member id (T-41; the spec
  gives no rule). This order breaks distance ties in `_ranked`, so changing
  it changes which member a derived bank entry gets.
- `hark/timeline/video.py` caches frame-diff and scene results keyed by
  `DIFF_VERSION` and `SCENE_VERSION`. The matching version is bumped
  whenever a cached result's format or meaning changes.
- `hark/timeline/_tags.py` `BACKGROUND` holds the mean CLAP cosine of each
  phrase over real background events (data asset A2). The values belong to
  the current CLAP checkpoint and are regenerated for another one.
- `hark/forge/engine/loop.py` `_geo_window` clamps the window bounds after
  `exp`, because `exp(log(hi))` can round past `hi`, which would let a
  family box exceed its SPACE range.
- `hark/forge/recipes/tonal.py` `_taper_end` sets the last gain sample to
  exactly 0.0, because the float cosine of pi/2 is a little above 0 and the
  taper would not end in true silence.
