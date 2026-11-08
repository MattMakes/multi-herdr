"""realness: a real recording or professional production, or cheap audio synthesized in code.

p_real comes from a model file: a logistic head on the CLAP embedding, a logistic head on 13 signal
measurements, and a fusion layer. Signal measurements also give evidence in words, and one rule decides
pure dead-steady tones. Without a trained model, hark ships the zero-shot fallback: the CLAP head
compares the sound with 6 real-side and 6 synth-side prompts.

Run ``python -m hark.listen.realness --train <labelled dir>`` to train a model file (see ``train_model``).
"""

from __future__ import annotations

import argparse
import json
import math
import os
import sys
from collections.abc import Mapping, Sequence
from pathlib import Path
from typing import Any

import numpy as np

from hark.core import audio, report
from hark.listen import vocabulary as voc

SR = 48000
ANALYSIS_SR = 32000
MODEL_PATH = Path(__file__).resolve().parent.parent / "data" / "listen" / "realness_model.json"
CACHE_VERSION = 1
LONG_S = 6.0
WIN_S = 4.0
DROP_LAST_S = 0.25
SILENT_DB = -60.0
LIFT_RMS_DB = -30.0
LIFT_PEAK_DB = -1.0
LIFT_FLOOR_DB = -80.0
GAIN_LINE_DB = 3.0
MIN_CLIP_S = 0.02
MIN_PEAK = 1e-7
ZERO_SAMPLE = 1e-6
BAND = (40.0, 15000.0)
PURE_CAP = 0.03
LEAN = 0.5
SILENCE_LINE = 0.02
DIGITAL_ZERO_DB = -199.0
VERDICTS = ("real or pro-produced", "uncertain", "likely code-synthesized", "silent", "error")
REAL, UNCERTAIN, SYNTH, SILENT, ERROR = VERDICTS
SIGNAL_INPUTS = ("silence_frac", "floor_db", "spec_floor_db", "log_flatness", "peakiness_db", "line_share",
                 "bandwidth_khz", "quiet_flat", "tonal_frames", "pitch_wobble", "env_resid_db", "texture_kurt",
                 "crest_db")
POSITIVE = ("floor_db", "spec_floor_db", "log_flatness", "bandwidth_khz", "quiet_flat", "pitch_wobble",
            "env_resid_db", "texture_kurt", "crest_db")
REAL_PROMPTS = ("a real recording made with a microphone", "a field recording of a real place",
                "a real foley recording of an object", "a professionally produced music track",
                "a professional cinematic sound effect from a sound library", "a natural acoustic sound")
SYNTH_PROMPTS = ("a cheap synthesized beep", "a pure sine wave test tone",
                 "a simple electronic synthesizer tone made by code", "an 8-bit video game sound effect",
                 "filtered white noise", "a basic drum machine loop")
SYNTH_CLASSES = ("Synthesizer", "Sine wave", "Beep, bleep", "Electronic tuner", "Chirp tone", "Dial tone",
                 "Pink noise", "White noise", "Static", "Buzz", "Hum", "Theremin", "Video game music", "Ringtone",
                 "Sound effect")
ZERO_SHOT_SCALE = 38.7


class ModelMissing(FileNotFoundError):
    """The realness model file is absent."""


# ================================================================ model


def load_model(path: str | Path | None = None, resolve: bool = True) -> dict:
    """The realness model file as a dict. A zero-shot model gets its prompt embeddings when ``resolve``."""
    p = Path(path) if path else MODEL_PATH
    if not p.exists():
        raise ModelMissing(f"realness model file {p} is missing; train one with "
                           "python -m hark.listen.realness --train <labelled dir>")
    model = json.loads(p.read_text(encoding="utf-8"))
    if resolve:
        resolve_head(model)
    return model


def is_zero_shot(model: Mapping) -> bool:
    """True for the zero-shot fallback model."""
    return model["heads"]["clap"].get("kind") == "zero_shot"


def resolve_head(model: dict) -> dict:
    """Add the prompt embeddings a zero-shot CLAP head needs (embeds 12 plain texts once, cached)."""
    head = model["heads"]["clap"]
    if head.get("kind") == "zero_shot" and "real" not in head:
        head["real"] = voc.embed_plain_text(model["prompts"]["real"]).astype(np.float64)
        head["synth"] = voc.embed_plain_text(model["prompts"]["synth"]).astype(np.float64)
    return model


def thresholds_of(model: Mapping, file: bool = False) -> dict:
    """{lo, hi} of the clip thresholds, or of the file thresholds."""
    t = model["file_thresholds"] if file else model["thresholds"]
    return {"lo": float(t["lo"]), "hi": float(t["hi"])}


def _lse(v: np.ndarray) -> float:
    m = float(np.max(v))
    return m + math.log(float(np.sum(np.exp(v - m))))


