"""The `hark` command: global flags, lazy command dispatch, BLAS thread hold, errors, exit codes.

Imports only the standard library and hark.core.config at module level.
Command modules are imported with importlib when they are needed.
"""

from __future__ import annotations

import argparse
import functools
import importlib
import os
import re
import shlex
import subprocess
import sys
from collections.abc import Callable, Sequence
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


def _outcome(fn: Callable[..., Any], *args: Any,
             errors: tuple[type[BaseException], ...] = (Exception,)) -> tuple[Any, BaseException | None]:
    """(result, None), or (None, exception) when fn raised one of errors."""
    caught = tuple(errors)
    result, error = None, None
    try:
        result = fn(*args)
    except caught as exc:
        error = exc
    return result, error


def _attempt(fn: Callable[..., Any], *args: Any, errors: tuple[type[BaseException], ...] = (Exception,),
             default: Any = None) -> Any:
    """fn(*args), or default when it raised one of errors."""
    result, error = _outcome(fn, *args, errors=errors)
    return default if error is not None else result


# ---------------------------------------------------------------- thread hold


def _threads_value(argv: Sequence[str], i: int) -> str | None:
    """The --threads value that argument i gives, or None."""
    arg = argv[i]
    if arg.startswith("--threads="):
        return arg.split("=", 1)[1]
    following = argv[i + 1] if i + 1 < len(argv) else None
    return following if arg == "--threads" else None


def _threads_values(argv: Sequence[str]) -> list[str]:
    values = (_threads_value(argv, i) for i in range(len(argv)))
    return [v for v in values if v is not None]


def _hold_count(argv: Sequence[str]) -> int | None:
    """The BLAS thread count: the last --threads above 0, else config.threads(); None when not an integer."""
    values = _threads_values(argv)
    wanted = _attempt(int, values[-1] if values else "0", errors=(ValueError,))
    if wanted is None:
        return None
    return wanted if wanted > 0 else _attempt(config.threads, errors=(ValueError,))


def blas_hold(argv: Sequence[str]) -> None:
    """Set OPENBLAS_NUM_THREADS before numpy loads, unless the user set it."""
    held = "OPENBLAS_NUM_THREADS" in os.environ or "numpy" in sys.modules
    count = None if held else _hold_count(argv)
    if count is not None:
        os.environ["OPENBLAS_NUM_THREADS"] = str(count)


# ---------------------------------------------------------------- command lookup


def find_command(argv: Sequence[str]) -> str | None:
    """The first argument that is not an option, skipping a global --threads value."""
    skipped = {i + 1 for i, arg in enumerate(argv) if arg == "--threads"}
    words = (arg for i, arg in enumerate(argv) if i not in skipped and not arg.startswith("-"))
    return next(words, None)


def _not_built(exc: BaseException, module: str) -> bool:
    name = getattr(exc, "name", None) or ""
    missing = isinstance(exc, ModuleNotFoundError) and bool(name)
    return missing and (module == name or module.startswith(name + "."))


def _unavailable(command: str, exc: BaseException) -> CommandUnavailable:
    module = MODULE_OF[command]
    if _not_built(exc, module):
        return CommandUnavailable(f'hark: the "{command}" command is not built yet (module {module} is missing)')
    return CommandUnavailable(f'hark: the "{command}" command is unavailable: {type(exc).__name__}: {_line(exc)}')


def _import_command(command: str) -> Any:
    module, error = _outcome(importlib.import_module, MODULE_OF[command])
    if error is not None:
        raise _unavailable(command, error)
    return module


def _import_noted(module: str) -> Any:
    """Import a command module; on failure print a note and return None."""
    found, error = _outcome(importlib.import_module, module)
    if error is not None:
        _say(f"(hark: module {module} unavailable: {type(error).__name__}: {_line(error)})")
    return found


def _import_all() -> list[Any]:
    modules = dict.fromkeys(module for _, module in COMMANDS)
    loaded = [_import_noted(module) for module in modules]
    return [module for module in loaded if module is not None]


# ---------------------------------------------------------------- parser


def _parser() -> tuple[argparse.ArgumentParser, Any]:
    from hark import __version__

    parser = argparse.ArgumentParser(prog="hark", description="Listen to, measure, make and review sound effects.")
    parser.add_argument("--version", action="version", version=f"hark {__version__}")
    parser.add_argument("--threads", dest="global_threads", type=int, default=None, metavar="N",
                        help="CPU threads for torch and BLAS (0 = default)")
    sub = parser.add_subparsers(dest="command", metavar="<command>")
    return parser, sub


def _build(argv: Sequence[str]) -> argparse.ArgumentParser:
    """The parser with the requested command only, or with every command."""
    parser, sub = _parser()
    command = find_command(argv)
    modules = [_import_command(command)] if command in MODULE_OF else _import_all()
    for module in modules:
        module.add_cli(sub)
    return parser


def _exit_code(stop: SystemExit) -> int:
    return EXIT_USAGE if isinstance(stop.code, str) else int(stop.code or 0)


