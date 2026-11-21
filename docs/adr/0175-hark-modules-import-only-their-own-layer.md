# hark modules import only their own layer

hark is built in layers, and a module never imports a layer above it or
code it does not need. `skills/hark/engine/hark/forge/engine/viz.py` sits
below the search layer, so `_closest_ref` matches references by spectral
balance (`tone.profile`) only, not by the search descriptor distance.

## Consequences

- `hark/surfaces/gallery_sound.py` keeps its own copy of the BS.1770
  K-weighting at 48 kHz (a high shelf, then a high-pass), so a gallery page
  builds without core or forge code. The copy must match
  `hark.core.features` `k_weight`.
- A test that stubs a submodule in `sys.modules` also replaces the
  attribute on the parent package (`hark.forge.recipes`,
  `hark.forge.engine.refsets`), because `from package import name` reads the
  package attribute first and an earlier test may already have imported
  the real module (`tests/fit_pipeline/test_codegen.py`).
- `hark/models/_common.py` filters exactly the message "NOTE: Redirects
  are currently not supported in MacOs." from the logger
  `torch.distributed.elastic.multiprocessing.redirects`. torch logs this 1
  line on macOS when it imports `torch.distributed`, and it says nothing
  about hark. Other records of that logger still pass.