def head_logit(head: Mapping, v: Sequence[float] | np.ndarray) -> float:
    """The logit of one head: fill NaN, clip, standardise, dot with the coefficients (zero-shot: prompt LSE)."""
    x = np.asarray(v, np.float64)
    if head.get("kind") == "zero_shot":
        s = float(head.get("logit_scale", ZERO_SHOT_SCALE))
        return _lse(s * (np.asarray(head["real"]) @ x)) - _lse(s * (np.asarray(head["synth"]) @ x))
    mean = np.asarray(head["mean"], np.float64)
    x = np.where(np.isfinite(x), x, mean)
    x = np.clip(x, np.asarray(head["clip"][0], np.float64), np.asarray(head["clip"][1], np.float64))
    z = (x - mean) / np.asarray(head["scale"], np.float64)
    return float(z @ np.asarray(head["coef"], np.float64) + float(head["intercept"]))


def sigmoid(s: float) -> float:
    """The logistic function."""
    return 1.0 / (1.0 + math.exp(-max(-500.0, min(500.0, s))))


def fuse(model: Mapping, logits: Mapping[str, float] | Sequence[float]) -> float:
    """p_real from the head logits (a mapping by head name, or a sequence in fusion order)."""
    f = model["fusion"]
    vals = [logits[h] for h in f["heads"]] if isinstance(logits, Mapping) else list(logits)
    return sigmoid(float(f["intercept"]) + sum(float(w) * float(v) for w, v in zip(f["coef"], vals)))


def verdict_of(p: float, th: Mapping[str, float]) -> str:
    """real or pro-produced at or above hi, likely code-synthesized at or below lo, else uncertain (R11)."""
    if p >= th["hi"]:
        return REAL
    return SYNTH if p <= th["lo"] else UNCERTAIN


# ================================================================ signal measurements (R3)


def _frames(y: np.ndarray, n: int, hop: int | None = None) -> np.ndarray:
    hop = hop or n
    if len(y) < n:
        return np.pad(y, (0, n - len(y)))[None, :]
    k = 1 + (len(y) - n) // hop
    idx = np.arange(n)[None, :] + hop * np.arange(k)[:, None]
    return y[idx]


def _floor_db(y: np.ndarray) -> float:
    r = 20 * np.log10(np.sqrt((_frames(y, 640) ** 2).mean(axis=1) + 1e-30))
    return float(max(-140.0, np.percentile(r, 10) - r.max()))


