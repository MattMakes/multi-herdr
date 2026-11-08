"""A single PNG that sets every generated sound next to the reference closest to it.

Each row holds 4 panels: the generated spectrogram, the spectrogram of its
matched reference, the 2 level envelopes, and the third-octave balance against
all references. matplotlib is imported only when a picture is drawn, on the Agg
backend.
"""

from __future__ import annotations

from collections.abc import Sequence
from typing import Any

import numpy as np
from scipy import signal

from hark.forge.engine import tone

SR = 48000
_OURS_COLOUR = "tab:blue"
_THEIRS_COLOUR = "tab:red"
_FLOOR_DB = -80.0
_PANELS = 4


def _as_mono(sound: Any) -> np.ndarray:
    return tone.as_mono(sound)


def _sized(samples: np.ndarray, want: int) -> np.ndarray:
    if len(samples) < want:
        return np.concatenate([samples, np.zeros(want - len(samples))])
    return samples[:want]


def _relative_db(levels: np.ndarray) -> np.ndarray:
    return np.maximum(levels - levels.max(), _FLOOR_DB)


def _ms(seconds: float) -> float:
    return seconds * 1000


def onset_align(x: Any, db: float = -30.0, pre_ms: float = 5.0) -> np.ndarray:
    """Mono copy that starts pre_ms before the first sample that comes within db of the peak."""
    mono = _as_mono(x)
    if len(mono) == 0:
        return mono
    mag = np.abs(mono)
    first_loud = int(np.argmax(mag >= mag.max() * 10 ** (db / 20)))
    lead = int(SR * pre_ms / 1000)
    return mono[max(0, first_loud - lead):]


def _window_for(span_s: float) -> int:
    if span_s < 0.6:
        return 512
    if span_s < 2.5:
        return 1024
    return 2048


