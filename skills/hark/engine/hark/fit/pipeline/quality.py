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

_PRE_ROLL = 240
_EPS = 1e-20


# ---------------------------------------------------------------- loading


def load(x: Any) -> np.ndarray:
    """A path as (n, 2) float64 at 48 kHz; an array as float64, shape kept."""
    if isinstance(x, (str, os.PathLike)):
        from hark.core import audio

        yv = np.asarray(audio.load(str(x), sr=SR), dtype=np.float64)
        return np.column_stack([yv, yv]) if yv.ndim == 1 else yv
    return np.asarray(x, dtype=np.float64)


def mono(x: np.ndarray) -> np.ndarray:
    """The channel mean of a 2-D array, or the 1-D array unchanged."""
    x = np.asarray(x, dtype=np.float64)
    return x.mean(axis=1) if x.ndim == 2 else x


def _to_db(pv: Any) -> Any:
    return 10.0 * np.log10(np.maximum(pv, _EPS))


def _power_frames(mv: np.ndarray, step_n: int) -> np.ndarray:
    kk = len(mv) // step_n
    if kk == 0:
        return np.zeros(0)
    return np.mean(mv[: kk * step_n].reshape(kk, step_n) ** 2, axis=1)


# ---------------------------------------------------------------- levels and onsets


def peak_rms_db(x: np.ndarray, win_s: float = 0.05) -> float:
    """The loudest 50 ms RMS (channel-mean power) in dB."""
    x = np.asarray(x, dtype=np.float64)
    pv = np.mean(x ** 2, axis=1) if x.ndim == 2 else x ** 2
    wv = max(1, int(round(win_s * SR)))
    if len(pv) <= wv:
        return float(_to_db(np.mean(pv) if len(pv) else 0.0))
    cv = np.concatenate([[0.0], np.cumsum(pv)])
    return float(_to_db(np.max((cv[wv:] - cv[:-wv]) / wv)))


def onset_index(m: np.ndarray, db: float = -30, hop: int = 48) -> int:
    """Start sample of the first 1 ms frame within ``-db`` dB of the loudest."""
    ev = _to_db(_power_frames(mono(m), hop))
    if len(ev) < 2:
        return 0
    return int(np.argmax(ev > ev.max() + db)) * hop


def env_db(m: np.ndarray, ms: float = 5) -> np.ndarray:
    """Non-overlapping frame power envelope in dB."""
    return _to_db(_power_frames(mono(m), max(1, int(round(ms * SR / 1000)))))


def _first_cross(ev: np.ndarray, jj: int, pk_i: int, lvl: float) -> float:
    """Frame time where e first reaches ``level`` in [j, peak], interpolated."""
    ii = jj + int(np.argmax(ev[jj: pk_i + 1] >= lvl))
    if ii == jj or ev[ii - 1] == ev[ii]:
        return float(ii)
    ra, rb = ev[ii - 1], ev[ii]
    return float(ii - 1) + (lvl - ra) / (rb - ra)


def attack(m: np.ndarray) -> tuple[float, float]:
    """(attack_ms, sharpness_db_per_ms) of the first onset."""
    ev = _to_db(_power_frames(mono(m), 6))
    if len(ev) < 4:
        return 0.0, 1.0
    start_i = int(np.argmax(ev > ev.max() - 20))
    pk_i = start_i + int(np.argmax(ev[start_i: start_i + 81]))
    pk_v = ev[pk_i]
    jj = pk_i
    while jj > 0 and ev[jj - 1] > pk_v - 20:
        jj -= 1
    tm20 = _first_cross(ev, jj, pk_i, pk_v - 20)
    tm3 = _first_cross(ev, jj, pk_i, pk_v - 3)
    rise_v = max((tm3 - tm20) * 0.125, 0.02)
    return 0.125 * (pk_i - start_i), 17.0 / rise_v


def t40(m: np.ndarray) -> float:
    """Ms from the loudest 5 ms frame until the first frame 40 dB under it."""
    ev = env_db(m)
    if len(ev) == 0:
        return 0.0
    ii = int(np.argmax(ev))
    under = np.nonzero(ev[ii:] < ev[ii] - 40)[0]
    kk = int(under[0]) if len(under) else len(ev) - 1 - ii
    return 5.0 * kk


