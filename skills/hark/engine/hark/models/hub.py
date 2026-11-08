"""Pinned model files in hark's own Hugging Face cache: resolve, fetch, ensure.

hark keeps no legacy pickle snapshot of its own cache (ARCHITECTURE 15, core Q1):
a Spec state is ``cached``, ``incomplete`` or ``missing``. Pickle weights load only
from a user's local model folder.

Filesystem only, except ``fetch``, which imports huggingface_hub inside a
function after ``require_ml``.
"""

from __future__ import annotations

import hashlib
import os
import re
from dataclasses import dataclass, field
from pathlib import Path

from hark.core import config
from hark.core.safe import outcome
from hark.models import MLMissing, require_ml

SAFETENSORS = ("model.safetensors", "model.safetensors.index.json")
PICKLE = ("pytorch_model.bin", "pytorch_model.bin.index.json")
_HEX40 = re.compile(r"^[0-9a-f]{40}$")
_UNSAFE = re.compile(r"[^0-9A-Za-z._-]")


def _is_commit(rev: str | None) -> bool:
    return bool(rev) and bool(_HEX40.match(rev))


@dataclass(frozen=True)
class Spec:
    """What hark wants to load for one model, and what the cache holds."""

    model_id: str
    revision: str
    state: str
    snapshot: str | None
    weights: str
    missing: tuple[str, ...] = field(default_factory=tuple)
    pinned: bool = False
    folder: bool = False

    @property
    def local(self) -> bool:
        return self.snapshot is not None

    @property
    def sha(self) -> str | None:
        if self.snapshot is None or self.folder:
            return None
        return os.path.basename(os.path.normpath(self.snapshot))

    @property
    def label(self) -> str:
        if self.folder:
            return f"the local model folder {self.model_id}"
        rev = self.revision[:7] if _is_commit(self.revision) else self.revision
        return f"{self.model_id}@{rev}"

    @property
    def cache_tag(self) -> str:
        if self.pinned:
            return ""
        if self.folder:
            return "--local" + _folder_hash(self.model_id, self.weights)
        base = self.sha or _UNSAFE.sub("_", self.revision)
        return "--" + base[:8]

    @property
    def cache_name(self) -> str:
        if self.folder:
            stem = _UNSAFE.sub("_", os.path.basename(os.path.normpath(_expand(self.model_id))))[:40] or "model"
            return f"local--{stem}{self.cache_tag}"
        return slug(self.model_id) + self.cache_tag


def _expand(model_id: str) -> str:
    return os.path.abspath(os.path.expanduser(model_id))


def _folder_hash(model_id: str, weights: str) -> str:
    path = os.path.normcase(_expand(model_id))
    try:
        st = os.stat(os.path.join(path, weights))
        text = f"{path}|{st.st_size}|{st.st_mtime_ns}"
    except OSError:
        text = path
    return hashlib.sha1(text.encode("utf-8")).hexdigest()[:8]


def slug(model_id: str) -> str:
    """Repo id with every `/` replaced by `--`."""
    return model_id.replace("/", "--")


def is_folder(model_id: str) -> bool:
    """True for an existing local folder that is not a pinned id."""
    return bool(model_id) and model_id not in config.PINS and os.path.isdir(os.path.expanduser(model_id))


def model_dir(model_id: str) -> str:
    """<HF_CACHE>/models--<slug>."""
    return os.path.join(config.HF_CACHE, f"models--{slug(model_id)}")


def snapshot_dir(model_id: str, revision: str) -> str | None:
    """Snapshot folder of a commit, or of the commit a ref file names; None when unknown."""
    base = model_dir(model_id)
    if _is_commit(revision):
        return os.path.join(base, "snapshots", revision)
    try:
        commit = Path(base, "refs", *revision.split("/")).read_text(encoding="utf-8").strip()
    except (OSError, ValueError):
        return None
    return os.path.join(base, "snapshots", commit) if _is_commit(commit) else None


def _present(folder: str | None, name: str) -> bool:
    return folder is not None and os.path.exists(os.path.join(folder, name))


def _missing(folder: str | None, needs: list[tuple[str, ...]]) -> tuple[str, ...]:
    return tuple(alts[0] for alts in needs if not any(_present(folder, a) for a in alts))


def _first_present(folder: str | None, alts: tuple[str, ...]) -> str:
    return next((a for a in alts if _present(folder, a)), alts[0])


