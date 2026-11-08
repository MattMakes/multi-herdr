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
from collections.abc import Callable, Mapping, Sequence
from pathlib import Path
from typing import Any

import numpy

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


def load_model(fpath: str | Path | None = None, resolve: bool = True) -> dict:
    """The realness model file as a dict. A zero-shot model gets its prompt embeddings when ``resolve``."""
    pth = Path(fpath) if fpath else MODEL_PATH
    if not pth.exists():
        raise ModelMissing(f"realness model file {pth} is missing; train one with "
                           "python -m hark.listen.realness --train <labelled dir>")
    model = json.loads(pth.read_text(encoding="utf-8"))
    if resolve:
        resolve_head(model)
    return model


def is_zero_shot(model: Mapping) -> bool:
    """True for the zero-shot fallback model."""
    return model["heads"]["clap"].get("kind") == "zero_shot"


def resolve_head(model: dict) -> dict:
    """Add the prompt embeddings a zero-shot CLAP head needs (embeds 12 plain texts once, cached)."""
    lead = model["heads"]["clap"]
    if lead.get("kind") == "zero_shot" and "real" not in lead:
        lead["real"] = voc.embed_plain_text(model["prompts"]["real"]).astype(numpy.float64)
        lead["synth"] = voc.embed_plain_text(model["prompts"]["synth"]).astype(numpy.float64)
    return model


def thresholds_of(model: Mapping, file: bool = False) -> dict:
    """{lo, hi} of the clip thresholds, or of the file thresholds."""
    tx = model["file_thresholds"] if file else model["thresholds"]
    return {"lo": float(tx["lo"]), "hi": float(tx["hi"])}


def _lse(vec: numpy.ndarray) -> float:
    mat = float(numpy.max(vec))
    return mat + math.log(float(numpy.sum(numpy.exp(vec - mat))))


def head_logit(lead: Mapping, vec: Sequence[float] | numpy.ndarray) -> float:
    """The logit of one head: fill NaN, clip, standardise, dot with the coefficients (zero-shot: prompt LSE)."""
    sig = numpy.asarray(vec, numpy.float64)
    if lead.get("kind") == "zero_shot":
        sv = float(lead.get("logit_scale", ZERO_SHOT_SCALE))
        return _lse(sv * (numpy.asarray(lead["real"]) @ sig)) - _lse(sv * (numpy.asarray(lead["synth"]) @ sig))
    mean = numpy.asarray(lead["mean"], numpy.float64)
    sig = numpy.where(numpy.isfinite(sig), sig, mean)
    sig = numpy.clip(sig, numpy.asarray(lead["clip"][0], numpy.float64), numpy.asarray(lead["clip"][1], numpy.float64))
    z = (sig - mean) / numpy.asarray(lead["scale"], numpy.float64)
    return float(z @ numpy.asarray(lead["coef"], numpy.float64) + float(lead["intercept"]))


def sigmoid(sv: float) -> float:
    """The logistic function."""
    return 1.0 / (1.0 + math.exp(-max(-500.0, min(500.0, sv))))


def fuse(model: Mapping, logits: Mapping[str, float] | Sequence[float]) -> float:
    """p_real from the head logits (a mapping by head name, or a sequence in fusion order)."""
    fv = model["fusion"]
    vals = [logits[h] for h in fv["heads"]] if isinstance(logits, Mapping) else list(logits)
    return sigmoid(float(fv["intercept"]) + sum(float(win_n) * float(vec) for win_n, vec in zip(fv["coef"], vals)))


def verdict_of(pth: float, th: Mapping[str, float]) -> str:
    """real or pro-produced at or above hi, likely code-synthesized at or below lo, else uncertain (R11)."""
    if pth >= th["hi"]:
        return REAL
    return SYNTH if pth <= th["lo"] else UNCERTAIN


# ================================================================ signal measurements (R3)


def _frames(yv: numpy.ndarray, count: int, hop: int | None = None) -> numpy.ndarray:
    hop = hop or count
    if len(yv) < count:
        return numpy.pad(yv, (0, count - len(yv)))[None, :]
    kk = 1 + (len(yv) - count) // hop
    idx = numpy.arange(count)[None, :] + hop * numpy.arange(kk)[:, None]
    return yv[idx]


def _floor_db(yv: numpy.ndarray) -> float:
    row_ = 20 * numpy.log10(numpy.sqrt((_frames(yv, 640) ** 2).mean(axis=1) + 1e-30))
    return float(max(-140.0, numpy.percentile(row_, 10) - row_.max()))


