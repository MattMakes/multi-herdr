"""Held-out benchmark for forge output: our sounds scored against the bench refs, on checks no recipe tunes for.

Run: `python -m hark.forge.engine.eval.bench --out results_<tag>.json --made <finals folder>`.
The numbers are for reading only. Do not pick candidates, guide a search or adjust a recipe with them.
"""

from __future__ import annotations

import argparse
import glob
import json
import math
import os
import time
from collections.abc import Callable, Sequence
from pathlib import Path
from typing import Any

import numpy as np

from hark.forge.engine.eval import frozen_tone as ft
from hark.forge.engine.eval import scrub_paths

SR = 48000
ENDS_ON_PEAK = ("riser", "pro_riser")
METRICS = (("typ_z", "high"), ("tone_db", "low"), ("env_oct", "low"), ("curve_db", "low"), ("phrase_z", "high"),
           ("margin", "high"), ("as_p", "high"))
SUMM_EXTRA = ("tone_ratio", "t40_ratio", "wins", "floor_db", "stereo_corr", "darken")

PHRASES = {
    "whoosh_fast": ["a quick swoosh as something flies past", "a short sharp swoosh of air"],
    "whoosh_big": ["a heavy swooping rush of air", "a big slow swoosh in a movie trailer"],
    "swish": ["a thin airy swish", "a light quick swoosh of a thrown object"],
    "swipe": ["fabric rustling in one quick swipe", "a sleeve brushing past quickly"],
    "riser": ["a tension build-up getting louder and louder", "a whooshing crescendo that swells to a climax"],
    "downlifter": ["a descending whoosh that fades out", "a falling pitch sweep winding down"],
    "impact": ["a huge low thud for a movie trailer", "a massive hit with a rumbling tail"],
    "sub_drop": ["a low frequency boom that drops in pitch", "a deep rumbling bass drop"],
    "click": ["a crisp tiny plastic click", "a short clean click of a button being pressed"],
    "tap": ["a fingertip tapping on glass", "a light tap of a finger on a phone screen"],
    "toggle": ["a light switch clicking on", "a small switch being flipped"],
    "hover_tick": ["a very short faint tick", "a tiny dry click"],
    "pop": ["a popping sound like a bubble bursting", "a quick plop"],
    "notify": ["a pleasant chime alert sound", "a single bell ding on a phone"],
    "shimmer": ["sparkling twinkling chimes", "a glittering magical sound"],
    "typing_key": ["one key being pressed on a computer keyboard", "a single keystroke on a mechanical keyboard"],
    "glitch": ["a digital glitch with stuttering noise", "a broken electronic signal stutter"],
    "logo_sting": ["a short musical logo with a ringing finish", "a bright struck bell ringing out"],
    "pro_click": ["a crisp bright digital click", "a tiny sharp interface click"],
    "pro_tap": ["a soft muted tap on a screen", "a short dull interface tap"],
    "pro_pop_in": ["a soft round thump as something appears on screen", "a short muffled pop of an element appearing"],
    "pro_hover_tick": ["a tiny bright tick of a cursor", "a faint high interface tick"],
    "pro_pop": ["a quick electronic blip", "a tiny synthetic pop"],
    "pro_notify": ["a soft digital notification ding", "a short pure alert tone"],
    "pro_shimmer": ["a glittering digital sparkle", "a shimmering burst of tiny tones"],
    "pro_typing_key": ["a light clean keystroke", "a soft tick of a keyboard key"],
    "pro_typing_run": ["someone typing a short line quickly", "a rapid run of keystrokes"],
    "pro_glitch": ["a short burst of digital stutter", "a crackling data glitch that cuts off"],
    "pro_whoosh_big": ["a deep heavy swoosh with a low rumble", "a slow powerful whoosh of air"],
    "pro_riser": ["a rising stutter effect that speeds up", "a pulsing electronic build-up"],
    "pro_zip": ["a fast rising electronic tone", "a quick swooping synth glide"],
    "pro_impact": ["a deep designed boom that cuts off", "a heavy low rumble hit in a launch video"],
    "pro_sub_drop": ["a deep sub bass boom that sinks", "a low electronic bass drop"],
    "pro_logo_sting": ["a short electronic logo sound", "a falling synth tone landing on a hum"],
}