def _welch(y: np.ndarray, out: dict) -> None:
    from scipy import signal as sps

    seg = min(2048, len(y))
    f, P = sps.welch(y, fs=ANALYSIS_SR, window="hann", nperseg=seg, noverlap=seg // 2, scaling="density")
    keep = (f >= BAND[0]) & (f <= BAND[1])
    f, P = f[keep], P[keep] + 1e-30
    out["log_flatness"] = float(np.log10(np.exp(np.mean(np.log(P))) / P.mean()))
    out["peakiness_db"] = float(10 * np.log10(P.max() / np.median(P)))
    k = max(1, int(math.floor(0.01 * len(P))))
    out["line_share"] = float(np.sort(P)[-k:].sum() / P.sum())
    db = 10 * np.log10(P)
    out["bandwidth_khz"] = float(f[np.nonzero(db >= db.max() - 60)[0][-1]] / 1000)


def _stft(y: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    fr = _frames(y, 1024, 256) * np.hanning(1024)[None, :]
    P = np.abs(np.fft.rfft(fr, axis=1)) ** 2 + 1e-30
    f = np.fft.rfftfreq(1024, 1 / ANALYSIS_SR)
    keep = (f >= BAND[0]) & (f <= BAND[1])
    return f[keep], P[:, keep]


def _quiet_flat(P: np.ndarray, e: np.ndarray, active: np.ndarray) -> float:
    ae = e[active]
    quiet = active & (e <= np.percentile(ae, 25)) if active.sum() >= 8 else active
    qs = P[quiet].mean(axis=0) if quiet.any() else P.mean(axis=0)
    return float(np.log10(np.exp(np.mean(np.log(qs))) / qs.mean()))


def _peaks(P: np.ndarray) -> np.ndarray:
    inner = P[:, 2:-2]
    return np.argmax(inner, axis=1) + 2


def _wobble(f: np.ndarray, P: np.ndarray, tonal: np.ndarray, pk: np.ndarray) -> float:
    trip = [i for i in range(len(tonal) - 2) if tonal[i] and tonal[i + 1] and tonal[i + 2]]
    if len(trip) < 4:
        return float("nan")
    L = np.log(P)
    rows = np.arange(len(P))
    a, b, c = L[rows, pk - 1], L[rows, pk], L[rows, pk + 1]
    freq = f[pk] + 0.5 * (a - c) / (a - 2 * b + c - 1e-12) * (ANALYSIS_SR / 1024)
    cents = 1200 * np.log2(np.maximum(freq, 20.0))
    d2 = [abs(cents[i + 2] - 2 * cents[i + 1] + cents[i]) for i in trip]
    return float(np.log10(np.median(d2) + 0.01))


def _spectral_frames(y: np.ndarray, out: dict) -> None:
    f, P = _stft(y)
    e = 10 * np.log10(P.sum(axis=1))
    dbP = 10 * np.log10(P)
    loud = e > e.max() - 30
    out["spec_floor_db"] = float(np.median(np.percentile(dbP[loud], 10, axis=1) - dbP[loud].max(axis=1)))
    active = e > e.max() - 50
    out["quiet_flat"] = _quiet_flat(P, e, active)
    pk = _peaks(P)
    rows = np.arange(len(P))
    lobe = sum(P[rows, pk + d] for d in range(-2, 3))
    tonal = active & (lobe / P.sum(axis=1) > 0.5)
    out["tonal_frames"] = float(tonal.sum() / max(1, active.sum()))
    out["pitch_wobble"] = _wobble(f, P, tonal, pk)


def _env_resid(y: np.ndarray) -> float:
    from scipy import signal as sps

    e = 10 * np.log10((_frames(y, 160) ** 2).mean(axis=1) + 1e-30)
    counted = e >= e.max() - 40
    if counted.sum() < 9:
        return float("nan")
    trend = sps.medfilt(e, 7)
    return float(np.log10(np.median(np.abs(e - trend)[counted]) + 0.01))


def _texture_kurt(y: np.ndarray) -> float:
    from scipy import signal as sps

    sos = sps.butter(4, [2000, 8000], "bandpass", fs=ANALYSIS_SR, output="sos")
    band = sps.sosfilt(sos, y)
    env = np.sqrt(np.convolve(band ** 2, np.ones(320) / 320, mode="same")) + 1e-9
    mask = (band != 0) & (env >= env.max() * 10 ** (-40 / 20))
    if mask.sum() <= 100:
        return float("nan")
    s = band[mask] / env[mask]
    return float(np.log(np.mean(s ** 4) / (np.mean(s ** 2) ** 2 + 1e-30) + 1e-9))


def _crest(y: np.ndarray) -> float:
    nz = y[y != 0]
    return float(20 * np.log10(1 / (np.sqrt(np.mean(nz ** 2)) + 1e-12))) if len(nz) else 0.0


def signal_features(x: np.ndarray) -> dict | None:
    """The 13 signal measurements of a mono 48 kHz clip, or None when it cannot be judged (R3)."""
    x = audio.to_mono(np.asarray(x, np.float64)) if np.ndim(x) == 2 else np.asarray(x, np.float64)
    if len(x) < MIN_CLIP_S * SR or np.max(np.abs(x)) < MIN_PEAK:
        return None
    out: dict[str, float] = {"silence_frac": float(np.mean(np.abs(x) < ZERO_SAMPLE))}
    pre = x / np.max(np.abs(x))  # level-free before the float32 resampler, so level never matters
    y = np.asarray(audio.resample(pre.astype(np.float32), SR, ANALYSIS_SR), np.float64)
    y = y / (np.max(np.abs(y)) + 1e-12)
    out["floor_db"] = _floor_db(y)
    _welch(y, out)
    _spectral_frames(y, out)
    out["env_resid_db"] = _env_resid(y)
    out["texture_kurt"] = _texture_kurt(y)
    out["crest_db"] = _crest(y)
    return {k: out[k] for k in SIGNAL_INPUTS}


def signal_vector(feats: Mapping[str, float]) -> np.ndarray:
    """The signal head input vector in model order."""
    return np.array([feats.get(k, float("nan")) for k in SIGNAL_INPUTS], np.float64)


def rms_db(x: np.ndarray) -> float:
    """RMS in dB (-200 for empty input)."""
    x = np.asarray(x, np.float64)
    return float(20 * np.log10(max(np.sqrt(np.mean(x ** 2)), 1e-10))) if x.size else -200.0


def audible(x: np.ndarray) -> bool:
    """True when the clip's RMS is above -60 dBFS and it has signal measurements."""
    return rms_db(x) > SILENT_DB and signal_features(x) is not None


def pure_tone(x_or_feats: np.ndarray | Mapping[str, float] | None) -> bool:
    """The pure dead-steady tone rule (R5); takes a clip or its signal measurements."""
    f = x_or_feats if isinstance(x_or_feats, Mapping) or x_or_feats is None else signal_features(x_or_feats)
    if not f:
        return False
    wob = f.get("pitch_wobble", float("nan"))
    return (f["tonal_frames"] >= 0.95 and f["line_share"] >= 0.9 and f["crest_db"] <= 4.5
            and (not np.isfinite(wob) or wob <= -1.0))


# ================================================================ lift and clips


def lift_gain(x: np.ndarray) -> float:
    """The quiet-source gain in dB (R1); 0 when no lift applies."""
    x = np.asarray(x, np.float64)
    if not x.size:
        return 0.0
    r, p = rms_db(x), float(20 * np.log10(max(float(np.max(np.abs(x))), 1e-10)))
    g = min(LIFT_RMS_DB - r, LIFT_PEAK_DB - p)
    return 0.0 if p < LIFT_FLOOR_DB or g <= 0 else float(g)


def clip_starts(n: int) -> list[int]:
    """Clip start samples of an n-sample sound (R2); one clip up to 6 s."""
    w = int(WIN_S * SR)
    if n <= int(LONG_S * SR):
        return [0]
    starts = list(range(0, n - w + 1, w))
    if starts[-1] + w != n:
        starts.append(n - w)
    if len(starts) > 2 and (starts[-1] - starts[-2]) < DROP_LAST_S * SR:
        starts.pop()
    return starts


def clips_of(x: np.ndarray) -> list[tuple[float, float, np.ndarray]]:
    """(local start s, local end s, samples) of every clip."""
    n = len(x)
    if n <= int(LONG_S * SR):
        return [(0.0, n / SR, x)]
    w = int(WIN_S * SR)
    return [(s / SR, (s + w) / SR, x[s:s + w]) for s in clip_starts(n)]


# ================================================================ facts and words


def pct(v: float) -> str:
    """A percentage: 1 decimal strictly between 0 and 9.5 %, else an integer."""
    p = 100 * v
    return f"{p:.1f}%" if 0 < p < 9.5 else f"{int(round(p))}%"


def fact_phrase(feature: str, side: str, v: float) -> str:
    """A fired fact in words."""
    real = side == "real"
    words = {
        "floor_db": (f"a noise floor like a microphone or room, {abs(v):.0f} dB under the peak" if real else
                     "quietest parts very far under the peak: cleaner than a microphone records"),
        "spec_floor_db": (f"a broadband noise floor {abs(v):.0f} dB under the spectral peak" if real else
                          "no measurable noise between the partials"),
        "line_share": "energy spread over the spectrum" if real else f"{pct(v)} of the energy in a few pure lines",
        "pitch_wobble": "natural pitch drift" if real else "dead-steady pitch with no natural drift",
        "env_resid_db": ("micro-fluctuations of a recorded source" if real else
                         "a mathematically smooth envelope (exponential or linear ramps)"),
        "texture_kurt": ("a crackly, impulsive high-frequency texture" if real else
                         "Gaussian noise like filtered random numbers"),
        "quiet_flat": ("quiet parts are broadband room or mic noise" if real else "quiet parts are tonal tails"),
        "tonal_frames": "few pure-tone frames" if real else f"{pct(v)} of frames ruled by one pure tone",
        "log_flatness": "a dense, noise-rich spectrum" if real else "a few pure lines over near-silence",
        "peakiness_db": "no dominant spectral line" if real else f"one line {v:.0f} dB above the median bin",
        "bandwidth_khz": f"content reaches {v:.1f} kHz",
        "crest_db": (f"crest {v:.0f} dB: sharp natural transients" if real else
                     f"crest only {v:.0f} dB: a steady, oscillator-like level"),
    }
    return words.get(feature, f"{feature} {v:.3g}")


def fired_facts(model: Mapping, feats: Mapping[str, float]) -> list[dict]:
    """Facts whose test passes, by precision descending, with their phrases (R6)."""
    out = []
    for f in model.get("facts", []):
        v = feats.get(f["feature"], float("nan"))
        if f["feature"] == "silence_frac" or not np.isfinite(v):
            continue
        if (v <= f["value"]) if f["op"] == "<=" else (v >= f["value"]):
            who = "were real/pro" if f["side"] == "real" else "were synthesized"
            text = f"{fact_phrase(f['feature'], f['side'], v)} ({pct(f['precision'])} of training clips past this {who})"
            out.append({"side": f["side"], "precision": f["precision"], "text": text})
    return sorted(out, key=lambda f: -f["precision"])


def silence_line(model: Mapping, v: float) -> str | None:
    """The digital-silence context line (R7), or None."""
    rows = [r for r in model.get("silence_base", []) if r["at_least"] <= v]
    if v < SILENCE_LINE or not rows:
        return None
    r = rows[-1]
    return (f"{pct(v)} of the samples are exact digital silence: weak evidence (edited, gated or synthesized); "
            f"{pct(r['real'])} of real and {pct(r['synth'])} of synthesized training clips had at least "
            f"{pct(r['at_least'])}")


def _pure_fact() -> dict:
    return {"side": "synth", "precision": 0.99, "text": "a pure, dead-steady tone such as a test tone or a bare "
            "oscillator (99% rule)"}


# ================================================================ clip scoring


def _score_clip(model: dict, emb: np.ndarray, feats: dict) -> dict:
    lc = head_logit(model["heads"]["clap"], emb)
    ls = head_logit(model["heads"]["signal"], signal_vector(feats))
    p = fuse(model, {"clap": lc, "signal": ls})
    pure = pure_tone(feats)
    facts = fired_facts(model, feats)
    if pure:
        p = min(p, PURE_CAP)
        facts = [_pure_fact()] + facts
    return {"p": p, "clap": lc, "signal": ls, "pure": pure, "facts": facts, "feats": feats, "emb": emb}


def _prompt_line(model: dict, emb: np.ndarray, side: str) -> str:
    cos = {s: voc.embed_plain_text(model["prompts"][s]).astype(np.float64) @ emb for s in ("real", "synth")}
    best = max(("real", "synth"), key=lambda s: cos[s].max())
    other = "synth" if best == "real" else "real"
    bi, oi = int(np.argmax(cos[best])), int(np.argmax(cos[other]))
    name = {"real": "real", "synth": "synthesized"}
    return (f"closest prompt '{model['prompts'][best][bi]}' ({name[best]}, cosine {cos[best][bi]:.2f}); best "
            f"{name[other]} prompt '{model['prompts'][other][oi]}' (cosine {cos[other][oi]:.2f}); prompt cosines "
            "add context and do not decide")


def clap_line(model: dict, c: dict) -> str:
    """The CLAP evidence line of one scored clip (R8)."""
    toward = "toward real" if c["clap"] >= 0 else "toward synthesized"
    lead = (f"pure-tone rule decided (the CLAP head alone: {c['clap']:+.1f} {toward})" if c["pure"] else
            f"CLAP head score {c['clap']:+.1f} ({toward})")
    return f"{lead}; {_prompt_line(model, c['emb'], 'real' if c['p'] >= LEAN else 'synth')}"


def _ast_line(clip: np.ndarray) -> str:
    from hark.models import ast

    model = ast.get()
    p = np.asarray(model.probs([np.asarray(clip, np.float32)]))[0]
    syn = sorted(((model.prob_of(p, lab), lab) for lab in SYNTH_CLASSES), reverse=True)[:3]
    top = model.top(p, k=3)
    return ("AudioSet synth classes: " + ", ".join(f"{lab} {pp:.2f}" for pp, lab in syn) +
            "; top classes: " + ", ".join(f"{lab} {pp:.2f}" for lab, pp in top))


# ================================================================ realness of one file


def _name(path: str, start: float | None, end: float | None, dur: float) -> str:
    base = os.path.basename(path)
    if start is None and end is None:
        return base
    return f"{base} [{report.fmt_t(start or 0.0)}-{report.fmt_t(end if end is not None else dur)}]"


def check_range(path: str, start: float | None, end: float | None) -> float | None:
    """The realness range checks (R10); returns the probed duration when known."""
    if start is not None and start < 0:
        raise ValueError("start must be >= 0 s")
    if end is not None and end <= (start or 0.0):
        raise ValueError("end must be greater than start")
    try:
        dur = audio.duration(path)
    except audio.AudioError:
        raise
    except Exception:
        return None
    if start is not None and start >= dur:
        raise ValueError(f"start {start:g} s is at or past the end of {path} ({dur:.3f} s long)")
    return dur


def _cache_path(path: str, start: float | None, end: float | None, ast: bool, model: Mapping) -> Path:
    key = voc.key_of("realness", audio.file_id(path), start, end, ast, model.get("version"), model.get("trained"),
                     CACHE_VERSION)
    return voc.cache_dir() / "realness" / f"r_{key}.json"


def _silent_result(path: str, name: str, x: np.ndarray, base: dict) -> dict:
    peak = 20 * np.log10(max(float(np.max(np.abs(x))) if x.size else 0.0, 1e-10))
    why = ("exact digital silence" if peak <= DIGITAL_ZERO_DB else
           f"silent: no part is above -60 dBFS RMS (peak {peak:.1f} dBFS)")
    return {**base, "p_real": None, "verdict": SILENT, "why": why, "evidence": ["nothing to judge: the audio is silent"],
            "summary": f"{name}: silent ({why})."}


def _scored(model: dict, clips: list, ast: bool) -> list[dict]:
    judged = [(a, b, c, f) for a, b, c in clips for f in [signal_features(c)] if f and rms_db(c) > SILENT_DB]
    if not judged:
        return []
    resolve_head(model)
    E = voc.embed_audio([c for _, _, c, _ in judged]).astype(np.float64)
    out = []
    for (a, b, c, f), e in zip(judged, E):
        s = _score_clip(model, e, f)
        s.update({"t0": a, "t1": b})
        if ast:
            s["ast"] = _ast_line(c)
        out.append(s)
    return out


def _evidence(model: dict, rep: dict, long_note: str | None, gain: float, x_stats: tuple[float, float]) -> list[str]:
    side = "real" if rep["p"] >= LEAN else "synth"
    ev = [long_note] if long_note else []
    ev.append(clap_line(model, rep))
    mine = [f["text"] for f in rep["facts"] if f["side"] == side][:3]
    other = [f["text"] for f in rep["facts"] if f["side"] != side][:1]
    ev += mine + [f"but: {t}" for t in other]
    sil = silence_line(model, rep["feats"]["silence_frac"])
    if sil:
        ev.append(sil)
    if rep.get("ast"):
        ev.append(rep["ast"])
    if gain >= GAIN_LINE_DB:
        ev.append(f"quiet source (RMS {x_stats[0]:.1f} dBFS, peak {x_stats[1]:.1f} dBFS) lifted by {gain:.1f} dB "
                  "before judging")
    w = model["fusion"]["coef"]
    if float(w[1]) != 0:
        ev.append(f"fusion: CLAP x {w[0]:.2f}, signal x {w[1]:.2f}; signal head score {rep['signal']:+.1f}")
    elif mine or other or sil:
        ev.append("the measurements are context, not votes: the signal head's fusion weight is 0")
    return ev


def _why_short(rep: dict) -> str:
    toward = "toward real" if rep["clap"] >= 0 else "toward synthesized"
    if rep["pure"]:
        return f"pure-tone rule (the CLAP head alone: {rep['clap']:+.1f})"
    side = "real" if rep["p"] >= LEAN else "synth"
    first = next((f["text"] for f in rep["facts"] if f["side"] == side), None)
    return f"CLAP head score {rep['clap']:+.1f} ({toward})" + (f"; {first}" if first else "")


def p_text(p: float, th: Mapping[str, float]) -> str:
    """p_real with 2 decimals, or 3 when 2 would read as a threshold it is not equal to."""
    two = f"{p:.2f}"
    return f"{p:.3f}" if any(two == f"{t:.2f}" and abs(p - t) > 1e-9 for t in th.values()) else two


def _note(model: Mapping, long: bool) -> str:
    zs = " The model is the zero-shot fallback (CLAP prompts only), so treat p_real as a rough reading." \
        if is_zero_shot(model) else ""
    if long:
        return ("The file p_real is the mean of its 4 s windows. The file thresholds were set on held-out long files "
                "(few synthesized ones, so the calibration is weak); the window calibration comes from held-out 4 s "
                "clips. Windows detect spans that are fully code-synthesized. The score depends on the sound, not "
                "its origin." + zs)
    return ("The calibration of p_real comes from held-out clips (about 92% precision at lo and at hi). The score "
            "depends on the sound, not its origin; a clean professional synth tone can read as synthesized." + zs)


def _segments(model: dict, clips: list, scored: list, s: float, dur: float) -> list[dict]:
    th = thresholds_of(model)
    by_t = {round(c["t0"], 6): c for c in scored}
    out = []
    for a, b, _ in clips:
        rec: dict[str, Any] = {"start": round(s + a, 3), "end": round(min(s + b, s + dur), 3)}
        c = by_t.get(round(a, 6))
        if c is None:
            rec.update(p_real=None, verdict=SILENT)
        else:
            rec.update(p_real=round(c["p"], 3), verdict=verdict_of(c["p"], th))
            side = "real" if c["p"] >= LEAN else "synth"
            first = next((f["text"] for f in c["facts"] if f["side"] == side), None)
            if first:
                rec["why"] = first
        out.append(rec)
    return out


def _merged_spans(segs: Sequence[dict]) -> list[tuple[float, float]]:
    spans: list[list[float]] = []
    for g in segs:
        if spans and g["start"] <= spans[-1][1] + 1e-6:
            spans[-1][1] = max(spans[-1][1], g["end"])
        else:
            spans.append([g["start"], g["end"]])
    return [(a, b) for a, b in spans]


def _span_text(spans: Sequence[tuple[float, float]]) -> str:
    text = ", ".join(f"{report.fmt_t(a)}-{report.fmt_t(b)}" for a, b in spans[:6])
    return text + (", ..." if len(spans) > 6 else "")


def _long_tail(model: dict, segs: list[dict]) -> str:
    lo = thresholds_of(model)["lo"]
    lean = [g for g in segs if g["p_real"] is not None and g["p_real"] < LEAN]
    if not lean:
        pmin = min(g["p_real"] for g in segs if g["p_real"] is not None)
        return (f" No window leans synthesized (lowest window p_real {pmin:.2f}); a quiet code-made layer can "
                "still sit below a louder real mix.")
    low = [g for g in lean if g["p_real"] <= lo]
    text = f" Windows that lean synthesized: {_span_text(_merged_spans(lean))}."
    if low:
        return text + f" {len(low)} at or below the clip lo {lo:.2f}: {_span_text(_merged_spans(low))}."
    return text + f" None at or below the clip lo {lo:.2f}."


def _long_fields(model: dict, clips: list, scored: list, s: float, dur: float) -> tuple[dict, str, str]:
    segs = _segments(model, clips, scored, s, dur)
    lo = thresholds_of(model)["lo"]
    ps = [c["p"] for c in scored]
    lean = sum(p < LEAN for p in ps)
    pure = sum(c["pure"] for c in scored)
    why = (f"{lean} of {len(scored)} windows lean synthesized (p_real < 0.5); mean CLAP head score "
           f"{np.mean([c['clap'] for c in scored]):+.1f}" + (f"; {pure} decided by the pure-tone rule" if pure else ""))
    fields = {"segments": segs, "synth_windows": sum(p <= lo for p in ps), "lean_synth_windows": lean,
              "p_real_min": round(min(ps), 3)}
    return fields, why, _long_tail(model, segs)


def _judge(path: str, start: float | None, end: float | None, ast: bool, model: dict) -> dict:
    x = np.asarray(audio.load(path, mono=True, start=start, end=end), np.float64)
    dur = len(x) / SR
    s = start or 0.0
    name = _name(path, start, end, dur)
    base: dict[str, Any] = {"path": path, "duration": round(dur, 3)}
    if start is not None or end is not None:
        base["range"] = [round(s, 3), round(s + dur, 3)]
    gain = lift_gain(x)
    stats = (rms_db(x), float(20 * np.log10(max(float(np.max(np.abs(x))) if x.size else 0, 1e-10))))
    if gain > 0:
        x = x * 10 ** (gain / 20)
        base["gain_db"] = round(gain, 1)
    clips = clips_of(x)
    scored = _scored(model, clips, ast)
    if not scored:
        return _silent_result(path, name, x, base)
    p = float(np.mean([c["p"] for c in scored]))
    long = len(clips) > 1
    th = thresholds_of(model, file=long)
    rep = max(scored, key=lambda c: c["p"]) if p >= LEAN else min(scored, key=lambda c: c["p"])
    verdict = verdict_of(p, th)
    r = {**base, "p_real": round(p, 3), "verdict": verdict}
    if long:
        fields, why, tail = _long_fields(model, clips, scored, s, dur)
        kind = "real" if p >= LEAN else "synthesized"
        note = (f"evidence from the most {kind}-sounding window {report.fmt_t(s + rep['t0'])}-"
                f"{report.fmt_t(min(s + rep['t1'], s + dur))} (p_real {rep['p']:.2f})")
        r.update(why=why, evidence=_evidence(model, rep, note, gain, stats), **fields)
        r["summary"] = (f"{name}: {verdict} (p_real {p_text(p, th)}, mean of {len(scored)} 4 s windows; file thresholds "
                        f"lo {th['lo']:.2f} / hi {th['hi']:.2f}, weak calibration): {why}." + tail)
    else:
        why = _why_short(rep)
        r.update(why=why, evidence=_evidence(model, rep, None, gain, stats))
        r["summary"] = f"{name}: {verdict} (p_real {p_text(p, th)}; clip thresholds lo {th['lo']:.2f} / hi {th['hi']:.2f}): {why}."
    if is_zero_shot(model):
        r["summary"] += " (zero-shot fallback model)"
    r["note"] = _note(model, long)
    return r


def realness(path: str, start: float | None = None, end: float | None = None, ast: bool = False,
             cache: bool = True) -> dict:
    """Is a sound a real recording or pro production, or cheap code-synthesized audio?"""
    check_range(path, start, end)
    model = load_model(resolve=False)
    cpath = _cache_path(path, start, end, ast, model) if cache else None
    hit = voc.load_json(cpath) if cpath else None
    if isinstance(hit, dict) and "verdict" in hit:
        return {**hit, "path": path}
    r = _judge(path, start, end, ast, model)
    if cpath:
        voc.save_json(cpath, report.clean(r))
    return r


# ================================================================ many files


def _error_row(path: str, why: str) -> dict:
    return {"path": path, "p_real": None, "verdict": ERROR, "why": why[:200],
            "summary": f"{os.path.basename(path) or path}: error: {why[:200]}"}


def bad_path_reason(path: str) -> str | None:
    """Why a given path names no media, or None when it does."""
    if not os.path.exists(path) and not any(ch in path for ch in "*?["):
        return "no such file or folder"
    return None if audio.list_media([path]) else "no audio or video files in it"


def realness_many(paths: Sequence[str], ast: bool = False, start: float | None = None,
                  end: float | None = None) -> dict:
    """realness of every media file under the given paths, with verdict counts."""
    paths = [paths] if isinstance(paths, str) else list(paths)
    files, bad = [], []
    for p in paths:
        reason = bad_path_reason(p)
        if reason:
            bad.append(_error_row(p, reason))
        else:
            files += audio.list_media([p])
    rows = []
    for f in files:
        try:
            rows.append(realness(f, start=start, end=end, ast=ast))
        except Exception as exc:  # one bad file never stops the batch
            rows.append(_error_row(f, f"{type(exc).__name__}: {voc.first_line(exc)}"))
    rows += bad
    counts = {v: sum(r["verdict"] == v for r in rows) for v in VERDICTS}
    return {"files": rows, "counts": counts, "summary": many_summary(len(rows), counts, start, end)}


def many_summary(n: int, counts: Mapping[str, int], start: float | None, end: float | None) -> str:
    """'<N> files: ...' with the verdict counts."""
    parts = [f"{counts[REAL]} real or pro-produced", f"{counts[UNCERTAIN]} uncertain", f"{counts[SYNTH]} likely "
             "code-synthesized"]
    if counts[SILENT]:
        parts.append(f"{counts[SILENT]} silent")
    if counts[ERROR]:
        parts.append(f"{counts[ERROR]} unreadable")
    rng = ""
    if start is not None or end is not None:
        rng = f" (each judged over {start or 0:g}-{'end' if end is None else f'{end:g}'} s)"
    return f"{n} files: " + ", ".join(parts) + rng + "."


# ================================================================ text output


def single_text(r: dict) -> str:
    """Text output of one file."""
    lines = [r["summary"]] + [f"  - {e}" for e in r.get("evidence", [])]
    for g in r.get("segments", []):
        p = "-" if g["p_real"] is None else f"{g['p_real']:.2f}"
        lean = ", leans synthesized" if g["verdict"] == UNCERTAIN and g["p_real"] is not None and g["p_real"] < LEAN else ""
        why = f" ({g['why']})" if g.get("why") and g["verdict"] != REAL else ""
        lines.append(f"  {report.fmt_t(g['start'])}-{report.fmt_t(g['end'])}  {p}  {g['verdict']}{lean}{why}")
    if r.get("note"):
        lines.append(f"  note: {r['note']}")
    return "\n".join(lines)


def table_text(result: dict, model: Mapping | None = None) -> str:
    """Text output of several files."""
    lines = [result["summary"]]
    if model:
        c, f = thresholds_of(model), thresholds_of(model, file=True)
        lines.append(f"-- clip thresholds lo {c['lo']:.2f} / hi {c['hi']:.2f}; file thresholds lo {f['lo']:.2f} / "
                     f"hi {f['hi']:.2f} (weakly calibrated); judge a trim of a multi-take file --")
    lines.append(f"{'p_real':>6}  verdict  file  reason")
    for r in result["files"]:
        p = "-" if r.get("p_real") is None else f"{r['p_real']:.2f}"
        kind = " (file)" if r.get("segments") else ""
        low = f" [{r['synth_windows']} low window(s)]" if r.get("synth_windows") else ""
        lines.append(f"{p:>6}  {r['verdict']}{kind}  {os.path.basename(r['path'])}{low}  {r.get('why', '')}")
    return "\n".join(lines)


# ================================================================ CLI


def _bad_paths(paths: Sequence[str]) -> list[tuple[str, str]]:
    return [(p, r) for p in paths for r in [bad_path_reason(p)] if r]


def _run(args: argparse.Namespace) -> int | None:
    import hark.models

    bad = _bad_paths(args.paths)
    for p, reason in bad:
        report.error_line("realness", f"{p}: {reason}")
    if bad:
        return 1
    hark.models.require_ml("realness")
    files = audio.list_media(list(args.paths))
    try:
        if len(files) == 1:
            r = realness(files[0], start=args.start, end=args.end, ast=args.ast)
            report.emit(r, args, text=single_text(r))
            return 0
        res = realness_many(args.paths, ast=args.ast, start=args.start, end=args.end)
    except (ValueError, audio.AudioError, ModelMissing) as exc:
        report.error_line("realness", str(exc))
        return 1
    report.emit(res, args, text=table_text(res, load_model(resolve=False)))
    return 1 if res["counts"][ERROR] else 0


def add_cli(sub: argparse._SubParsersAction) -> None:
    """Register ``realness``."""
    p = sub.add_parser("realness", help="Real recording or pro production, or cheap code-synthesized audio?")
    p.add_argument("paths", nargs="+", help="files or folders (audio or video)")
    p.add_argument("--start", type=float, default=None)
    p.add_argument("--end", type=float, default=None)
    p.add_argument("--ast", action="store_true", help="also run the AudioSet tagger for evidence (slower)")
    report.add_common(p)
    p.set_defaults(func=_run)


def main(argv: Sequence[str] | None = None) -> int:
    """``python -m hark.listen.realness --train DIR [-o PATH] [--forge-negatives N]``."""
    from hark.listen import realness_train

    return realness_train.main(argv)


if __name__ == "__main__":
    sys.exit(main())