def spec_db(x: Any, span_s: float) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    """Short-time spectrum in dB over span_s. The loudest cell is 0 dB and the floor is -80 dB."""
    mono = _sized(_as_mono(x), max(256, int(span_s * SR)))
    win = min(_window_for(span_s), len(mono))
    framing = {"nperseg": win, "noverlap": win - win // 8, "boundary": None, "padded": False}
    freqs, times, cells = signal.stft(mono, SR, **framing)
    level = 20 * np.log10(np.abs(cells) + 1e-12)
    return _ms(times), freqs, _relative_db(level)


def envelope_db(x: Any, span_s: float, hop_ms: float = 2.0) -> tuple[np.ndarray, np.ndarray]:
    """Level per hop as RMS in dB under the peak, with a floor at -80 dB."""
    step = max(1, int(SR * hop_ms / 1000))
    mono = _sized(_as_mono(x), max(1, int(span_s * SR)))
    count = max(1, len(mono) // step)
    blocks = _sized(mono, count * step).reshape(count, step)
    power_db = 10 * np.log10(np.mean(blocks ** 2, axis=1) + 1e-20)
    centres = (np.arange(count) + 0.5) * hop_ms
    return centres, _relative_db(power_db)


def _balance_gap(a: np.ndarray, b: np.ndarray) -> float:
    return float(np.sqrt(np.mean((a - b) ** 2)))


def _closest_ref(ours: np.ndarray, candidates: Sequence[np.ndarray]) -> int | None:
    # This module is under the search layer, so the match uses spectral balance only.
    if not candidates:
        return None
    own = tone.profile(ours)
    gaps = [_balance_gap(own, tone.profile(c)) for c in candidates]
    return int(np.argmin(gaps))


def _draw_spectrogram(panel: Any, sound: np.ndarray, span: float, heading: str) -> None:
    ms, freqs, level = spec_db(sound, span)
    panel.pcolormesh(ms, np.maximum(freqs, 1.0), level, vmin=-80, vmax=0, shading="auto", cmap="magma")
    panel.set_yscale("log")
    panel.set_ylim(40, 24000)
    panel.set_xlim(0, _ms(span))
    panel.set_title(heading)


def _draw_envelopes(panel: Any, ours: np.ndarray, match: np.ndarray | None, span: float, labels: tuple) -> None:
    step_ms = max(2.0, _ms(span) / 300)
    panel.plot(*envelope_db(ours, span, step_ms), color=_OURS_COLOUR, label=labels[0])
    if match is not None:
        panel.plot(*envelope_db(match, span, step_ms), color=_THEIRS_COLOUR, label=labels[1])
    panel.set_ylim(-70, 3)
    panel.set_xlim(0, _ms(span))
    panel.set_title("envelope (dB re peak)")
    panel.legend(loc="upper right")


def _draw_balance(panel: Any, ours: np.ndarray, pool: Sequence[np.ndarray], with_key: bool) -> None:
    bands = np.asarray(tone.CEN)
    own = tone.profile(ours)
    heading = "1/3-octave balance"
    if pool:
        table = np.array([tone.profile(c) for c in pool])
        middle = np.median(table, axis=0)
        panel.fill_between(bands, table.min(axis=0), table.max(axis=0), color=_THEIRS_COLOUR, alpha=0.2,
                           label="refs min-max")
        panel.plot(bands, middle, "--", color=_THEIRS_COLOUR, label="refs median")
        heading = f"{heading}: {_balance_gap(own, middle):.1f} dB from the refs"
    panel.plot(bands, own, color=_OURS_COLOUR, label="made")
    panel.set_xscale("log")
    panel.set_xlim(25, 20000)
    panel.set_ylim(-60, 0)
    panel.set_title(heading)
    if with_key:
        panel.legend(loc="lower left")


def _draw_row(panels: Any, entry: tuple, named_refs: list, match_at: int | None, limit_s: float,
              top: bool) -> None:
    label, sound = entry
    ours = onset_align(sound)
    match = None if match_at is None else onset_align(named_refs[match_at][1])
    match_label = "" if match_at is None else named_refs[match_at][0]
    longest = max(len(ours), 0 if match is None else len(match))
    span = min(limit_s, longest / SR) or 0.01
    _draw_spectrogram(panels[0], ours, span, f"made: {label}")
    if match is None:
        panels[1].axis("off")
    else:
        _draw_spectrogram(panels[1], match, span, f"nearest ref: {match_label}")
    _draw_envelopes(panels[2], ours, match, span, (label, match_label))
    _draw_balance(panels[3], ours, [s for _, s in named_refs], top)


def _render(pyplot: Any, target: Any, ours: list, theirs: list, matches: Sequence[int | None],
            heading: str | None, limit_s: float) -> int:
    count = max(1, len(ours))
    layout = {"width_ratios": [1.25, 1.25, 1, 1]}
    size_in = (15, 2.25 * count + (0.5 if heading else 0.2))
    fig, grid = pyplot.subplots(count, _PANELS, figsize=size_in, gridspec_kw=layout, squeeze=False)
    for at, entry in enumerate(ours):
        _draw_row(grid[at], entry, theirs, matches[at], limit_s, at == 0)
    grid[-1][0].set_xlabel("ms from onset")
    if heading:
        fig.suptitle(heading)
    fig.tight_layout()
    fig.savefig(str(target), dpi=90)
    pyplot.close(fig)
    return count


def compare_png(path: Any, made: Sequence[tuple[str, Any]], refs: Sequence[tuple[str, Any]] = (),
                pairs: Sequence[int | None] | None = None, title: str | None = None, max_s: float = 8.0) -> dict:
    """Draw the comparison PNG and return {'file', 'rows', 'pairs'}."""
    import matplotlib

    matplotlib.use("Agg")
    import matplotlib.pyplot as plt

    ours = [(label, _as_mono(sound)) for label, sound in made]
    theirs = [(label, _as_mono(sound)) for label, sound in refs]
    if pairs is None:
        pool = [s for _, s in theirs]
        pairs = [_closest_ref(s, pool) for _, s in ours]
    with matplotlib.rc_context({"font.size": 7.5}):
        count = _render(plt, path, ours, theirs, pairs, title, max_s)
    named = [None if at is None else theirs[at][0] for at in pairs]
    return {"file": str(path), "rows": count, "pairs": named}