CONFUSIONS = {
    "whoosh_fast": ["a steam hiss", "a car driving past", "tearing paper", "a bird flapping its wings"],
    "whoosh_big": ["a strong wind storm", "an airplane taking off", "ocean waves crashing", "a car driving past"],
    "swish": ["a short hiss", "a sword being drawn", "a page being turned", "a bird flying past"],
    "swipe": ["paper crumpling", "a broom sweeping", "a gust of wind", "a zipper"],
    "riser": ["a jet engine starting", "a siren", "steady rain", "an orchestra tuning"],
    "downlifter": ["a siren", "a car passing by", "a falling bomb whistle", "wind"],
    "impact": ["a gunshot", "a door being slammed", "a bass drum", "distant fireworks"],
    "sub_drop": ["an earthquake rumble", "a kick drum", "a truck engine idling", "thunder in the distance"],
    "click": ["a wooden stick hit", "a finger snap", "a drop of water", "a stapler"],
    "tap": ["a knock on a door", "a raindrop", "a keyboard key", "a ballpoint pen click"],
    "toggle": ["a door latch", "a stapler", "a seatbelt buckle", "a ball bouncing"],
    "hover_tick": ["a clock ticking", "a drop of water", "a typewriter", "a cricket chirp"],
    "pop": ["a hand clap", "a gunshot", "a kiss", "a tongue click"],
    "notify": ["a doorbell", "a microwave beep", "a church bell", "a cash register"],
    "shimmer": ["a harp glissando", "breaking glass", "a music box", "a sleigh bell jingle"],
    "typing_key": ["a ballpoint pen click", "a stapler", "a light switch", "a dripping tap"],
    "glitch": ["radio static", "a modem dialing", "a scratched record", "an electric buzz"],
    "logo_sting": ["a church bell", "a doorbell", "a gong", "a cymbal crash"],
    "pro_pop_in": ["a kick drum", "a knock on a door", "a ball bouncing", "a hand clap"],
    "pro_pop": ["a microwave beep", "a bird chirp", "a tongue click", "a kiss"],
    "pro_typing_run": ["rain on a roof", "a clock ticking", "popcorn popping", "a horse galloping"],
    "pro_riser": ["a helicopter", "a machine gun", "a dial-up modem", "a ticking clock"],
    "pro_zip": ["a whistle", "a bird singing", "a mosquito buzzing", "a slide whistle"],
    "pro_logo_sting": ["a doorbell", "a car horn", "an error buzzer", "a church bell"],
}

# Pro categories without their own list reuse the plain category's list.
_SAME_AS = {"pro_click": "click", "pro_tap": "tap", "pro_hover_tick": "hover_tick", "pro_notify": "notify",
            "pro_shimmer": "shimmer", "pro_typing_key": "typing_key", "pro_glitch": "glitch",
            "pro_whoosh_big": "whoosh_big", "pro_impact": "impact", "pro_sub_drop": "sub_drop"}

_AIR = "Whoosh, swoosh, swish"
_BEEP = "Beep, bleep"
_THUD = "Thump, thud"
_POP = ["Burst, pop", "Plop"]
_BELLS = ["Ding", "Chime", "Bell", "Glockenspiel", "Jingle, tinkle", "Ding-dong", "Ringtone", "Marimba, xylophone",
          "Vibraphone"]
_HITS = ["Boom", _THUD, "Explosion", "Bang", "Bass drum"]
_KEYS = ["Computer keyboard", "Typing", "Typewriter", "Clicking"]
_LOGO = ["Chime", "Bell", "Ding", "Jingle, tinkle", "Gong", "Cymbal", "Glockenspiel"]

AS_LABELS = {
    "whoosh_fast": [_AIR, "Whip"],
    "swish": [_AIR, "Whip"],
    "whoosh_big": [_AIR],
    "downlifter": [_AIR],
    "swipe": [_AIR, "Rustle"],
    "riser": [_AIR, "Cymbal", "Drum roll"],
    "impact": _HITS,
    "sub_drop": ["Boom", "Rumble", _THUD],
    "click": ["Clicking", "Tick", "Tap"],
    "tap": ["Tap", "Knock", "Clicking", "Tick"],
    "toggle": ["Clicking", "Mechanisms", "Tick"],
    "hover_tick": ["Tick", "Clicking", "Tick-tock"],
    "pop": list(_POP),
    "notify": _BELLS,
    "shimmer": ["Chime", "Wind chime", "Jingle, tinkle", "Glockenspiel", "Bell", "Jingle bell"],
    "typing_key": _KEYS,
    "glitch": ["Static", "Noise", _BEEP, "White noise", "Crackle"],
    "logo_sting": _LOGO,
    "pro_pop_in": [_THUD] + _POP,
    "pro_pop": _POP + [_BEEP, "Chirp tone"],
    "pro_notify": _BELLS + [_BEEP],
    "pro_typing_run": _KEYS,
    "pro_riser": [_AIR, "Drum roll", _BEEP],
    "pro_zip": ["Chirp tone", "Sine wave", _BEEP, _AIR],
    "pro_impact": _HITS + ["Rumble"],
    "pro_logo_sting": _LOGO + [_BEEP, "Synthesizer"],
}
for _alias, _plain in _SAME_AS.items():
    CONFUSIONS.setdefault(_alias, CONFUSIONS[_plain])
    AS_LABELS.setdefault(_alias, AS_LABELS[_plain])


# ---------------------------------------------------------------- signal access

def _read(source: Any) -> np.ndarray:
    """Float samples of an array, or of an audio file loaded at SR."""
    if isinstance(source, (str, os.PathLike)):
        from hark.core.audio import load

        source = load(str(source), sr=SR)
    return np.asarray(source, dtype=float)


