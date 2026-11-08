#!/usr/bin/env python3
"""Fit a music track's bar grid and draft a bar-line edit to the film's arc.

The grid comes from the beat tracker, refined by a least-squares fit and a
snap to the measured transients. The edit is a dynamic-programming walk over
the track's bars that follows a loudness arc, one value per film bar.
Run with the hark venv Python and -I. --self-test runs the built-in checks.
"""
from __future__ import annotations

import argparse
import json
import math
import os
import subprocess
import sys

import numpy as np

SR = 22050
HOP = 256
N_FFT = 2048


class ArrangeError(Exception):
    pass


def say(*parts, file=None):
    text = " ".join(str(p) for p in parts)
    print(text.encode("ascii", "replace").decode("ascii"), file=file or sys.stdout)


def decode(path: str) -> np.ndarray:
    if not os.path.isfile(path):
        raise ArrangeError(f"track not found: {path}")
    r = subprocess.run(["ffmpeg", "-v", "error", "-nostdin", "-i", path, "-vn", "-f", "f32le", "-acodec",
                        "pcm_f32le", "-ac", "1", "-ar", str(SR), "-"], capture_output=True)
    if r.returncode != 0:
        raise ArrangeError(f"decode failed for {path}: {r.stderr.decode('utf-8', 'replace')[:200]}")
    return np.frombuffer(r.stdout, dtype=np.float32).astype(np.float64)


# ------------------------------------------------------------------ the arc

def default_arc(n: int, d: int) -> list[float]:
    big_until = d + max(2, round(0.6 * (n - d)))
    arc = []
    for k in range(1, n + 1):
        if k < d:
            x = (k - 1) / max(1, d - 2)
            v = -14 + 10 * min(1.0, x / 0.45)
            if 0.55 <= x < 0.8:
                v = -9.5
            if x >= 0.8:
                v = -9 + 4 * (x - 0.8) / 0.2
        elif k < big_until:
            v = 0.0
        else:
            v = -13 + 2 * min(3, n - k)
        arc.append(round(v, 1))
    return arc


def read_arc(spec: str | None) -> tuple[list[float] | None, dict]:
    if not spec:
        return None, {}
    if spec.endswith(".json"):
        with open(spec, encoding="utf-8") as fh:
            data = json.load(fh)
        meta = {k: data[k] for k in ("film_bpm", "beats_per_bar", "drop_film_bar") if data.get(k) is not None}
        return [float(v) for v in data["arc"]], meta
    return [float(v) for v in spec.split(",") if v.strip()], {}


# ------------------------------------------------------------------ grid

def fit_grid(beats: np.ndarray, bpm: float | None) -> dict:
    ibi = np.diff(beats)
    T0 = 60.0 / bpm if bpm else float(np.median(ibi))
    k = np.concatenate([[0], np.cumsum(np.maximum(1, np.round(ibi / T0)))])
    use = np.ones(len(beats), bool)
    t0, T = beats[0], T0
    for _ in range(5):
        kk, tt = k[use], beats[use]
        if bpm:
            T = 60.0 / bpm
            t0 = float(np.mean(tt - kk * T))
        else:
            A = np.vstack([np.ones_like(kk), kk]).T
            t0, T = np.linalg.lstsq(A, tt, rcond=None)[0]
        res = beats - (t0 + k * T)
        use = np.abs(res) < 0.035
        if use.sum() < 8:
            use = np.ones(len(beats), bool)
    res = beats - (t0 + k * T)
    inl = np.abs(res) < 0.035
    return {"t0": float(t0), "T": float(T), "median_residual_ms": float(np.median(np.abs(res)) * 1000),
            "inlier_frac": float(inl.mean())}


def flux_curve(y: np.ndarray) -> tuple[np.ndarray, float]:
    import librosa
    S = np.abs(librosa.stft(y, n_fft=512, hop_length=64, window="hann", center=True))
    L = np.log1p(100 * S)
    f = np.concatenate([[0.0], np.maximum(0, np.diff(L, axis=1)).sum(axis=0)])
    return f, 64 / SR


