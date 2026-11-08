"""How close a render is to its recording: the fit quality measure.

Every distance here is measured on 48 kHz float64 audio. Lower scores are
better. CLAP and describe are optional model checks that load the listen unit
only when a caller asks for them.
"""

from __future__ import annotations

import math
import os
import tempfile
from typing import Any, Callable

import numpy as np

SR = 48000
FRAMES = (256, 1024, 4096)
N_MEL = 64
WIDTH_W = 0.04
WIDTH_CAP_DB = 20
TAIL_CORR_W = 1.0
CEN = 1000.0 * 2.0 ** (np.arange(-15, 13) / 3.0)   # the 28 tone band centres

_LEAD = 240
_TINY = 1e-20


# ---------------------------------------------------------------- loading


def load(x: Any) -> np.ndarray:
    """A path as (n, 2) float64 at 48 kHz; an array as float64, shape kept."""
    if isinstance(x, (str, os.PathLike)):
        from hark.core import audio

        y = np.asarray(audio.load(str(x), sr=SR), dtype=np.float64)
        return np.column_stack([y, y]) if y.ndim == 1 else y
    return np.asarray(x, dtype=np.float64)


def mono(x: np.ndarray) -> np.ndarray:
    """The channel mean of a 2-D array, or the 1-D array unchanged."""
    x = np.asarray(x, dtype=np.float64)
    return x.mean(axis=1) if x.ndim == 2 else x


def _db(p: Any) -> Any:
    return 10.0 * np.log10(np.maximum(p, _TINY))


def _frame_power(m: np.ndarray, hop: int) -> np.ndarray:
    k = len(m) // hop
    if k == 0:
        return np.zeros(0)
    return np.mean(m[: k * hop].reshape(k, hop) ** 2, axis=1)


# ---------------------------------------------------------------- levels and onsets


def peak_rms_db(x: np.ndarray, win_s: float = 0.05) -> float:
    """The loudest 50 ms RMS (channel-mean power) in dB."""
    x = np.asarray(x, dtype=np.float64)
    p = np.mean(x ** 2, axis=1) if x.ndim == 2 else x ** 2
    w = max(1, int(round(win_s * SR)))
    if len(p) <= w:
        return float(_db(np.mean(p) if len(p) else 0.0))
    c = np.concatenate([[0.0], np.cumsum(p)])
    return float(_db(np.max((c[w:] - c[:-w]) / w)))


def onset_index(m: np.ndarray, db: float = -30, hop: int = 48) -> int:
    """Start sample of the first 1 ms frame within ``-db`` dB of the loudest."""
    e = _db(_frame_power(mono(m), hop))
    if len(e) < 2:
        return 0
    return int(np.argmax(e > e.max() + db)) * hop


def env_db(m: np.ndarray, ms: float = 5) -> np.ndarray:
    """Non-overlapping frame power envelope in dB."""
    return _db(_frame_power(mono(m), max(1, int(round(ms * SR / 1000)))))


def _crossing(e: np.ndarray, j: int, peak: int, level: float) -> float:
    """Frame time where e first reaches ``level`` in [j, peak], interpolated."""
    i = j + int(np.argmax(e[j: peak + 1] >= level))
    if i == j or e[i - 1] == e[i]:
        return float(i)
    a, b = e[i - 1], e[i]
    return float(i - 1) + (level - a) / (b - a)


def attack(m: np.ndarray) -> tuple[float, float]:
    """(attack_ms, sharpness_db_per_ms) of the first onset."""
    e = _db(_frame_power(mono(m), 6))
    if len(e) < 4:
        return 0.0, 1.0
    on = int(np.argmax(e > e.max() - 20))
    peak = on + int(np.argmax(e[on: on + 81]))
    pk = e[peak]
    j = peak
    while j > 0 and e[j - 1] > pk - 20:
        j -= 1
    t20 = _crossing(e, j, peak, pk - 20)
    t3 = _crossing(e, j, peak, pk - 3)
    rise = max((t3 - t20) * 0.125, 0.02)
    return 0.125 * (peak - on), 17.0 / rise


