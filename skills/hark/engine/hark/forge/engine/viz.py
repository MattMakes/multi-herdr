"""One PNG that shows each made sound beside its nearest reference.

Per row: the made spectrogram, the paired ref's spectrogram, both envelopes,
and the 1/3-octave balance against the refs. matplotlib loads lazily with the
Agg backend.
"""

from __future__ import annotations

from collections.abc import Sequence
from typing import Any

import numpy as np
from scipy import signal

from hark.forge.engine import tone

SR = 48000
_MADE, _REF = "tab:blue", "tab:red"


def _mono(x: Any) -> np.ndarray:
    return tone.as_mono(x)


def onset_align(x: Any, db: float = -30.0, pre_ms: float = 5.0) -> np.ndarray:
    """The mono sound from pre_ms before its first sample within db of the peak."""
    m = _mono(x)
    if not len(m):
        return m
    a = np.abs(m)
    i = int(np.argmax(a >= a.max() * 10 ** (db / 20)))
    return m[max(0, i - int(SR * pre_ms / 1000)):]


def _fit(m: np.ndarray, n: int) -> np.ndarray:
    return m[:n] if len(m) >= n else np.concatenate([m, np.zeros(n - len(m))])


def spec_db(x: Any, span_s: float) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    """Log-magnitude STFT over span_s: 0 dB = loudest bin, floor -80."""
    m = _fit(_mono(x), max(256, int(span_s * SR)))
    nfft = 512 if span_s < 0.6 else 1024 if span_s < 2.5 else 2048
    nfft = min(nfft, len(m))
    f, t, Z = signal.stft(m, SR, nperseg=nfft, noverlap=nfft - nfft // 8, boundary=None, padded=False)
    S = 20 * np.log10(np.abs(Z) + 1e-12)
    S = np.maximum(S - S.max(), -80.0)
    return t * 1000, f, S


def envelope_db(x: Any, span_s: float, hop_ms: float = 2.0) -> tuple[np.ndarray, np.ndarray]:
    """RMS envelope in dB re its peak, floor -80."""
    hop = max(1, int(SR * hop_ms / 1000))
    m = _fit(_mono(x), max(1, int(span_s * SR)))
    k = max(1, len(m) // hop)
    fr = _fit(m, k * hop).reshape(k, hop)
    e = 10 * np.log10(np.mean(fr ** 2, axis=1) + 1e-20)
    return (np.arange(k) + 0.5) * hop_ms, np.maximum(e - e.max(), -80.0)


def _nearest(made: np.ndarray, refs: Sequence[np.ndarray]) -> int | None:
    # viz sits below the search layer, so it pairs by tonal balance alone.
    if not refs:
        return None
    p = tone.profile(made)
    d = [float(np.sqrt(np.mean((p - tone.profile(r)) ** 2))) for r in refs]
    return int(np.argmin(d))


def _plot_spec(ax: Any, x: np.ndarray, span: float, title: str) -> None:
    t, f, S = spec_db(x, span)
    ax.pcolormesh(t, np.maximum(f, 1.0), S, vmin=-80, vmax=0, shading="auto", cmap="magma")
    ax.set_yscale("log")
    ax.set_ylim(40, 24000)
    ax.set_xlim(0, span * 1000)
    ax.set_title(title)


def _plot_env(ax: Any, made: np.ndarray, ref: np.ndarray | None, span: float, names: tuple) -> None:
    hop = max(2.0, span * 1000 / 300)
    ax.plot(*envelope_db(made, span, hop), color=_MADE, label=names[0])
    if ref is not None:
        ax.plot(*envelope_db(ref, span, hop), color=_REF, label=names[1])
    ax.set_ylim(-70, 3)
    ax.set_xlim(0, span * 1000)
    ax.set_title("envelope (dB re peak)")
    ax.legend(loc="upper right")


def _plot_tone(ax: Any, made: np.ndarray, refs: Sequence[np.ndarray], legend: bool) -> None:
    cen = np.asarray(tone.CEN)
    p = tone.profile(made)
    title = "1/3-octave balance"
    if refs:
        P = np.array([tone.profile(r) for r in refs])
        med = np.median(P, axis=0)
        ax.fill_between(cen, P.min(axis=0), P.max(axis=0), color=_REF, alpha=0.2, label="refs min-max")
        ax.plot(cen, med, "--", color=_REF, label="refs median")
        title += f": {float(np.sqrt(np.mean((p - med) ** 2))):.1f} dB from the refs"
    ax.plot(cen, p, color=_MADE, label="made")
    ax.set_xscale("log")
    ax.set_xlim(25, 20000)
    ax.set_ylim(-60, 0)
    ax.set_title(title)
    if legend:
        ax.legend(loc="lower left")


def _row(axes: Any, item: tuple, refs: list, j: int | None, max_s: float, first: bool) -> None:
    name, x = item
    made = onset_align(x)
    ref = onset_align(refs[j][1]) if j is not None else None
    span = min(max_s, max(len(made), len(ref) if ref is not None else 0) / SR) or 0.01
    _plot_spec(axes[0], made, span, f"made: {name}")
    if ref is not None:
        _plot_spec(axes[1], ref, span, f"nearest ref: {refs[j][0]}")
    else:
        axes[1].axis("off")
    _plot_env(axes[2], made, ref, span, (name, refs[j][0] if j is not None else ""))
    _plot_tone(axes[3], made, [r for _, r in refs], first)


def compare_png(path: Any, made: Sequence[tuple[str, Any]], refs: Sequence[tuple[str, Any]] = (),
                pairs: Sequence[int | None] | None = None, title: str | None = None, max_s: float = 8.0) -> dict:
    """Write the comparison picture; return {'file', 'rows', 'pairs'}."""
    import matplotlib

    matplotlib.use("Agg")
    import matplotlib.pyplot as plt

    made = [(n, _mono(x)) for n, x in made]
    refs = [(n, _mono(x)) for n, x in refs]
    if pairs is None:
        pairs = [_nearest(x, [r for _, r in refs]) for _, x in made]
    rows = max(1, len(made))
    with matplotlib.rc_context({"font.size": 7.5}):
        fig, axes = plt.subplots(rows, 4, figsize=(15, 2.25 * rows + (0.5 if title else 0.2)),
                                 gridspec_kw={"width_ratios": [1.25, 1.25, 1, 1]}, squeeze=False)
        for i, item in enumerate(made):
            _row(axes[i], item, refs, pairs[i], max_s, i == 0)
        axes[-1][0].set_xlabel("ms from onset")
        if title:
            fig.suptitle(title)
        fig.tight_layout()
        fig.savefig(str(path), dpi=90)
        plt.close(fig)
    return {"file": str(path), "rows": rows,
            "pairs": [refs[j][0] if j is not None else None for j in pairs]}