def _downmix(samples: np.ndarray) -> np.ndarray:
    return samples.mean(axis=1) if samples.ndim == 2 else samples


def frames_db(m: np.ndarray, ms: float) -> np.ndarray:
    """RMS level in dB of each ms-long frame; the last partial frame is zero-filled or dropped to fit."""
    hop = max(1, int(SR * ms / 1000))
    count = max(1, len(m) // hop)
    padded = np.zeros(count * hop)
    padded[: min(len(m), count * hop)] = m[: count * hop]
    power = np.mean(padded.reshape(count, hop) ** 2, axis=1)
    return 20 * np.log10(np.sqrt(power) + 1e-12)


# ---------------------------------------------------------------- envelope and artifact checks

def env_feats(x: Any) -> dict:
    """Onset, attack, t40, active span and peak time of the downmixed signal."""
    mono = _downmix(_read(x))
    fine = frames_db(mono, 1)
    peak_ms = int(np.argmax(fine))
    start_ms = int(np.argmax(fine >= fine[peak_ms] - 20))
    coarse = frames_db(mono, 5)
    final_frame = int(np.flatnonzero(coarse >= coarse.max() - 40)[-1])
    span = (final_frame + 1) * 0.005 - start_ms / 1000
    return {"onset_s": round(start_ms / 1000, 4), "attack_ms": float(max(1, peak_ms - start_ms)),
            "t40_ms": ft.t40_ms(mono), "active_s": round(max(0.005, span), 3), "peak_s": round(peak_ms / 1000, 4)}


def curve(x: Any, pre: int, post: int) -> np.ndarray:
    """Level in 10 ms steps relative to the peak, floored at -60 dB, with the peak placed at index pre."""
    levels = frames_db(_downmix(_read(x)), 10)
    top = int(np.argmax(levels))
    relative = np.maximum(levels - levels[top], -60.0)
    shaped = np.full(pre + post, -60.0)
    shift = pre - top
    first, stop = max(0, -shift), min(len(relative), len(shaped) - shift)
    if stop > first:
        shaped[first + shift:stop + shift] = relative[first:stop]
    return shaped


_ENV_FLOORS = (("attack_ms", 1.0), ("t40_ms", 5.0), ("active_s", 0.01))


class EnvRef:
    """Envelope reference owned by the benchmark, independent of the live tone code."""

    PRE = 10

    def __init__(self, refs: Sequence[Any]):
        clips = [_read(item) for item in refs]
        self.feats = [env_feats(clip) for clip in clips]
        spans = [fe["onset_s"] + fe["active_s"] - fe["peak_s"] for fe in self.feats]
        longest = max(spans, default=0.1)
        self.post = int(min(800, max(10, math.ceil(1.25 * longest / 0.01))))
        self.C = np.array([curve(clip, self.PRE, self.post) for clip in clips]) if clips else None

    def check(self, x: Any, exclude: int | None = None) -> dict:
        """How far the envelope of x sits from the refs that are kept."""
        chosen = [idx for idx in range(len(self.feats)) if idx != exclude]
        if not chosen:
            return {}
        mine = env_feats(x)
        report = {key: mine[key] for key in ("attack_ms", "t40_ms", "active_s", "onset_s")}
        octaves, typical = self._octave_offsets(mine, chosen)
        report["env_oct"] = round(float(np.mean([abs(val) for val in octaves.values()])), 3)
        report["curve_db"] = self._curve_gap(x, chosen)
        report.update(_ratio_fields(octaves))
        report["ref_median"] = {key: round(val, 3) for key, val in typical.items()}
        return report

    def _curve_gap(self, x: Any, chosen: list[int]) -> float:
        gap = curve(x, self.PRE, self.post) - np.median(self.C[chosen], axis=0)
        return round(float(np.sqrt(np.mean(gap ** 2))), 2)

    def _octave_offsets(self, mine: dict, chosen: list[int]) -> tuple[dict, dict]:
        """log2 distance of each feature from the kept refs' median, plus the medians."""
        octaves, typical = {}, {}
        for key, low in _ENV_FLOORS:
            centre = float(np.median([self.feats[idx][key] for idx in chosen]))
            typical[key] = centre
            octaves[key] = math.log2(max(mine[key], low) / max(centre, low))
        return octaves, typical


def _ratio_fields(octaves: dict) -> dict:
    """attack_ratio, t40_ratio and active_ratio from the octave offsets."""
    return {key.split("_")[0] + "_ratio": round(2 ** val, 3) for key, val in octaves.items()}


def _head_defects(raw: np.ndarray, mono: np.ndarray, scale: float) -> dict:
    mean_level = abs(float(np.mean(mono))) / scale
    return {"clipped_samples": int(np.sum(np.abs(raw) >= 0.999)),
            "dc_db": round(float(20 * np.log10(mean_level + 1e-12)), 1),
            "first_sample": round(abs(float(mono[0])) / scale, 3)}


def _tail_defects(mono: np.ndarray, scale: float) -> dict:
    fine, coarse = frames_db(mono, 1), frames_db(mono, 5)
    return {"tail_db": round(float(frames_db(mono[-480:], 10)[-1] - coarse.max()), 1),
            "last_ms_rel": round(float(np.sqrt(np.mean(mono[-48:] ** 2))) / scale, 3),
            "lead_s": round(int(np.argmax(fine >= fine.max() - 50)) / 1000, 3)}


def _defect_numbers(raw: np.ndarray) -> dict:
    mono = _downmix(raw)
    scale = float(np.max(np.abs(mono))) + 1e-12
    found = _head_defects(raw, mono, scale)
    found.update(_tail_defects(mono, scale))
    found["peak_dbfs"] = round(float(20 * np.log10(np.max(np.abs(raw)) + 1e-12)), 2)
    return found


_PEAK_END_EXEMPT = ("end click", "tail cut", "late start")


def artifacts(x: Any, cat: str | None = None) -> dict:
    """Defect measurements plus `flags`, the names of the defects found."""
    found = _defect_numbers(_read(x))
    rules = (("clipped", found["clipped_samples"] >= 3), ("dc offset", found["dc_db"] > -30),
             ("start click", found["first_sample"] > 0.05), ("end click", found["last_ms_rel"] > 0.02),
             ("tail cut", found["tail_db"] > -30), ("late start", found["lead_s"] > 0.1))
    peak_end = cat in ENDS_ON_PEAK
    found["flags"] = [name for name, hit in rules if hit and not (peak_end and name in _PEAK_END_EXEMPT)]
    return found


def _pair_stats(unit: np.ndarray, levels: list) -> tuple[list, list, list]:
    """Cosine and tone gap for each pair of variants, plus the pairs that look like copies."""
    sims, gaps, twins = [], [], []
    total = len(levels)
    for one in range(total):
        for two in range(one + 1, total):
            sim = float(unit[one] @ unit[two])
            gap = float(np.sqrt(np.mean((levels[one] - levels[two]) ** 2)))
            sims.append(sim)
            gaps.append(gap)
            if sim > 0.985 and gap < 1.5:
                twins.append([one, two])
    return sims, gaps, twins


def diversity(xs: Sequence[Any], E: Any) -> dict:
    """How different one category's variants are, by embedding and by tone."""
    total = len(xs)
    if total < 2:
        return {"n": total}
    vectors = np.asarray(E, dtype=float)
    unit = vectors / (np.linalg.norm(vectors, axis=1, keepdims=True) + 1e-12)
    sims, gaps, twins = _pair_stats(unit, [ft.profile(clip) for clip in xs])
    return {"n": total, "cos_mean": round(float(np.mean(sims)), 3), "cos_max": round(max(sims), 3),
            "tone_min_db": round(min(gaps), 2), "tone_mean_db": round(float(np.mean(gaps)), 2),
            "near_duplicates": twins}


def _brightness(chunk: np.ndarray) -> float:
    mags = np.abs(np.fft.rfft(chunk * np.hanning(len(chunk))))
    hz = np.fft.rfftfreq(len(chunk), 1 / SR)
    return float((mags * hz).sum() / (mags.sum() + 1e-12))


def _decay_shift(mono: np.ndarray) -> float | None:
    """Octaves the spectral centroid moves between the peak and half the t40 later."""
    tail_ms = ft.t40_ms(mono)
    width = int(0.02 * SR)
    at_peak = int(np.argmax(np.abs(mono)))
    later = at_peak + int(tail_ms / 2000 * SR)
    if tail_ms < 40 or later + width > len(mono):
        return None
    early = max(_brightness(mono[at_peak:at_peak + width]), 20)
    return round(math.log2(max(_brightness(mono[later:later + width]), 20) / early), 2)


def _channel_correlation(raw: np.ndarray) -> float:
    if raw.ndim != 2 or raw.shape[1] < 2:
        return 1.0
    left = raw[:, 0] - raw[:, 0].mean()
    right = raw[:, 1] - raw[:, 1].mean()
    norm = math.sqrt(float(np.sum(left ** 2) * np.sum(right ** 2)))
    if norm <= 0:
        return 1.0
    return round(float(np.sum(left * right)) / norm, 3)


def texture(x: Any) -> dict:
    """Noise floor, left/right correlation and how much the sound darkens as it decays."""
    raw = _read(x)
    mono = _downmix(raw)
    coarse = frames_db(mono, 5)
    floor = max(float(np.percentile(coarse, 10) - coarse.max()), -120.0)
    return {"floor_db": round(floor, 1), "stereo_corr": _channel_correlation(raw), "darken": _decay_shift(mono)}


# ---------------------------------------------------------------- model-based checks

def _typicality(e: np.ndarray, A: np.ndarray, k: int) -> float:
    from hark.listen.compare import typicality

    return float(typicality(e, A, k=k)["z"])


class Yardstick:
    """A reference set: its CLAP embeddings plus a frozen tone reference and an envelope reference."""

    def __init__(self, paths: Sequence[str], xs: Sequence[Any], E: Any):
        self.paths = list(paths)
        self.E = np.asarray(E, dtype=float) if len(paths) else np.zeros((0, 1))
        self.tone = ft.ToneRef(xs)
        self.env = EnvRef(xs) if len(xs) else None

    def check(self, x: Any, e: np.ndarray, exclude: int | None = None) -> dict:
        """Typicality, closest cosine, tone and envelope figures against the refs that are kept."""
        chosen = [idx for idx in range(len(self.paths)) if idx != exclude]
        report: dict = {}
        vec = np.asarray(e)
        if len(chosen) >= 3:
            report["typ_z"] = round(_typicality(vec, self.E[chosen], 3), 2)
        if not chosen:
            return report
        report["nn_cos"] = round(float(np.max(self.E[chosen] @ vec)), 3)
        tone = self.tone.check(x, exclude)
        flagged = tone["outside"]
        report["tone_db"] = tone["tone_db"]
        report["tone_ratio"] = tone["tone_ratio"]
        report["n_outside"] = len(flagged)
        report["outside"] = sorted(flagged, key=lambda band: -abs(band[1]))[:3]
        report.update(self.env.check(x, exclude))
        return report


def _embeddings(vocab: Any, paths: Sequence[str]) -> np.ndarray:
    embed = getattr(vocab, "embed_files", None)
    if embed is None:
        from hark.listen.vocabulary import embed_files as embed
    return np.asarray(embed(list(paths)), dtype=float)


def _text_scores(z: np.ndarray, n_phrases: int, decoys: Sequence[str]) -> dict:
    wanted, rivals = z[:n_phrases], z[n_phrases:]
    has_rivals = len(rivals) > 0
    best_rival = float(np.max(rivals)) if has_rivals else -9.0
    return {"phrase_z": round(float(np.mean(wanted)), 2), "phrase_min": round(float(np.min(wanted)), 2),
            "margin": round(float(np.mean(wanted - best_rival)), 2),
            "wins": round(float(np.mean((wanted >= 2.5) & (wanted > best_rival))), 2),
            "best_confusion": decoys[int(np.argmax(rivals))] if has_rivals else None}


def _tagger_scores(tagger: Any, path: str, wanted: Sequence[str]) -> dict:
    probs = np.asarray(tagger.probs_file(path))
    leaders = [(name, float(prob)) for name, prob in tagger.top(probs, k=3)]
    best = max((float(probs[tagger.index[name]]) for name in wanted), default=0.0)
    return {"as_top": leaders, "as_names": any(name in wanted for name, _ in leaders), "as_p": round(best, 3)}


_DETAIL_KEYS = ("clipped_samples", "dc_db", "first_sample", "tail_db", "lead_s", "peak_dbfs")


class _Cat:
    """Everything loaded for one category: clips, embeddings, text scores and both yardsticks."""

    def __init__(self, cat: str, made: list[str], B: list[str], F: list[str], models: tuple):
        vocab, self.tagger = models
        self.cat, self.made, self.B, self.F = cat, made, B, F
        self.allp = made + B + F
        self.X = [_read(path) for path in self.allp]
        self.E = _embeddings(vocab, self.allp)
        self._score_texts(vocab)
        known = getattr(self.tagger, "index", {}) or {}
        self.labs = [name for name in AS_LABELS.get(cat, []) if name in known]
        self._build_yardsticks()

    def _score_texts(self, vocab: Any) -> None:
        phrases = PHRASES.get(self.cat, [])
        self.conf = CONFUSIONS.get(self.cat, [])
        self.nph = len(phrases)
        self.Z = None
        if phrases:
            self.Z = np.asarray(vocab.calibrate(self.E, vocab.embed_text(phrases + self.conf)).z_q)

    def _build_yardsticks(self) -> None:
        split_b = len(self.made)
        split_f = split_b + len(self.B)
        self.ib = list(range(split_b, split_f))
        self.if_ = list(range(split_f, len(self.allp)))
        self.ys_b = Yardstick(self.B, [self.X[idx] for idx in self.ib], self.E[split_b:split_f])
        self.ys_f = Yardstick(self.F, [self.X[idx] for idx in self.if_], self.E[split_f:])

    def row(self, i: int, ys: Yardstick, exclude: int | None = None) -> dict:
        clip, path = self.X[i], self.allp[i]
        line: dict = {"file": path.replace("\\", "/"), "length_s": round(len(clip) / SR, 3)}
        if ys.paths:
            line.update(ys.check(clip, self.E[i], exclude))
        if self.Z is not None:
            line.update(_text_scores(self.Z[i], self.nph, self.conf))
        line.update(_tagger_scores(self.tagger, path, self.labs))
        line.update(texture(clip))
        defects = artifacts(clip, self.cat)
        line["artifacts"] = defects["flags"]
        line["artifact_detail"] = {key: defects[key] for key in _DETAIL_KEYS}
        return line


def summ(rows: Sequence[dict]) -> dict:
    """Median of each metric key, the share of rows with as_names, and the total count of flags."""
    if not rows:
        return {}
    digest: dict = {"n": len(rows)}
    digest.update(_metric_medians(rows))
    named = [bool(line["as_names"]) for line in rows if "as_names" in line]
    if named:
        digest["as_names"] = round(float(np.mean(named)), 2)
    digest["artifacts"] = int(sum(len(line.get("artifacts") or []) for line in rows))
    return digest


def _metric_medians(rows: Sequence[dict]) -> dict:
    medians = {}
    for key in [name for name, _ in METRICS] + list(SUMM_EXTRA):
        present = [line[key] for line in rows if line.get(key) is not None]
        if present:
            medians[key] = round(float(np.median(present)), 2)
    return medians


def _passes(value: float, limit: float, direction: str) -> bool:
    return value <= limit if direction == "low" else value >= limit


def _real_bounds(real: Sequence[dict], metrics: Sequence[tuple], q: float) -> tuple[dict, dict]:
    bounds, direction = {}, {}
    for key, kind in metrics:
        present = [line[key] for line in real if line.get(key) is not None]
        if present:
            bounds[key] = float(np.quantile(present, q if kind == "low" else 1 - q))
            direction[key] = kind
    return bounds, direction


def _count_inside(line: dict, bounds: dict, direction: dict) -> int:
    return sum(1 for key, limit in bounds.items()
               if line.get(key) is not None and _passes(line[key], limit, direction[key]))


def inside_real(made: Sequence[dict], real: Sequence[dict], metrics: Sequence[tuple] = METRICS,
                q: float = 0.8) -> dict:
    """For each made row, how many metrics fall inside the q limits of the real rows."""
    bounds, direction = _real_bounds(real, metrics, q)
    counts = [_count_inside(line, bounds, direction) for line in made]
    return {"per_sound": counts, "n_checks": len(bounds), "limits": {key: round(val, 3) for key, val in bounds.items()}}


def _variety(state: _Cat, picks: list[int]) -> dict:
    if not picks:
        return {"n": 0}
    return diversity([state.X[idx] for idx in picks], state.E[picks])


def _real_rows(state: _Cat, res: dict) -> None:
    """Score the refs against each other: forge vs bench, bench leave-one-out, bench vs forge."""
    if state.B:
        res["real_cross"] = [state.row(idx, state.ys_b) for idx in state.if_]
    if len(state.B) >= 2:
        res["real_loo"] = [state.row(idx, state.ys_b, exclude=pos) for pos, idx in enumerate(state.ib)]
    if state.F:
        res["real_rev"] = [state.row(idx, state.ys_f) for idx in state.ib]


def _add_real(state: _Cat, res: dict) -> None:
    _real_rows(state, res)
    res["diversity_bench"] = _variety(state, state.ib)
    res["diversity_forge"] = _variety(state, state.if_)
    pooled = (res.get("real_cross") or []) + (res.get("real_loo") or [])
    if res["made"] and pooled:
        res["inside"] = inside_real(res["made"], pooled)
        res["real_per_sound"] = inside_real(pooled, pooled)["per_sound"]


_SUMMARY_SETS = (("made", "diversity"), ("real_cross", "diversity_forge"), ("real_loo", "diversity_bench"))


def _set_summaries(res: dict) -> dict:
    digests = {}
    for key, variety_key in _SUMMARY_SETS:
        if key not in res:
            continue
        digest = summ(res[key])
        spread = res.get(variety_key) or {}
        if digest and spread.get("n", 0) >= 2:
            digest["var_cos"] = spread["cos_mean"]
        digests[key] = digest
    return digests


def _registry_entry(reg: dict, cat: str) -> tuple[list, list, str]:
    entry = (reg.get("categories") or {}).get(cat) or {}
    return list(entry.get("bench") or []), list(entry.get("forge") or []), entry.get("note", "")


def bench_category(cat: str, made_paths: Sequence[str], reg: dict, models: tuple, real: bool = True,
                   log: Callable | None = print) -> dict:
    """Score the made sounds of one category, and the real-recording ceilings."""
    bench_refs, forge_refs, note = _registry_entry(reg, cat)
    made = list(made_paths)
    if not made + bench_refs + forge_refs:
        return {"n_bench": 0, "n_forge": 0, "note": note, "made": [], "summary": {}}
    state = _Cat(cat, made, bench_refs, forge_refs, models)
    res: dict = {"n_bench": len(bench_refs), "n_forge": len(forge_refs), "note": note,
                 "made": [state.row(idx, state.ys_b) for idx in range(len(made))]}
    res["diversity"] = _variety(state, list(range(len(made))))
    if real:
        _add_real(state, res)
    res["summary"] = _set_summaries(res)
    if log:
        log(_progress_line(cat, res))
    return res


def _progress_line(cat: str, res: dict) -> str:
    text = "%s: %d made, %s bench, %s forge refs" % (cat, len(res["made"]), res["n_bench"], res["n_forge"])
    counts = res.get("inside")
    if not counts:
        return text
    mine, theirs = np.median(counts["per_sound"]), np.median(res["real_per_sound"])
    return text + "; inside the real 80%% limits on %s of %s checks (a real recording: %s)" % (
        format(mine, "g"), counts["n_checks"], format(theirs, "g"))


# ---------------------------------------------------------------- collecting inputs and running

def _ignored(path: Any) -> bool:
    name = os.path.basename(str(path)).lower()
    return name.startswith(("audition", "_"))


def _wavs_in(folder: Path) -> list[str]:
    return sorted(str(item) for item in folder.glob("*.wav") if not _ignored(item))


def _scan_made(made: Any) -> dict[str, list[str]]:
    found: dict[str, list[str]] = {}
    for sub in sorted(item for item in Path(made).iterdir() if item.is_dir()):
        finals = sub / "final"
        found[sub.name] = _wavs_in(finals if finals.is_dir() else sub)
    return found


def gather(made: Any = None, files: Sequence[str] | None = None) -> dict[str, list[str]]:
    """Made sounds by category from <cat>/final/*.wav (or <cat>/*.wav), plus any CAT=GLOB entries."""
    found = _scan_made(made) if made else {}
    for spec in files or []:
        cat, pattern = spec.split("=", 1)
        found.setdefault(cat, []).extend(sorted(hit for hit in glob.glob(pattern) if not _ignored(hit)))
    return found


def recipe_phrases() -> set[str]:
    """TARGET and ALTS strings of every recipe, trimmed and lower-cased."""
    from hark.forge import recipes as registry

    texts: set[str] = set()
    for name in registry.names():
        try:
            module = registry.load(name)
        except Exception:  # noqa: BLE001 - skip a recipe that fails to import mid-edit
            continue
        candidates = [getattr(module, "TARGET", None)] + list(getattr(module, "ALTS", None) or [])
        texts.update(text.strip().lower() for text in candidates if isinstance(text, str))
    return texts


def _load_models() -> tuple:
    from hark.listen import vocabulary
    from hark.models import ast

    return vocabulary.get(), ast.get()


def _guard_independence() -> None:
    ours = {text.strip().lower() for group in PHRASES.values() for text in group}
    shared = sorted(ours & recipe_phrases())
    if shared:
        raise SystemExit("bench: these phrasings are also recipe TARGET or ALTS texts, so the benchmark is not "
                         "independent: " + "; ".join(shared))


def _pick_categories(opts: argparse.Namespace, made: dict, reg: dict) -> list[str]:
    if opts.cats:
        chosen = opts.cats
    elif opts.real_only:
        chosen = sorted(reg.get("categories") or {})
    else:
        chosen = sorted(made)
    missing = sorted(name for name in made if name in chosen and name not in PHRASES)
    if missing:
        raise SystemExit("bench: made categories " + ", ".join(missing) + " have no phrasings; known: "
                         + ", ".join(sorted(PHRASES)))
    return chosen


_OPTIONS = (
    ("--made", {"default": None}),
    ("--files", {"nargs": "+", "default": None}),
    ("--out", {"required": True}),
    ("--tag", {"default": None}),
    ("--cats", {"nargs": "+", "default": None}),
    ("--real-only", {"action": "store_true"}),
    ("--no-real", {"action": "store_true"}),
    ("--registry", {"default": None}),
    ("--retable", {"action": "store_true"}),
)


def _arg_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="python -m hark.forge.engine.eval.bench")
    for flag, settings in _OPTIONS:
        parser.add_argument(flag, **settings)
    return parser


