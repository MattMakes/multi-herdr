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
    text = " ".join(str(pp) for pp in parts)
    print(text.encode("ascii", "replace").decode("ascii"), file=file or sys.stdout)


def decode(src: str) -> np.ndarray:
    if not os.path.isfile(src):
        raise ArrangeError(f"track not found: {src}")
    rep = subprocess.run(["ffmpeg", "-v", "error", "-nostdin", "-i", src, "-vn", "-f", "f32le", "-acodec",
                        "pcm_f32le", "-ac", "1", "-ar", str(SR), "-"], capture_output=True)
    if rep.returncode != 0:
        raise ArrangeError(f"decode failed for {src}: {rep.stderr.decode('utf-8', 'replace')[:200]}")
    return np.frombuffer(rep.stdout, dtype=np.float32).astype(np.float64)


# ------------------------------------------------------------------ the arc

def default_arc(count: int, dd: int) -> list[float]:
    big_until = dd + max(2, round(0.6 * (count - dd)))
    curve_ = []
    for ki in range(1, count + 1):
        if ki < dd:
            xv = (ki - 1) / max(1, dd - 2)
            val = -14 + 10 * min(1.0, xv / 0.45)
            if 0.55 <= xv < 0.8:
                val = -9.5
            if xv >= 0.8:
                val = -9 + 4 * (xv - 0.8) / 0.2
        elif ki < big_until:
            val = 0.0
        else:
            val = -13 + 2 * min(3, count - ki)
        curve_.append(round(val, 1))
    return curve_


def read_arc(spec: str | None) -> tuple[list[float] | None, dict]:
    if not spec:
        return None, {}
    if spec.endswith(".json"):
        with open(spec, encoding="utf-8") as fh:
            data = json.load(fh)
        meta = {ki: data[ki] for ki in ("film_bpm", "beats_per_bar", "drop_film_bar") if data.get(ki) is not None}
        return [float(val) for val in data["arc"]], meta
    return [float(val) for val in spec.split(",") if val.strip()], {}


# ------------------------------------------------------------------ grid

def fit_grid(beat_t: np.ndarray, tempo: float | None) -> dict:
    ibi = np.diff(beat_t)
    T0 = 60.0 / tempo if tempo else float(np.median(ibi))
    ki = np.concatenate([[0], np.cumsum(np.maximum(1, np.round(ibi / T0)))])
    use = np.ones(len(beat_t), bool)
    origin, period = beat_t[0], T0
    for _ in range(5):
        kk, tt = ki[use], beat_t[use]
        if tempo:
            period = 60.0 / tempo
            origin = float(np.mean(tt - kk * period))
        else:
            A = np.vstack([np.ones_like(kk), kk]).T
            origin, period = np.linalg.lstsq(A, tt, rcond=None)[0]
        fit = beat_t - (origin + ki * period)
        use = np.abs(fit) < 0.035
        if use.sum() < 8:
            use = np.ones(len(beat_t), bool)
    fit = beat_t - (origin + ki * period)
    inl = np.abs(fit) < 0.035
    return {"t0": float(origin), "T": float(period), "median_residual_ms": float(np.median(np.abs(fit)) * 1000),
            "inlier_frac": float(inl.mean())}


def flux_curve(wave: np.ndarray) -> tuple[np.ndarray, float]:
    import librosa
    spec_mag = np.abs(librosa.stft(wave, n_fft=512, hop_length=64, window="hann", center=True))
    L = np.log1p(100 * spec_mag)
    flux = np.concatenate([[0.0], np.maximum(0, np.diff(L, axis=1)).sum(axis=0)])
    return flux, 64 / SR


