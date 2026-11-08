"""`hark doctor`: is the install whole, are the models cached, and how fast do they load.

Unless asked to load the models, the check stays light: it looks for packages
with find_spec, reads the cache from disk and never imports torch.
"""

from __future__ import annotations

import argparse
import importlib
import json
import os
import platform
import shutil
from pathlib import Path
from typing import Any

from hark.core import config, report
from hark.core._packages import (  # noqa: F401  (re-exported names)
    ML_PACKAGES,
    OPTIONAL_PACKAGES,
    PACKAGES,
    packages,
)
from hark.models import MLMissing, hub, install_command, missing_ml
from hark.models._cache_report import fetch, models

DATA_FILES = ("listen/vocab.json", "listen/vocab_stats.json", "listen/realness_model.json",
              "timeline/launch_profile.json")
# These data files are built by the user, not shipped (ARCHITECTURE 12). When one
# is absent the doctor adds a note with the command that builds it.
OPTIONAL_DATA = {"listen/vocab_stats.json": "python -m hark.listen.vocabulary --build-stats REF ..."}


# the sound bank -------------------------------------------------------


def _count_entries(path: str) -> int | None:
    try:
        with open(path, encoding="utf-8") as handle:
            table = json.load(handle).get("entries")
    except (OSError, ValueError, AttributeError):
        return None
    if not isinstance(table, dict):
        return None
    return len(table)


def bank_info() -> dict:
    """What the loaded sound bank holds, or {error} when it cannot load."""
    try:
        bank = importlib.import_module("hark.forge.bank.bank")
        names = list(bank.ids())
        kinds: dict[str, int] = {}
        for sound in names:
            kind = bank.get(sound).get("type") or "forge"
            kinds[kind] = kinds.get(kind, 0) + 1
        files = []
        lister = getattr(bank, "bank_files", None)
        if callable(lister):
            shipped = os.path.abspath(str(Path(bank.__file__).parent / "data"))
            for item in map(str, lister()):
                inside = os.path.dirname(os.path.abspath(item)) == shipped
                files.append({"file": os.path.basename(item) if inside else item, "entries": _count_entries(item)})
        return {
            "version": bank.VERSION,
            "sounds": len(names),
            "types": kinds,
            "pluck": len([s for s in names if s.startswith("pluck/")]),
            "files": files,
            "extra": [piece for piece in os.environ.get("HARK_BANK", "").split(os.pathsep) if piece],
        }
    except Exception as exc:  # noqa: BLE001 - report a broken bank and keep checking
        return {"error": f"{type(exc).__name__}: {report.one_line(exc)}"}


# load timing ----------------------------------------------------------


def _load_models() -> dict:
    """Load and time both models; MLMissing or RuntimeError propagate."""
    from hark.models._load_timing import time_both

    return time_both()


# problems and notes ---------------------------------------------------


def _package_problems(pkgs: dict[str, str]) -> list[str]:
    tolerated = OPTIONAL_PACKAGES | ML_PACKAGES
    return [f"package {module} {state}" for module, state in pkgs.items()
            if state.startswith("MISSING") and module not in tolerated]


def _tool_problems(tools: dict[str, str]) -> list[str]:
    return [f"{tool} missing" for tool, where in tools.items() if where.startswith("MISSING")]


def _data_files() -> dict[str, bool]:
    return {rel: os.path.isfile(os.path.join(config.DATA_DIR, rel)) for rel in DATA_FILES}


def _failed(outcomes: dict, model_id: str) -> bool:
    return outcomes.get(model_id, "").startswith("failed")


