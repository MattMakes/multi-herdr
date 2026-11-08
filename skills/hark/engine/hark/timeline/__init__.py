"""The hark timeline unit: timeline, sheet, critique and video.

Calling the package itself, ``hark.timeline(path, ...)``, forwards to
``hark.timeline.timeline.timeline``. Attributes resolve on first use, so a bare
``import hark.timeline`` stays cheap.
"""

from __future__ import annotations

import importlib
import sys
import types

_SUBMODULES = frozenset({"timeline", "sheet", "video", "critique"})

_EXPORTS = {
    "timeline": ("detect_events", "level_envs", "level_peak", "loudness", "music_grid",
                 "read_cues", "sync", "top_accents", "novelty_bounds"),
    "critique": ("scorecard", "real_thresholds"),
}
_OWNER = {attr: f"{__name__}.{mod}" for mod, attrs in _EXPORTS.items() for attr in attrs}


def __getattr__(name: str):
    home = _OWNER.get(name)
    if home is not None:
        return getattr(importlib.import_module(home), name)
    if name in _SUBMODULES:
        return importlib.import_module(f"{__name__}.{name}")
    raise AttributeError(f"module {__name__!r} has no attribute {name!r}")


class _TimelinePackage(types.ModuleType):
    """Module type whose instances can be called like the timeline function."""

    def __call__(self, *args, **kwargs):
        entry = importlib.import_module(f"{__name__}.timeline").timeline
        return entry(*args, **kwargs)


sys.modules[__name__].__class__ = _TimelinePackage