def snap(y: np.ndarray, t0: float, T: float, dur: float) -> tuple[float, float, float]:
    f, dt = flux_curve(y)
    s = min(0.1, 0.3 * T)
    shifts = np.arange(-s, s + 1e-9, 0.001)
    scores = []
    for sh in shifts:
        start = (t0 + sh) % T
        pts = np.arange(start, dur, T)
        idx = np.round(pts / dt).astype(int)
        idx = idx[(idx >= 0) & (idx < len(f))]
        scores.append(float(f[idx].mean()) if len(idx) else 0.0)
    scores = np.array(scores)
    b = int(np.argmax(scores))
    med = float(np.median(scores))
    return float(shifts[b]), (float(scores[b] / med) if med > 0 else 1.0), t0 + float(shifts[b])


def zscore(v: np.ndarray) -> np.ndarray:
    sd = float(np.std(v))
    return (v - float(np.mean(v))) / sd if sd > 0 else np.zeros_like(v)


def downbeat(S: np.ndarray, chroma: np.ndarray, grid: np.ndarray, T: float, B: int):
    freqs = np.fft.rfftfreq(N_FFT, 1 / SR)
    low = S[freqs < 150].sum(axis=0)
    lo_on = np.concatenate([[0.0], np.maximum(0, np.diff(low))])
    nfr = S.shape[1]
    fr = np.round(grid * SR / HOP).astype(int)
    tf = int(round(T * SR / HOP))
    lows, chg = [], []
    for f in fr:
        a, b = max(0, f - 2), min(nfr, f + 4)
        lows.append(float(lo_on[a:b].max()) if b > a else 0.0)
        p = chroma[:, max(0, f - tf):max(1, f)].mean(axis=1) if f > 0 else np.zeros(chroma.shape[0])
        n = chroma[:, f:min(nfr, f + tf)].mean(axis=1) if f < nfr else np.zeros(chroma.shape[0])
        den = float(np.linalg.norm(p) * np.linalg.norm(n))
        chg.append(1 - float(p @ n) / den if den > 0 else 0.0)
    z = zscore(np.array(lows)) + zscore(np.array(chg))
    scores = [float(z[p::B].mean()) if len(z[p::B]) else -1e9 for p in range(B)]
    return np.array(scores), z


def local_disagreements(z: np.ndarray, grid: np.ndarray, B: int, phase: int) -> list[dict]:
    out = []
    win, hop = 8 * B, 4 * B
    for s in range(0, max(1, len(z) - win + 1), hop):
        seg = z[s:s + win]
        idx = np.arange(s, s + len(seg))
        sc = [float(seg[(idx % B) == p].mean()) if np.any((idx % B) == p) else -1e9 for p in range(B)]
        best = int(np.argmax(sc))
        gap = sc[best] - sc[phase]
        if best != phase and gap >= 0.5:
            item = {"from": round(float(grid[s]), 2), "to": round(float(grid[min(len(grid) - 1, s + len(seg) - 1)]), 2),
                    "phase": best, "gap": round(gap, 2)}
            if out and out[-1]["phase"] == best and out[-1]["to"] >= item["from"]:
                out[-1]["to"] = item["to"]
                out[-1]["gap"] = max(out[-1]["gap"], item["gap"])
            else:
                out.append(item)
    return out


# ------------------------------------------------------------------ bars and the walk

def bar_features(y, S, chroma, onset_env, bars_t, bar_len):
    import librosa
    mel = librosa.feature.melspectrogram(S=S ** 2, sr=SR)
    mfcc = librosa.feature.mfcc(S=librosa.power_to_db(mel), n_mfcc=13)
    med = float(np.median(onset_env)) if len(onset_env) else 0.0
    feats, loud, dens = [], [], []
    for t in bars_t:
        a, b = int(t * SR), int((t + bar_len) * SR)
        seg = y[a:b]
        loud.append(20 * math.log10(math.sqrt(float(np.mean(seg ** 2))) + 1e-10) if len(seg) else -200.0)
        fa, fb = int(t * SR / HOP), max(int(t * SR / HOP) + 1, int((t + bar_len) * SR / HOP))
        oe = onset_env[fa:fb]
        dens.append(float(np.mean(oe > 1.5 * med)) if len(oe) else 0.0)
        c = chroma[:, fa:fb].mean(axis=1)
        m = mfcc[1:13, fa:fb].mean(axis=1)
        c = c / (np.linalg.norm(c) + 1e-12)
        m = m / (np.linalg.norm(m) + 1e-12)
        feats.append(np.concatenate([c, m]))
    return np.array(feats), np.array(loud), np.array(dens)