def _resolve_folder(model_id: str) -> Spec:
    path = _expand(model_id)
    weights_alts = SAFETENSORS + PICKLE
    missing = _missing(path, [("config.json",), weights_alts])
    weights = _first_present(path, weights_alts)
    state = "incomplete" if missing else "cached"
    return Spec(model_id, "local", state, None if missing else path, weights, missing, False, True)


def _wanted(model_id: str, revision: str | None) -> tuple[str, list[tuple[str, ...]], tuple, bool]:
    pin = config.PINS.get(model_id)
    if pin and revision in (None, pin["revision"]):
        needs = [(f,) for f in pin["files"]] + [(pin["weights"],)]
        return pin["revision"], needs, (pin["weights"],), True
    return revision or "main", [("config.json",), SAFETENSORS], SAFETENSORS, False


def resolve(model_id: str, revision: str | None = None) -> Spec:
    """What the cache holds for a model at the wanted revision (filesystem only)."""
    if is_folder(model_id):
        return _resolve_folder(model_id)
    wanted, needs, weights_alts, pinned = _wanted(model_id, revision)
    folder = snapshot_dir(model_id, wanted)
    missing = _missing(folder, needs)
    if not missing:
        return Spec(model_id, wanted, "cached", folder, _first_present(folder, weights_alts), (), pinned, False)
    state = "incomplete" if folder is not None and os.path.isdir(folder) else "missing"
    return Spec(model_id, wanted, state, None, weights_alts[0], missing, pinned, False)


def cache_name(model_id: str, revision: str | None = None) -> str:
    """The embedding cache folder name of a model."""
    return resolve(model_id, revision).cache_name


def allow_list(spec: Spec) -> list[str]:
    """Exact file names a download may fetch for this spec."""
    pin = config.PINS.get(spec.model_id)
    if spec.pinned and pin:
        return list(pin["files"]) + [pin["weights"]]
    return list(config.CUSTOM_ALLOW)


def _download(repo_id: str, revision: str, cache_dir: str, allow_patterns: list[str]) -> str:
    require_ml("model download")
    from huggingface_hub import snapshot_download

    return snapshot_download(repo_id=repo_id, revision=revision, cache_dir=cache_dir, allow_patterns=allow_patterns)


def _folder_error(spec: Spec) -> RuntimeError:
    return RuntimeError(f"{spec.label} lacks {' and '.join(spec.missing)}; hark loads a local folder as it is and "
                        "never downloads into it; put config.json and model.safetensors (or an older checkpoint's "
                        "pytorch_model.bin) there, or set the model variable to a Hub id")


def _offline_error(spec: Spec, flag: str) -> RuntimeError:
    where = (f"is incomplete in the cache (missing {', '.join(spec.missing)})" if spec.state == "incomplete"
             else f"is not in the cache {config.HF_CACHE}")
    return RuntimeError(f"{spec.label} {where}, and {flag} is set, so hark will not download; run once without "
                        f"{flag} (hark doctor --fetch downloads about 1.1 GB), copy a cache there, or point "
                        "HARK_HOME at one")


def _still_missing(spec: Spec, again: Spec) -> RuntimeError:
    extra = "" if spec.pinned else "; hark loads safetensors weights only"
    return RuntimeError(f"{spec.label}: after the download, {', '.join(again.missing) or 'files'} are still missing "
                        f"in {model_dir(spec.model_id)}{extra}")


def _check_fetchable(spec: Spec) -> None:
    flag = None if spec.folder else config.offline()
    if spec.folder:
        raise _folder_error(spec)
    if flag:
        raise _offline_error(spec, flag)


def _download_or_raise(spec: Spec) -> None:
    config.ensure_dirs()
    _, error = outcome(_download, spec.model_id, spec.revision, config.HF_CACHE, allow_list(spec))
    if isinstance(error, MLMissing):
        raise error
    if error is not None:
        raise RuntimeError(f"could not download {spec.label} into {config.HF_CACHE}: "
                           f"{type(error).__name__}: {error}") from error


def fetch(spec: Spec) -> Spec:
    """Download the wanted revision into <HARK_HOME>/hf and return the new Spec."""
    _check_fetchable(spec)
    _download_or_raise(spec)
    again = resolve(spec.model_id, None if spec.pinned else spec.revision)
    if again.state != "cached":
        raise _still_missing(spec, again)
    return again


def ensure(model_id: str, revision: str | None = None) -> Spec:
    """A Spec with a loadable snapshot; downloads when needed."""
    spec = resolve(model_id, revision)
    return spec if spec.local else fetch(spec)
