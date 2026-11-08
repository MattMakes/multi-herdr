"""Call a function and keep the exception it raises as a value.

Standard library only. It keeps try/except blocks out of small functions.
"""

from __future__ import annotations

from collections.abc import Callable
from typing import Any

Errors = tuple[type[BaseException], ...]


def outcome(fn: Callable[..., Any], *args: Any, errors: Errors = (Exception,)) -> tuple[Any, BaseException | None]:
    """(result, None), or (None, exception) when fn raised one of errors."""
    caught = tuple(errors)
    result, error = None, None
    try:
        result = fn(*args)
    except caught as exc:
        error = exc
    return result, error


def attempt(fn: Callable[..., Any], *args: Any, errors: Errors = (Exception,), default: Any = None) -> Any:
    """fn(*args), or default when it raised one of errors."""
    result, error = outcome(fn, *args, errors=errors)
    return default if error is not None else result