def drop_candidates(rel: np.ndarray, nov: np.ndarray) -> list[dict]:
    n = len(rel)
    lift = np.array([0.0] + [rel[i] - float(np.median(rel[max(0, i - 4):i])) for i in range(1, n)])
    nxt = np.array([float(np.mean(rel[i:i + 4])) for i in range(n)])
    elig = (rel > -3) & (nxt > -4)
    if not elig.any():
        elig = rel > np.percentile(rel, 75)
    score = lift / 3 + 2 * nov
    idx = [i for i in np.argsort(-score) if elig[i]][:6]
    return [{"bar": int(i), "score": round(float(score[i]), 3), "lift_db": round(float(lift[i]), 2),
             "novelty": round(float(nov[i]), 3)} for i in idx]


INF = 1e18


def _jumps(feat: np.ndarray, loud: np.ndarray, jump: float) -> tuple[np.ndarray, np.ndarray]:
    """The cost of going from source bar i to bar j, and the continuation mask (j = i + 1)."""
    n = len(feat)
    dist = np.linalg.norm(feat[:, None, :] - feat[None, :, :], axis=2)
    D = dist + np.abs(loud[:, None] - loud[None, :]) / 6
    nxt = np.minimum(np.arange(n) + 1, n - 1)
    J = jump + 2.5 * D[nxt, :]
    cont = np.zeros((n, n), bool)
    cont[np.arange(n - 1), np.arange(1, n)] = True
    J[cont] = 0.0
    J[np.arange(n), np.arange(n)] += 1.5
    return J, cont


def _pin(cost: np.ndarray, bar: int) -> np.ndarray:
    """Only bar stays reachable."""
    m = np.full(len(cost), INF)
    m[bar] = cost[bar]
    return m


def _pins(K: int, opt: dict) -> list[tuple[int, int]]:
    """(film bar index, source bar) pairs the walk must pass, in the order they apply."""
    pins = []
    if K > 1 and opt["drop_bar"] is not None:
        pins.append((opt["drop_k"], opt["drop_bar"]))
    if K > 1 and opt["end_bar"] is not None:
        pins.append((K - 1, opt["end_bar"]))
    return pins


def _apply_pins(cost: np.ndarray, k: int, pins: list) -> np.ndarray:
    for pk, bar in pins:
        cost = _pin(cost, bar) if pk == k else cost
    return cost


def _start_cost(rel: np.ndarray, first: float, start_bar) -> np.ndarray:
    cost = np.abs(rel - first) / 3
    if start_bar is None:
        return cost + 0.4 * np.arange(len(rel))
    return _pin(cost, start_bar)


def _backtrack(cost: np.ndarray, back: list) -> tuple[list[int], float]:
    j = int(np.argmin(cost))
    total = float(cost[j])
    path = [j]
    for arg in reversed(back):
        j = int(arg[j])
        path.append(j)
    return path[::-1], total


def walk(feat, loud, rel, arc, opt: dict):
    """The cheapest source-bar path for the film arc.

    opt: jump, phrase, drop_k, drop_bar, start_bar, end_bar.
    """
    n, K = len(feat), len(arc)
    J, cont = _jumps(feat, loud, opt["jump"])
    pins = _pins(K, opt)
    cost = _apply_pins(_start_cost(rel, arc[0], opt["start_bar"]), 0, pins)
    back = []
    for k in range(1, K):
        step = J + (0.0 if k % opt["phrase"] == 0 else 1.0) * (~cont)
        tot = cost[:, None] + step
        arg = np.argmin(tot, axis=0)
        cost = _apply_pins(tot[arg, np.arange(n)] + np.abs(rel - arc[k]) / 3, k, pins)
        back.append(arg)
    return _backtrack(cost, back)


# ------------------------------------------------------------------ main flow

def _check_drop_arg(drop: str) -> None:
    if drop in ("auto", "none"):
        return
    try:
        int(drop)
    except ValueError:
        raise ArrangeError(f"--drop must be auto, none or a bar number, not {drop!r}")