def snap(wave: np.ndarray, origin: float, period: float, dur: float) -> tuple[float, float, float]:
    flux, dt = flux_curve(wave)
    sv = min(0.1, 0.3 * period)
    shifts = np.arange(-sv, sv + 1e-9, 0.001)
    scores = []
    for sh in shifts:
        start = (origin + sh) % period
        pts = np.arange(start, dur, period)
        idx = np.round(pts / dt).astype(int)
        idx = idx[(idx >= 0) & (idx < len(flux))]
        scores.append(float(flux[idx].mean()) if len(idx) else 0.0)
    scores = np.array(scores)
    bv = int(np.argmax(scores))
    med = float(np.median(scores))
    return float(shifts[bv]), (float(scores[bv] / med) if med > 0 else 1.0), origin + float(shifts[bv])


def zscore(val: np.ndarray) -> np.ndarray:
    sd = float(np.std(val))
    return (val - float(np.mean(val))) / sd if sd > 0 else np.zeros_like(val)


def downbeat(spec_mag: np.ndarray, chroma: np.ndarray, beat_grid: np.ndarray, period: float, per_bar: int):
    freqs = np.fft.rfftfreq(N_FFT, 1 / SR)
    low = spec_mag[freqs < 150].sum(axis=0)
    lo_on = np.concatenate([[0.0], np.maximum(0, np.diff(low))])
    nfr = spec_mag.shape[1]
    fr = np.round(beat_grid * SR / HOP).astype(int)
    tf = int(round(period * SR / HOP))
    lows, chg = [], []
    for flux in fr:
        cli, bv = max(0, flux - 2), min(nfr, flux + 4)
        lows.append(float(lo_on[cli:bv].max()) if bv > cli else 0.0)
        pp = chroma[:, max(0, flux - tf):max(1, flux)].mean(axis=1) if flux > 0 else np.zeros(chroma.shape[0])
        count = chroma[:, flux:min(nfr, flux + tf)].mean(axis=1) if flux < nfr else np.zeros(chroma.shape[0])
        den = float(np.linalg.norm(pp) * np.linalg.norm(count))
        chg.append(1 - float(pp @ count) / den if den > 0 else 0.0)
    z = zscore(np.array(lows)) + zscore(np.array(chg))
    scores = [float(z[pp::per_bar].mean()) if len(z[pp::per_bar]) else -1e9 for pp in range(per_bar)]
    return np.array(scores), z


def local_disagreements(z: np.ndarray, beat_grid: np.ndarray, per_bar: int, ph: int) -> list[dict]:
    dest = []
    win, hop = 8 * per_bar, 4 * per_bar
    for sv in range(0, max(1, len(z) - win + 1), hop):
        span = z[sv:sv + win]
        idx = np.arange(sv, sv + len(span))
        sc = [float(span[(idx % per_bar) == pp].mean()) if np.any((idx % per_bar) == pp) else -1e9 for pp in range(per_bar)]
        best = int(np.argmax(sc))
        gap = sc[best] - sc[ph]
        if best != ph and gap >= 0.5:
            item = {"from": round(float(beat_grid[sv]), 2), "to": round(float(beat_grid[min(len(beat_grid) - 1, sv + len(span) - 1)]), 2),
                    "phase": best, "gap": round(gap, 2)}
            if dest and dest[-1]["phase"] == best and dest[-1]["to"] >= item["from"]:
                dest[-1]["to"] = item["to"]
                dest[-1]["gap"] = max(dest[-1]["gap"], item["gap"])
            else:
                dest.append(item)
    return dest


# ------------------------------------------------------------------ bars and the walk

