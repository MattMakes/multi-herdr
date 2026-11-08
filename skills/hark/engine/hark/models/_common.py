"""Helpers shared by the CLAP and AudioSet model wrappers.

torch and transformers are imported only inside functions, after require_ml.
"""

from __future__ import annotations

import contextlib
import hashlib
import logging
import os
import warnings
from collections.abc import Iterator
from typing import Any

import numpy as np

from hark.core import audio, config
from hark.models import hub, require_ml

NO_BF16_CAPS = frozenset({"DEFAULT", "NO AVX", "AVX", "AVX2", "ZVECTOR", "VSX"})
NO_BF16_ONEDNN = frozenset({"SSE41", "AVX", "AVX2", "AVX2_VNNI", "AVX512_CORE", "AVX512_CORE_VNNI"})
PICKLE_WEIGHTS = frozenset(hub.PICKLE)
# torch logs this 1 line on macOS when it imports torch.distributed; it says nothing about hark.
REDIRECTS_LOGGER = "torch.distributed.elastic.multiprocessing.redirects"
REDIRECTS_NOTE = "NOTE: Redirects are currently not supported in MacOs."


def _not_redirects_note(record: logging.LogRecord) -> bool:
    return record.getMessage() != REDIRECTS_NOTE


def drop_redirects_note() -> None:
    """Filter the torch redirects note by its exact message; other records of that logger pass."""
    logger = logging.getLogger(REDIRECTS_LOGGER)
    if _not_redirects_note not in logger.filters:
        logger.addFilter(_not_redirects_note)


def key24(text: str) -> str:
    """First 24 hex of SHA-1 of the text."""
    return hashlib.sha1(text.encode("utf-8")).hexdigest()[:24]


def wanted(model_id: str | None, configured: str, configured_rev: str | None) -> tuple[str, str | None]:
    """Model id to load and its revision override (only for the configured id)."""
    chosen = model_id or configured
    return chosen, (configured_rev if chosen == configured else None)


def torch_ready(feature: str) -> Any:
    """Check the extras, import torch, set its thread count."""
    require_ml(feature)
    drop_redirects_note()
    import torch

    torch.set_num_threads(config.threads())
    return torch


@contextlib.contextmanager
def hush_loaders() -> Iterator[None]:
    """Quiet transformers during a load: errors only, no progress bars, no mel or future warnings."""
    drop_redirects_note()
    import transformers

    transformers.logging.set_verbosity_error()
    with contextlib.suppress(Exception):
        transformers.utils.logging.disable_progress_bar()
    logging.getLogger("transformers").setLevel(logging.ERROR)
    with warnings.catch_warnings():
        warnings.filterwarnings("ignore", message=".*mel.*")
        warnings.filterwarnings("ignore", category=FutureWarning)
        yield


def load_error(spec: hub.Spec, cls: str, exc: BaseException) -> RuntimeError:
    """The one-line load failure, with advice that fits the folder kind."""
    head = f"cannot load {spec.label}: {cls} failed: {type(exc).__name__}: {exc}"
    if spec.folder:
        tail = "the files can be damaged or this transformers version cannot read them"
    else:
        tail = (f"the files in {spec.snapshot} can be damaged or this transformers version cannot read them; "
                "delete that folder and hark fetches the pinned files again, or run hark doctor")
    return RuntimeError(f"{head}; {tail}")


def load_pretrained(cls: Any, spec: hub.Spec, **extra: Any) -> Any:
    """Load a class from the spec's snapshot with local files only."""
    kwargs: dict[str, Any] = {"local_files_only": True, **extra}
    try:
        with hush_loaders():
            return cls.from_pretrained(spec.snapshot, **kwargs)
    except Exception as exc:  # noqa: BLE001 - every load failure becomes one RuntimeError
        raise load_error(spec, cls.__name__, exc) from exc


def load_options(spec: hub.Spec) -> dict:
    """use_safetensors unless the spec's weights are a pickle."""
    return {} if spec.weights in PICKLE_WEIGHTS else {"use_safetensors": True}


def _cap_says_no(torch: Any) -> bool:
    try:
        return str(torch.backends.cpu.get_cpu_capability()).upper() in NO_BF16_CAPS
    except Exception:  # noqa: BLE001 - a probe error is ignored
        return False


def _cpu_says_yes(torch: Any) -> bool:
    for probe in ("_is_avx512_bf16_supported", "_is_amx_tile_supported"):
        try:
            if getattr(torch.cpu, probe)():
                return True
        except Exception:  # noqa: BLE001 - a missing probe counts as no
            continue
    return False


def native_bf16(torch: Any) -> bool:
    """True only when the CPU has native bf16 matrix instructions."""
    if _cap_says_no(torch):
        return False
    if os.environ.get("ONEDNN_MAX_CPU_ISA", "").upper() in NO_BF16_ONEDNN:
        return False
    return _cpu_says_yes(torch)


def choose_precision(torch: Any, model: Any) -> tuple[Any, str]:
    """bf16 after a smoke test on a native-bf16 CPU when asked; else fp32."""
    if config.PRECISION != "bf16" or not native_bf16(torch):
        return model.float(), "fp32"
    try:
        model = model.to(torch.bfloat16)
        with torch.inference_mode():
            a = torch.ones((2, 2), dtype=torch.bfloat16)
            _ = a @ a
        return model, "bf16"
    except Exception:  # noqa: BLE001 - any bf16 failure falls back to fp32 (E-M14)
        return model.float(), "fp32"


def to_model_inputs(torch: Any, inputs: Any, dtype: Any) -> dict:
    """Floating input tensors to the model dtype; other tensors as they are."""
    return {k: (v.to(dtype) if hasattr(v, "is_floating_point") and v.is_floating_point() else v)
            for k, v in dict(inputs).items()}


def pool_output(out: Any) -> Any:
    """The tensor of a features call: the pooled output when it is an output object."""
    if hasattr(out, "pooler_output") and out.pooler_output is not None:
        return out.pooler_output
    return out


def unit_rows(a: np.ndarray) -> np.ndarray:
    """Divide each row by its L2 norm + 1e-12, float32."""
    a = np.asarray(a, dtype=np.float32)
    return (a / (np.linalg.norm(a, axis=-1, keepdims=True) + 1e-12)).astype(np.float32)


def embed_store(kind: str, spec: hub.Spec, precision: str) -> str:
    """<EMB_CACHE>/<kind>/<cache_name>--<precision>, made."""
    folder = os.path.join(config.EMB_CACHE, kind, f"{spec.cache_name}--{precision}")
    os.makedirs(folder, exist_ok=True)
    return folder


def read_npy(path: str) -> np.ndarray | None:
    """A cached vector, or None when absent or unreadable."""
    try:
        return np.load(path, allow_pickle=False) if os.path.isfile(path) else None
    except (OSError, ValueError):
        return None


def write_npy(path: str, value: np.ndarray) -> None:
    """Save with a temporary file and replace_quietly; failures only cost a recompute."""
    tmp = f"{path[:-4]}.{os.getpid()}.tmp.npy"
    try:
        np.save(tmp, np.asarray(value, dtype=np.float32))
    except OSError:
        return
    audio.replace_quietly(tmp, path)


def file_key(path: str, *parts: object) -> str:
    """Cache key of a file range: SHA-1 of file_id and the parts (None prints as None)."""
    return key24("|".join([audio.file_id(path)] + [repr(p) if isinstance(p, float) else str(p) for p in parts]))