def _welch_psd(yv: numpy.ndarray) -> tuple[numpy.ndarray, numpy.ndarray]:
    from scipy import signal as sps

    seg = min(2048, len(yv))
    fv, P = sps.welch(yv, fs=ANALYSIS_SR, window="hann", nperseg=seg, noverlap=seg // 2, scaling="density")
    keep = (fv >= BAND[0]) & (fv <= BAND[1])
    return fv[keep], P[keep] + 1e-30


def _line_share(P: numpy.ndarray) -> float:
    kk = max(1, int(math.floor(0.01 * len(P))))
    return float(numpy.sort(P)[-kk:].sum() / P.sum())


def _bandwidth_khz(fv: numpy.ndarray, P: numpy.ndarray) -> float:
    db = 10 * numpy.log10(P)
    return float(fv[numpy.nonzero(db >= db.max() - 60)[0][-1]] / 1000)


def _welch(yv: numpy.ndarray, result: dict) -> None:
    fv, P = _welch_psd(yv)
    result["log_flatness"] = float(numpy.log10(numpy.exp(numpy.mean(numpy.log(P))) / P.mean()))
    result["peakiness_db"] = float(10 * numpy.log10(P.max() / numpy.median(P)))
    result["line_share"] = _line_share(P)
    result["bandwidth_khz"] = _bandwidth_khz(fv, P)


def _stft(yv: numpy.ndarray) -> tuple[numpy.ndarray, numpy.ndarray]:
    fr = _frames(yv, 1024, 256) * numpy.hanning(1024)[None, :]
    P = numpy.abs(numpy.fft.rfft(fr, axis=1)) ** 2 + 1e-30
    fv = numpy.fft.rfftfreq(1024, 1 / ANALYSIS_SR)
    keep = (fv >= BAND[0]) & (fv <= BAND[1])
    return fv[keep], P[:, keep]


def _quiet_flat(P: numpy.ndarray, ex: numpy.ndarray, active: numpy.ndarray) -> float:
    ae = ex[active]
    quiet = active & (ex <= numpy.percentile(ae, 25)) if active.sum() >= 8 else active
    qs = P[quiet].mean(axis=0) if quiet.any() else P.mean(axis=0)
    return float(numpy.log10(numpy.exp(numpy.mean(numpy.log(qs))) / qs.mean()))


def _peaks(P: numpy.ndarray) -> numpy.ndarray:
    inner = P[:, 2:-2]
    return numpy.argmax(inner, axis=1) + 2


def _wobble(fv: numpy.ndarray, P: numpy.ndarray, tonal: numpy.ndarray, pk: numpy.ndarray) -> float:
    trip = [ix for ix in range(len(tonal) - 2) if tonal[ix] and tonal[ix + 1] and tonal[ix + 2]]
    if len(trip) < 4:
        return float("nan")
    L = numpy.log(P)
    records = numpy.arange(len(P))
    lhs, rhs, cand = L[records, pk - 1], L[records, pk], L[records, pk + 1]
    freq = fv[pk] + 0.5 * (lhs - cand) / (lhs - 2 * rhs + cand - 1e-12) * (ANALYSIS_SR / 1024)
    cents = 1200 * numpy.log2(numpy.maximum(freq, 20.0))
    d2 = [abs(cents[ix + 2] - 2 * cents[ix + 1] + cents[ix]) for ix in trip]
    return float(numpy.log10(numpy.median(d2) + 0.01))


def _spec_floor(ex: numpy.ndarray, P: numpy.ndarray) -> float:
    dbP = 10 * numpy.log10(P)
    loud = ex > ex.max() - 30
    return float(numpy.median(numpy.percentile(dbP[loud], 10, axis=1) - dbP[loud].max(axis=1)))


def _tonal(P: numpy.ndarray, active: numpy.ndarray) -> tuple[numpy.ndarray, numpy.ndarray]:
    """The peak bin of every frame and the frames whose peak lobe holds over half the energy."""
    pk = _peaks(P)
    records = numpy.arange(len(P))
    lobe = sum(P[records, pk + dd] for dd in range(-2, 3))
    return pk, active & (lobe / P.sum(axis=1) > 0.5)


def _spectral_frames(yv: numpy.ndarray, result: dict) -> None:
    fv, P = _stft(yv)
    ex = 10 * numpy.log10(P.sum(axis=1))
    result["spec_floor_db"] = _spec_floor(ex, P)
    active = ex > ex.max() - 50
    result["quiet_flat"] = _quiet_flat(P, ex, active)
    pk, tonal = _tonal(P, active)
    result["tonal_frames"] = float(tonal.sum() / max(1, active.sum()))
    result["pitch_wobble"] = _wobble(fv, P, tonal, pk)


def _env_resid(yv: numpy.ndarray) -> float:
    from scipy import signal as sps

    ex = 10 * numpy.log10((_frames(yv, 160) ** 2).mean(axis=1) + 1e-30)
    counted = ex >= ex.max() - 40
    if counted.sum() < 9:
        return float("nan")
    trend = sps.medfilt(ex, 7)
    return float(numpy.log10(numpy.median(numpy.abs(ex - trend)[counted]) + 0.01))


def _texture_kurt(yv: numpy.ndarray) -> float:
    from scipy import signal as sps

    sos = sps.butter(4, [2000, 8000], "bandpass", fs=ANALYSIS_SR, output="sos")
    band = sps.sosfilt(sos, yv)
    envl = numpy.sqrt(numpy.convolve(band ** 2, numpy.ones(320) / 320, mode="same")) + 1e-9
    mask = (band != 0) & (envl >= envl.max() * 10 ** (-40 / 20))
    if mask.sum() <= 100:
        return float("nan")
    sv = band[mask] / envl[mask]
    return float(numpy.log(numpy.mean(sv ** 4) / (numpy.mean(sv ** 2) ** 2 + 1e-30) + 1e-9))


def _crest(yv: numpy.ndarray) -> float:
    nz = yv[yv != 0]
    return float(20 * numpy.log10(1 / (numpy.sqrt(numpy.mean(nz ** 2)) + 1e-12))) if len(nz) else 0.0


def _analysis_signal(sig: numpy.ndarray) -> numpy.ndarray:
    """The clip peak-normalised, resampled to the analysis rate and normalised again."""
    pre = sig / numpy.max(numpy.abs(sig))  # level-free before the float32 resampler, so level never matters
    yv = numpy.asarray(audio.resample(pre.astype(numpy.float32), SR, ANALYSIS_SR), numpy.float64)
    return yv / (numpy.max(numpy.abs(yv)) + 1e-12)


def _judgeable(sig: numpy.ndarray) -> bool:
    return len(sig) >= MIN_CLIP_S * SR and numpy.max(numpy.abs(sig)) >= MIN_PEAK


def signal_features(sig: numpy.ndarray) -> dict | None:
    """The 13 signal measurements of a mono 48 kHz clip, or None when it cannot be judged (R3)."""
    sig = audio.to_mono(numpy.asarray(sig, numpy.float64)) if numpy.ndim(sig) == 2 else numpy.asarray(sig, numpy.float64)
    if not _judgeable(sig):
        return None
    yv = _analysis_signal(sig)
    result: dict[str, float] = {"silence_frac": float(numpy.mean(numpy.abs(sig) < ZERO_SAMPLE)), "floor_db": _floor_db(yv)}
    _welch(yv, result)
    _spectral_frames(yv, result)
    result.update(env_resid_db=_env_resid(yv), texture_kurt=_texture_kurt(yv), crest_db=_crest(yv))
    return {kk: result[kk] for kk in SIGNAL_INPUTS}


def signal_vector(feats: Mapping[str, float]) -> numpy.ndarray:
    """The signal head input vector in model order."""
    return numpy.array([feats.get(kk, float("nan")) for kk in SIGNAL_INPUTS], numpy.float64)


def rms_db(sig: numpy.ndarray) -> float:
    """RMS in dB (-200 for empty input)."""
    sig = numpy.asarray(sig, numpy.float64)
    return float(20 * numpy.log10(max(numpy.sqrt(numpy.mean(sig ** 2)), 1e-10))) if sig.size else -200.0


def audible(sig: numpy.ndarray) -> bool:
    """True when the clip's RMS is above -60 dBFS and it has signal measurements."""
    return rms_db(sig) > SILENT_DB and signal_features(sig) is not None


def pure_tone(x_or_feats: numpy.ndarray | Mapping[str, float] | None) -> bool:
    """The pure dead-steady tone rule (R5); takes a clip or its signal measurements."""
    fv = x_or_feats if isinstance(x_or_feats, Mapping) or x_or_feats is None else signal_features(x_or_feats)
    if not fv:
        return False
    wob = fv.get("pitch_wobble", float("nan"))
    return (fv["tonal_frames"] >= 0.95 and fv["line_share"] >= 0.9 and fv["crest_db"] <= 4.5
            and (not numpy.isfinite(wob) or wob <= -1.0))


# ================================================================ lift and clips


def lift_gain(sig: numpy.ndarray) -> float:
    """The quiet-source gain in dB (R1); 0 when no lift applies."""
    sig = numpy.asarray(sig, numpy.float64)
    if not sig.size:
        return 0.0
    row_, pth = rms_db(sig), float(20 * numpy.log10(max(float(numpy.max(numpy.abs(sig))), 1e-10)))
    grp = min(LIFT_RMS_DB - row_, LIFT_PEAK_DB - pth)
    return 0.0 if pth < LIFT_FLOOR_DB or grp <= 0 else float(grp)


def clip_starts(count: int) -> list[int]:
    """Clip start samples of an n-sample sound (R2); one clip up to 6 s."""
    win_n = int(WIN_S * SR)
    if count <= int(LONG_S * SR):
        return [0]
    starts = list(range(0, count - win_n + 1, win_n))
    if starts[-1] + win_n != count:
        starts.append(count - win_n)
    if len(starts) > 2 and (starts[-1] - starts[-2]) < DROP_LAST_S * SR:
        starts.pop()
    return starts


def clips_of(sig: numpy.ndarray) -> list[tuple[float, float, numpy.ndarray]]:
    """(local start s, local end s, samples) of every clip."""
    count = len(sig)
    if count <= int(LONG_S * SR):
        return [(0.0, count / SR, sig)]
    win_n = int(WIN_S * SR)
    return [(sv / SR, (sv + win_n) / SR, sig[sv:sv + win_n]) for sv in clip_starts(count)]


# ================================================================ facts and words


def pct(vec: float) -> str:
    """A percentage: 1 decimal strictly between 0 and 9.5 %, else an integer."""
    pth = 100 * vec
    return f"{pth:.1f}%" if 0 < pth < 9.5 else f"{int(round(pth))}%"


FACT_WORDS: dict[str, tuple[Callable[[float], str], Callable[[float], str]]] = {
    "floor_db": (lambda v: f"a noise floor like a microphone or room, {abs(v):.0f} dB under the peak",
                 lambda v: "quietest parts very far under the peak: cleaner than a microphone records"),
    "spec_floor_db": (lambda v: f"a broadband noise floor {abs(v):.0f} dB under the spectral peak",
                      lambda v: "no measurable noise between the partials"),
    "line_share": (lambda v: "energy spread over the spectrum", lambda v: f"{pct(v)} of the energy in a few pure lines"),
    "pitch_wobble": (lambda v: "natural pitch drift", lambda v: "dead-steady pitch with no natural drift"),
    "env_resid_db": (lambda v: "micro-fluctuations of a recorded source",
                     lambda v: "a mathematically smooth envelope (exponential or linear ramps)"),
    "texture_kurt": (lambda v: "a crackly, impulsive high-frequency texture",
                     lambda v: "Gaussian noise like filtered random numbers"),
    "quiet_flat": (lambda v: "quiet parts are broadband room or mic noise", lambda v: "quiet parts are tonal tails"),
    "tonal_frames": (lambda v: "few pure-tone frames", lambda v: f"{pct(v)} of frames ruled by one pure tone"),
    "log_flatness": (lambda v: "a dense, noise-rich spectrum", lambda v: "a few pure lines over near-silence"),
    "peakiness_db": (lambda v: "no dominant spectral line", lambda v: f"one line {v:.0f} dB above the median bin"),
    "bandwidth_khz": (lambda v: f"content reaches {v:.1f} kHz", lambda v: f"content reaches {v:.1f} kHz"),
    "crest_db": (lambda v: f"crest {v:.0f} dB: sharp natural transients",
                 lambda v: f"crest only {v:.0f} dB: a steady, oscillator-like level"),
}


def fact_phrase(feature: str, side: str, vec: float) -> str:
    """A fired fact in words."""
    pair = FACT_WORDS.get(feature)
    if pair is None:
        return f"{feature} {vec:.3g}"
    return pair[0 if side == "real" else 1](vec)


def fired_facts(model: Mapping, feats: Mapping[str, float]) -> list[dict]:
    """Facts whose test passes, by precision descending, with their phrases (R6)."""
    result = []
    for fv in model.get("facts", []):
        vec = feats.get(fv["feature"], float("nan"))
        if fv["feature"] == "silence_frac" or not numpy.isfinite(vec):
            continue
        if (vec <= fv["value"]) if fv["op"] == "<=" else (vec >= fv["value"]):
            who = "were real/pro" if fv["side"] == "real" else "were synthesized"
            text = f"{fact_phrase(fv['feature'], fv['side'], vec)} ({pct(fv['precision'])} of training clips past this {who})"
            result.append({"side": fv["side"], "precision": fv["precision"], "text": text})
    return sorted(result, key=lambda fv: -fv["precision"])


def silence_line(model: Mapping, vec: float) -> str | None:
    """The digital-silence context line (R7), or None."""
    records = [row_ for row_ in model.get("silence_base", []) if row_["at_least"] <= vec]
    if vec < SILENCE_LINE or not records:
        return None
    row_ = records[-1]
    return (f"{pct(vec)} of the samples are exact digital silence: weak evidence (edited, gated or synthesized); "
            f"{pct(row_['real'])} of real and {pct(row_['synth'])} of synthesized training clips had at least "
            f"{pct(row_['at_least'])}")


def _pure_fact() -> dict:
    return {"side": "synth", "precision": 0.99, "text": "a pure, dead-steady tone such as a test tone or a bare "
            "oscillator (99% rule)"}


# ================================================================ clip scoring


def _score_clip(model: dict, emb: numpy.ndarray, feats: dict) -> dict:
    lc = head_logit(model["heads"]["clap"], emb)
    ls = head_logit(model["heads"]["signal"], signal_vector(feats))
    pth = fuse(model, {"clap": lc, "signal": ls})
    pure = pure_tone(feats)
    facts = fired_facts(model, feats)
    if pure:
        pth = min(pth, PURE_CAP)
        facts = [_pure_fact()] + facts
    return {"p": pth, "clap": lc, "signal": ls, "pure": pure, "facts": facts, "feats": feats, "emb": emb}


def _prompt_line(model: dict, emb: numpy.ndarray, side: str) -> str:
    cos = {sv: voc.embed_plain_text(model["prompts"][sv]).astype(numpy.float64) @ emb for sv in ("real", "synth")}
    winner = max(("real", "synth"), key=lambda sv: cos[sv].max())
    other = "synth" if winner == "real" else "real"
    bi, oi = int(numpy.argmax(cos[winner])), int(numpy.argmax(cos[other]))
    name = {"real": "real", "synth": "synthesized"}
    return (f"closest prompt '{model['prompts'][winner][bi]}' ({name[winner]}, cosine {cos[winner][bi]:.2f}); best "
            f"{name[other]} prompt '{model['prompts'][other][oi]}' (cosine {cos[other][oi]:.2f}); prompt cosines "
            "add context and do not decide")


def clap_line(model: dict, cand: dict) -> str:
    """The CLAP evidence line of one scored clip (R8)."""
    toward = "toward real" if cand["clap"] >= 0 else "toward synthesized"
    lead = (f"pure-tone rule decided (the CLAP head alone: {cand['clap']:+.1f} {toward})" if cand["pure"] else
            f"CLAP head score {cand['clap']:+.1f} ({toward})")
    return f"{lead}; {_prompt_line(model, cand['emb'], 'real' if cand['p'] >= LEAN else 'synth')}"


def _ast_line(clip: numpy.ndarray) -> str:
    from hark.models import ast

    model = ast.get()
    pth = numpy.asarray(model.probs([numpy.asarray(clip, numpy.float32)]))[0]
    syn = sorted(((model.prob_of(pth, lab), lab) for lab in SYNTH_CLASSES), reverse=True)[:3]
    best_n = model.top(pth, k=3)
    return ("AudioSet synth classes: " + ", ".join(f"{lab} {pp:.2f}" for pp, lab in syn) +
            "; top classes: " + ", ".join(f"{lab} {pp:.2f}" for lab, pp in best_n))


# ================================================================ realness of one file


def _name(fpath: str, t_from: float | None, t_to: float | None, secs: float) -> str:
    base = os.path.basename(fpath)
    if t_from is None and t_to is None:
        return base
    return f"{base} [{report.fmt_t(t_from or 0.0)}-{report.fmt_t(t_to if t_to is not None else secs)}]"


def _probed_duration(fpath: str) -> float | None:
    try:
        return audio.duration(fpath)
    except audio.AudioError:
        raise
    except Exception:
        return None


def _past_end(fpath: str, t_from: float | None, secs: float | None) -> str | None:
    if t_from is None or secs is None or t_from < secs:
        return None
    return f"start {t_from:g} s is at or past the end of {fpath} ({secs:.3f} s long)"


def check_range(fpath: str, t_from: float | None, t_to: float | None) -> float | None:
    """The realness range checks (R10); returns the probed duration when known."""
    sv = 0.0 if t_from is None else t_from
    voc.raise_first([(sv < 0, "start must be >= 0 s"), (t_to is not None and t_to <= sv, "end must be greater than start")])
    secs = _probed_duration(fpath)
    voc.raise_first([(True, _past_end(fpath, t_from, secs))])
    return secs


def _cache_path(fpath: str, t_from: float | None, t_to: float | None, ast: bool, model: Mapping) -> Path:
    kname = voc.key_of("realness", audio.file_id(fpath), t_from, t_to, ast, model.get("version"), model.get("trained"),
                     CACHE_VERSION)
    return voc.cache_dir() / "realness" / f"r_{kname}.json"


def _silent_result(fpath: str, name: str, sig: numpy.ndarray, base: dict) -> dict:
    peak = 20 * numpy.log10(max(float(numpy.max(numpy.abs(sig))) if sig.size else 0.0, 1e-10))
    why = ("exact digital silence" if peak <= DIGITAL_ZERO_DB else
           f"silent: no part is above -60 dBFS RMS (peak {peak:.1f} dBFS)")
    return {**base, "p_real": None, "verdict": SILENT, "why": why, "evidence": ["nothing to judge: the audio is silent"],
            "summary": f"{name}: silent ({why})."}


def _scored(model: dict, clips: list, ast: bool) -> list[dict]:
    judged = [(lhs, rhs, cand, fv) for lhs, rhs, cand in clips for fv in [signal_features(cand)] if fv and rms_db(cand) > SILENT_DB]
    if not judged:
        return []
    resolve_head(model)
    E = voc.embed_audio([cand for _, _, cand, _ in judged]).astype(numpy.float64)
    result = []
    for (lhs, rhs, cand, fv), ex in zip(judged, E):
        sv = _score_clip(model, ex, fv)
        sv.update({"t0": lhs, "t1": rhs})
        if ast:
            sv["ast"] = _ast_line(cand)
        result.append(sv)
    return result


def _side(pth: float) -> str:
    return "real" if pth >= LEAN else "synth"


def _fact_lines(rep: dict) -> tuple[list[str], list[str]]:
    side = _side(rep["p"])
    mine = [fv["text"] for fv in rep["facts"] if fv["side"] == side][:3]
    other = [fv["text"] for fv in rep["facts"] if fv["side"] != side][:1]
    return mine, [f"but: {tx}" for tx in other]


def _gain_line(gain: float, x_stats: tuple[float, float]) -> str | None:
    if gain < GAIN_LINE_DB:
        return None
    return (f"quiet source (RMS {x_stats[0]:.1f} dBFS, peak {x_stats[1]:.1f} dBFS) lifted by {gain:.1f} dB "
            "before judging")


def _fusion_line(model: dict, rep: dict, any_fact: bool) -> str | None:
    win_n = model["fusion"]["coef"]
    if float(win_n[1]) != 0:
        return f"fusion: CLAP x {win_n[0]:.2f}, signal x {win_n[1]:.2f}; signal head score {rep['signal']:+.1f}"
    return "the measurements are context, not votes: the signal head's fusion weight is 0" if any_fact else None


def _evidence(model: dict, rep: dict, long_note: str | None, gain: float, x_stats: tuple[float, float]) -> list[str]:
    mine, other = _fact_lines(rep)
    sil = silence_line(model, rep["feats"]["silence_frac"])
    lines = [long_note, clap_line(model, rep), *mine, *other, sil, rep.get("ast"), _gain_line(gain, x_stats),
             _fusion_line(model, rep, bool(mine or other or sil))]
    return [line for line in lines if line]


def _why_short(rep: dict) -> str:
    toward = "toward real" if rep["clap"] >= 0 else "toward synthesized"
    if rep["pure"]:
        return f"pure-tone rule (the CLAP head alone: {rep['clap']:+.1f})"
    side = "real" if rep["p"] >= LEAN else "synth"
    first = next((fv["text"] for fv in rep["facts"] if fv["side"] == side), None)
    return f"CLAP head score {rep['clap']:+.1f} ({toward})" + (f"; {first}" if first else "")


def p_text(pth: float, th: Mapping[str, float]) -> str:
    """p_real with 2 decimals, or 3 when 2 would read as a threshold it is not equal to."""
    two = f"{pth:.2f}"
    return f"{pth:.3f}" if any(two == f"{tx:.2f}" and abs(pth - tx) > 1e-9 for tx in th.values()) else two


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


def _segments(model: dict, clips: list, graded: list, sv: float, secs: float) -> list[dict]:
    th = thresholds_of(model)
    by_t = {round(cand["t0"], 6): cand for cand in graded}
    result = []
    for lhs, rhs, _ in clips:
        rec: dict[str, Any] = {"start": round(sv + lhs, 3), "end": round(min(sv + rhs, sv + secs), 3)}
        cand = by_t.get(round(lhs, 6))
        if cand is None:
            rec.update(p_real=None, verdict=SILENT)
        else:
            rec.update(p_real=round(cand["p"], 3), verdict=verdict_of(cand["p"], th))
            side = "real" if cand["p"] >= LEAN else "synth"
            first = next((fv["text"] for fv in cand["facts"] if fv["side"] == side), None)
            if first:
                rec["why"] = first
        result.append(rec)
    return result


def _merged_spans(segs: Sequence[dict]) -> list[tuple[float, float]]:
    spans: list[list[float]] = []
    for grp in segs:
        if spans and grp["start"] <= spans[-1][1] + 1e-6:
            spans[-1][1] = max(spans[-1][1], grp["end"])
        else:
            spans.append([grp["start"], grp["end"]])
    return [(lhs, rhs) for lhs, rhs in spans]


def _span_text(spans: Sequence[tuple[float, float]]) -> str:
    text = ", ".join(f"{report.fmt_t(lhs)}-{report.fmt_t(rhs)}" for lhs, rhs in spans[:6])
    return text + (", ..." if len(spans) > 6 else "")


def _lowest_text(segs: list[dict]) -> str:
    pmin = min(grp["p_real"] for grp in segs if grp["p_real"] is not None)
    return (f" No window leans synthesized (lowest window p_real {pmin:.2f}); a quiet code-made layer can "
            "still sit below a louder real mix.")


def _long_tail(model: dict, segs: list[dict]) -> str:
    lo = thresholds_of(model)["lo"]
    lean = [grp for grp in segs if grp["p_real"] is not None and grp["p_real"] < LEAN]
    if not lean:
        return _lowest_text(segs)
    low = [grp for grp in lean if grp["p_real"] <= lo]
    text = f" Windows that lean synthesized: {_span_text(_merged_spans(lean))}."
    if low:
        return text + f" {len(low)} at or below the clip lo {lo:.2f}: {_span_text(_merged_spans(low))}."
    return text + f" None at or below the clip lo {lo:.2f}."


def _long_fields(model: dict, clips: list, graded: list, sv: float, secs: float) -> tuple[dict, str, str]:
    segs = _segments(model, clips, graded, sv, secs)
    lo = thresholds_of(model)["lo"]
    ps = [cand["p"] for cand in graded]
    lean = sum(pth < LEAN for pth in ps)
    pure = sum(cand["pure"] for cand in graded)
    why = (f"{lean} of {len(graded)} windows lean synthesized (p_real < 0.5); mean CLAP head score "
           f"{numpy.mean([cand['clap'] for cand in graded]):+.1f}" + (f"; {pure} decided by the pure-tone rule" if pure else ""))
    fields = {"segments": segs, "synth_windows": sum(pth <= lo for pth in ps), "lean_synth_windows": lean,
              "p_real_min": round(min(ps), 3)}
    return fields, why, _long_tail(model, segs)


def _base(fpath: str, t_from: float | None, t_to: float | None, secs: float) -> dict:
    base: dict[str, Any] = {"path": fpath, "duration": round(secs, 3)}
    if t_from is not None or t_to is not None:
        sv = t_from or 0.0
        base["range"] = [round(sv, 3), round(sv + secs, 3)]
    return base


def _lifted(sig: numpy.ndarray, base: dict) -> tuple[numpy.ndarray, float, tuple[float, float]]:
    """The quiet-source lift: (lifted audio, gain dB, (RMS, peak) before the lift)."""
    gain = lift_gain(sig)
    stats = (rms_db(sig), float(20 * numpy.log10(max(float(numpy.max(numpy.abs(sig))) if sig.size else 0, 1e-10))))
    if gain > 0:
        base["gain_db"] = round(gain, 1)
        return sig * 10 ** (gain / 20), gain, stats
    return sig, gain, stats


def _long_result(cx: dict, pth: float, rep: dict, th: dict) -> dict:
    sv, secs, graded = cx["s"], cx["dur"], cx["scored"]
    fields, why, tail = _long_fields(cx["model"], cx["clips"], graded, sv, secs)
    kind = "real" if pth >= LEAN else "synthesized"
    note = (f"evidence from the most {kind}-sounding window {report.fmt_t(sv + rep['t0'])}-"
            f"{report.fmt_t(min(sv + rep['t1'], sv + secs))} (p_real {rep['p']:.2f})")
    summary = (f"{cx['name']}: {verdict_of(pth, th)} (p_real {p_text(pth, th)}, mean of {len(graded)} 4 s windows; file "
               f"thresholds lo {th['lo']:.2f} / hi {th['hi']:.2f}, weak calibration): {why}." + tail)
    return {"why": why, "evidence": _evidence(cx["model"], rep, note, cx["gain"], cx["stats"]), **fields,
            "summary": summary}


def _short_result(cx: dict, pth: float, rep: dict, th: dict) -> dict:
    why = _why_short(rep)
    summary = (f"{cx['name']}: {verdict_of(pth, th)} (p_real {p_text(pth, th)}; clip thresholds lo {th['lo']:.2f} / "
               f"hi {th['hi']:.2f}): {why}.")
    return {"why": why, "evidence": _evidence(cx["model"], rep, None, cx["gain"], cx["stats"]), "summary": summary}


def _verdict_result(cx: dict, base: dict) -> dict:
    graded, model = cx["scored"], cx["model"]
    pth = float(numpy.mean([cand["p"] for cand in graded]))
    long = len(cx["clips"]) > 1
    th = thresholds_of(model, file=long)
    rep = max(graded, key=lambda cand: cand["p"]) if pth >= LEAN else min(graded, key=lambda cand: cand["p"])
    row_ = {**base, "p_real": round(pth, 3), "verdict": verdict_of(pth, th),
         **(_long_result if long else _short_result)(cx, pth, rep, th)}
    if is_zero_shot(model):
        row_["summary"] += " (zero-shot fallback model)"
    row_["note"] = _note(model, long)
    return row_


def _judge(fpath: str, t_from: float | None, t_to: float | None, ast: bool, model: dict) -> dict:
    sig = numpy.asarray(audio.load(fpath, mono=True, start=t_from, end=t_to), numpy.float64)
    secs = len(sig) / SR
    name = _name(fpath, t_from, t_to, secs)
    base = _base(fpath, t_from, t_to, secs)
    sig, gain, stats = _lifted(sig, base)
    clips = clips_of(sig)
    graded = _scored(model, clips, ast)
    if not graded:
        return _silent_result(fpath, name, sig, base)
    cx = {"model": model, "clips": clips, "scored": graded, "s": t_from or 0.0, "dur": secs, "name": name,
          "gain": gain, "stats": stats}
    return _verdict_result(cx, base)


def realness(path: str, start: float | None = None, end: float | None = None, ast: bool = False,
             cache: bool = True) -> dict:
    """Is a sound a real recording or pro production, or cheap code-synthesized audio?"""
    check_range(path, start, end)
    model = load_model(resolve=False)
    cpath = _cache_path(path, start, end, ast, model) if cache else None
    hit = voc.load_json(cpath) if cpath else None
    if isinstance(hit, dict) and "verdict" in hit:
        return {**hit, "path": path}
    row_ = _judge(path, start, end, ast, model)
    if cpath:
        voc.save_json(cpath, report.clean(row_))
    return row_


# ================================================================ many files


def _error_row(fpath: str, why: str) -> dict:
    return {"path": fpath, "p_real": None, "verdict": ERROR, "why": why[:200],
            "summary": f"{os.path.basename(fpath) or fpath}: error: {why[:200]}"}


def bad_path_reason(fpath: str) -> str | None:
    """Why a given path names no media, or None when it does."""
    if not os.path.exists(fpath) and not any(ch in fpath for ch in "*?["):
        return "no such file or folder"
    return None if audio.list_media([fpath]) else "no audio or video files in it"


def realness_many(paths: Sequence[str], ast: bool = False, start: float | None = None,
                  end: float | None = None) -> dict:
    """realness of every media file under the given paths, with verdict counts."""
    paths = [paths] if isinstance(paths, str) else list(paths)
    files, bad = [], []
    for pth in paths:
        reason = bad_path_reason(pth)
        if reason:
            bad.append(_error_row(pth, reason))
        else:
            files += audio.list_media([pth])
    records = []
    for fv in files:
        try:
            records.append(realness(fv, start=start, end=end, ast=ast))
        except Exception as exc:  # one bad file never stops the batch
            records.append(_error_row(fv, f"{type(exc).__name__}: {voc.first_line(exc)}"))
    records += bad
    counts = {vec: sum(row_["verdict"] == vec for row_ in records) for vec in VERDICTS}
    return {"files": records, "counts": counts, "summary": many_summary(len(records), counts, start, end)}


def many_summary(count: int, counts: Mapping[str, int], t_from: float | None, t_to: float | None) -> str:
    """'<N> files: ...' with the verdict counts."""
    pieces = [f"{counts[REAL]} real or pro-produced", f"{counts[UNCERTAIN]} uncertain", f"{counts[SYNTH]} likely "
             "code-synthesized"]
    if counts[SILENT]:
        pieces.append(f"{counts[SILENT]} silent")
    if counts[ERROR]:
        pieces.append(f"{counts[ERROR]} unreadable")
    rng = ""
    if t_from is not None or t_to is not None:
        rng = f" (each judged over {t_from or 0:g}-{'end' if t_to is None else f'{t_to:g}'} s)"
    return f"{count} files: " + ", ".join(pieces) + rng + "."


# ================================================================ text output


def _segment_line(grp: dict) -> str:
    pth = "-" if grp["p_real"] is None else f"{grp['p_real']:.2f}"
    leans = grp["verdict"] == UNCERTAIN and grp["p_real"] is not None and grp["p_real"] < LEAN
    why = f" ({grp['why']})" if grp.get("why") and grp["verdict"] != REAL else ""
    return (f"  {report.fmt_t(grp['start'])}-{report.fmt_t(grp['end'])}  {pth}  {grp['verdict']}"
            f"{', leans synthesized' if leans else ''}{why}")


def single_text(row_: dict) -> str:
    """Text output of one file."""
    lines = [row_["summary"]] + [f"  - {ex}" for ex in row_.get("evidence", [])]
    lines += [_segment_line(grp) for grp in row_.get("segments", [])]
    if row_.get("note"):
        lines.append(f"  note: {row_['note']}")
    return "\n".join(lines)


def table_text(result: dict, model: Mapping | None = None) -> str:
    """Text output of several files."""
    lines = [result["summary"]]
    if model:
        cand, fv = thresholds_of(model), thresholds_of(model, file=True)
        lines.append(f"-- clip thresholds lo {cand['lo']:.2f} / hi {cand['hi']:.2f}; file thresholds lo {fv['lo']:.2f} / "
                     f"hi {fv['hi']:.2f} (weakly calibrated); judge a trim of a multi-take file --")
    lines.append(f"{'p_real':>6}  verdict  file  reason")
    for row_ in result["files"]:
        pth = "-" if row_.get("p_real") is None else f"{row_['p_real']:.2f}"
        kind = " (file)" if row_.get("segments") else ""
        low = f" [{row_['synth_windows']} low window(s)]" if row_.get("synth_windows") else ""
        lines.append(f"{pth:>6}  {row_['verdict']}{kind}  {os.path.basename(row_['path'])}{low}  {row_.get('why', '')}")
    return "\n".join(lines)


# ================================================================ CLI


def _bad_paths(paths: Sequence[str]) -> list[tuple[str, str]]:
    return [(pth, row_) for pth in paths for row_ in [bad_path_reason(pth)] if row_]


def _judge_paths(args: argparse.Namespace) -> int:
    files = audio.list_media(list(args.paths))
    if len(files) == 1:
        row_ = realness(files[0], start=args.start, end=args.end, ast=args.ast)
        report.emit(row_, args, text=single_text(row_))
        return 0
    res = realness_many(args.paths, ast=args.ast, start=args.start, end=args.end)
    report.emit(res, args, text=table_text(res, load_model(resolve=False)))
    return 1 if res["counts"][ERROR] else 0


def _run(args: argparse.Namespace) -> int | None:
    import hark.models

    bad = _bad_paths(args.paths)
    for pth, reason in bad:
        report.error_line("realness", f"{pth}: {reason}")
    if bad:
        return 1
    hark.models.require_ml("realness")
    try:
        return _judge_paths(args)
    except (ValueError, audio.AudioError, ModelMissing) as exc:
        report.error_line("realness", str(exc))
        return 1


def add_cli(sub: argparse._SubParsersAction) -> None:
    """Register ``realness``."""
    pth = sub.add_parser("realness", help="Real recording or pro production, or cheap code-synthesized audio?")
    pth.add_argument("paths", nargs="+", help="files or folders (audio or video)")
    pth.add_argument("--start", type=float, default=None)
    pth.add_argument("--end", type=float, default=None)
    pth.add_argument("--ast", action="store_true", help="also run the AudioSet tagger for evidence (slower)")
    report.add_common(pth)
    pth.set_defaults(func=_run)


def main(argv: Sequence[str] | None = None) -> int:
    """``python -m hark.listen.realness --train DIR [-o PATH] [--forge-negatives N]``."""
    from hark.listen import realness_train

    return realness_train.main(argv)


if __name__ == "__main__":
    sys.exit(main())