def bar_features(wave, spec_mag, chroma, onset_env, bars_t, bar_len):
    import librosa
    mel = librosa.feature.melspectrogram(S=spec_mag ** 2, sr=SR)
    mfcc = librosa.feature.mfcc(S=librosa.power_to_db(mel), n_mfcc=13)
    med = float(np.median(onset_env)) if len(onset_env) else 0.0
    feats, loud, dens = [], [], []
    for tv in bars_t:
        cli, bv = int(tv * SR), int((tv + bar_len) * SR)
        span = wave[cli:bv]
        loud.append(20 * math.log10(math.sqrt(float(np.mean(span ** 2))) + 1e-10) if len(span) else -200.0)
        fa, fb = int(tv * SR / HOP), max(int(tv * SR / HOP) + 1, int((tv + bar_len) * SR / HOP))
        oe = onset_env[fa:fb]
        dens.append(float(np.mean(oe > 1.5 * med)) if len(oe) else 0.0)
        cv = chroma[:, fa:fb].mean(axis=1)
        mv = mfcc[1:13, fa:fb].mean(axis=1)
        cv = cv / (np.linalg.norm(cv) + 1e-12)
        mv = mv / (np.linalg.norm(mv) + 1e-12)
        feats.append(np.concatenate([cv, mv]))
    return np.array(feats), np.array(loud), np.array(dens)


def drop_candidates(rel: np.ndarray, nov: np.ndarray) -> list[dict]:
    count = len(rel)
    lift = np.array([0.0] + [rel[ix] - float(np.median(rel[max(0, ix - 4):ix])) for ix in range(1, count)])
    nxt = np.array([float(np.mean(rel[ix:ix + 4])) for ix in range(count)])
    elig = (rel > -3) & (nxt > -4)
    if not elig.any():
        elig = rel > np.percentile(rel, 75)
    score = lift / 3 + 2 * nov
    idx = [ix for ix in np.argsort(-score) if elig[ix]][:6]
    return [{"bar": int(ix), "score": round(float(score[ix]), 3), "lift_db": round(float(lift[ix]), 2),
             "novelty": round(float(nov[ix]), 3)} for ix in idx]


INF = 1e18


def _jumps(feat: np.ndarray, loud: np.ndarray, jump: float) -> tuple[np.ndarray, np.ndarray]:
    """The cost of going from source bar i to bar j, and the continuation mask (j = i + 1)."""
    count = len(feat)
    dist = np.linalg.norm(feat[:, None, :] - feat[None, :, :], axis=2)
    D = dist + np.abs(loud[:, None] - loud[None, :]) / 6
    nxt = np.minimum(np.arange(count) + 1, count - 1)
    J = jump + 2.5 * D[nxt, :]
    cont = np.zeros((count, count), bool)
    cont[np.arange(count - 1), np.arange(1, count)] = True
    J[cont] = 0.0
    J[np.arange(count), np.arange(count)] += 1.5
    return J, cont


def _pin(costs: np.ndarray, bar: int) -> np.ndarray:
    """Only bar stays reachable."""
    mv = np.full(len(costs), INF)
    mv[bar] = costs[bar]
    return mv


def _pins(K: int, opt: dict) -> list[tuple[int, int]]:
    """(film bar index, source bar) pairs the walk must pass, in the order they apply."""
    pins = []
    if K > 1 and opt["drop_bar"] is not None:
        pins.append((opt["drop_k"], opt["drop_bar"]))
    if K > 1 and opt["end_bar"] is not None:
        pins.append((K - 1, opt["end_bar"]))
    return pins


def _apply_pins(costs: np.ndarray, ki: int, pins: list) -> np.ndarray:
    for pk, bar in pins:
        costs = _pin(costs, bar) if pk == ki else costs
    return costs


def _start_cost(rel: np.ndarray, first: float, start_bar) -> np.ndarray:
    costs = np.abs(rel - first) / 3
    if start_bar is None:
        return costs + 0.4 * np.arange(len(rel))
    return _pin(costs, start_bar)


def _backtrack(costs: np.ndarray, back: list) -> tuple[list[int], float]:
    jj = int(np.argmin(costs))
    total = float(costs[jj])
    route = [jj]
    for arg in reversed(back):
        jj = int(arg[jj])
        route.append(jj)
    return route[::-1], total