def _plain_json(obj: Any) -> Any:
    """Round-trip through JSON so numpy values become plain numbers and lists."""
    def convert(item: Any) -> Any:
        if isinstance(item, np.ndarray):
            return item.tolist()
        if isinstance(item, np.generic):
            return item.item()
        return str(item)

    return json.loads(json.dumps(obj, default=convert))


def _rewrite_table(out: Path) -> int:
    saved = json.loads(out.read_text())
    for block in (saved.get("categories") or {}).values():
        block.setdefault("summary", {})
    target = out.with_suffix(".md")
    target.write_text(table(saved))
    print("wrote %s" % target)
    return 0


_TEXT_SOURCES = (("phrases", PHRASES), ("confusions", CONFUSIONS), ("as_labels", AS_LABELS))


def _header(opts: argparse.Namespace, out: Path, reg_path: Any, cats: list[str]) -> dict:
    head = {"tag": opts.tag or out.stem.removeprefix("results_"), "made_from": opts.made,
            "registry": str(reg_path), "created": time.strftime("%Y-%m-%d %H:%M")}
    for key, source in _TEXT_SOURCES:
        head[key] = {cat: source.get(cat) for cat in cats}
    return head


def _save(out: Path, res: dict) -> None:
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(res, indent=1))
    target = out.with_suffix(".md")
    target.write_text(table(res))
    print("wrote %s and %s (%s s)" % (out, target, res["seconds"]))


