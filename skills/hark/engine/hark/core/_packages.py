"""Which of hark's Python packages are importable, found without importing any of them."""

from __future__ import annotations

import importlib.metadata
import importlib.util

from hark.core.safe import attempt, outcome

PACKAGES: dict[str, str] = {
    "numpy": "numpy",
    "scipy": "scipy",
    "soundfile": "soundfile",
    "soxr": "soxr",
    "librosa": "librosa",
    "PIL": "pillow",
    "matplotlib": "matplotlib",
    "torch": "torch",
    "transformers": "transformers",
    "mcp": "mcp",
}
ML_PACKAGES = frozenset({"torch", "transformers"})
OPTIONAL_PACKAGES = frozenset({"mcp"})


def packages() -> dict[str, str]:
    """For each import name: its version, `ok`, or `MISSING (<Type>)`; nothing gets imported."""
    states: dict[str, str] = {}
    for module, dist in PACKAGES.items():
        found, failure = outcome(importlib.util.find_spec, module, errors=(ImportError, ValueError))
        if found is None:
            why = "ModuleNotFoundError" if failure is None else type(failure).__name__
            states[module] = f"MISSING ({why})"
        else:
            states[module] = attempt(importlib.metadata.version, dist,
                                     errors=(importlib.metadata.PackageNotFoundError,), default="ok")
    return states