def walk(feat, loud, rel, curve_, opt: dict):
    """The cheapest source-bar path for the film arc.

    opt: jump, phrase, drop_k, drop_bar, start_bar, end_bar.
    """
    count, K = len(feat), len(curve_)
    J, cont = _jumps(feat, loud, opt["jump"])
    pins = _pins(K, opt)
    costs = _apply_pins(_start_cost(rel, curve_[0], opt["start_bar"]), 0, pins)
    back = []
    for ki in range(1, K):
        step = J + (0.0 if ki % opt["phrase"] == 0 else 1.0) * (~cont)
        tot = costs[:, None] + step
        arg = np.argmin(tot, axis=0)
        costs = _apply_pins(tot[arg, np.arange(count)] + np.abs(rel - curve_[ki]) / 3, ki, pins)
        back.append(arg)
    return _backtrack(costs, back)


# ------------------------------------------------------------------ main flow

def _check_drop_arg(drop: str) -> None:
    if drop in ("auto", "none"):
        return
    try:
        int(drop)
    except ValueError:
        raise ArrangeError(f"--drop must be auto, none or a bar number, not {drop!r}")


def _beats(wave: np.ndarray, tempo) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    """(magnitude spectrogram, onset envelope, beat times)."""
    import librosa
    spec_mag = np.abs(librosa.stft(wave, n_fft=N_FFT, hop_length=HOP))
    onset_env = librosa.onset.onset_strength(S=librosa.amplitude_to_db(spec_mag, ref=np.max), sr=SR, hop_length=HOP)
    _, bt = librosa.beat.beat_track(onset_envelope=onset_env, sr=SR, hop_length=HOP,
                                    start_bpm=tempo or 120.0, tightness=400)
    beat_t = librosa.frames_to_time(bt, sr=SR, hop_length=HOP)
    if len(beat_t) < 8:
        raise ArrangeError(f"only {len(beat_t)} beats tracked: not a steady track, or too short")
    return spec_mag, onset_env, beat_t


def _grid(wave: np.ndarray, beat_t: np.ndarray, cli, dur: float) -> dict:
    """The fitted beat grid: g (fit_grid), t0, T, shift, ratio and the grid times."""
    gd = fit_grid(beat_t, cli.bpm)
    origin, period = gd["t0"], gd["T"]
    shift, ratio = 0.0, 1.0
    if not cli.no_snap:
        shift, ratio, origin = snap(wave, origin, period, dur)
    start = origin % period
    if start - period >= -0.03:
        start -= period  # a beat a few ms before 0 is the track's first beat, not a lost one
    return {"g": gd, "t0": origin, "T": period, "shift": shift, "ratio": ratio,
            "grid": np.maximum(0.0, np.arange(start, dur, period))}


def _phase(scores: np.ndarray, per_bar: int, phase_arg: str) -> tuple[int, int, float, bool]:
    """(downbeat phase, alternative phase, confidence, low confidence)."""
    order = list(np.argsort(-scores))
    auto = int(order[0])
    second = int(order[1]) if per_bar > 1 else auto
    conf = float(scores[order[0]] - scores[order[1]]) if per_bar > 1 else 99.0
    if phase_arg == "auto":
        return auto, second, conf, conf < 0.5
    ph = int(phase_arg)
    if not 0 <= ph < per_bar:
        raise ArrangeError(f"--phase must be auto or 0..{per_bar - 1}")
    return ph, (auto if auto != ph else second), conf, False


def _bar_times(beat_grid: np.ndarray, ph: int, per_bar: int, bar_len: float, dur: float) -> list[float]:
    bars_t = [float(beat_grid[ix]) for ix in range(ph, len(beat_grid), per_bar) if beat_grid[ix] + bar_len <= dur + 0.001]
    if len(bars_t) < 2:
        raise ArrangeError(f"only {len(bars_t)} whole bar(s) in the track: need at least 2")
    return bars_t


def _diffs(val: np.ndarray, fn) -> np.ndarray:
    return np.array([0.0] + [float(fn(val[ix], val[ix - 1])) for ix in range(1, len(val))])