def t40(m: np.ndarray) -> float:
    """Ms from the loudest 5 ms frame until the first frame 40 dB under it."""
    e = env_db(m)
    if len(e) == 0:
        return 0.0
    i = int(np.argmax(e))
    below = np.nonzero(e[i:] < e[i] - 40)[0]
    k = int(below[0]) if len(below) else len(e) - 1 - i
    return 5.0 * k


# ---------------------------------------------------------------- spectra


def _hann(n: int) -> np.ndarray:
    return np.hanning(n + 2)[1:-1]


def _stft_power(m: np.ndarray, n_fft: int, hop: int) -> np.ndarray:
    k = max(1, int(math.ceil(len(m) / hop)))
    buf = np.zeros((k - 1) * hop + n_fft)
    seg = m[: len(buf) - n_fft // 2]
    buf[n_fft // 2: n_fft // 2 + len(seg)] = seg
    idx = np.arange(n_fft)[None, :] + hop * np.arange(k)[:, None]
    return np.abs(np.fft.rfft(buf[idx] * _hann(n_fft), axis=1)) ** 2


def _mel_bank(n_fft: int) -> np.ndarray:
    def mel(f):
        return 2595.0 * np.log10(1.0 + f / 700.0)

    pts = 700.0 * (10 ** (np.linspace(mel(30.0), mel(20000.0), N_MEL + 2) / 2595.0) - 1.0)
    f = np.arange(n_fft // 2 + 1) * SR / n_fft
    bank = np.zeros((N_MEL, len(f)))
    for i in range(N_MEL):
        lo, c, hi = pts[i], pts[i + 1], pts[i + 2]
        w = np.minimum((f - lo) / (c - lo), (hi - f) / (hi - c))
        w = np.clip(w, 0.0, None)
        if not np.any(w > 0):
            w[int(np.argmin(np.abs(f - c)))] = 1.0
        bank[i] = w
    return bank


_MEL_CACHE: dict[int, np.ndarray] = {}


def melspec(m: np.ndarray, n_fft: int) -> np.ndarray:
    """Log-mel power in dB, frames x 64."""
    bank = _MEL_CACHE.get(n_fft)
    if bank is None:
        bank = _MEL_CACHE.setdefault(n_fft, _mel_bank(n_fft))
    p = _stft_power(mono(m), n_fft, n_fft // 4)
    return _db(p @ bank.T + _TINY)


def _band_edges() -> np.ndarray:
    return np.column_stack([CEN / 2 ** (1 / 6), CEN * 2 ** (1 / 6)])


def bandgram(m: np.ndarray, nfft: int = 1024, hop: int = 512) -> np.ndarray:
    """Third-octave band power in dB per frame (lowest 3 bands merged)."""
    p = _stft_power(mono(m), nfft, hop)
    f = np.arange(nfft // 2 + 1) * SR / nfft
    edges = _band_edges()
    edges[2, 0] = 22.0
    out = np.zeros((p.shape[0], len(edges)))
    for b in range(2, len(edges)):
        sel = (f >= edges[b, 0]) & (f < edges[b, 1])
        out[:, b] = p[:, sel].sum(axis=1)
    return _db(out)


# ---------------------------------------------------------------- alignment


def _aligned(a: np.ndarray, b: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    ma, mb = mono(a), mono(b)
    on = max(0, onset_index(ma) - _LEAD)
    ma, mb = ma[on:], mb[on:]
    ma = ma * 10 ** (-peak_rms_db(ma) / 20)
    mb = mb * 10 ** (-peak_rms_db(mb) / 20)
    n = max(len(ma), len(mb))
    return np.pad(ma, (0, n - len(ma))), np.pad(mb, (0, n - len(mb)))


# ---------------------------------------------------------------- single metrics


def _mel_one(a: np.ndarray, b: np.ndarray, n_fft: int) -> float:
    ma, mb = melspec(a, n_fft), melspec(b, n_fft)
    top = max(ma.max(), mb.max())
    ma, mb = np.maximum(ma, top - 80), np.maximum(mb, top - 80)
    act = np.maximum(ma, mb) > top - 60
    return float(np.mean(np.abs(ma - mb)[act])) if act.any() else 0.0


def mel_distance(a: np.ndarray, b: np.ndarray, detail: bool = False) -> Any:
    """Mean multi-resolution log-mel distance (dB) of 2 aligned mono sounds."""
    d = {n: _mel_one(a, b, n) for n in FRAMES}
    total = float(np.mean(list(d.values())))
    return (total, d) if detail else total


def _active_envs(a: np.ndarray, b: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """The 5 ms dB envelopes on the frames within 60 dB of the joint top, floored there."""
    ea, eb = env_db(a), env_db(b)
    n = min(len(ea), len(eb))
    if n == 0:
        return np.zeros(0), np.zeros(0)
    ea, eb = ea[:n], eb[:n]
    top = max(ea.max(), eb.max())
    ea, eb = np.maximum(ea, top - 60), np.maximum(eb, top - 60)
    act = np.maximum(ea, eb) > top - 60
    return ea[act], eb[act]


def env_corr(a: np.ndarray, b: np.ndarray) -> float:
    """Pearson correlation of the 5 ms dB envelopes over active frames."""
    xa, xb = _active_envs(a, b)
    ok = len(xa) > 3 and xa.std() >= 1e-9 and xb.std() >= 1e-9
    return float(np.corrcoef(xa, xb)[0, 1]) if ok else 0.0


def excess(a: np.ndarray, b: np.ndarray) -> tuple[float, float]:
    """(excess_db, missing_db) of render b against original a."""
    ga, gb = bandgram(a), bandgram(b)
    n = min(len(ga), len(gb))
    ga, gb = ga[:n], gb[:n]
    top = max(ga.max(), gb.max())
    ga, gb = np.maximum(ga, top - 100), np.maximum(gb, top - 100)
    act = np.maximum(ga, gb) > top - 90
    if not act.any():
        return 0.0, 0.0
    ex = float(np.mean(np.maximum(0.0, gb - ga - 6)[act]))
    mi = float(np.mean(np.maximum(0.0, ga - gb - 6)[act]))
    return ex, mi


def profile(m: np.ndarray) -> np.ndarray:
    """The tone profile: 28 third-octave band levels in dB relative to their total, floored at -60."""
    m = mono(m)
    p = np.abs(np.fft.rfft(m)) ** 2 if len(m) else np.zeros(1)
    f = np.fft.rfftfreq(max(len(m), 1), 1.0 / SR)
    edges = _band_edges()
    bands = np.array([p[(f >= lo) & (f < hi)].sum() for lo, hi in edges]) + 1e-20
    return np.maximum(10 * np.log10(bands / bands.sum()), -60.0)


def _profile(m: np.ndarray) -> np.ndarray:
    return profile(m)


def tone_db(a: np.ndarray, b: np.ndarray) -> float:
    """RMS difference of the 2 third-octave balance profiles, dB."""
    return float(np.sqrt(np.mean((_profile(a) - _profile(b)) ** 2)))


def _null_stereo() -> dict:
    return {"side_db": (-40.0, -40.0), "width_db": 0.0, "tail_corr": (None, None), "tail_corr_err": 0.0}


def _band_ratio(sp: dict, end_frame: int) -> tuple[float, np.ndarray, np.ndarray]:
    pm = np.asarray(sp["PM"])[:, :end_frame].sum(axis=1) + 1e-30
    ps = np.asarray(sp["PS"])[:, :end_frame].sum(axis=1) + 1e-30
    side = max(-40.0, 10 * math.log10(ps.sum() / pm.sum()))
    return side, np.clip(10 * np.log10(ps / pm), -40, 10), 10 * np.log10(pm)


def _stereo_pair(a: np.ndarray, b: np.ndarray) -> tuple | None:
    """Both sounds as 2 channels from 2 ms before the onset, padded to one length; None when too short."""
    sa, sb = _two(a), _two(b)
    on = max(0, onset_index(sa.mean(axis=1)) - _LEAD)
    sa, sb = sa[on:], sb[on:]
    n = max(len(sa), len(sb))
    if n < 64:
        return None
    return np.pad(sa, ((0, n - len(sa)), (0, 0))), np.pad(sb, ((0, n - len(sb)), (0, 0)))


def _side_bands(space: Any, pair: tuple, end: int) -> tuple[list, list, np.ndarray]:
    """Side level per sound, side ratio per band per sound, and the original's band levels."""
    side, ratio, levels = [], [], []
    for s in pair:
        sp = space.side_powers(s[:end])
        sd, r, lv = _band_ratio(sp, len(np.asarray(sp["t"])))
        side.append(round(sd, 2))
        ratio.append(r)
        levels.append(lv)
    return side, ratio, levels[0]


def _width(ratio: list, level: np.ndarray) -> float:
    w = np.clip((level - level.max() + 40) / 40, 0, 1)
    return float(np.sum(w * np.minimum(np.abs(ratio[0] - ratio[1]), 30)) / max(w.sum(), 1e-9))


def _tail_corr(space: Any, sa: np.ndarray, sb: np.ndarray) -> tuple:
    span = space.tail_span(sa)
    if span is None:
        return None, None
    i, j = int(span[0]), int(span[1])
    return tuple(_round(space.corr(s[i:j, 0], s[i:j, 1]), 4) for s in (sa, sb))


def _stereo_measures(sa: np.ndarray, sb: np.ndarray) -> dict:
    from hark.fit.analysis import space

    end = max(int(space.active_span(sa)[2]), min(len(sa), 960))
    side, ratio, level = _side_bands(space, (sa, sb), end)
    tc = _tail_corr(space, sa, sb)
    err = abs(tc[0] - tc[1]) if None not in tc else 0.0
    return {"side_db": tuple(side), "width_db": round(_width(ratio, level), 3), "tail_corr": tc,
            "tail_corr_err": round(err, 4)}


def stereo(a: np.ndarray, b: np.ndarray) -> dict:
    """side_db, width_db, tail_corr and tail_corr_err of render b against a."""
    a, b = np.asarray(a, dtype=np.float64), np.asarray(b, dtype=np.float64)
    pair = _stereo_pair(a, b) if a.ndim >= 2 or b.ndim >= 2 else None
    return _null_stereo() if pair is None else _stereo_measures(*pair)


def _two(x: np.ndarray) -> np.ndarray:
    if x.ndim == 1:
        return np.column_stack([x, x])
    if x.shape[1] == 1:
        return np.column_stack([x[:, 0], x[:, 0]])
    return x[:, :2]


def _round(v: Any, nd: int) -> Any:
    return None if v is None else round(float(v), nd)


# ---------------------------------------------------------------- the metrics dict


def _mono_measures(original: np.ndarray, resynth: np.ndarray) -> dict:
    a, b = _aligned(original, resynth)
    mel, parts = mel_distance(a, b, detail=True)
    ex, mi = excess(a, b)
    q = {
        "mel_db": round(mel, 3),
        "mel_256": round(parts[256], 2), "mel_1024": round(parts[1024], 2), "mel_4096": round(parts[4096], 2),
        "env_corr": round(env_corr(a, b), 4),
    }
    q.update(_attack_measures(a, b))
    q.update({"excess_db": round(ex, 3), "missing_db": round(mi, 3), "t40_ms": (round(t40(a), 1), round(t40(b), 1))})
    return q


def _attack_measures(a: np.ndarray, b: np.ndarray) -> dict:
    at_a, sh_a = attack(a)
    at_b, sh_b = attack(b)
    return {"attack_ms": (round(at_a, 2), round(at_b, 2)), "sharp_db_ms": (round(sh_a, 2), round(sh_b, 2)),
            "onset_err": round(abs(math.log2(sh_b / sh_a)), 3)}


def quality(original: Any, resynth: Any, clap: bool = False, describe: bool = False) -> dict:
    """The metrics dict of a render against its recording."""
    a, b = load(original), load(resynth)
    q = _mono_measures(a, b)
    q["tone_db"] = round(tone_db(a, b), 3)
    q["level_db"] = round(peak_rms_db(b) - peak_rms_db(a), 2)
    q["len_s"] = (round(len(a) / SR, 4), round(len(b) / SR, 4))
    q.update(stereo(a, b))
    if clap:
        q["clap_cos"] = round(clap_cos(a, b), 4)
    if describe:
        q["describe"] = describe_pair(a, b)
    return q


def score(q: dict) -> float:
    """One number, lower is better."""
    s = (q["mel_db"] + 0.3 * q["tone_db"] + 8.0 * (1.0 - q["env_corr"]) + min(q["onset_err"], 2.0))
    s += 1.5 * q.get("excess_db", 0.0) + q.get("missing_db", 0.0)
    s += WIDTH_W * min(q.get("width_db", 0.0), WIDTH_CAP_DB) + TAIL_CORR_W * q.get("tail_corr_err", 0.0)
    if q.get("clap_cos") is not None:
        s += 20.0 * (1.0 - q["clap_cos"])
    return float(s)


def floor(render_fn: Callable[[int], np.ndarray], seeds: tuple = (11, 12), clap: bool = False) -> dict:
    """The distance between 2 takes of one model."""
    return quality(render_fn(seeds[0]), render_fn(seeds[1]), clap=clap)


# ---------------------------------------------------------------- optional model checks


def clap_cos(a: np.ndarray, b: np.ndarray) -> float:
    """Cosine of the CLAP audio embeddings of 2 sounds (needs the ML extras)."""
    from hark.models import require_ml

    require_ml("CLAP")
    from hark.listen import vocabulary

    e = vocabulary.embed_audio([mono(a).astype(np.float32), mono(b).astype(np.float32)])
    return float(np.dot(e[0], e[1]))


def _describe_one(path: str) -> dict:
    from hark.listen.describe import describe

    r = describe(path)
    tags = r.get("top_tags") or []
    labels = r.get("audioset") or []
    top = tags[0] if tags else {}
    lab = labels[0] if labels else {}
    return {
        "tag": top.get("term"), "z": _round(top.get("z"), 2),
        "audioset": lab.get("label"), "p": _round(lab.get("p"), 2),
        "tags3": [t.get("term") for t in tags[:3]],
    }


def describe_pair(a: np.ndarray, b: np.ndarray) -> dict:
    """The listen unit's describe of both sounds: top tag and top AudioSet label."""
    from hark.models import require_ml

    require_ml("describe")
    from hark.core import audio

    out = {}
    with tempfile.TemporaryDirectory() as tmp:
        for key, x in (("original", a), ("resynth", b)):
            path = os.path.join(tmp, key + ".wav")
            audio.write_wav(path, np.asarray(x, dtype=np.float32), SR, subtype="FLOAT")
            out[key] = _describe_one(path)
    return out


# ---------------------------------------------------------------- text and picture


def _stereo_text(q: dict) -> str:
    sd = q.get("side_db") or (-40, -40)
    if max(sd) <= -40:
        return ""
    s = f" | side {sd[0]:.1f}/{sd[1]:.1f} dB width err {q.get('width_db', 0):.1f} dB"
    tc = q.get("tail_corr") or (None, None)
    if None not in tc:
        s += f" tail r {tc[0]:.2f}/{tc[1]:.2f}"
    return s


def text(q: dict, fl: dict | None = None) -> str:
    """One readable line of a quality dict."""
    mel = f"mel {q['mel_db']:.2f} dB"
    if fl and fl.get("mel_db") is not None:
        mel += f" (floor {fl['mel_db']:.2f})"
    sh = q.get("sharp_db_ms", (0, 0))
    t4 = q.get("t40_ms", (0, 0))
    s = (f"{mel} | env r {q['env_corr']:.3f} | tone {q['tone_db']:.2f} dB | onset {q['onset_err']:.2f} oct"
         f" ({sh[0]:.0f}/{sh[1]:.0f} dB/ms) | excess {q.get('excess_db', 0):.2f} missing {q.get('missing_db', 0):.2f}"
         f" | t40 {t4[0]:.0f}/{t4[1]:.0f} ms | level {q.get('level_db', 0):+.2f} dB")
    s += _stereo_text(q)
    if q.get("clap_cos") is not None:
        s += f" | CLAP {q['clap_cos']:.3f}"
    d = q.get("describe")
    if d:
        s += f" | tags {d['original'].get('tag')} / {d['resynth'].get('tag')}"
    return s


def _spec_db(m: np.ndarray) -> np.ndarray:
    return _db(_stft_power(m, 1024, 128)).T


def _columns(original: Any, resynth: Any, extra: tuple | None) -> list:
    """(name, mono signal) per column, zero-padded to one length."""
    cols = [("original", mono(load(original))), ("resynthesis", mono(load(resynth)))]
    if extra is not None:
        cols.append((extra[0], mono(load(extra[1]))))
    n = max(len(c[1]) for c in cols)
    return [(name, np.pad(m, (0, n - len(m)))) for name, m in cols]


def _spec_panels(fig: Any, grid: Any, cols: list) -> None:
    specs = [_spec_db(m) for _, m in cols]
    top = max(s.max() for s in specs)
    f = np.maximum(np.arange(513) * SR / 1024, 1)
    t = np.arange(specs[0].shape[1]) * 128 / SR
    for i, ((name, _), s) in enumerate(zip(cols, specs)):
        ax = fig.add_subplot(grid[0, i])
        ax.pcolormesh(t, f, s, vmin=top - 90, vmax=top, shading="auto")
        ax.set_yscale("log")
        ax.set_ylim(40, 22000)
        ax.set_title(name)


def _heading(title: str | None, q: dict | None) -> str:
    head = title or ""
    if q is not None:
        head = (head + "\n" if head else "") + text(q)
    return head


def pair_png(path: str, original: Any, resynth: Any, title: str | None = None, q: dict | None = None,
             extra: tuple | None = None) -> str:
    """Write the side-by-side picture of original, render and an optional third sound."""
    import matplotlib

    matplotlib.use("Agg")
    import matplotlib.pyplot as plt

    cols = _columns(original, resynth, extra)
    fig = plt.figure(figsize=(4.4 * len(cols), 6.2), dpi=110)
    grid = fig.add_gridspec(2, len(cols))
    _spec_panels(fig, grid, cols)
    _env_panel(fig.add_subplot(grid[1, : max(1, len(cols) - 1)]), cols)
    _tone_panel(fig.add_subplot(grid[1, len(cols) - 1]), cols)
    head = _heading(title, q)
    if head:
        fig.suptitle(head, fontsize=7)
    os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
    fig.savefig(path, format="png")
    plt.close(fig)
    return path


def _env_panel(ax: Any, cols: list) -> None:
    envs = [env_db(m) for _, m in cols]
    top = max(e.max() for e in envs if len(e)) if any(len(e) for e in envs) else 0.0
    for (name, _), e in zip(cols, envs):
        ax.plot(np.arange(len(e)) * 0.005, e, label=name, lw=0.8)
    ax.set_ylim(max(top - 80, -120), top + 3)
    ax.legend(fontsize=6)


def _tone_panel(ax: Any, cols: list) -> None:
    for name, m in cols:
        ax.semilogx(CEN, _profile(m), label=name, lw=0.8)
    ax.legend(fontsize=6)