def main(argv: Sequence[str] | None = None) -> int:
    """Run the benchmark, or with --retable only rebuild the markdown table."""
    from hark.forge.engine import refsets

    opts = _arg_parser().parse_args(argv)
    out = Path(opts.out)
    if opts.retable:
        return _rewrite_table(out)
    reg_path = opts.registry or refsets.registry_path()
    reg = refsets.load(reg_path)
    _guard_independence()
    made = {} if opts.real_only else gather(opts.made, opts.files)
    cats = _pick_categories(opts, made, reg)
    started = time.time()
    models = _load_models()
    res = _header(opts, out, reg_path, cats)
    res["categories"] = {}
    for cat in cats:
        res["categories"][cat] = bench_category(cat, made.get(cat, []), reg, models, real=not opts.no_real)
    res["seconds"] = round(time.time() - started, 1)
    _save(out, scrub_paths.scrub(_plain_json(res), registry=reg))
    return 0


# ---------------------------------------------------------------- markdown report

_COLUMNS = (("typ z", "typ_z"), ("tone dB", "tone_db"), ("env oct", "env_oct"), ("curve dB", "curve_db"),
            ("t40 x", "t40_ratio"), ("phrase z", "phrase_z"), ("margin", "margin"), ("AS names", "as_names"),
            ("artifacts", "artifacts"), ("var cos", "var_cos"))