def _beats(y: np.ndarray, bpm) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    """(magnitude spectrogram, onset envelope, beat times)."""
    import librosa
    S = np.abs(librosa.stft(y, n_fft=N_FFT, hop_length=HOP))
    onset_env = librosa.onset.onset_strength(S=librosa.amplitude_to_db(S, ref=np.max), sr=SR, hop_length=HOP)
    _, bt = librosa.beat.beat_track(onset_envelope=onset_env, sr=SR, hop_length=HOP,
                                    start_bpm=bpm or 120.0, tightness=400)
    beats = librosa.frames_to_time(bt, sr=SR, hop_length=HOP)
    if len(beats) < 8:
        raise ArrangeError(f"only {len(beats)} beats tracked: not a steady track, or too short")
    return S, onset_env, beats


def _grid(y: np.ndarray, beats: np.ndarray, a, dur: float) -> dict:
    """The fitted beat grid: g (fit_grid), t0, T, shift, ratio and the grid times."""
    g = fit_grid(beats, a.bpm)
    t0, T = g["t0"], g["T"]
    shift, ratio = 0.0, 1.0
    if not a.no_snap:
        shift, ratio, t0 = snap(y, t0, T, dur)
    start = t0 % T
    if start - T >= -0.03:
        start -= T  # a beat a few ms before 0 is the track's first beat, not a lost one
    return {"g": g, "t0": t0, "T": T, "shift": shift, "ratio": ratio,
            "grid": np.maximum(0.0, np.arange(start, dur, T))}


def _phase(scores: np.ndarray, B: int, phase_arg: str) -> tuple[int, int, float, bool]:
    """(downbeat phase, alternative phase, confidence, low confidence)."""
    order = list(np.argsort(-scores))
    auto = int(order[0])
    second = int(order[1]) if B > 1 else auto
    conf = float(scores[order[0]] - scores[order[1]]) if B > 1 else 99.0
    if phase_arg == "auto":
        return auto, second, conf, conf < 0.5
    phase = int(phase_arg)
    if not 0 <= phase < B:
        raise ArrangeError(f"--phase must be auto or 0..{B - 1}")
    return phase, (auto if auto != phase else second), conf, False


def _bar_times(grid: np.ndarray, phase: int, B: int, bar_len: float, dur: float) -> list[float]:
    bars_t = [float(grid[i]) for i in range(phase, len(grid), B) if grid[i] + bar_len <= dur + 0.001]
    if len(bars_t) < 2:
        raise ArrangeError(f"only {len(bars_t)} whole bar(s) in the track: need at least 2")
    return bars_t


def _diffs(v: np.ndarray, fn) -> np.ndarray:
    return np.array([0.0] + [float(fn(v[i], v[i - 1])) for i in range(1, len(v))])


def _film(a, meta: dict, arc) -> tuple[int, int, list]:
    """(film bars, drop film bar, arc)."""
    K = len(arc) if arc else int(a.bars)
    drop_film = int(a.drop_film_bar or meta.get("drop_film_bar") or round(0.7 * K))
    if not 1 <= drop_film <= K:
        raise ArrangeError(f"the drop film bar {drop_film} is outside 1..{K}")
    return K, drop_film, (default_arc(K, drop_film) if arc is None else arc)


def _drop_bar(drop: str, cands: list):
    if drop == "auto":
        return cands[0]["bar"] if cands else None
    return None if drop == "none" else int(drop)


def _check_source_bars(nb: int, pairs) -> None:
    for name, v in pairs:
        if v is not None and not 0 <= v < nb:
            raise ArrangeError(f"{name} {v} is outside the source bars 0..{nb - 1}")


def _runs(path: list[int], K: int) -> list[tuple[int, int]]:
    """(first, last) film-bar index of each run of consecutive source bars."""
    runs, k = [], 0
    while k < K:
        e = k
        while e + 1 < K and path[e + 1] == path[e] + 1:
            e += 1
        runs.append((k, e))
        k = e + 1
    return runs


def _segments(path: list[int], K: int, bars_t: list[float], B: int, T: float, at0: float,
              film_bar_len: float) -> list[dict]:
    segs = []
    for k, e in _runs(path, K):
        src_from = bars_t[path[k]]
        segs.append({"src_from": round(src_from, 4), "src_to": round(src_from + (e - k + 1) * B * T, 4),
                     "at": round(at0 + k * film_bar_len, 4),
                     "src_bars": [path[k], path[e]], "film_bars": [k + 1, e + 1]})
    return segs


