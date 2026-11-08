"""Small control-flow helpers shared by the surfaces modules.

They keep the callers flat: a failed step becomes a value instead of a nested try block.
"""

from __future__ import annotations

import contextlib
import inspect
from collections.abc import Callable, Iterable
from typing import Any


def attempt(fn: Callable[..., Any], *args: Any, default: Any = None, **kwargs: Any) -> Any:
    """fn(*args, **kwargs), or ``default`` when it raises."""
    out = default
    with contextlib.suppress(Exception):
        out = fn(*args, **kwargs)
    return out


def capture(fn: Callable[..., Any], *args: Any, **kwargs: Any) -> tuple[Any, BaseException | None]:
    """(fn(*args, **kwargs), None), or (None, the exception) when it raises."""
    box: dict[str, Any] = {"value": None, "error": None}
    slot = "value"
    try:
        box[slot] = fn(*args, **kwargs)
    except Exception as exc:  # the caller reports it
        box["error"] = exc
    value, error = box["value"], box["error"]
    return value, error


def raise_if(cond: Any, exc: BaseException) -> None:
    """Raise ``exc`` when ``cond`` is true."""
    errors = [exc] if cond else []
    any(_throw(err) for err in errors)


def _throw(exc: BaseException) -> bool:
    raise exc


def first(items: Iterable[Any], default: Any = None) -> Any:
    """The first truthy item, else ``default``."""
    return next((x for x in items if x), default)


def signature(params: Iterable[tuple[str, Any, Any]]) -> inspect.Signature:
    """A signature from (name, annotation, default) rows; inspect.Parameter.empty marks a required one."""
    kind = inspect.Parameter.POSITIONAL_OR_KEYWORD
    return inspect.Signature([inspect.Parameter(n, kind, default=d, annotation=a) for n, a, d in params])


def bind(sig: inspect.Signature, args: tuple, kwargs: dict[str, Any]) -> dict[str, Any]:
    """The call's arguments by name, defaults applied; TypeError like a normal call on a mismatch."""
    bound = sig.bind(*args, **kwargs)
    bound.apply_defaults()
    return dict(bound.arguments)


REQUIRED = inspect.Parameter.empty