_ROW_SETS = (("made", "made"), ("real x", "real_cross"), ("real loo", "real_loo"))
LEGEND = ("Each category has up to 3 rows: made (our sounds against the bench refs), real x (the forge refs against "
          "the bench refs, the main yardstick) and real loo (each bench ref against the other bench refs). Model "
          "checks (typ z, phrase z, margin) saturate; read progress from tone, envelope, t40 x, artifacts and "
          "variety. Inside = how many checks fall inside the real 80% limits.")


def _fmt(value: Any) -> str:
    if value is None:
        return "-"
    if isinstance(value, (int, float)):
        return format(value, "g")
    return str(value)


def _inside_text(block: dict) -> str:
    counts = block.get("inside")
    if not counts:
        return "-"
    return "%s / %s (real %s)" % (format(np.median(counts["per_sound"]), "g"), counts["n_checks"],
                                  format(np.median(block["real_per_sound"]), "g"))


def _set_digests(block: dict):
    """(label, key, digest) for each row set that has a summary."""
    digests = block.get("summary") or {}
    for name, key in _ROW_SETS:
        digest = digests.get(key)
        if digest:
            yield name, key, digest


def _metric_lines(cat: str, block: dict) -> list[str]:
    lines = []
    for name, key, digest in _set_digests(block):
        cells = [cat, name, _fmt(digest.get("n"))]
        cells.extend(_fmt(digest.get(field)) for _, field in _COLUMNS)
        cells.append(_inside_text(block) if key == "made" else "")
        lines.append("| " + " | ".join(cells) + " |")
    return lines


