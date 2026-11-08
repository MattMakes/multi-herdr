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

from hark.core.safe import outcome

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
    return float(v) if math.isfinite(v) else None


def _clean_dict(obj: dict) -> dict:
    return {str(k): clean(v) for k, v in obj.items()}


def _clean_seq(obj: list | tuple) -> list:
    return [clean(v) for v in obj]


def _same(obj: Any) -> Any:
    return obj


def _clean_numpy(obj: Any) -> Any:
    """numpy arrays and scalars become Python lists and numbers; anything else its str."""
    import numpy as np

    plain = obj.tolist() if isinstance(obj, (np.ndarray, np.generic)) else str(obj)
    return clean(plain)


def _clean_other(obj: Any) -> Any:
    return _clean_numpy(obj) if "numpy" in sys.modules else str(obj)


_CLEANERS: tuple[tuple[Any, Callable[[Any], Any]], ...] = (
    (dict, _clean_dict),
    ((list, tuple), _clean_seq),
    ((bool, int, str, type(None)), _same),
    (float, _clean_float),
    (Path, str),
)


def clean(obj: Any) -> Any:
    """Make a result JSON-safe: numpy to Python, NaN/Inf to None, Path to str."""
    convert = next((fn for kinds, fn in _CLEANERS if isinstance(obj, kinds)), _clean_other)
    return convert(obj)


def to_json(result: dict, indent: int = 1) -> str:
    """JSON text with every non-ASCII character escaped."""
    return json.dumps(clean(result), indent=indent, ensure_ascii=True)


def _compact(value: Any) -> str:
    text = json.dumps(clean(value), ensure_ascii=False, separators=(",", ":"))
    return text[:_TEXT_CUT] + " ..." if len(text) > _TEXT_CUT else text


def _shown(value: Any) -> Any:
    return _compact(value) if isinstance(value, (list, tuple, dict)) else clean(value)


def render_text(result: dict) -> str:
    """Summary line, then one `key: value` line per other field."""
    rows = [f"{key}: {_shown(value)}" for key, value in result.items() if key not in ("summary", "text")]
    return "\n".join([str(result.get("summary", "")), *rows])


def _encodable(stream: Any, text: str) -> str:
    """text with every character the stream cannot encode as a backslash escape."""
    encoding = getattr(stream, "encoding", None) or "utf-8"
    return text.encode(encoding, "backslashreplace").decode(encoding)


def _write(stream: Any, text: str) -> None:
    stream.write(_encodable(stream, text))


def _write_out(result: dict, out: str | None) -> None:
    if not out:
        return
    target = Path(out)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps(clean(result), indent=1, ensure_ascii=False), encoding="utf-8")


def _text_of(result: dict, text: str | None) -> str:
    return str(text if text is not None else result.get("text") or render_text(result))


def emit(result: dict, args: argparse.Namespace, text: str | None = None) -> dict:
    """Write --out, then print JSON (with --json) or the text form. Returns result."""
    _write_out(result, getattr(args, "out", None))
    as_json = getattr(args, "json", False)
    body = to_json(result, indent=1) if as_json else _text_of(result, text)
    _write(sys.stdout, body + "\n")
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
    hundredths = int(round(float(t) * 100))
    minutes, rest = divmod(hundredths, 6000)
    return f"{minutes}:{rest / 100:05.2f}"


def _expected_types() -> tuple[type[BaseException], ...]:
    from hark.core.audio import AudioError

    return (AudioError, ValueError, OSError)


def _expected_failure(cmd: str, args: argparse.Namespace, exc: BaseException) -> int:
    msg = one_line(exc)
    error_line(cmd, msg)
    if getattr(args, "json", False):
        _write(sys.stdout, to_json({"summary": f"error: {msg}", "error": msg}) + "\n")
    return 1


def expected_errors(cmd: str) -> Callable[[Handler], Handler]:
    """Decorator for listen handlers: known errors give a line without the type, exit 1."""

    def wrap(handler: Handler) -> Handler:
        @functools.wraps(handler)
        def run(args: argparse.Namespace) -> int | None:
            code, error = outcome(handler, args, errors=_expected_types())
            return code if error is None else _expected_failure(cmd, args, error)

        return run

    return wrap