def _music(file: str, segs: list, bpm: float, film_bpm: float) -> dict:
    none = abs(film_bpm / bpm - 1) < 0.002
    return {"file": file, "src_bpm": round(film_bpm if none else bpm, 3), "target_bpm": film_bpm,
            "stretch": "none" if none else "rubberband", "gain_db": 0,
            "segments": [{"label": f"source bars {s['src_bars'][0]}-{s['src_bars'][1]} -> film bars "
                                   f"{s['film_bars'][0]}-{s['film_bars'][1]}",
                          "src_from": s["src_from"], "src_to": s["src_to"], "at": s["at"],
                          "fade_in": 0.008, "fade_out": 0.06} for s in segs]}


def _grid_at(grid: np.ndarray, i: int, fallback: float) -> float:
    return float(grid[i]) if i < len(grid) else fallback


def _bar_rows(bars_t: list[float], table: dict) -> list[dict]:
    return [{"i": i, "t": round(bars_t[i], 4), "rel_db": round(float(table["rel"][i]), 2),
             "novelty": round(float(table["nov"][i]), 3), "rise_db": round(float(table["rise"][i]), 2),
             "density": round(float(table["dens"][i]), 3)} for i in range(len(bars_t))]


def _bar_table(y, S, chroma, onset_env, bars_t: list[float], bar_len: float) -> dict:
    """Per source bar: features, loudness, density, level under the top, novelty and rise."""
    feat, loud, dens = bar_features(y, S, chroma, onset_env, bars_t, bar_len)
    return {"feat": feat, "loud": loud, "dens": dens, "rel": loud - np.percentile(loud, 90),
            "nov": _diffs(feat, lambda u, w: np.linalg.norm(u - w)), "rise": _diffs(loud, lambda u, w: u - w)}


def _analyse_track(a, B: int) -> dict:
    """Decode, beat-track, fit the grid, read the downbeat and the bar table."""
    import librosa
    y = decode(a.track)
    dur = len(y) / SR
    S, onset_env, beats = _beats(y, a.bpm)
    v = _grid(y, beats, a, dur)
    grid, T = v["grid"], v["T"]
    chroma = librosa.feature.chroma_stft(S=S ** 2, sr=SR)
    scores, z = downbeat(S, chroma, grid, T, B)
    phase, alt, conf, low_conf = _phase(scores, B, a.phase)
    bars_t = _bar_times(grid, phase, B, B * T, dur)
    v.update(dur=dur, scores=scores, phase=phase, alt=alt, conf=conf, low_conf=low_conf, bars_t=bars_t,
             disagree=local_disagreements(z, grid, B, phase),
             table=_bar_table(y, S, chroma, onset_env, bars_t, B * T))
    return v


def arrange(a) -> dict:
    _check_drop_arg(a.drop)
    arc, meta = read_arc(a.arc)
    film_bpm = float(meta.get("film_bpm", a.film_bpm))
    B = int(meta.get("beats_per_bar", a.beats_per_bar))
    v = _analyse_track(a, B)
    tb, bars_t, T = v["table"], v["bars_t"], v["T"]
    nb = len(bars_t)
    cands = drop_candidates(tb["rel"], tb["nov"])
    K, drop_film, arc = _film(a, meta, arc)
    drop_bar = _drop_bar(a.drop, cands)
    _check_source_bars(nb, (("--drop", drop_bar), ("--start-bar", a.start_bar), ("--end-bar", a.end_bar)))
    opt = {"jump": a.jump, "phrase": max(1, a.phrase), "drop_k": drop_film - 1, "drop_bar": drop_bar,
           "start_bar": a.start_bar, "end_bar": a.end_bar}
    path, total = walk(tb["feat"], tb["loud"], tb["rel"], arc, opt)
    segs = _segments(path, K, bars_t, B, T, a.film_offset, B * 60.0 / film_bpm)
    bpm = 60.0 / T
    g, t0 = v["g"], v["t0"]
    res = {"track": a.track, "duration": round(v["dur"], 3), "bpm": round(bpm, 3), "beat_period": round(T, 6),
           "film_bpm": film_bpm, "beats_per_bar": B, "film_bars": K, "drop_film_bar": drop_film, "arc": arc,
           "stretch_ratio_to_film": round(film_bpm / bpm, 5), "t0": round(t0, 4),
           "transient_shift_ms": round(v["shift"] * 1000, 1), "transient_fit_ratio": round(v["ratio"], 3),
           "grid_median_residual_ms": round(g["median_residual_ms"], 2),
           "grid_inlier_frac": round(g["inlier_frac"], 3), "downbeat_phase": v["phase"],
           "phase_scores": [round(float(s), 3) for s in v["scores"]], "phase_confidence": round(v["conf"], 3),
           "phase_low_confidence": bool(v["low_conf"]),
           "phase_alternative": {"phase": v["alt"], "first_downbeat": round(_grid_at(v["grid"], v["alt"], t0), 4)},
           "local_phase_disagreements": v["disagree"], "drop_source_bar": drop_bar,
           "bars": _bar_rows(bars_t, tb), "drop_candidates": cands, "map": path, "segments": segs,
           "music": _music(a.file or a.track, segs, bpm, film_bpm), "total_cost": round(total, 3)}
    res["summary"] = summary(res, _grid_at(v["grid"], v["phase"], t0), a.phase)
    return res