def _texture_section(res: dict) -> list[str]:
    lines = ["", "## Texture", "", "| category | set | floor dB | stereo corr | darken |", "|---|---|---|---|---|"]
    for cat, block in (res.get("categories") or {}).items():
        for name, _, digest in _set_digests(block):
            cells = [cat, name] + [_fmt(digest.get(field)) for field in ("floor_db", "stereo_corr", "darken")]
            lines.append("| " + " | ".join(cells) + " |")
    return lines


def _variety_text(spread: dict) -> str:
    if not spread or spread.get("n", 0) < 2:
        return "-"
    return "cos %s / %s, tone min %s dB, %d near duplicate pair(s)" % (
        spread["cos_mean"], spread["cos_max"], spread["tone_min_db"], len(spread["near_duplicates"]))


def _report_head(res: dict, columns: list[str]) -> list[str]:
    origin = "made from %s, registry %s, created %s" % (res.get("made_from"), res.get("registry"), res.get("created"))
    return ["# forge benchmark: %s" % res.get("tag"), "", origin, "", LEGEND, "",
            "| " + " | ".join(columns) + " |", "|" + "---|" * len(columns)]


def table(res: dict) -> str:
    """The results dict as a markdown report."""
    columns = ["category", "set", "n"] + [title for title, _ in _COLUMNS] + ["inside"]
    lines = _report_head(res, columns)
    blocks = res.get("categories") or {}
    notes = []
    for cat, block in blocks.items():
        lines.extend(_metric_lines(cat, block) or ["| %s | no rows |" % cat + " |" * (len(columns) - 2)])
        if block.get("note"):
            notes.append("- %s note: %s" % (cat, block["note"]))
    lines += [""] + notes + _texture_section(res) + ["", "## Diversity", ""]
    for cat, block in blocks.items():
        lines.append("- %s: %s [bench: %s]" % (cat, _variety_text(block.get("diversity") or {}),
                                               _variety_text(block.get("diversity_bench"))))
    return "\n".join(lines) + "\n"


if __name__ == "__main__":
    raise SystemExit(main())