def _parse(parser: argparse.ArgumentParser, argv: Sequence[str]) -> argparse.Namespace | int:
    """The parsed arguments, or the exit code of help or a parser error."""
    args, stop = _outcome(parser.parse_args, list(argv), errors=(SystemExit,))
    return args if stop is None else _exit_code(stop)


def _apply_threads(args: argparse.Namespace) -> None:
    later = getattr(args, "threads", None)
    value = later if later else getattr(args, "global_threads", None)
    if value:
        os.environ["HARK_THREADS"] = str(value)


def _safe_stdout(command: str) -> None:
    """Escape unencodable characters on stdout, except for the mcp protocol channel."""
    reconfigure = getattr(sys.stdout, "reconfigure", None)
    if command == "mcp" or not callable(reconfigure):
        return
    _outcome(functools.partial(reconfigure, errors="backslashreplace"), errors=(ValueError, OSError))


# ---------------------------------------------------------------- ML install prompt


def _isatty(stream: Any) -> bool:
    check = getattr(stream, "isatty", None)
    return bool(callable(check) and _attempt(check, errors=(ValueError, OSError), default=False))


def _is_tty() -> bool:
    return _isatty(sys.stdin) and _isatty(sys.stdout)


def _install_step(part: str) -> bool:
    proc = subprocess.run(shlex.split(part), check=False)
    if proc.returncode != 0:
        _say(f"hark: error: the install step failed (exit {proc.returncode}): {part}")
    return proc.returncode == 0


def _run_install(command: str) -> bool:
    """Run the install command, split at &&, without a shell. Stops at the first failed step."""
    steps = [part.strip() for part in command.split("&&")]
    return all(_install_step(part) for part in steps)


def _ask_install(command: str, exc: Any) -> bool:
    _say(f"hark {command}: {exc.feature} needs the {exc.extra} extras ({', '.join(exc.missing)}).")
    _say(exc.command)
    sys.stderr.write("Install them now? [y/N] ")
    sys.stderr.flush()
    answer = sys.stdin.readline().strip().lower()
    return answer in ("y", "yes")


def _prompt_install(command: str, exc: Any) -> int | None:
    """Ask, then install. Returns an exit code, or None to run the handler again."""
    if not _ask_install(command, exc):
        return EXIT_INSTALL
    installed = _run_install(exc.command)
    importlib.invalidate_caches()
    return None if installed else 1


def _install_line(command: str, exc: Any) -> str:
    return (f"hark {command}: error: MLMissing: {exc.feature} needs the {exc.extra} extras. "
            f"Install them with: {exc.command}")


def _missing_extra(command: str, exc: Any) -> int | None:
    """Prompt on a TTY; else one error line. Returns an exit code, or None to run again."""
    if _is_tty():
        return _prompt_install(command, exc)
    _say(_install_line(command, exc))
    return EXIT_INSTALL


# ---------------------------------------------------------------- run


def _handler_error(command: str, exc: BaseException) -> int:
    if os.environ.get("HARK_DEBUG"):
        raise exc
    _say(f"hark {command}: error: {type(exc).__name__}: {_line(exc)}")
    return 1


def _invoke(handler: Any, args: argparse.Namespace) -> tuple[int, BaseException | None]:
    """(exit code, None), or (0, exception) when the handler raised."""
    code, error = _outcome(handler, args)
    return (0 if code is None else int(code)), error


def _after_install(handler: Any, args: argparse.Namespace, command: str,
                   exc: BaseException) -> tuple[int, BaseException | None]:
    stop = _missing_extra(command, exc)
    return (stop, None) if stop is not None else _invoke(handler, args)


def _failure(command: str, exc: BaseException, ml_type: type[BaseException]) -> int:
    """Exit 3 when the extras are still missing after the prompt; else the one-line error."""
    return EXIT_INSTALL if isinstance(exc, ml_type) else _handler_error(command, exc)


def _run(handler: Any, args: argparse.Namespace, command: str) -> int:
    from hark.models import MLMissing

    code, error = _invoke(handler, args)
    if isinstance(error, MLMissing):
        code, error = _after_install(handler, args, command, error)
    return code if error is None else _failure(command, error, MLMissing)


def _dispatch(parser: argparse.ArgumentParser, args: argparse.Namespace) -> int:
    _apply_threads(args)
    handler = getattr(args, "func", None)
    if not args.command or handler is None:
        parser.print_help()
        return 0
    _safe_stdout(args.command)
    return _run(handler, args, args.command)


def main(argv: Sequence[str] | None = None) -> int:
    """Run the hark command line; returns the exit code."""
    argv = list(sys.argv[1:] if argv is None else argv)
    blas_hold(argv)
    parser, error = _outcome(_build, argv, errors=(CommandUnavailable,))
    if error is not None:
        _say(str(error))
        return EXIT_USAGE
    args = _parse(parser, argv)
    return args if isinstance(args, int) else _dispatch(parser, args)


if __name__ == "__main__":
    sys.exit(main())
