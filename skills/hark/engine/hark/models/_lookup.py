"""What the hark model cache holds for one request, checked on disk only.

A model reaches hark in one of two ways. A Hub id is looked up under
``<HARK_HOME>/hf`` at a revision (a pinned commit for the default models). A
directory on disk is read as it stands and never written to. Each lookup
reports one of three states: ``cached``, ``incomplete`` or ``missing``.
"""

from __future__ import annotations

import os
from pathlib import Path

from hark.core import config
from hark.models._spec import Spec, absolute_path, looks_like_sha, slug  # noqa: F401  (slug is re-exported)

SAFETENSORS = ("model.safetensors", "model.safetensors.index.json")
PICKLE = ("pytorch_model.bin", "pytorch_model.bin.index.json")


def is_folder(model_id: str) -> bool:
    """Whether the id names a directory on disk rather than a pinned model."""
    if not model_id or model_id in config.PINS:
        return False
    return os.path.isdir(os.path.expanduser(model_id))


def model_dir(model_id: str) -> str:
    """The cache directory of one repo: ``<HF_CACHE>/models--<slug>``."""
    return os.path.join(config.HF_CACHE, "models--" + slug(model_id))


def snapshot_dir(model_id: str, revision: str) -> str | None:
    """Map a commit, or a ref that points at one, to its snapshot path."""
    repo = model_dir(model_id)
    if looks_like_sha(revision):
        target = revision
    else:
        ref_file = Path(repo, "refs", *revision.split("/"))
        try:
            target = ref_file.read_text(encoding="utf-8").strip()
        except (OSError, ValueError):
            return None
        if not looks_like_sha(target):
            return None
    return os.path.join(repo, "snapshots", target)


def _has(where: str | None, name: str) -> bool:
    if where is None:
        return False
    return os.path.exists(os.path.join(where, name))


def _gaps(where: str | None, groups: list[tuple[str, ...]]) -> tuple[str, ...]:
    """The first name of every group that has no member on disk."""
    absent = []
    for group in groups:
        if not any(_has(where, name) for name in group):
            absent.append(group[0])
    return tuple(absent)


def _pick(where: str | None, choices: tuple[str, ...]) -> str:
    for name in choices:
        if _has(where, name):
            return name
    return choices[0]


def _lookup_dir(model_id: str) -> Spec:
    root = absolute_path(model_id)
    choices = SAFETENSORS + PICKLE
    gaps = _gaps(root, [("config.json",), choices])
    return Spec(
        model_id=model_id,
        revision="local",
        state="incomplete" if gaps else "cached",
        snapshot=None if gaps else root,
        weights=_pick(root, choices),
        missing=gaps,
        pinned=False,
        folder=True,
    )


def _request(model_id: str, revision: str | None):
    """Revision, required file groups, weight choices and pin flag for a Hub id."""
    pin = config.PINS.get(model_id)
    if pin is not None and revision in (None, pin["revision"]):
        weight = pin["weights"]
        groups = [(name,) for name in pin["files"]]
        groups.append((weight,))
        return pin["revision"], groups, (weight,), True
    return revision or "main", [("config.json",), SAFETENSORS], SAFETENSORS, False


def resolve(model_id: str, revision: str | None = None) -> Spec:
    """Inspect the cache for a model at a revision without any network use."""
    if is_folder(model_id):
        return _lookup_dir(model_id)
    rev, groups, choices, pinned = _request(model_id, revision)
    where = snapshot_dir(model_id, rev)
    gaps = _gaps(where, groups)
    if gaps:
        state = "incomplete" if where is not None and os.path.isdir(where) else "missing"
        return Spec(model_id, rev, state, None, choices[0], gaps, pinned, False)
    return Spec(model_id, rev, "cached", where, _pick(where, choices), (), pinned, False)
