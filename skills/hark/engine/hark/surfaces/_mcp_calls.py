"""How an MCP tool reaches the hark function behind it, and how that call is guarded.

Calls run one at a time under ``_LOCK``, with the function's stdout moved to
stderr so that it never corrupts the stdio protocol stream.
"""

from __future__ import annotations

import contextlib
import importlib
import importlib.util
import inspect
import sys
import threading
from dataclasses import dataclass
from typing import Any, Callable

from hark.surfaces._flow import attempt
from hark.surfaces._mcp_args import ToolError
from hark.surfaces._mcp_catalog import TOOL_BY_NAME, TOOLS, Tool
from hark.surfaces._mcp_text import format_result

_LOCK = threading.RLock()


@dataclass
class ToolResult:
    """What one tool call returns: reply text, and JPEG bytes when it drew a picture."""

    text: str
    image: bytes | None = None


# backing modules ------------------------------------------------------


def _importable(module: str) -> bool:
    if module in sys.modules:
        return True
    try:
        return importlib.util.find_spec(module) is not None
    except (ImportError, ValueError):
        return False


def present_tools() -> list[str]:
    return [spec.name for spec in TOOLS if _importable(spec.module)]


def _load_failure(spec: Tool, exc: BaseException) -> ToolError:
    return ToolError(f"{spec.name}: module {spec.module} failed to load ({type(exc).__name__}: {exc}); "
                     "retry if hark is being updated, else run hark doctor")


def backing(tool: str, func: str | None = None) -> Callable:
    """The function behind a tool; ToolError when its module is missing or fails to import."""
    spec = TOOL_BY_NAME[tool]
    attr = func or spec.func
    try:
        module = importlib.import_module(spec.module)
    except ModuleNotFoundError as exc:
        gone = exc.name or ""
        if not gone or not (spec.module == gone or spec.module.startswith(gone + ".")):
            raise _load_failure(spec, exc) from None
        others = ", ".join(present_tools()) or "none"
        raise ToolError(f"{tool} is not part of this install (module {spec.module} is missing); "
                        f"present tools: {others}") from None
    except Exception as exc:
        raise _load_failure(spec, exc) from None
    target = getattr(module, attr, None)
    if target is None:
        raise ToolError(f"{tool} is not part of this install ({spec.module}.{attr} is missing)")
    return target


def filter_kwargs(fn: Callable, kwargs: dict[str, Any]) -> tuple[dict[str, Any], list[str]]:
    """Drop None values, then keep the names fn accepts; also return the names it refused."""
    given = {name: v for name, v in kwargs.items() if v is not None}
    accepted = attempt(lambda: inspect.signature(fn).parameters)
    takes_any = accepted is None or any(p.kind is p.VAR_KEYWORD for p in accepted.values())
    if takes_any:
        return given, []
    return ({name: v for name, v in given.items() if name in accepted},
            [name for name in given if name not in accepted])


def call_backing(tool: str, fn: Callable, kwargs: dict[str, Any]) -> tuple[Any, list[str]]:
    """Call a backing function under the server lock, with its stdout moved to stderr."""
    chosen, ignored = filter_kwargs(fn, kwargs)
    with _LOCK, contextlib.redirect_stdout(sys.stderr):
        try:
            return fn(**chosen), ignored
        except ToolError:
            raise
        except Exception as exc:
            if type(exc).__name__ == "MLMissing":
                feature = getattr(exc, "feature", "this tool")
                command = getattr(exc, "command", "")
                raise ToolError(f"{tool} failed: MLMissing: {feature} needs the ml extras. "
                                f"Install them with: {command}") from None
            raise ToolError(f"{tool} failed: {type(exc).__name__}: {exc}") from None


def _as_record(res: Any) -> dict:
    return dict(res) if isinstance(res, dict) else {"summary": str(res)}


def _run(tool: str, kwargs: dict[str, Any], full: bool) -> ToolResult:
    res, ignored = call_backing(tool, backing(tool), kwargs)
    return ToolResult(format_result(res, full, ignored))