def _findings(state: dict) -> tuple[list[str], list[str], list[str]]:
    """Problems, notes and the model ids that are neither cached nor a folder nor failed."""
    rows, outcomes, offline = state["models"], state.get("fetched", {}), state["offline"]
    absent = [model_id for model_id, row in rows.items()
              if row["snapshot"] is None and not hub.is_folder(model_id) and not _failed(outcomes, model_id)]
    data_dir = f"{Path(config.PKG_DIR).name}/data"

    faults = _package_problems(state["packages"])
    if state["bank"].get("error") is not None:
        faults.append(f"sound bank: {state['bank']['error']}")
    faults += _tool_problems({k: state[k] for k in ("ffmpeg", "ffprobe")})
    faults += [f"data file {data_dir}/{file_name} missing" for file_name, present in state["data_files"].items()
                 if not present and file_name not in OPTIONAL_DATA]
    faults += [f"{model_id}: {said}" for model_id, said in outcomes.items() if said.startswith("failed")]
    faults += [f"{model_id}: the local model folder lacks {' and '.join(row['missing'])} "
                 "(hark never downloads into a folder)"
                 for model_id, row in rows.items()
                 if hub.is_folder(model_id) and row["cached"] == "incomplete" and not _failed(outcomes, model_id)]
    if absent and offline:
        faults.append(f"{' and '.join(absent)} not cached in {config.HF_CACHE} and {offline} is set; run hark "
                        f"doctor --fetch once without {offline}, or copy a cache there")

    advice: list[str] = []
    if absent and not offline:
        advice.append(f"{' and '.join(absent)} not cached yet; the first call that needs them downloads the pinned "
                     f"safetensors files (about 1.1 GB for both) into {config.HF_CACHE}; or run hark doctor "
                     "--fetch now")
    lacking = missing_ml()
    if lacking:
        advice.append(f"the ML extras are not installed ({', '.join(lacking)}); listening commands need them; "
                     f"install them with: {install_command('ml')}")
    if not shutil.which("agy"):
        advice.append("agy not found on PATH; the optional LLM critique needs it")
    advice += [f"data file {data_dir}/{file_name} is not built; build it with: {OPTIONAL_DATA[file_name]}"
              for file_name, present in state["data_files"].items() if not present and file_name in OPTIONAL_DATA]
    return faults, advice, absent


def _summary(bank: dict, faults: list[str], advice: list[str]) -> str:
    pieces = []
    if "error" not in bank:
        kinds = bank.get("types", {})
        plucks = bank.get("pluck", 0)
        extra = f"; extra files from HARK_BANK: {', '.join(bank['extra'])}" if bank.get("extra") else ""
        pieces.append(f"Sound bank {bank.get('version')}: {bank.get('sounds')} sounds "
                      f"({kinds.get('forge', 0) - plucks} forge, {plucks} plucks, {kinds.get('fit', 0)} fitted "
                      f"from CC0 recordings{extra}).")
    if faults:
        pieces.append("Problems: " + "; ".join(piece.rstrip(".") for piece in faults) + ".")
    else:
        pieces.append("hark looks healthy.")
    pieces.extend(n.rstrip(".") + "." for n in advice)
    return " ".join(piece for piece in pieces if piece)


def check(load: bool = False, fetch_missing: bool = False) -> dict:
    """The full doctor report (2.7); a problem it can name never raises."""
    config.ensure_dirs()
    state: dict[str, Any] = {
        "python": platform.python_version(),
        "hark_home": config.HOME,
        "threads": config.threads(),
        "precision": config.PRECISION,
        "clap_model": config.CLAP_MODEL,
        "ast_model": config.AST_MODEL,
        "packages": packages(),
        "ffmpeg": shutil.which("ffmpeg") or "MISSING (install ffmpeg and put it on PATH)",
        "ffprobe": shutil.which("ffprobe") or "MISSING (ffprobe comes with ffmpeg; put it on PATH)",
        "torch_cuda_used": False,
        "offline": config.offline(),
    }
    if fetch_missing:
        state["fetched"] = fetch()
    state["models"] = models()
    state["models_cached"] = {model_id: row["snapshot"] is not None for model_id, row in state["models"].items()}
    state["agy"] = shutil.which("agy")
    state["data_files"] = _data_files()
    state["bank"] = bank_info()
    faults, advice, absent = _findings(state)
    if load and not (state["offline"] and absent):
        try:
            state.update(_load_models())
        except MLMissing as exc:
            faults.append(f"model load failed: {exc.feature} needs torch and transformers; "
                            f"install them with: {exc.command}")
        except RuntimeError as exc:
            faults.append(f"model load failed: {report.one_line(exc)}")
    state["problems"] = faults
    state["notes"] = advice
    state["summary"] = _summary(state["bank"], faults, advice)
    return state


# command line ---------------------------------------------------------


def run_cli(args: argparse.Namespace) -> int:
    """Run `hark doctor`; the exit code is 0 even when it finds problems."""
    report.emit(check(load=args.load, fetch_missing=args.fetch), args)
    return 0


def add_cli(sub: Any) -> None:
    """Add the `hark doctor` subcommand."""
    parser = sub.add_parser("doctor", help="Check the install, models, data files and CPU settings.",
                            description="Check the install, models, data files and CPU settings.")
    parser.add_argument("--load", action="store_true", help="also load both models and time them")
    parser.add_argument("--fetch", action="store_true", help="download the pinned models that are not cached")
    report.add_common(parser)
    parser.set_defaults(func=run_cli)
