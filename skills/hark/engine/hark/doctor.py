"""`hark doctor`: install check, model cache state, optional load timing and fetch.

Without ``load`` it imports nothing heavy.
"""

from __future__ import annotations

import argparse
import importlib
import importlib.metadata
import importlib.util
import json
import os
import platform
import shutil
import time
from pathlib import Path
from typing import Any

from hark.core import config, report
from hark.core.safe import attempt, outcome
from hark.models import MLMissing, hub, install_command, missing_ml

PACKAGES: dict[str, str] = {
    "numpy": "numpy",
    "scipy": "scipy",
    "soundfile": "soundfile",
    "soxr": "soxr",
    "librosa": "librosa",
    "PIL": "pillow",
    "matplotlib": "matplotlib",
    "torch": "torch",
    "transformers": "transformers",
    "mcp": "mcp",
}
ML_PACKAGES = frozenset({"torch", "transformers"})
OPTIONAL_PACKAGES = frozenset({"mcp"})
DATA_FILES = ("listen/vocab.json", "listen/vocab_stats.json", "listen/realness_model.json",
              "timeline/launch_profile.json")


# ---------------------------------------------------------------- basics


def _version(dist: str) -> str:
    return attempt(importlib.metadata.version, dist, errors=(importlib.metadata.PackageNotFoundError,), default="ok")


def _package_state(name: str, dist: str) -> str:
    spec, error = outcome(importlib.util.find_spec, name, errors=(ImportError, ValueError))
    missing = type(error).__name__ if error is not None else "ModuleNotFoundError"
    return _version(dist) if spec is not None else f"MISSING ({missing})"


def packages() -> dict[str, str]:
    """Import name -> version, `ok`, or `MISSING (<Type>)`. Imports nothing."""
    return {name: _package_state(name, dist) for name, dist in PACKAGES.items()}


def _tools() -> dict[str, str]:
    ffmpeg = shutil.which("ffmpeg") or "MISSING (install ffmpeg and put it on PATH)"
    ffprobe = shutil.which("ffprobe") or "MISSING (ffprobe comes with ffmpeg; put it on PATH)"
    return {"ffmpeg": ffmpeg, "ffprobe": ffprobe}


def _basics() -> dict[str, Any]:
    return {
        "python": platform.python_version(),
        "hark_home": config.HOME,
        "threads": config.threads(),
        "precision": config.PRECISION,
        "clap_model": config.CLAP_MODEL,
        "ast_model": config.AST_MODEL,
    }


def _model_ids() -> list[tuple[str, str | None]]:
    return [(config.CLAP_MODEL, config.CLAP_REVISION), (config.AST_MODEL, config.AST_REVISION)]


# ---------------------------------------------------------------- models


def _model_row(spec: hub.Spec) -> dict:
    return {"revision": spec.revision, "cached": spec.state, "loads": spec.sha, "weights": spec.weights,
            "snapshot": spec.snapshot, "missing": list(spec.missing)}


def models() -> dict:
    """Model id -> cache state, filesystem only."""
    return {mid: _model_row(hub.resolve(mid, rev)) for mid, rev in _model_ids()}


def _fetch_one(model_id: str, revision: str | None) -> str:
    spec = hub.resolve(model_id, revision)
    if spec.state == "cached":
        return "a local folder (nothing to download)" if spec.folder else "already cached"
    try:
        new = hub.fetch(spec)
    except RuntimeError as exc:
        return f"failed: {report.one_line(exc)}"
    return f"downloaded {(new.sha or new.revision)[:7]}"


def fetch() -> dict:
    """Download every configured model that is not cached. Never raises."""
    return {mid: _fetch_one(mid, rev) for mid, rev in _model_ids()}


# ---------------------------------------------------------------- bank


def _entry_count(path: str) -> int | None:
    try:
        with open(path, encoding="utf-8") as fh:
            entries = json.load(fh).get("entries")
    except (OSError, ValueError, AttributeError):
        return None
    return len(entries) if isinstance(entries, dict) else None


def _bank_files(bank: Any) -> list[dict]:
    """One {file, entries} row per bank file; the base name for hark's own bank folder."""
    lister = getattr(bank, "bank_files", None)
    if not callable(lister):
        return []
    own = os.path.abspath(str(Path(bank.__file__).parent / "data"))
    rows = []
    for path in map(str, lister()):
        name = os.path.basename(path) if os.path.dirname(os.path.abspath(path)) == own else path
        rows.append({"file": name, "entries": _entry_count(path)})
    return rows