# ---------------------------------------------------------------- spectra


def _hann_win(cnt: int) -> np.ndarray:
    return np.hanning(cnt + 2)[1:-1]


def _power_stft(mv: np.ndarray, fft_n: int, step_n: int) -> np.ndarray:
    kk = max(1, int(math.ceil(len(mv) / step_n)))
    bufv = np.zeros((kk - 1) * step_n + fft_n)
    chunk = mv[: len(bufv) - fft_n // 2]
    bufv[fft_n // 2: fft_n // 2 + len(chunk)] = chunk
    sel_i = np.arange(fft_n)[None, :] + step_n * np.arange(kk)[:, None]
    return np.abs(np.fft.rfft(bufv[sel_i] * _hann_win(fft_n), axis=1)) ** 2


def _mel_filters(fft_n: int) -> np.ndarray:
    def to_mel(fq):
        return 2595.0 * np.log10(1.0 + fq / 700.0)

    points = 700.0 * (10 ** (np.linspace(to_mel(30.0), to_mel(20000.0), N_MEL + 2) / 2595.0) - 1.0)
    fq = np.arange(fft_n // 2 + 1) * SR / fft_n
    bank = np.zeros((N_MEL, len(fq)))
    for ii in range(N_MEL):
        low_i, cv, high_i = points[ii], points[ii + 1], points[ii + 2]
        wv = np.minimum((fq - low_i) / (cv - low_i), (high_i - fq) / (high_i - cv))
        wv = np.clip(wv, 0.0, None)
        if not np.any(wv > 0):
            wv[int(np.argmin(np.abs(fq - cv)))] = 1.0
        bank[ii] = wv
    return bank


_MEL_MEMO: dict[int, np.ndarray] = {}


def melspec(m: np.ndarray, n_fft: int) -> np.ndarray:
    """Log-mel power in dB, frames x 64."""
    bank = _MEL_MEMO.get(n_fft)
    if bank is None:
        bank = _MEL_MEMO.setdefault(n_fft, _mel_filters(n_fft))
    pv = _power_stft(mono(m), n_fft, n_fft // 4)
    return _to_db(pv @ bank.T + _EPS)


def _edges_of_bands() -> np.ndarray:
    return np.column_stack([CEN / 2 ** (1 / 6), CEN * 2 ** (1 / 6)])


def bandgram(m: np.ndarray, nfft: int = 1024, hop: int = 512) -> np.ndarray:
    """Third-octave band power in dB per frame (lowest 3 bands merged)."""
    pv = _power_stft(mono(m), nfft, hop)
    fq = np.arange(nfft // 2 + 1) * SR / nfft
    edg = _edges_of_bands()
    edg[2, 0] = 22.0
    acc = np.zeros((pv.shape[0], len(edg)))
    for rb in range(2, len(edg)):
        pick = (fq >= edg[rb, 0]) & (fq < edg[rb, 1])
        acc[:, rb] = pv[:, pick].sum(axis=1)
    return _to_db(acc)


# ---------------------------------------------------------------- alignment


def _lined_up(ra: np.ndarray, rb: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    mel_a, mel_b = mono(ra), mono(rb)
    start_i = max(0, onset_index(mel_a) - _PRE_ROLL)
    mel_a, mel_b = mel_a[start_i:], mel_b[start_i:]
    mel_a = mel_a * 10 ** (-peak_rms_db(mel_a) / 20)
    mel_b = mel_b * 10 ** (-peak_rms_db(mel_b) / 20)
    cnt = max(len(mel_a), len(mel_b))
    return np.pad(mel_a, (0, cnt - len(mel_a))), np.pad(mel_b, (0, cnt - len(mel_b)))


# ---------------------------------------------------------------- single metrics


def _mel_single(ra: np.ndarray, rb: np.ndarray, fft_n: int) -> float:
    mel_a, mel_b = melspec(ra, fft_n), melspec(rb, fft_n)
    upper = max(mel_a.max(), mel_b.max())
    mel_a, mel_b = np.maximum(mel_a, upper - 80), np.maximum(mel_b, upper - 80)
    live = np.maximum(mel_a, mel_b) > upper - 60
    return float(np.mean(np.abs(mel_a - mel_b)[live])) if live.any() else 0.0


def mel_distance(a: np.ndarray, b: np.ndarray, detail: bool = False) -> Any:
    """Mean multi-resolution log-mel distance (dB) of 2 aligned mono sounds."""
    dv = {cnt: _mel_single(a, b, cnt) for cnt in FRAMES}
    tot = float(np.mean(list(dv.values())))
    return (tot, dv) if detail else tot


def _envs_when_on(ra: np.ndarray, rb: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """The 5 ms dB envelopes on the frames within 60 dB of the joint top, floored there."""
    env_a, env_b = env_db(ra), env_db(rb)
    cnt = min(len(env_a), len(env_b))
    if cnt == 0:
        return np.zeros(0), np.zeros(0)
    env_a, env_b = env_a[:cnt], env_b[:cnt]
    upper = max(env_a.max(), env_b.max())
    env_a, env_b = np.maximum(env_a, upper - 60), np.maximum(env_b, upper - 60)
    live = np.maximum(env_a, env_b) > upper - 60
    return env_a[live], env_b[live]


def env_corr(a: np.ndarray, b: np.ndarray) -> float:
    """Pearson correlation of the 5 ms dB envelopes over active frames."""
    sig_a, sig_b = _envs_when_on(a, b)
    good = len(sig_a) > 3 and sig_a.std() >= 1e-9 and sig_b.std() >= 1e-9
    return float(np.corrcoef(sig_a, sig_b)[0, 1]) if good else 0.0


def excess(a: np.ndarray, b: np.ndarray) -> tuple[float, float]:
    """(excess_db, missing_db) of render b against original a."""
    gain_a, gain_b = bandgram(a), bandgram(b)
    cnt = min(len(gain_a), len(gain_b))
    gain_a, gain_b = gain_a[:cnt], gain_b[:cnt]
    upper = max(gain_a.max(), gain_b.max())
    gain_a, gain_b = np.maximum(gain_a, upper - 100), np.maximum(gain_b, upper - 100)
    live = np.maximum(gain_a, gain_b) > upper - 90
    if not live.any():
        return 0.0, 0.0
    exv = float(np.mean(np.maximum(0.0, gain_b - gain_a - 6)[live]))
    mi_v = float(np.mean(np.maximum(0.0, gain_a - gain_b - 6)[live]))
    return exv, mi_v


def profile(m: np.ndarray) -> np.ndarray:
    """The tone profile: 28 third-octave band levels in dB relative to their total, floored at -60."""
    m = mono(m)
    pv = np.abs(np.fft.rfft(m)) ** 2 if len(m) else np.zeros(1)
    fq = np.fft.rfftfreq(max(len(m), 1), 1.0 / SR)
    edg = _edges_of_bands()
    band_l = np.array([pv[(fq >= low_i) & (fq < high_i)].sum() for low_i, high_i in edg]) + 1e-20
    return np.maximum(10 * np.log10(band_l / band_l.sum()), -60.0)


def _level_profile(mv: np.ndarray) -> np.ndarray:
    return profile(mv)


def tone_db(a: np.ndarray, b: np.ndarray) -> float:
    """RMS difference of the 2 third-octave balance profiles, dB."""
    return float(np.sqrt(np.mean((_level_profile(a) - _level_profile(b)) ** 2)))


def _empty_stereo() -> dict:
    return {"side_db": (-40.0, -40.0), "width_db": 0.0, "tail_corr": (None, None), "tail_corr_err": 0.0}


def _ratio_per_band(spv: dict, last_frame: int) -> tuple[float, np.ndarray, np.ndarray]:
    pmv = np.asarray(spv["PM"])[:, :last_frame].sum(axis=1) + 1e-30
    psv = np.asarray(spv["PS"])[:, :last_frame].sum(axis=1) + 1e-30
    side_v = max(-40.0, 10 * math.log10(psv.sum() / pmv.sum()))
    return side_v, np.clip(10 * np.log10(psv / pmv), -40, 10), 10 * np.log10(pmv)


def _pair_stereo(ra: np.ndarray, rb: np.ndarray) -> tuple | None:
    """Both sounds as 2 channels from 2 ms before the onset, padded to one length; None when too short."""
    st_a, st_b = _pair_of(ra), _pair_of(rb)
    start_i = max(0, onset_index(st_a.mean(axis=1)) - _PRE_ROLL)
    st_a, st_b = st_a[start_i:], st_b[start_i:]
    cnt = max(len(st_a), len(st_b))
    if cnt < 64:
        return None
    return np.pad(st_a, ((0, cnt - len(st_a)), (0, 0))), np.pad(st_b, ((0, cnt - len(st_b)), (0, 0)))


def _bands_of_side(space: Any, pair_v: tuple, stop_i: int) -> tuple[list, list, np.ndarray]:
    """Side level per sound, side ratio per band per sound, and the original's band levels."""
    side_v, rat, lvls = [], [], []
    for sv in pair_v:
        spv = space.side_powers(sv[:stop_i])
        sdv, rv, lvv = _ratio_per_band(spv, len(np.asarray(spv["t"])))
        side_v.append(round(sdv, 2))
        rat.append(rv)
        lvls.append(lvv)
    return side_v, rat, lvls[0]


def _width_db_of(rat: list, lvl: np.ndarray) -> float:
    wv = np.clip((lvl - lvl.max() + 40) / 40, 0, 1)
    return float(np.sum(wv * np.minimum(np.abs(rat[0] - rat[1]), 30)) / max(wv.sum(), 1e-9))


def _corr_of_tail(space: Any, st_a: np.ndarray, st_b: np.ndarray) -> tuple:
    spn = space.tail_span(st_a)
    if spn is None:
        return None, None
    ii, jj = int(spn[0]), int(spn[1])
    return tuple(_rounded(space.corr(sv[ii:jj, 0], sv[ii:jj, 1]), 4) for sv in (st_a, st_b))


def _stereo_stats(st_a: np.ndarray, st_b: np.ndarray) -> dict:
    from hark.fit.analysis import space

    stop_i = max(int(space.active_span(st_a)[2]), min(len(st_a), 960))
    side_v, rat, lvl = _bands_of_side(space, (st_a, st_b), stop_i)
    tcorr = _corr_of_tail(space, st_a, st_b)
    errv = abs(tcorr[0] - tcorr[1]) if None not in tcorr else 0.0
    return {"side_db": tuple(side_v), "width_db": round(_width_db_of(rat, lvl), 3), "tail_corr": tcorr,
            "tail_corr_err": round(errv, 4)}


def stereo(a: np.ndarray, b: np.ndarray) -> dict:
    """side_db, width_db, tail_corr and tail_corr_err of render b against a."""
    a, b = np.asarray(a, dtype=np.float64), np.asarray(b, dtype=np.float64)
    pair_v = _pair_stereo(a, b) if a.ndim >= 2 or b.ndim >= 2 else None
    return _empty_stereo() if pair_v is None else _stereo_stats(*pair_v)


def _pair_of(x: np.ndarray) -> np.ndarray:
    if x.ndim == 1:
        return np.column_stack([x, x])
    if x.shape[1] == 1:
        return np.column_stack([x[:, 0], x[:, 0]])
    return x[:, :2]


def _rounded(vv: Any, ndv: int) -> Any:
    return None if vv is None else round(float(vv), ndv)


# ---------------------------------------------------------------- the metrics dict


def _mono_stats(original: np.ndarray, resynth: np.ndarray) -> dict:
    ra, rb = _lined_up(original, resynth)
    to_mel, pieces = mel_distance(ra, rb, detail=True)
    exv, mi_v = excess(ra, rb)
    res = {
        "mel_db": round(to_mel, 3),
        "mel_256": round(pieces[256], 2), "mel_1024": round(pieces[1024], 2), "mel_4096": round(pieces[4096], 2),
        "env_corr": round(env_corr(ra, rb), 4),
    }
    res.update(_onset_stats(ra, rb))
    res.update({"excess_db": round(exv, 3), "missing_db": round(mi_v, 3), "t40_ms": (round(t40(ra), 1), round(t40(rb), 1))})
    return res


def _onset_stats(ra: np.ndarray, rb: np.ndarray) -> dict:
    atk_a, shp_a = attack(ra)
    atk_b, shp_b = attack(rb)
    return {"attack_ms": (round(atk_a, 2), round(atk_b, 2)), "sharp_db_ms": (round(shp_a, 2), round(shp_b, 2)),
            "onset_err": round(abs(math.log2(shp_b / shp_a)), 3)}


def quality(original: Any, resynth: Any, clap: bool = False, describe: bool = False) -> dict:
    """The metrics dict of a render against its recording."""
    ra, rb = load(original), load(resynth)
    res = _mono_stats(ra, rb)
    res["tone_db"] = round(tone_db(ra, rb), 3)
    res["level_db"] = round(peak_rms_db(rb) - peak_rms_db(ra), 2)
    res["len_s"] = (round(len(ra) / SR, 4), round(len(rb) / SR, 4))
    res.update(stereo(ra, rb))
    if clap:
        res["clap_cos"] = round(clap_cos(ra, rb), 4)
    if describe:
        res["describe"] = describe_pair(ra, rb)
    return res


def score(q: dict) -> float:
    """One number, lower is better."""
    sv = (q["mel_db"] + 0.3 * q["tone_db"] + 8.0 * (1.0 - q["env_corr"]) + min(q["onset_err"], 2.0))
    sv += 1.5 * q.get("excess_db", 0.0) + q.get("missing_db", 0.0)
    sv += WIDTH_W * min(q.get("width_db", 0.0), WIDTH_CAP_DB) + TAIL_CORR_W * q.get("tail_corr_err", 0.0)
    if q.get("clap_cos") is not None:
        sv += 20.0 * (1.0 - q["clap_cos"])
    return float(sv)


def floor(render_fn: Callable[[int], np.ndarray], seeds: tuple = (11, 12), clap: bool = False) -> dict:
    """The distance between 2 takes of one model."""
    return quality(render_fn(seeds[0]), render_fn(seeds[1]), clap=clap)


# ---------------------------------------------------------------- optional model checks


def clap_cos(a: np.ndarray, b: np.ndarray) -> float:
    """Cosine of the CLAP audio embeddings of 2 sounds (needs the ML extras)."""
    from hark.models import require_ml

    require_ml("CLAP")
    from hark.listen import vocabulary

    ev = vocabulary.embed_audio([mono(a).astype(np.float32), mono(b).astype(np.float32)])
    return float(np.dot(ev[0], ev[1]))


def _words_for_one(path: str) -> dict:
    from hark.listen.describe import describe

    rv = describe(path)
    tag_l = rv.get("top_tags") or []
    lbls = rv.get("audioset") or []
    upper = tag_l[0] if tag_l else {}
    lbl = lbls[0] if lbls else {}
    return {
        "tag": upper.get("term"), "z": _rounded(upper.get("z"), 2),
        "audioset": lbl.get("label"), "p": _rounded(lbl.get("p"), 2),
        "tags3": [tv.get("term") for tv in tag_l[:3]],
    }


def describe_pair(a: np.ndarray, b: np.ndarray) -> dict:
    """The listen unit's describe of both sounds: top tag and top AudioSet label."""
    from hark.models import require_ml

    require_ml("describe")
    from hark.core import audio

    acc = {}
    with tempfile.TemporaryDirectory() as tmpd:
        for kname, x in (("original", a), ("resynth", b)):
            path = os.path.join(tmpd, kname + ".wav")
            audio.write_wav(path, np.asarray(x, dtype=np.float32), SR, subtype="FLOAT")
            acc[kname] = _words_for_one(path)
    return acc


# ---------------------------------------------------------------- text and picture


def _stereo_words(res: dict) -> str:
    sdv = res.get("side_db") or (-40, -40)
    if max(sdv) <= -40:
        return ""
    sv = f" | side {sdv[0]:.1f}/{sdv[1]:.1f} dB width err {res.get('width_db', 0):.1f} dB"
    tcorr = res.get("tail_corr") or (None, None)
    if None not in tcorr:
        sv += f" tail r {tcorr[0]:.2f}/{tcorr[1]:.2f}"
    return sv


def text(q: dict, fl: dict | None = None) -> str:
    """One readable line of a quality dict."""
    to_mel = f"mel {q['mel_db']:.2f} dB"
    if fl and fl.get("mel_db") is not None:
        to_mel += f" (floor {fl['mel_db']:.2f})"
    shp = q.get("sharp_db_ms", (0, 0))
    tm4 = q.get("t40_ms", (0, 0))
    sv = (f"{to_mel} | env r {q['env_corr']:.3f} | tone {q['tone_db']:.2f} dB | onset {q['onset_err']:.2f} oct"
         f" ({shp[0]:.0f}/{shp[1]:.0f} dB/ms) | excess {q.get('excess_db', 0):.2f} missing {q.get('missing_db', 0):.2f}"
         f" | t40 {tm4[0]:.0f}/{tm4[1]:.0f} ms | level {q.get('level_db', 0):+.2f} dB")
    sv += _stereo_words(q)
    if q.get("clap_cos") is not None:
        sv += f" | CLAP {q['clap_cos']:.3f}"
    dv = q.get("describe")
    if dv:
        sv += f" | tags {dv['original'].get('tag')} / {dv['resynth'].get('tag')}"
    return sv


def _spectrum_db(mv: np.ndarray) -> np.ndarray:
    return _to_db(_power_stft(mv, 1024, 128)).T


def _panel_cols(original: Any, resynth: Any, more: tuple | None) -> list:
    """(name, mono signal) per column, zero-padded to one length."""
    colset = [("original", mono(load(original))), ("resynthesis", mono(load(resynth)))]
    if more is not None:
        colset.append((more[0], mono(load(more[1]))))
    cnt = max(len(cv[1]) for cv in colset)
    return [(name, np.pad(mv, (0, cnt - len(mv)))) for name, mv in colset]


def _panels_spec(canvas: Any, grid_v: Any, colset: list) -> None:
    spec_l = [_spectrum_db(mv) for _, mv in colset]
    upper = max(sv.max() for sv in spec_l)
    fq = np.maximum(np.arange(513) * SR / 1024, 1)
    tv = np.arange(spec_l[0].shape[1]) * 128 / SR
    for ii, ((name, _), sv) in enumerate(zip(colset, spec_l)):
        axh = canvas.add_subplot(grid_v[0, ii])
        axh.pcolormesh(tv, fq, sv, vmin=upper - 90, vmax=upper, shading="auto")
        axh.set_yscale("log")
        axh.set_ylim(40, 22000)
        axh.set_title(name)


def _panel_title(ttl: str | None, res: dict | None) -> str:
    head_n = ttl or ""
    if res is not None:
        head_n = (head_n + "\n" if head_n else "") + text(res)
    return head_n


def pair_png(path: str, original: Any, resynth: Any, title: str | None = None, q: dict | None = None,
             extra: tuple | None = None) -> str:
    """Write the side-by-side picture of original, render and an optional third sound."""
    import matplotlib

    matplotlib.use("Agg")
    import matplotlib.pyplot as plt

    colset = _panel_cols(original, resynth, extra)
    canvas = plt.figure(figsize=(4.4 * len(colset), 6.2), dpi=110)
    grid_v = canvas.add_gridspec(2, len(colset))
    _panels_spec(canvas, grid_v, colset)
    _panel_env(canvas.add_subplot(grid_v[1, : max(1, len(colset) - 1)]), colset)
    _panel_tone(canvas.add_subplot(grid_v[1, len(colset) - 1]), colset)
    head_n = _panel_title(title, q)
    if head_n:
        canvas.suptitle(head_n, fontsize=7)
    os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
    canvas.savefig(path, format="png")
    plt.close(canvas)
    return path


def _panel_env(axh: Any, colset: list) -> None:
    env_l = [env_db(mv) for _, mv in colset]
    upper = max(ev.max() for ev in env_l if len(ev)) if any(len(ev) for ev in env_l) else 0.0
    for (name, _), ev in zip(colset, env_l):
        axh.plot(np.arange(len(ev)) * 0.005, ev, label=name, lw=0.8)
    axh.set_ylim(max(upper - 80, -120), upper + 3)
    axh.legend(fontsize=6)


def _panel_tone(axh: Any, colset: list) -> None:
    for name, mv in colset:
        axh.semilogx(CEN, _level_profile(mv), label=name, lw=0.8)
    axh.legend(fontsize=6)
