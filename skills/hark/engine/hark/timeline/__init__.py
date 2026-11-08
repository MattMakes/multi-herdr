"""hark timeline unit: timeline, sheet, critique and the video helpers.

The package is also callable: ``hark.timeline(path, ...)`` runs ``hark.timeline.timeline.timeline``.
Names load on first access, so importing the package imports nothing heavy.
"""

from __future__ import annotations

import importlib
import sys
import types

_LAZY = {
    "detect_events": "hark.timeline.timeline",
    "level_envs": "hark.timeline.timeline",
    "level_peak": "hark.timeline.timeline",
    "loudness": "hark.timeline.timeline",
    "music_grid": "hark.timeline.timeline",
    "read_cues": "hark.timeline.timeline",
    "sync": "hark.timeline.timeline",
    "top_accents": "hark.timeline.timeline",
    "novelty_bounds": "hark.timeline.timeline",
    "scorecard": "hark.timeline.critique",
    "real_thresholds": "hark.timeline.critique",
}


class _CallablePackage(types.ModuleType):
    def __call__(self, *args, **kwargs):
        from hark.timeline.timeline import timeline as run

        return run(*args, **kwargs)


def __getattr__(name: str):
    target = _LAZY.get(name)
    if target is None:
        if name in ("timeline", "sheet", "video", "critique"):
            return importlib.import_module(f"{__name__}.{name}")
        raise AttributeError(f"module {__name__!r} has no attribute {name!r}")
    return getattr(importlib.import_module(target), name)


sys.modules[__name__].__class__ = _CallablePackage