def summary(r: dict, first_down: float, phase_arg: str) -> str:
    bar = r["beat_period"] * r["beats_per_bar"]
    s = (f"{r['bpm']:.2f} BPM (grid residual {r['grid_median_residual_ms']:.1f} ms, "
         f"{100 * r['grid_inlier_frac']:.0f}% inliers); downbeats at {first_down:.3f} + {bar:.3f}*n s "
         f"(phase {r['downbeat_phase']}, confidence {r['phase_confidence']:.2f}")
    alt = r["phase_alternative"]
    if r["phase_low_confidence"]:
        s += (f", LOW: the other grid is {alt['first_downbeat']:.3f} + {bar:.3f}*n s, select it with "
              f"--phase {alt['phase']}")
    elif phase_arg != "auto":
        s += f", the automatic choice was phase {alt['phase']}"
    s += (f"); {len(r['bars'])} source bars, {r['film_bars']} film bars, {len(r['segments'])} segment(s); "
          f"drop on film bar {r['drop_film_bar']} from "
          + (f"source bar {r['drop_source_bar']}" if r["drop_source_bar"] is not None else "the bar the arc chose")
          + f"; stretch ratio {r['stretch_ratio_to_film']:.4f}")
    if r["local_phase_disagreements"]:
        s += "; local phase disagreements: " + ", ".join(
            f"{d['from']:.1f}-{d['to']:.1f} s phase {d['phase']}" for d in r["local_phase_disagreements"][:4])
    return s


def print_text(r: dict, out: str | None):
    say(r["summary"])
    say(f"transient shift {r['transient_shift_ms']} ms, fit ratio {r['transient_fit_ratio']}")
    say("phase scores: " + " ".join(f"{p}:{v}" for p, v in enumerate(r["phase_scores"])))
    say("drop candidates: " + ", ".join(f"bar {c['bar']} ({c['score']})" for c in r["drop_candidates"]))
    if r["phase_low_confidence"]:
        say("LOW downbeat confidence: look where the first note falls, where sections enter and where chords "
            f"change, then rerun with --phase {r['downbeat_phase']} or --phase {r['phase_alternative']['phase']}")
    top = {c["bar"] for c in r["drop_candidates"][:3]}
    say("bar      t  rel_dB    nov  rise  dens")
    for b in r["bars"]:
        mark = "  <- drop candidate" if b["i"] in top else ""
        say(f"{b['i']:3d} {b['t']:7.3f} {b['rel_db']:7.2f} {b['novelty']:6.3f} {b['rise_db']:5.1f} "
            f"{b['density']:5.2f}{mark}")
    say("map film:source " + " ".join(f"{k + 1}:{s}" for k, s in enumerate(r["map"])))
    say(json.dumps({"music": r["music"]}, indent=1))
    if out:
        say("wrote", out)