def _film(cli, meta: dict, curve_) -> tuple[int, int, list]:
    """(film bars, drop film bar, arc)."""
    K = len(curve_) if curve_ else int(cli.bars)
    drop_film = int(cli.drop_film_bar or meta.get("drop_film_bar") or round(0.7 * K))
    if not 1 <= drop_film <= K:
        raise ArrangeError(f"the drop film bar {drop_film} is outside 1..{K}")
    return K, drop_film, (default_arc(K, drop_film) if curve_ is None else curve_)


def _drop_bar(drop: str, cands: list):
    if drop == "auto":
        return cands[0]["bar"] if cands else None
    return None if drop == "none" else int(drop)


def _check_source_bars(nb: int, pairs) -> None:
    for name, val in pairs:
        if val is not None and not 0 <= val < nb:
            raise ArrangeError(f"{name} {val} is outside the source bars 0..{nb - 1}")


def _runs(route: list[int], K: int) -> list[tuple[int, int]]:
    """(first, last) film-bar index of each run of consecutive source bars."""
    runs, ki = [], 0
    while ki < K:
        end_i = ki
        while end_i + 1 < K and route[end_i + 1] == route[end_i] + 1:
            end_i += 1
        runs.append((ki, end_i))
        ki = end_i + 1
    return runs


def _segments(route: list[int], K: int, bars_t: list[float], per_bar: int, period: float, at0: float,
              film_bar_len: float) -> list[dict]:
    spans = []
    for ki, end_i in _runs(route, K):
        src_from = bars_t[route[ki]]
        spans.append({"src_from": round(src_from, 4), "src_to": round(src_from + (end_i - ki + 1) * per_bar * period, 4),
                     "at": round(at0 + ki * film_bar_len, 4),
                     "src_bars": [route[ki], route[end_i]], "film_bars": [ki + 1, end_i + 1]})
    return spans


def _music(file: str, spans: list, tempo: float, film_bpm: float) -> dict:
    none = abs(film_bpm / tempo - 1) < 0.002
    return {"file": file, "src_bpm": round(film_bpm if none else tempo, 3), "target_bpm": film_bpm,
            "stretch": "none" if none else "rubberband", "gain_db": 0,
            "segments": [{"label": f"source bars {sv['src_bars'][0]}-{sv['src_bars'][1]} -> film bars "
                                   f"{sv['film_bars'][0]}-{sv['film_bars'][1]}",
                          "src_from": sv["src_from"], "src_to": sv["src_to"], "at": sv["at"],
                          "fade_in": 0.008, "fade_out": 0.06} for sv in spans]}


def _grid_at(beat_grid: np.ndarray, ix: int, fallback: float) -> float:
    return float(beat_grid[ix]) if ix < len(beat_grid) else fallback


def _bar_rows(bars_t: list[float], table: dict) -> list[dict]:
    return [{"i": ix, "t": round(bars_t[ix], 4), "rel_db": round(float(table["rel"][ix]), 2),
             "novelty": round(float(table["nov"][ix]), 3), "rise_db": round(float(table["rise"][ix]), 2),
             "density": round(float(table["dens"][ix]), 3)} for ix in range(len(bars_t))]


def _bar_table(wave, spec_mag, chroma, onset_env, bars_t: list[float], bar_len: float) -> dict:
    """Per source bar: features, loudness, density, level under the top, novelty and rise."""
    feat, loud, dens = bar_features(wave, spec_mag, chroma, onset_env, bars_t, bar_len)
    return {"feat": feat, "loud": loud, "dens": dens, "rel": loud - np.percentile(loud, 90),
            "nov": _diffs(feat, lambda u, wv: np.linalg.norm(u - wv)), "rise": _diffs(loud, lambda u, wv: u - wv)}


