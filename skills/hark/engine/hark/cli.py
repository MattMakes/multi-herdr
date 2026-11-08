"""The `hark` command: global flags, lazy command dispatch, BLAS thread hold, errors, exit codes.

Imports only the standard library and hark.core.config at module level.
Command modules are imported with importlib when they are needed.
"""

from __future__ import annotations

import argparse
import importlib
import os
import re
import shlex
import subprocess
import sys
from collections.abc import Sequence
from typing import Any

from hark.core import config

COMMANDS: tuple[tuple[str, str], ...] = (
    ("describe", "hark.listen.describe"),
    ("match", "hark.listen.match"),
    ("rank", "hark.listen.search"),
    ("find", "hark.listen.search"),
    ("compare", "hark.listen.compare"),
    ("ab", "hark.listen.compare"),
    ("realness", "hark.listen.realness"),
    ("timeline", "hark.timeline.timeline"),
    ("sheet", "hark.timeline.sheet"),
    ("critique", "hark.timeline.critique"),
    ("profile", "hark.listen.profile"),
    ("forge", "hark.forge.engine.cli"),
    ("fit", "hark.fit.pipeline.cli"),
    ("sfx", "hark.forge.bank.sfx"),
    ("kit", "hark.forge.bank.kit"),
    ("gallery", "hark.surfaces.gallery"),
    ("measure", "hark.core.features"),
    ("llm", "hark.surfaces.llm"),
    ("mcp", "hark.surfaces.mcp_server"),
    ("doctor", "hark.doctor"),
)
MODULE_OF = dict(COMMANDS)
EXIT_USAGE = 2
EXIT_INSTALL = 3
_SPACE = re.compile(r"\s+")


class CommandUnavailable(Exception):
    """The module of a requested command cannot be imported."""


def _say(text: str) -> None:
    sys.stderr.write(text + "\n")
    sys.stderr.flush()


def _line(exc: BaseException) -> str:
    return _SPACE.sub(" ", str(exc)).strip() or "unknown error"


# ---------------------------------------------------------------- thread hold


def _threads_values(argv: Sequence[str]) -> list[str]:
    values = []
    for i, arg in enumerate(argv):
        if arg == "--threads" and i + 1 < len(argv):
            values.append(argv[i + 1])
        elif arg.startswith("--threads="):
            values.append(arg.split("=", 1)[1])
    return values


def _hold_count(argv: Sequence[str]) -> int | None:
    values = _threads_values(argv)
    try:
        wanted = int(values[-1]) if values else 0
    except ValueError:
        return None
    if wanted > 0:
        return wanted
    try:
        return config.threads()
    except ValueError:
        return None


def blas_hold(argv: Sequence[str]) -> None:
    """Set OPENBLAS_NUM_THREADS before numpy loads, unless the user set it."""
    if "OPENBLAS_NUM_THREADS" in os.environ or "numpy" in sys.modules:
        return
    count = _hold_count(argv)
    if count is not None:
        os.environ["OPENBLAS_NUM_THREADS"] = str(count)


# ---------------------------------------------------------------- command lookup


def find_command(argv: Sequence[str]) -> str | None:
    """The first argument that is not an option, skipping a global --threads value."""
    skip = False
    for arg in argv:
        if skip:
            skip = False
        elif arg == "--threads":
            skip = True
        elif not arg.startswith("-"):
            return arg
    return None


def _not_built(exc: ModuleNotFoundError, module: str) -> bool:
    name = exc.name or ""
    return bool(name) and (module == name or module.startswith(name + "."))


def _import(module: str) -> Any:
    return importlib.import_module(module)


def _import_command(command: str) -> Any:
    module = MODULE_OF[command]
    try:
        return _import(module)
    except ModuleNotFoundError as exc:
        if _not_built(exc, module):
            raise CommandUnavailable(f'hark: the "{command}" command is not built yet (module {module} is missing)')
        raise CommandUnavailable(f'hark: the "{command}" command is unavailable: {type(exc).__name__}: {_line(exc)}')
    except Exception as exc:  # noqa: BLE001 - any import failure makes the command unavailable
        raise CommandUnavailable(f'hark: the "{command}" command is unavailable: {type(exc).__name__}: {_line(exc)}')


def _import_all() -> list[Any]:
    loaded, seen = [], set()
    for _, module in COMMANDS:
        if module in seen:
            continue
        seen.add(module)
        try:
            loaded.append(_import(module))
        except Exception as exc:  # noqa: BLE001 - a broken module is skipped with a note
            _say(f"(hark: module {module} unavailable: {type(exc).__name__}: {_line(exc)})")
    return loaded


