"""Output convention of every command: summary first, --json, --out, one-line errors."""

from __future__ import annotations

import argparse
import functools
import json
import math
import re
import sys
from collections.abc import Callable
from pathlib import Path
from typing import Any

Handler = Callable[[argparse.Namespace], "int | None"]

_TEXT_CUT = 600
_SPACE = re.compile(r"\s+")


def add_common(parser: argparse.ArgumentParser) -> None:
    """Add the shared flags --json, --out FILE and --threads N."""
    parser.add_argument("--json", action="store_true", help="print the full result as JSON")
    parser.add_argument("--out", metavar="FILE", default=None, help="also write the full result as JSON to FILE")
    parser.add_argument("--threads", type=int, default=None, metavar="N",
                        help="CPU threads for this process (0 = default)")


def _clean_float(v: float) -> float | None:
    return v if math.isfinite(v) else None


def _clean_numpy(obj: Any) -> Any:
    import numpy as np

    if isinstance(obj, np.ndarray):
        return clean(obj.tolist())
    if isinstance(obj, np.bool_):
        return bool(obj)
    if isinstance(obj, np.integer):
        return int(obj)
    if isinstance(obj, np.floating):
        return _clean_float(float(obj))
    return str(obj)


def clean(obj: Any) -> Any:
    """Make a result JSON-safe: numpy to Python, NaN/Inf to None, Path to str."""
    if isinstance(obj, dict):
        return {str(k): clean(v) for k, v in obj.items()}
    if isinstance(obj, (list, tuple)):
        return [clean(v) for v in obj]
    if obj is None or isinstance(obj, (bool, int, str)):
        return obj
    if isinstance(obj, float):
        return _clean_float(obj)
    if isinstance(obj, Path):
        return str(obj)
    if "numpy" in sys.modules:
        return _clean_numpy(obj)
    return str(obj)


def to_json(result: dict, indent: int = 1) -> str:
    """JSON text with every non-ASCII character escaped."""
    return json.dumps(clean(result), indent=indent, ensure_ascii=True)


def _compact(value: Any) -> str:
    text = json.dumps(clean(value), ensure_ascii=False, separators=(",", ":"))
    return text[:_TEXT_CUT] + " ..." if len(text) > _TEXT_CUT else text


def render_text(result: dict) -> str:
    """Summary line, then one `key: value` line per other field."""
    lines = [str(result.get("summary", ""))]
    for key, value in result.items():
        if key in ("summary", "text"):
            continue
        shown = _compact(value) if isinstance(value, (list, tuple, dict)) else clean(value)
        lines.append(f"{key}: {shown}")
    return "\n".join(lines)


def _write(stream: Any, text: str) -> None:
    try:
        stream.write(text)
    except UnicodeEncodeError:
        encoding = getattr(stream, "encoding", None) or "ascii"
        stream.write(text.encode(encoding, "backslashreplace").decode(encoding))


def _write_out(result: dict, out: str) -> None:
    target = Path(out)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps(clean(result), indent=1, ensure_ascii=False), encoding="utf-8")


def emit(result: dict, args: argparse.Namespace, text: str | None = None) -> dict:
    """Write --out, then print JSON (with --json) or the text form. Returns result."""
    out = getattr(args, "out", None)
    if out:
        _write_out(result, out)
    if getattr(args, "json", False):
        _write(sys.stdout, to_json(result, indent=1) + "\n")
    else:
        body = text if text is not None else result.get("text") or render_text(result)
        _write(sys.stdout, str(body) + "\n")
    return result


def one_line(msg: Any) -> str:
    """Collapse whitespace runs to one space; empty becomes `unknown error`."""
    return _SPACE.sub(" ", str(msg)).strip() or "unknown error"


def error_line(cmd: str, msg: str) -> None:
    """Print `hark <cmd>: error: <msg>` on stderr, as one line."""
    _write(sys.stderr, f"hark {cmd}: error: {one_line(msg)}\n")


def fmt_t(t: float | None) -> str:
    """Seconds as m:ss.ss; None gives `-`."""
    if t is None:
        return "-"
    minutes, seconds = divmod(float(t), 60.0)
    if round(seconds, 2) >= 60.0:
        minutes, seconds = minutes + 1, 0.0
    return f"{int(minutes)}:{seconds:05.2f}"


def _expected_types() -> tuple[type[BaseException], ...]:
    from hark.core.audio import AudioError

    return (AudioError, ValueError, OSError)


def expected_errors(cmd: str) -> Callable[[Handler], Handler]:
    """Decorator for listen handlers: known errors give a line without the type, exit 1."""

    def wrap(handler: Handler) -> Handler:
        @functools.wraps(handler)
        def run(args: argparse.Namespace) -> int | None:
            try:
                return handler(args)
            except _expected_types() as exc:
                msg = one_line(exc)
                error_line(cmd, msg)
                if getattr(args, "json", False):
                    _write(sys.stdout, to_json({"summary": f"error: {msg}", "error": msg}) + "\n")
                return 1

        return run

    return wrap
