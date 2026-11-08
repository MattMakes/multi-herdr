"""Per-model cache rows for `hark doctor`, and a download of the models that are missing."""

from __future__ import annotations

from hark.core import config, report
from hark.models import hub


def _pinned() -> list[tuple[str, str | None]]:
    return [(config.CLAP_MODEL, config.CLAP_REVISION), (config.AST_MODEL, config.AST_REVISION)]


def models() -> dict:
    """Cache state of each configured model, read from disk only."""
    table = {}
    for model_id, rev in _pinned():
        spec = hub.resolve(model_id, rev)
        table[model_id] = {"revision": spec.revision, "cached": spec.state, "loads": spec.sha,
                           "weights": spec.weights, "snapshot": spec.snapshot, "missing": list(spec.missing)}
    return table


def fetch() -> dict:
    """Download each configured model that is not cached yet; never raises."""
    outcomes = {}
    for model_id, rev in _pinned():
        spec = hub.resolve(model_id, rev)
        if spec.state == "cached":
            outcomes[model_id] = "a local folder (nothing to download)" if spec.folder else "already cached"
            continue
        try:
            got = hub.fetch(spec)
        except RuntimeError as exc:
            outcomes[model_id] = f"failed: {report.one_line(exc)}"
        else:
            outcomes[model_id] = f"downloaded {(got.sha or got.revision)[:7]}"
    return outcomes