def _analyse_track(cli, per_bar: int) -> dict:
    """Decode, beat-track, fit the grid, read the downbeat and the bar table."""
    import librosa
    wave = decode(cli.track)
    dur = len(wave) / SR
    spec_mag, onset_env, beat_t = _beats(wave, cli.bpm)
    val = _grid(wave, beat_t, cli, dur)
    beat_grid, period = val["grid"], val["T"]
    chroma = librosa.feature.chroma_stft(S=spec_mag ** 2, sr=SR)
    scores, z = downbeat(spec_mag, chroma, beat_grid, period, per_bar)
    ph, alt, conf, low_conf = _phase(scores, per_bar, cli.phase)
    bars_t = _bar_times(beat_grid, ph, per_bar, per_bar * period, dur)
    val.update(dur=dur, scores=scores, phase=ph, alt=alt, conf=conf, low_conf=low_conf, bars_t=bars_t,
             disagree=local_disagreements(z, beat_grid, per_bar, ph),
             table=_bar_table(wave, spec_mag, chroma, onset_env, bars_t, per_bar * period))
    return val


def arrange(cli) -> dict:
    _check_drop_arg(cli.drop)
    curve_, meta = read_arc(cli.arc)
    film_bpm = float(meta.get("film_bpm", cli.film_bpm))
    per_bar = int(meta.get("beats_per_bar", cli.beats_per_bar))
    val = _analyse_track(cli, per_bar)
    tb, bars_t, period = val["table"], val["bars_t"], val["T"]
    nb = len(bars_t)
    cands = drop_candidates(tb["rel"], tb["nov"])
    K, drop_film, curve_ = _film(cli, meta, curve_)
    drop_bar = _drop_bar(cli.drop, cands)
    _check_source_bars(nb, (("--drop", drop_bar), ("--start-bar", cli.start_bar), ("--end-bar", cli.end_bar)))
    opt = {"jump": cli.jump, "phrase": max(1, cli.phrase), "drop_k": drop_film - 1, "drop_bar": drop_bar,
           "start_bar": cli.start_bar, "end_bar": cli.end_bar}
    route, total = walk(tb["feat"], tb["loud"], tb["rel"], curve_, opt)
    spans = _segments(route, K, bars_t, per_bar, period, cli.film_offset, per_bar * 60.0 / film_bpm)
    tempo = 60.0 / period
    gd, origin = val["g"], val["t0"]
    fit = {"track": cli.track, "duration": round(val["dur"], 3), "bpm": round(tempo, 3), "beat_period": round(period, 6),
           "film_bpm": film_bpm, "beats_per_bar": per_bar, "film_bars": K, "drop_film_bar": drop_film, "arc": curve_,
           "stretch_ratio_to_film": round(film_bpm / tempo, 5), "t0": round(origin, 4),
           "transient_shift_ms": round(val["shift"] * 1000, 1), "transient_fit_ratio": round(val["ratio"], 3),
           "grid_median_residual_ms": round(gd["median_residual_ms"], 2),
           "grid_inlier_frac": round(gd["inlier_frac"], 3), "downbeat_phase": val["phase"],
           "phase_scores": [round(float(sv), 3) for sv in val["scores"]], "phase_confidence": round(val["conf"], 3),
           "phase_low_confidence": bool(val["low_conf"]),
           "phase_alternative": {"phase": val["alt"], "first_downbeat": round(_grid_at(val["grid"], val["alt"], origin), 4)},
           "local_phase_disagreements": val["disagree"], "drop_source_bar": drop_bar,
           "bars": _bar_rows(bars_t, tb), "drop_candidates": cands, "map": route, "segments": spans,
           "music": _music(cli.file or cli.track, spans, tempo, film_bpm), "total_cost": round(total, 3)}
    fit["summary"] = summary(fit, _grid_at(val["grid"], val["phase"], origin), cli.phase)
    return fit