def parse(argv):
    p = argparse.ArgumentParser(prog="arrange.py", description="Fit a track's bar grid and draft a bar-line edit.")
    p.add_argument("track", nargs="?", metavar="TRACK")
    p.add_argument("--bpm", type=float)
    p.add_argument("--phase", default="auto")
    p.add_argument("--film-bpm", type=float, default=120.0)
    p.add_argument("--beats-per-bar", type=int, default=4)
    p.add_argument("--bars", type=int, default=30)
    p.add_argument("--arc")
    p.add_argument("--drop-film-bar", type=int)
    p.add_argument("--drop", default="auto")
    p.add_argument("--start-bar", type=int)
    p.add_argument("--end-bar", type=int)
    p.add_argument("--phrase", type=int, default=2)
    p.add_argument("--film-offset", type=float, default=0.0)
    p.add_argument("--jump", type=float, default=3.0)
    p.add_argument("--no-snap", action="store_true")
    p.add_argument("--file")
    p.add_argument("--out")
    p.add_argument("--json", action="store_true")
    p.add_argument("--self-test", action="store_true")
    return p.parse_args(argv)


def _write_out(out: str | None, r: dict) -> None:
    if not out:
        return
    os.makedirs(os.path.dirname(os.path.abspath(out)), exist_ok=True)
    with open(out, "w", encoding="utf-8") as fh:
        json.dump(r, fh, indent=1)


def _run(a) -> int:
    try:
        r = arrange(a)
    except ArrangeError as e:
        say(f"arrange.py: {e}", file=sys.stderr)
        return 1
    _write_out(a.out, r)
    if a.json:
        say(json.dumps(r, indent=1))
    else:
        print_text(r, a.out)
    return 0


def main(argv=None) -> int:
    a = parse(sys.argv[1:] if argv is None else argv)
    if a.self_test:
        return self_test()
    if not a.track:
        say("arrange.py: give a TRACK", file=sys.stderr)
        return 2
    return _run(a)


def _quiet(argv, err=False):
    """main(argv) with stdout (or stderr) captured. Returns (code, text)."""
    import contextlib
    import io
    buf = io.StringIO()
    with (contextlib.redirect_stderr(buf) if err else contextlib.redirect_stdout(buf)):
        code = main(argv)
    return code, buf.getvalue()


def _test_grid(ck, song: str, out: str) -> None:
    code, _ = _quiet([song, "--bars", "8", "--out", out])
    r = json.load(open(out))
    ck("S11 bpm 120 +-0.05", code == 0 and abs(r["bpm"] - 120) <= 0.05)
    down = r["bars"][0]["t"]
    ck("S11 the downbeat phase is the accent", abs(((down + 0.25) % 2.0) - 0.25) < 0.03)
    ck("S11 stretch none for film 120", r["music"]["stretch"] == "none" and r["music"]["src_bpm"] == 120)
    bar_times = [b["t"] for b in r["bars"]]
    ck("S11 segments start on bar times", all(any(abs(s["src_from"] - t) < 1e-3 for t in bar_times)
                                              for s in r["segments"]))
    drop = r["map"][r["drop_film_bar"] - 1]
    ck("S11 the drop bar is in the louder half", r["bars"][drop]["t"] >= 19.5)


def _test_errors(ck, song: str, d: str) -> None:
    code, err = _quiet([song, "--bars", "8", "--drop", "99"], err=True)
    ck("S12 --drop 99 names the range 0..19", code == 1 and "0..19" in err)
    ck("bad --drop word", _quiet([song, "--drop", "loud"], err=True)[0] == 1)
    ck("missing track", _quiet([os.path.join(d, "none.wav")], err=True)[0] == 1)


def self_test() -> int:
    import tempfile
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    import fixtures as fx
    ck = fx.Checker()
    with tempfile.TemporaryDirectory() as d:
        song = fx.write(os.path.join(d, "song.wav"), fx.click_track(120, 20, louder_from=10))
        out = os.path.join(d, "arr.json")
        _test_grid(ck, song, out)
        _test_errors(ck, song, d)
        ck("default arc shape", default_arc(15, 9)[8] == 0.0 and default_arc(15, 9)[0] == -14.0)
        arcf = fx.put_json(os.path.join(d, "arc.json"), {"film_bpm": 120, "beats_per_bar": 4, "drop_film_bar": 3,
                                                         "arc": [-14, -10, 0, 0, -9]})
        code, _ = _quiet([song, "--arc", arcf, "--out", out])
        r = json.load(open(out))
        ck("arc file: 5 film bars, drop on film bar 3", code == 0 and r["film_bars"] == 5 and r["drop_film_bar"] == 3)
    return ck.code()


if __name__ == "__main__":
    sys.exit(main())
