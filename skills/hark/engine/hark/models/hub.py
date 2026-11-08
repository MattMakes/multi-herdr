"""Model download for hark: fetch a missing snapshot, or confirm a cached one.

The on-disk lookup lives in ``hark.models._lookup``; this module re-exports it.
Only the network step needs huggingface_hub, and it imports the package lazily
after the ML extras check passes.
"""

from __future__ import annotations

from hark.core import config
from hark.core.safe import outcome
from hark.models import MLMissing, require_ml
from hark.models._lookup import (  # noqa: F401  (re-exported names)
    PICKLE,
    SAFETENSORS,
    Spec,
    is_folder,
    model_dir,
    resolve,
    slug,
    snapshot_dir,
)


def cache_name(model_id: str, revision: str | None = None) -> str:
    """Folder name for the embedding cache of this model."""
    return resolve(model_id, revision).cache_name


def allow_list(spec: Spec) -> list[str]:
    """The only file names a download for this spec may pull."""
    pin = config.PINS.get(spec.model_id)
    if not (spec.pinned and pin):
        return list(config.CUSTOM_ALLOW)
    return [*pin["files"], pin["weights"]]


def _download(repo_id: str, revision: str, cache_dir: str, allow_patterns: list[str]) -> str:
    require_ml("model download")
    from huggingface_hub import snapshot_download

    return snapshot_download(repo_id=repo_id, revision=revision, cache_dir=cache_dir, allow_patterns=allow_patterns)


def _refuse_reason(spec: Spec) -> RuntimeError | None:
    """Why this spec must not be downloaded, or None when a download may go ahead."""
    if spec.folder:
        return RuntimeError(
            f"{spec.label} lacks {' and '.join(spec.missing)}; hark loads a local folder as it is and "
            "never downloads into it; put config.json and model.safetensors (or an older checkpoint's "
            "pytorch_model.bin) there, or set the model variable to a Hub id")
    switch = config.offline()
    if not switch:
        return None
    if spec.state == "incomplete":
        problem = f"is incomplete in the cache (missing {', '.join(spec.missing)})"
    else:
        problem = f"is not in the cache {config.HF_CACHE}"
    return RuntimeError(
        f"{spec.label} {problem}, and {switch} is set, so hark will not download; run once without "
        f"{switch} (hark doctor --fetch downloads about 1.1 GB), copy a cache there, or point "
        "HARK_HOME at one")


def fetch(spec: Spec) -> Spec:
    """Pull the wanted revision into ``<HARK_HOME>/hf`` and return a fresh Spec."""
    refusal = _refuse_reason(spec)
    if refusal is not None:
        raise refusal
    config.ensure_dirs()
    _, failure = outcome(_download, spec.model_id, spec.revision, config.HF_CACHE, allow_list(spec))
    if isinstance(failure, MLMissing):
        raise failure
    if failure is not None:
        raise RuntimeError(f"could not download {spec.label} into {config.HF_CACHE}: "
                           f"{type(failure).__name__}: {failure}") from failure
    after = resolve(spec.model_id, None if spec.pinned else spec.revision)
    if after.state == "cached":
        return after
    note = "" if spec.pinned else "; hark loads safetensors weights only"
    raise RuntimeError(f"{spec.label}: after the download, {', '.join(after.missing) or 'files'} are still missing "
                       f"in {model_dir(spec.model_id)}{note}")


def ensure(model_id: str, revision: str | None = None) -> Spec:
    """Return a Spec whose snapshot can load, downloading first if necessary."""
    found = resolve(model_id, revision)
    if found.local:
        return found
    return fetch(found)
