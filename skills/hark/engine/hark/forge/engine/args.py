"""Bind the options of a function that takes many optional arguments.

`bind` gives the same results and the same TypeError cases as a plain signature with these defaults, and
`signature` gives `inspect` and `help()` that signature.
"""

from __future__ import annotations

import inspect
from typing import Any


def bind(name: str, defaults: dict, args: tuple, opts: dict) -> dict:
    """Optional arguments bound like a signature with these defaults, in order: positional, then keyword."""
    if len(args) > len(defaults):
        raise TypeError(f"{name}() takes at most {len(defaults)} optional positional arguments ({len(args)} given)")
    a = dict(zip(defaults, args))
    twice = sorted(set(a) & set(opts))
    unknown = sorted(set(opts) - set(defaults))
    if twice or unknown:
        what = f"multiple values for {twice[0]!r}" if twice else f"an unexpected keyword argument {unknown[0]!r}"
        raise TypeError(f"{name}() got {what}")
    return {**defaults, **a, **opts}


def signature(lead: tuple[str, ...], defaults: dict) -> inspect.Signature:
    """The signature: the lead names without defaults, then the defaults in order."""
    P = inspect.Parameter
    return inspect.Signature([P(n, P.POSITIONAL_OR_KEYWORD) for n in lead]
                             + [P(k, P.POSITIONAL_OR_KEYWORD, default=v) for k, v in defaults.items()])


def with_signature(fn: Any, lead: tuple[str, ...], defaults: dict) -> Any:
    fn.__signature__ = signature(lead, defaults)
    return fn