def summary(rep: dict, first_down: float, phase_arg: str) -> str:
    bar = rep["beat_period"] * rep["beats_per_bar"]
    sv = (f"{rep['bpm']:.2f} BPM (grid residual {rep['grid_median_residual_ms']:.1f} ms, "
         f"{100 * rep['grid_inlier_frac']:.0f}% inliers); downbeats at {first_down:.3f} + {bar:.3f}*n s "
         f"(phase {rep['downbeat_phase']}, confidence {rep['phase_confidence']:.2f}")
    alt = rep["phase_alternative"]
    if rep["phase_low_confidence"]:
        sv += (f", LOW: the other grid is {alt['first_downbeat']:.3f} + {bar:.3f}*n s, select it with "
              f"--phase {alt['phase']}")
    elif phase_arg != "auto":
        sv += f", the automatic choice was phase {alt['phase']}"
    sv += (f"); {len(rep['bars'])} source bars, {rep['film_bars']} film bars, {len(rep['segments'])} segment(s); "
          f"drop on film bar {rep['drop_film_bar']} from "
          + (f"source bar {rep['drop_source_bar']}" if rep["drop_source_bar"] is not None else "the bar the arc chose")
          + f"; stretch ratio {rep['stretch_ratio_to_film']:.4f}")
    if rep["local_phase_disagreements"]:
        sv += "; local phase disagreements: " + ", ".join(
            f"{dd['from']:.1f}-{dd['to']:.1f} s phase {dd['phase']}" for dd in rep["local_phase_disagreements"][:4])
    return sv


def print_text(rep: dict, dest: str | None):
    say(rep["summary"])
    say(f"transient shift {rep['transient_shift_ms']} ms, fit ratio {rep['transient_fit_ratio']}")
    say("phase scores: " + " ".join(f"{pp}:{val}" for pp, val in enumerate(rep["phase_scores"])))
    say("drop candidates: " + ", ".join(f"bar {cv['bar']} ({cv['score']})" for cv in rep["drop_candidates"]))
    if rep["phase_low_confidence"]:
        say("LOW downbeat confidence: look where the first note falls, where sections enter and where chords "
            f"change, then rerun with --phase {rep['downbeat_phase']} or --phase {rep['phase_alternative']['phase']}")
    top = {cv["bar"] for cv in rep["drop_candidates"][:3]}
    say("bar      t  rel_dB    nov  rise  dens")
    for bv in rep["bars"]:
        mark = "  <- drop candidate" if bv["i"] in top else ""
        say(f"{bv['i']:3d} {bv['t']:7.3f} {bv['rel_db']:7.2f} {bv['novelty']:6.3f} {bv['rise_db']:5.1f} "
            f"{bv['density']:5.2f}{mark}")
    say("map film:source " + " ".join(f"{ki + 1}:{sv}" for ki, sv in enumerate(rep["map"])))
    say(json.dumps({"music": rep["music"]}, indent=1))
    if dest:
        say("wrote", dest)


def parse(argv):
    ap = argparse.ArgumentParser(prog="arrange.py", description="Fit a track's bar grid and draft a bar-line edit.")
    ap.add_argument("track", nargs="?", metavar="TRACK")
    ap.add_argument("--bpm", type=float)
    ap.add_argument("--phase", default="auto")
    ap.add_argument("--film-bpm", type=float, default=120.0)
    ap.add_argument("--beats-per-bar", type=int, default=4)
    ap.add_argument("--bars", type=int, default=30)
    ap.add_argument("--arc")
    ap.add_argument("--drop-film-bar", type=int)
    ap.add_argument("--drop", default="auto")
    ap.add_argument("--start-bar", type=int)
    ap.add_argument("--end-bar", type=int)
    ap.add_argument("--phrase", type=int, default=2)
    ap.add_argument("--film-offset", type=float, default=0.0)
    ap.add_argument("--jump", type=float, default=3.0)
    ap.add_argument("--no-snap", action="store_true")
    ap.add_argument("--file")
    ap.add_argument("--out")
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--self-test", action="store_true")
    return ap.parse_args(argv)