def _bank_types(bank: Any, ids: list[str]) -> dict[str, int]:
    counts: dict[str, int] = {}
    for sound in ids:
        kind = bank.get(sound).get("type") or "forge"
        counts[kind] = counts.get(kind, 0) + 1
    return counts


def bank_info() -> dict:
    """Summary of the sound bank as loaded, or {error}."""
    try:
        bank = importlib.import_module("hark.forge.bank.bank")
        ids = list(bank.ids())
        return {
            "version": bank.VERSION,
            "sounds": len(ids),
            "types": _bank_types(bank, ids),
            "pluck": sum(1 for s in ids if s.startswith("pluck/")),
            "files": _bank_files(bank),
            "extra": [p for p in os.environ.get("HARK_BANK", "").split(os.pathsep) if p],
        }
    except Exception as exc:  # noqa: BLE001 - any bank failure is reported, the check continues
        return {"error": f"{type(exc).__name__}: {report.one_line(exc)}"}


# ---------------------------------------------------------------- load timing


def _time_clap() -> dict:
    import numpy as np

    from hark.models import clap

    t0 = time.perf_counter()
    model = clap.get()
    loaded = time.perf_counter() - t0
    t1 = time.perf_counter()
    model.embed_audio([np.zeros(48000, dtype=np.float32)])
    return {"clap_load_s": round(loaded, 1), "clap_one_window_s": round(time.perf_counter() - t1, 2),
            "clap_precision": model.precision, "clap_revision": model.spec.revision, "clap_weights": model.weights}


def _time_ast() -> dict:
    import numpy as np

    from hark.models import ast

    t0 = time.perf_counter()
    model = ast.get()
    loaded = time.perf_counter() - t0
    t1 = time.perf_counter()
    model.probs([np.zeros(48000, dtype=np.float32)])
    return {"ast_load_s": round(loaded, 1), "ast_one_window_s": round(time.perf_counter() - t1, 2),
            "ast_revision": model.spec.revision, "ast_weights": model.weights}


def _load_models() -> dict:
    """Load both models and time them. Raises RuntimeError or MLMissing."""
    out = _time_clap()
    out.update(_time_ast())
    import torch

    from hark.models import clap

    out["native_bf16"] = clap.native_bf16(torch)
    return out


def _load_step(result: dict, problems: list[str]) -> None:
    try:
        result.update(_load_models())
    except MLMissing as exc:
        problems.append(f"model load failed: {exc.feature} needs torch and transformers; "
                        f"install them with: {exc.command}")
    except RuntimeError as exc:
        problems.append(f"model load failed: {report.one_line(exc)}")


# ---------------------------------------------------------------- problems and notes


def _package_problems(pkgs: dict[str, str]) -> list[str]:
    skip = OPTIONAL_PACKAGES | ML_PACKAGES
    return [f"package {name} {state}" for name, state in pkgs.items()
            if state.startswith("MISSING") and name not in skip]


def _tool_problems(tools: dict[str, str]) -> list[str]:
    return [f"{name} missing" for name, value in tools.items() if value.startswith("MISSING")]


def _data_files() -> dict[str, bool]:
    return {name: os.path.isfile(os.path.join(config.DATA_DIR, name)) for name in DATA_FILES}


def _uncached(rows: dict, fetched: dict) -> list[str]:
    return [mid for mid, row in rows.items()
            if row["snapshot"] is None and not hub.is_folder(mid) and not fetched.get(mid, "").startswith("failed")]


def _folder_problems(rows: dict, fetched: dict) -> list[str]:
    return [f"{mid}: the local model folder lacks {' and '.join(row['missing'])} (hark never downloads into a folder)"
            for mid, row in rows.items()
            if hub.is_folder(mid) and row["cached"] == "incomplete" and not fetched.get(mid, "").startswith("failed")]


def _offline_problem(names: list[str], flag: str) -> str:
    return (f"{' and '.join(names)} not cached in {config.HF_CACHE} and {flag} is set; run hark doctor --fetch once "
            f"without {flag}, or copy a cache there")


def _model_notes(uncached: list[str], flag: str | None) -> list[str]:
    if not uncached or flag:
        return []
    return [f"{' and '.join(uncached)} not cached yet; the first call that needs them downloads the pinned "
            f"safetensors files (about 1.1 GB for both) into {config.HF_CACHE}; or run hark doctor --fetch now"]