# ---------------------------------------------------------------- parser


def _parser() -> tuple[argparse.ArgumentParser, Any]:
    from hark import __version__

    parser = argparse.ArgumentParser(prog="hark", description="Listen to, measure, make and review sound effects.")
    parser.add_argument("--version", action="version", version=f"hark {__version__}")
    parser.add_argument("--threads", dest="global_threads", type=int, default=None, metavar="N",
                        help="CPU threads for torch and BLAS (0 = default)")
    sub = parser.add_subparsers(dest="command", metavar="<command>")
    return parser, sub


def _register(sub: Any, modules: list[Any]) -> None:
    for module in modules:
        module.add_cli(sub)


def _build(argv: Sequence[str]) -> argparse.ArgumentParser:
    parser, sub = _parser()
    command = find_command(argv)
    if command in MODULE_OF:
        _register(sub, [_import_command(command)])
    else:
        _register(sub, _import_all())
    return parser


def _parse(parser: argparse.ArgumentParser, argv: Sequence[str]) -> argparse.Namespace | int:
    try:
        return parser.parse_args(list(argv))
    except SystemExit as exc:
        return int(exc.code or 0) if not isinstance(exc.code, str) else EXIT_USAGE


def _apply_threads(args: argparse.Namespace) -> None:
    later = getattr(args, "threads", None)
    value = later if later else getattr(args, "global_threads", None)
    if value:
        os.environ["HARK_THREADS"] = str(value)


def _safe_stdout(command: str) -> None:
    if command == "mcp":
        return
    reconfigure = getattr(sys.stdout, "reconfigure", None)
    if callable(reconfigure):
        try:
            reconfigure(errors="backslashreplace")
        except (ValueError, OSError):
            pass


# ---------------------------------------------------------------- ML install prompt


def _is_tty() -> bool:
    try:
        return sys.stdin.isatty() and sys.stdout.isatty()
    except (AttributeError, ValueError):
        return False


def _run_install(command: str) -> bool:
    for part in command.split("&&"):
        proc = subprocess.run(shlex.split(part.strip()), check=False)
        if proc.returncode != 0:
            _say(f"hark: error: the install step failed (exit {proc.returncode}): {part.strip()}")
            return False
    return True


def _ask_install(command: str, exc: Any) -> bool:
    _say(f"hark {command}: {exc.feature} needs the {exc.extra} extras ({', '.join(exc.missing)}).")
    _say(exc.command)
    sys.stderr.write("Install them now? [y/N] ")
    sys.stderr.flush()
    answer = sys.stdin.readline().strip().lower()
    return answer in ("y", "yes")


def _missing_extra(command: str, exc: Any) -> int | None:
    """Prompt on a TTY; else one error line. Returns an exit code, or None to run again."""
    if not _is_tty():
        _say(f"hark {command}: error: MLMissing: {exc.feature} needs the {exc.extra} extras. "
             f"Install them with: {exc.command}")
        return EXIT_INSTALL
    if not _ask_install(command, exc):
        return EXIT_INSTALL
    if not _run_install(exc.command):
        return 1
    importlib.invalidate_caches()
    return None


# ---------------------------------------------------------------- run


def _handler_error(command: str, exc: BaseException) -> int:
    if os.environ.get("HARK_DEBUG"):
        raise exc
    _say(f"hark {command}: error: {type(exc).__name__}: {_line(exc)}")
    return 1


def _call(handler: Any, args: argparse.Namespace) -> int:
    code = handler(args)
    return 0 if code is None else int(code)


def _run(handler: Any, args: argparse.Namespace, command: str) -> int:
    from hark.models import MLMissing

    for attempt in range(2):
        try:
            return _call(handler, args)
        except MLMissing as exc:
            code = _missing_extra(command, exc) if attempt == 0 else EXIT_INSTALL
            if code is not None:
                return code
        except Exception as exc:  # noqa: BLE001 - every handler error becomes one line, exit 1
            return _handler_error(command, exc)
    return EXIT_INSTALL


def main(argv: Sequence[str] | None = None) -> int:
    """Run the hark command line; returns the exit code."""
    argv = list(sys.argv[1:] if argv is None else argv)
    blas_hold(argv)
    try:
        parser = _build(argv)
    except CommandUnavailable as exc:
        _say(str(exc))
        return EXIT_USAGE
    args = _parse(parser, argv)
    if isinstance(args, int):
        return args
    _apply_threads(args)
    handler = getattr(args, "func", None)
    if not args.command or handler is None:
        parser.print_help()
        return 0
    _safe_stdout(args.command)
    return _run(handler, args, args.command)


if __name__ == "__main__":
    sys.exit(main())