def _write_out(dest: str | None, rep: dict) -> None:
    if not dest:
        return
    os.makedirs(os.path.dirname(os.path.abspath(dest)), exist_ok=True)
    with open(dest, "w", encoding="utf-8") as fh:
        json.dump(rep, fh, indent=1)


def _run(cli) -> int:
    try:
        rep = arrange(cli)
    except ArrangeError as end_i:
        say(f"arrange.py: {end_i}", file=sys.stderr)
        return 1
    _write_out(cli.out, rep)
    if cli.json:
        say(json.dumps(rep, indent=1))
    else:
        print_text(rep, cli.out)
    return 0


def main(argv=None) -> int:
    cli = parse(sys.argv[1:] if argv is None else argv)
    if cli.self_test:
        return self_test()
    if not cli.track:
        say("arrange.py: give a TRACK", file=sys.stderr)
        return 2
    return _run(cli)


def _quiet(argv, err=False):
    """main(argv) with stdout (or stderr) captured. Returns (code, text)."""
    import contextlib
    import io
    buf = io.StringIO()
    with (contextlib.redirect_stderr(buf) if err else contextlib.redirect_stdout(buf)):
        code = main(argv)
    return code, buf.getvalue()


def _test_grid(ck, song: str, dest: str) -> None:
    code, _ = _quiet([song, "--bars", "8", "--out", dest])
    rep = json.load(open(dest))
    ck("S11 bpm 120 +-0.05", code == 0 and abs(rep["bpm"] - 120) <= 0.05)
    down = rep["bars"][0]["t"]
    ck("S11 the downbeat phase is the accent", abs(((down + 0.25) % 2.0) - 0.25) < 0.03)
    ck("S11 stretch none for film 120", rep["music"]["stretch"] == "none" and rep["music"]["src_bpm"] == 120)
    bar_times = [bv["t"] for bv in rep["bars"]]
    ck("S11 segments start on bar times", all(any(abs(sv["src_from"] - tv) < 1e-3 for tv in bar_times)
                                              for sv in rep["segments"]))
    drop = rep["map"][rep["drop_film_bar"] - 1]
    ck("S11 the drop bar is in the louder half", rep["bars"][drop]["t"] >= 19.5)


def _test_errors(ck, song: str, tmp: str) -> None:
    code, err = _quiet([song, "--bars", "8", "--drop", "99"], err=True)
    ck("S12 --drop 99 names the range 0..19", code == 1 and "0..19" in err)
    ck("bad --drop word", _quiet([song, "--drop", "loud"], err=True)[0] == 1)
    ck("missing track", _quiet([os.path.join(tmp, "none.wav")], err=True)[0] == 1)


def self_test() -> int:
    import tempfile
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    import fixtures as fx
    ck = fx.Checker()
    with tempfile.TemporaryDirectory() as tmp:
        song = fx.write(os.path.join(tmp, "song.wav"), fx.click_track(120, 20, louder_from=10))
        dest = os.path.join(tmp, "arr.json")
        _test_grid(ck, song, dest)
        _test_errors(ck, song, tmp)
        ck("default arc shape", default_arc(15, 9)[8] == 0.0 and default_arc(15, 9)[0] == -14.0)
        arcf = fx.put_json(os.path.join(tmp, "arc.json"), {"film_bpm": 120, "beats_per_bar": 4, "drop_film_bar": 3,
                                                         "arc": [-14, -10, 0, 0, -9]})
        code, _ = _quiet([song, "--arc", arcf, "--out", dest])
        rep = json.load(open(dest))
        ck("arc file: 5 film bars, drop on film bar 3", code == 0 and rep["film_bars"] == 5 and rep["drop_film_bar"] == 3)
    return ck.code()


if __name__ == "__main__":
    sys.exit(main())