def _ml_note() -> list[str]:
    absent = missing_ml()
    if not absent:
        return []
    return [f"the ML extras are not installed ({', '.join(absent)}); listening commands need them; "
            f"install them with: {install_command('ml')}"]


def _bank_line(bank: dict) -> str:
    if "error" in bank:
        return ""
    types = bank.get("types", {})
    forge = types.get("forge", 0) - bank.get("pluck", 0)
    extra = f"; extra files from HARK_BANK: {', '.join(bank['extra'])}" if bank.get("extra") else ""
    return (f"Sound bank {bank.get('version')}: {bank.get('sounds')} sounds ({forge} forge, {bank.get('pluck', 0)} "
            f"plucks, {types.get('fit', 0)} fitted from CC0 recordings{extra}).")


def _summary(bank: dict, problems: list[str], notes: list[str]) -> str:
    health = ("Problems: " + "; ".join(p.rstrip(".") for p in problems) + ".") if problems else "hark looks healthy."
    parts = [_bank_line(bank), health] + [n.rstrip(".") + "." for n in notes]
    return " ".join(p for p in parts if p)


# ---------------------------------------------------------------- check


def _gather(fetch_missing: bool) -> dict[str, Any]:
    result: dict[str, Any] = _basics()
    result["packages"] = packages()
    result.update(_tools())
    result["torch_cuda_used"] = False
    result["offline"] = config.offline()
    if fetch_missing:
        result["fetched"] = fetch()
    result["models"] = models()
    result["models_cached"] = {mid: row["snapshot"] is not None for mid, row in result["models"].items()}
    result["agy"] = shutil.which("agy")
    result["data_files"] = _data_files()
    result["bank"] = bank_info()
    return result


def _install_problems(result: dict) -> list[str]:
    bank_error = result["bank"].get("error")
    problems = _package_problems(result["packages"])
    problems += [f"sound bank: {bank_error}"] if bank_error is not None else []
    problems += _tool_problems({k: result[k] for k in ("ffmpeg", "ffprobe")})
    data_dir = f"{Path(config.PKG_DIR).name}/data"
    return problems + [f"data file {data_dir}/{n} missing" for n, ok in result["data_files"].items() if not ok]


def _model_problems(result: dict, uncached: list[str]) -> list[str]:
    fetched, offline = result.get("fetched", {}), result["offline"]
    problems = [f"{mid}: {msg}" for mid, msg in fetched.items() if msg.startswith("failed")]
    problems += _folder_problems(result["models"], fetched)
    return problems + ([_offline_problem(uncached, offline)] if uncached and offline else [])


def _notes(result: dict, uncached: list[str]) -> list[str]:
    agy = [] if shutil.which("agy") else ["agy not found on PATH; the optional LLM critique needs it"]
    return _model_notes(uncached, result["offline"]) + _ml_note() + agy


def _problems(result: dict) -> tuple[list[str], list[str], list[str]]:
    uncached = _uncached(result["models"], result.get("fetched", {}))
    problems = _install_problems(result) + _model_problems(result, uncached)
    return problems, _notes(result, uncached), uncached


def check(load: bool = False, fetch_missing: bool = False) -> dict:
    """The doctor result (2.7). Never raises for a problem it can name."""
    config.ensure_dirs()
    result = _gather(fetch_missing)
    problems, notes, uncached = _problems(result)
    if load and not (result["offline"] and uncached):
        _load_step(result, problems)
    result["problems"] = problems
    result["notes"] = notes
    result["summary"] = _summary(result["bank"], problems, notes)
    return result


# ---------------------------------------------------------------- CLI


def run_cli(args: argparse.Namespace) -> int:
    """Handler of `hark doctor`; exit 0 also with problems."""
    report.emit(check(load=args.load, fetch_missing=args.fetch), args)
    return 0


def add_cli(sub: Any) -> None:
    """Register `hark doctor`."""
    p = sub.add_parser("doctor", help="Check the install, models, data files and CPU settings.",
                       description="Check the install, models, data files and CPU settings.")
    p.add_argument("--load", action="store_true", help="also load both models and time them")
    p.add_argument("--fetch", action="store_true", help="download the pinned models that are not cached")
    report.add_common(p)
    p.set_defaults(func=run_cli)
