"""Optional ML extras: detect them without importing them, and name the install command.

Standard library only.
"""

from __future__ import annotations

import importlib.util
import shlex
import sys

ML_MODULES: tuple[str, ...] = ("torch", "transformers")
EXTRA_MODULES: dict[str, tuple[str, ...]] = {"ml": ML_MODULES, "mcp": ("mcp",)}


class MLMissing(RuntimeError):
    """An optional extra is missing. Carries the feature, the absent modules and the install command."""

    def __init__(self, feature: str, missing: tuple[str, ...], command: str, extra: str = "ml") -> None:
        self.feature = feature
        self.missing = tuple(missing)
        self.command = command
        self.extra = extra
        super().__init__(f"{feature} needs the {extra} extras. Install them with: {command}")


def _findable(name: str) -> bool:
    """True when the top-level module is loaded or has an import spec. Imports nothing."""
    loaded = sys.modules.get(name) is not None
    return loaded or importlib.util.find_spec(name) is not None


def _absent(names: tuple[str, ...]) -> tuple[str, ...]:
    return tuple(name for name in names if not _findable(name))


def missing_ml() -> tuple[str, ...]:
    """Import names of the ML extras that cannot be found. Imports nothing."""
    return _absent(ML_MODULES)


def ml_available() -> bool:
    """True when torch and transformers can be found."""
    return not missing_ml()


def install_command(extra: str = "ml") -> str:
    """The exact shell command that installs an extra into this Python."""
    py = shlex.quote(sys.executable)
    if extra == "mcp":
        return f'{py} -m pip install "mcp>=2"'
    return (f'{py} -m pip install --index-url https://download.pytorch.org/whl/cpu "torch>=2.6"'
            f' && {py} -m pip install "transformers>=4.40"')


def _modules_of(extra: str) -> tuple[str, ...]:
    modules = EXTRA_MODULES.get(extra)
    if modules is None:
        raise ValueError(f"unknown extra: {extra}")
    return modules


def require_extra(extra: str, feature: str) -> None:
    """Raise MLMissing when a module of the extra ("ml" or "mcp") is absent."""
    modules = _modules_of(extra)
    absent = _absent(modules)
    if absent:
        raise MLMissing(feature, absent, install_command(extra), extra)


def require_ml(feature: str) -> None:
    """Raise MLMissing when torch or transformers is absent."""
    require_extra("ml", feature)
