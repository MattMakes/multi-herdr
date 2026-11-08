"""The held-out forge benchmark: made sounds against the bench refs, with checks no recipe optimises.

Run: `python -m hark.forge.engine.eval.bench --out results_<tag>.json --made <finals folder>`.
Report only. Never select a candidate, steer a search or tune a recipe from these numbers.
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
_PRO_BASE = {"pro_click": "click", "pro_tap": "tap", "pro_hover_tick": "hover_tick", "pro_notify": "notify",
             "pro_shimmer": "shimmer", "pro_typing_key": "typing_key", "pro_glitch": "glitch",
             "pro_whoosh_big": "whoosh_big", "pro_impact": "impact", "pro_sub_drop": "sub_drop"}
for _pro, _base in _PRO_BASE.items():
    CONFUSIONS.setdefault(_pro, CONFUSIONS[_base])

_WHOOSH = "Whoosh, swoosh, swish"
_NOTIFY = ["Ding", "Chime", "Bell", "Glockenspiel", "Jingle, tinkle", "Ding-dong", "Ringtone", "Marimba, xylophone",
           "Vibraphone"]
_IMPACT = ["Boom", "Thump, thud", "Explosion", "Bang", "Bass drum"]
_TYPING = ["Computer keyboard", "Typing", "Typewriter", "Clicking"]
_STING = ["Chime", "Bell", "Ding", "Jingle, tinkle", "Gong", "Cymbal", "Glockenspiel"]
AS_LABELS = {
    "whoosh_fast": [_WHOOSH, "Whip"], "swish": [_WHOOSH, "Whip"], "whoosh_big": [_WHOOSH], "downlifter": [_WHOOSH],
    "swipe": [_WHOOSH, "Rustle"], "riser": [_WHOOSH, "Cymbal", "Drum roll"], "impact": _IMPACT,
    "sub_drop": ["Boom", "Rumble", "Thump, thud"], "click": ["Clicking", "Tick", "Tap"],
    "tap": ["Tap", "Knock", "Clicking", "Tick"], "toggle": ["Clicking", "Mechanisms", "Tick"],
    "hover_tick": ["Tick", "Clicking", "Tick-tock"], "pop": ["Burst, pop", "Plop"], "notify": _NOTIFY,
    "shimmer": ["Chime", "Wind chime", "Jingle, tinkle", "Glockenspiel", "Bell", "Jingle bell"],
    "typing_key": _TYPING, "glitch": ["Static", "Noise", "Beep, bleep", "White noise", "Crackle"],
    "logo_sting": _STING, "pro_pop_in": ["Thump, thud", "Burst, pop", "Plop"],
    "pro_pop": ["Burst, pop", "Plop", "Beep, bleep", "Chirp tone"], "pro_notify": _NOTIFY + ["Beep, bleep"],
    "pro_typing_run": _TYPING, "pro_riser": [_WHOOSH, "Drum roll", "Beep, bleep"],
    "pro_zip": ["Chirp tone", "Sine wave", "Beep, bleep", _WHOOSH], "pro_impact": _IMPACT + ["Rumble"],
    "pro_logo_sting": _STING + ["Beep, bleep", "Synthesizer"],
}
for _pro in ("pro_click", "pro_tap", "pro_hover_tick", "pro_shimmer", "pro_typing_key", "pro_glitch",
             "pro_whoosh_big", "pro_sub_drop"):
    AS_LABELS.setdefault(_pro, AS_LABELS[_PRO_BASE[_pro]])


# ---------------------------------------------------------------- audio helpers

def _load(x: Any) -> np.ndarray:
    if isinstance(x, (str, os.PathLike)):
        from hark.core.audio import load

        return np.asarray(load(str(x), sr=SR), dtype=float)
    return np.asarray(x, dtype=float)


def _mono(x: Any) -> np.ndarray:
    a = _load(x)
    return a.mean(axis=1) if a.ndim == 2 else a


def frames_db(m: np.ndarray, ms: float) -> np.ndarray:
    """Frame RMS levels in dB of ms-long frames (zero-padded to whole frames)."""
    n = max(1, int(SR * ms / 1000))
    k = max(1, len(m) // n)
    a = np.zeros(k * n)
    a[: min(len(m), k * n)] = m[: k * n]
    return 20 * np.log10(np.sqrt(np.mean(a.reshape(k, n) ** 2, axis=1)) + 1e-12)


# ---------------------------------------------------------------- model-free checks

def env_feats(x: Any) -> dict:
    """Onset, attack, t40, active length and peak time of the mono signal."""
    m = _mono(x)
    e1 = frames_db(m, 1)
    p1 = int(np.argmax(e1))
    onset = int(np.argmax(e1 >= e1[p1] - 20))
    e5 = frames_db(m, 5)
    last = int(np.flatnonzero(e5 >= e5.max() - 40)[-1])
    return {"onset_s": round(onset / 1000, 4), "attack_ms": float(max(1, p1 - onset)), "t40_ms": ft.t40_ms(m),
            "active_s": round(max(0.005, (last + 1) * 0.005 - onset / 1000), 3), "peak_s": round(p1 / 1000, 4)}


def curve(x: Any, pre: int, post: int) -> np.ndarray:
    """10 ms level curve re the peak, floor -60, with the peak frame at index pre."""
    e = frames_db(_mono(x), 10)
    p = int(np.argmax(e))
    c = np.maximum(e - e[p], -60.0)
    out = np.full(pre + post, -60.0)
    for i, v in enumerate(c):
        j = i - p + pre
        if 0 <= j < len(out):
            out[j] = v
    return out


_ENV_KEYS = (("attack_ms", 1.0), ("t40_ms", 5.0), ("active_s", 0.01))


class EnvRef:
    """The benchmark's own envelope reference (separate from the live tone module)."""

    PRE = 10

    def __init__(self, refs: Sequence[Any]):
        xs = [_load(r) for r in refs]
        self.feats = [env_feats(x) for x in xs]
        tail = max((f["onset_s"] + f["active_s"] - f["peak_s"] for f in self.feats), default=0.1)
        self.post = int(min(800, max(10, math.ceil(1.25 * tail / 0.01))))
        self.C = np.array([curve(x, self.PRE, self.post) for x in xs]) if xs else None

    def check(self, x: Any, exclude: int | None = None) -> dict:
        """Envelope distance of x to the kept refs."""
        keep = [i for i in range(len(self.feats)) if i != exclude]
        if not keep:
            return {}
        f = env_feats(x)
        out = {k: f[k] for k in ("attack_ms", "t40_ms", "active_s", "onset_s")}
        logs, med = {}, {}
        for k, floor in _ENV_KEYS:
            med[k] = float(np.median([self.feats[i][k] for i in keep]))
            logs[k] = math.log2(max(f[k], floor) / max(med[k], floor))
        c = curve(x, self.PRE, self.post)
        out["env_oct"] = round(float(np.mean([abs(v) for v in logs.values()])), 3)
        out["curve_db"] = round(float(np.sqrt(np.mean((c - np.median(self.C[keep], axis=0)) ** 2))), 2)
        out.update({f"{k.split('_')[0]}_ratio": round(2 ** v, 3) for k, v in logs.items()})
        out["ref_median"] = {k: round(v, 3) for k, v in med.items()}
        return out


def _art_numbers(x: Any) -> dict:
    a = _load(x)
    m = a.mean(axis=1) if a.ndim == 2 else a
    pk = float(np.max(np.abs(m))) + 1e-12
    e1, e5 = frames_db(m, 1), frames_db(m, 5)
    return {"clipped_samples": int(np.sum(np.abs(a) >= 0.999)),
            "dc_db": round(float(20 * np.log10(abs(float(np.mean(m))) / pk + 1e-12)), 1),
            "first_sample": round(abs(float(m[0])) / pk, 3),
            "tail_db": round(float(frames_db(m[-480:], 10)[-1] - e5.max()), 1),
            "last_ms_rel": round(float(np.sqrt(np.mean(m[-48:] ** 2))) / pk, 3),
            "lead_s": round(int(np.argmax(e1 >= e1.max() - 50)) / 1000, 3),
            "peak_dbfs": round(float(20 * np.log10(np.max(np.abs(a)) + 1e-12)), 2)}


def artifacts(x: Any, cat: str | None = None) -> dict:
    """Artifact numbers and the flag list (`flags`)."""
    d = _art_numbers(x)
    tests = [("clipped", d["clipped_samples"] >= 3), ("dc offset", d["dc_db"] > -30),
             ("start click", d["first_sample"] > 0.05), ("end click", d["last_ms_rel"] > 0.02),
             ("tail cut", d["tail_db"] > -30), ("late start", d["lead_s"] > 0.1)]
    ends = cat in ENDS_ON_PEAK
    d["flags"] = [name for name, hit in tests
                  if hit and not (ends and name in ("end click", "tail cut", "late start"))]
    return d


def diversity(xs: Sequence[Any], E: Any) -> dict:
    """Embedding and tone variety among one category's variants."""
    n = len(xs)
    if n < 2:
        return {"n": n}
    U = np.asarray(E, dtype=float)
    U = U / (np.linalg.norm(U, axis=1, keepdims=True) + 1e-12)
    P = [ft.profile(x) for x in xs]
    cos, td, near = [], [], []
    for i in range(n):
        for j in range(i + 1, n):
            c, d = float(U[i] @ U[j]), float(np.sqrt(np.mean((P[i] - P[j]) ** 2)))
            cos.append(c)
            td.append(d)
            if c > 0.985 and d < 1.5:
                near.append([i, j])
    return {"n": n, "cos_mean": round(float(np.mean(cos)), 3), "cos_max": round(max(cos), 3),
            "tone_min_db": round(min(td), 2), "tone_mean_db": round(float(np.mean(td)), 2), "near_duplicates": near}


def _centroid(seg: np.ndarray) -> float:
    F = np.abs(np.fft.rfft(seg * np.hanning(len(seg))))
    f = np.fft.rfftfreq(len(seg), 1 / SR)
    return float((F * f).sum() / (F.sum() + 1e-12))


def _darken(m: np.ndarray) -> float | None:
    t40 = ft.t40_ms(m)
    n = int(0.02 * SR)
    p = int(np.argmax(np.abs(m)))
    late = p + int(t40 / 2000 * SR)
    if t40 < 40 or late + n > len(m):
        return None
    return round(math.log2(max(_centroid(m[late:late + n]), 20) / max(_centroid(m[p:p + n]), 20)), 2)


def _corr(a: np.ndarray) -> float:
    if a.ndim != 2 or a.shape[1] < 2:
        return 1.0
    L, R = a[:, 0] - a[:, 0].mean(), a[:, 1] - a[:, 1].mean()
    den = math.sqrt(float(np.sum(L ** 2) * np.sum(R ** 2)))
    return round(float(np.sum(L * R)) / den, 3) if den > 0 else 1.0


def texture(x: Any) -> dict:
    """Noise floor, stereo correlation and decay darkening."""
    a = _load(x)
    m = a.mean(axis=1) if a.ndim == 2 else a
    e5 = frames_db(m, 5)
    return {"floor_db": round(max(float(np.percentile(e5, 10) - e5.max()), -120.0), 1), "stereo_corr": _corr(a),
            "darken": _darken(m)}


# ---------------------------------------------------------------- model checks

def _typicality(e: np.ndarray, A: np.ndarray, k: int) -> float:
    from hark.listen.compare import typicality

    return float(typicality(e, A, k=k)["z"])


class Yardstick:
    """One ref set: CLAP embeddings, a frozen tone reference and an envelope reference."""

    def __init__(self, paths: Sequence[str], xs: Sequence[Any], E: Any):
        self.paths, self.E = list(paths), np.asarray(E, dtype=float) if len(paths) else np.zeros((0, 1))
        self.tone = ft.ToneRef(xs)
        self.env = EnvRef(xs) if len(xs) else None

    def check(self, x: Any, e: np.ndarray, exclude: int | None = None) -> dict:
        """Typicality, nearest cosine, tone and envelope numbers against the kept refs."""
        keep = [i for i in range(len(self.paths)) if i != exclude]
        out: dict = {}
        if len(keep) >= 3:
            out["typ_z"] = round(_typicality(np.asarray(e), self.E[keep], 3), 2)
        if keep:
            out["nn_cos"] = round(float(np.max(self.E[keep] @ np.asarray(e))), 3)
            t = self.tone.check(x, exclude)
            out.update(tone_db=t["tone_db"], tone_ratio=t["tone_ratio"], n_outside=len(t["outside"]),
                       outside=sorted(t["outside"], key=lambda o: -abs(o[1]))[:3])
            out.update(self.env.check(x, exclude))
        return out


def _embed_files(vocab: Any, paths: Sequence[str]) -> np.ndarray:
    fn = getattr(vocab, "embed_files", None)
    if fn is None:
        from hark.listen.vocabulary import embed_files as fn
    return np.asarray(fn(list(paths)), dtype=float)


def _phrase_terms(z: np.ndarray, nph: int, conf: Sequence[str]) -> dict:
    zp, zc = z[:nph], z[nph:]
    best_c = float(np.max(zc)) if len(zc) else -9.0
    return {"phrase_z": round(float(np.mean(zp)), 2), "phrase_min": round(float(np.min(zp)), 2),
            "margin": round(float(np.mean(zp - best_c)), 2),
            "wins": round(float(np.mean((zp >= 2.5) & (zp > best_c))), 2),
            "best_confusion": conf[int(np.argmax(zc))] if len(zc) else None}


def _as_terms(tagger: Any, path: str, labs: Sequence[str]) -> dict:
    p = np.asarray(tagger.probs_file(path))
    top = [(lab, float(pr)) for lab, pr in tagger.top(p, k=3)]
    return {"as_top": top, "as_names": any(lab in labs for lab, _ in top),
            "as_p": round(max((float(p[tagger.index[lab]]) for lab in labs), default=0.0), 3)}


class _Cat:
    """The loaded state of one category run."""

    def __init__(self, cat: str, made: list[str], B: list[str], F: list[str], models: tuple):
        self.cat, self.made, self.B, self.F = cat, made, B, F
        self.allp = made + B + F
        self.X = [_load(p) for p in self.allp]
        vocab, self.tagger = models
        self.E = _embed_files(vocab, self.allp)
        phr, self.conf = PHRASES.get(cat, []), CONFUSIONS.get(cat, [])
        self.nph = len(phr)
        self.Z = np.asarray(vocab.calibrate(self.E, vocab.embed_text(phr + self.conf)).z_q) if phr else None
        index = getattr(self.tagger, "index", {}) or {}
        self.labs = [lab for lab in AS_LABELS.get(cat, []) if lab in index]
        nm, nb = len(made), len(B)
        self.ib, self.if_ = list(range(nm, nm + nb)), list(range(nm + nb, len(self.allp)))
        self.ys_b = Yardstick(B, [self.X[i] for i in self.ib], self.E[nm:nm + nb])
        self.ys_f = Yardstick(F, [self.X[i] for i in self.if_], self.E[nm + nb:])

    def row(self, i: int, ys: Yardstick, exclude: int | None = None) -> dict:
        x = self.X[i]
        r: dict = {"file": self.allp[i].replace("\\", "/"), "length_s": round(len(x) / SR, 3)}
        if ys.paths:
            r.update(ys.check(x, self.E[i], exclude))
        if self.Z is not None:
            r.update(_phrase_terms(self.Z[i], self.nph, self.conf))
        r.update(_as_terms(self.tagger, self.allp[i], self.labs))
        r.update(texture(x))
        art = artifacts(x, self.cat)
        r["artifacts"] = art["flags"]
        r["artifact_detail"] = {k: art[k] for k in ("clipped_samples", "dc_db", "first_sample", "tail_db", "lead_s",
                                                    "peak_dbfs")}
        return r


def summ(rows: Sequence[dict]) -> dict:
    """Medians of the metric keys, the as_names share and the total flag count."""
    if not rows:
        return {}
    out: dict = {"n": len(rows)}
    for k in [m for m, _ in METRICS] + list(SUMM_EXTRA):
        vals = [r[k] for r in rows if r.get(k) is not None]
        if vals:
            out[k] = round(float(np.median(vals)), 2)
    names = [bool(r["as_names"]) for r in rows if "as_names" in r]
    if names:
        out["as_names"] = round(float(np.mean(names)), 2)
    out["artifacts"] = int(sum(len(r.get("artifacts") or []) for r in rows))
    return out


def inside_real(made: Sequence[dict], real: Sequence[dict], metrics: Sequence[tuple] = METRICS,
                q: float = 0.8) -> dict:
    """Per made row, the number of metrics inside the real q limits."""
    limits, kinds = {}, {}
    for key, kind in metrics:
        vals = [r[key] for r in real if r.get(key) is not None]
        if vals:
            limits[key] = float(np.quantile(vals, q if kind == "low" else 1 - q))
            kinds[key] = kind
    per = []
    for r in made:
        per.append(sum(1 for k, lim in limits.items() if r.get(k) is not None and
                       (r[k] <= lim if kinds[k] == "low" else r[k] >= lim)))
    return {"per_sound": per, "n_checks": len(limits), "limits": {k: round(v, 3) for k, v in limits.items()}}


def _real_sets(c: _Cat, res: dict) -> None:
    if c.B:
        res["real_cross"] = [c.row(i, c.ys_b) for i in c.if_]
    if len(c.B) >= 2:
        res["real_loo"] = [c.row(i, c.ys_b, exclude=k) for k, i in enumerate(c.ib)]
    if c.F:
        res["real_rev"] = [c.row(i, c.ys_f) for i in c.ib]
    res["diversity_bench"] = diversity([c.X[i] for i in c.ib], c.E[c.ib]) if c.ib else {"n": 0}
    res["diversity_forge"] = diversity([c.X[i] for i in c.if_], c.E[c.if_]) if c.if_ else {"n": 0}
    real = (res.get("real_cross") or []) + (res.get("real_loo") or [])
    if res["made"] and real:
        res["inside"] = inside_real(res["made"], real)
        res["real_per_sound"] = inside_real(real, real)["per_sound"]


def _summaries(res: dict) -> dict:
    out = {}
    for key, div in (("made", "diversity"), ("real_cross", "diversity_forge"), ("real_loo", "diversity_bench")):
        if key not in res:
            continue
        s = summ(res[key])
        d = res.get(div) or {}
        if s and d.get("n", 0) >= 2:
            s["var_cos"] = d["cos_mean"]
        out[key] = s
    return out


def bench_category(cat: str, made_paths: Sequence[str], reg: dict, models: tuple, real: bool = True,
                   log: Callable | None = print) -> dict:
    """Score one category's made sounds and real ceilings."""
    entry = (reg.get("categories") or {}).get(cat) or {}
    B, F = list(entry.get("bench") or []), list(entry.get("forge") or [])
    made = list(made_paths)
    if not made + B + F:
        return {"n_bench": 0, "n_forge": 0, "note": entry.get("note", ""), "made": [], "summary": {}}
    c = _Cat(cat, made, B, F, models)
    res: dict = {"n_bench": len(B), "n_forge": len(F), "note": entry.get("note", ""),
                 "made": [c.row(i, c.ys_b) for i in range(len(made))]}
    res["diversity"] = diversity([c.X[i] for i in range(len(made))], c.E[: len(made)]) if made else {"n": 0}
    if real:
        _real_sets(c, res)
    res["summary"] = _summaries(res)
    if log:
        log(_cat_line(cat, res))
    return res


def _cat_line(cat: str, res: dict) -> str:
    s = f"{cat}: {len(res['made'])} made, {res['n_bench']} bench, {res['n_forge']} forge refs"
    ins = res.get("inside")
    if ins:
        s += (f"; inside the real 80% limits on {np.median(ins['per_sound']):g} of {ins['n_checks']} checks "
              f"(a real recording: {np.median(res['real_per_sound']):g})")
    return s


# ---------------------------------------------------------------- run

def _skip(path: Any) -> bool:
    b = os.path.basename(str(path)).lower()
    return b.startswith("audition") or b.startswith("_")


def gather(made: Any = None, files: Sequence[str] | None = None) -> dict[str, list[str]]:
    """Made sounds per category: <cat>/final/*.wav (else <cat>/*.wav), plus CAT=GLOB specs."""
    out: dict[str, list[str]] = {}
    if made:
        for sub in sorted(p for p in Path(made).iterdir() if p.is_dir()):
            d = sub / "final" if (sub / "final").is_dir() else sub
            out[sub.name] = sorted(str(f) for f in d.glob("*.wav") if not _skip(f))
    for spec in files or []:
        cat, pat = spec.split("=", 1)
        out.setdefault(cat, []).extend(sorted(f for f in glob.glob(pat) if not _skip(f)))
    return out


def recipe_phrases() -> set[str]:
    """Every recipe's TARGET and ALTS text, stripped and lower-case."""
    from hark.forge import recipes as registry

    out: set[str] = set()
    for name in registry.names():
        try:
            mod = registry.load(name)
        except Exception:  # noqa: BLE001 - a recipe under edit is skipped
            continue
        for t in [getattr(mod, "TARGET", None)] + list(getattr(mod, "ALTS", None) or []):
            if isinstance(t, str):
                out.add(t.strip().lower())
    return out


def _load_models() -> tuple:
    from hark.listen import vocabulary
    from hark.models import ast

    return vocabulary.get(), ast.get()


def _check_phrases() -> None:
    mine = {t.strip().lower() for ts in PHRASES.values() for t in ts}
    clash = sorted(mine & recipe_phrases())
    if clash:
        raise SystemExit("bench: these phrasings are also recipe TARGET or ALTS texts, so the benchmark is not "
                         f"independent: {'; '.join(clash)}")


def _categories(a: argparse.Namespace, made: dict, reg: dict) -> list[str]:
    cats = a.cats or (sorted(reg.get("categories") or {}) if a.real_only else sorted(made))
    unknown = sorted(c for c in made if c in cats and c not in PHRASES)
    if unknown:
        raise SystemExit(f"bench: made categories {', '.join(unknown)} have no phrasings; known: "
                         f"{', '.join(sorted(PHRASES))}")
    return cats


def _parser() -> argparse.ArgumentParser:
    ap = argparse.ArgumentParser(prog="python -m hark.forge.engine.eval.bench")
    ap.add_argument("--made", default=None)
    ap.add_argument("--files", nargs="+", default=None)
    ap.add_argument("--out", required=True)
    ap.add_argument("--tag", default=None)
    ap.add_argument("--cats", nargs="+", default=None)
    ap.add_argument("--real-only", action="store_true")
    ap.add_argument("--no-real", action="store_true")
    ap.add_argument("--registry", default=None)
    ap.add_argument("--retable", action="store_true")
    return ap


def _json_round(obj: Any) -> Any:
    def default(o: Any) -> Any:
        if isinstance(o, np.ndarray):
            return o.tolist()
        if isinstance(o, np.generic):
            return o.item()
        return str(o)

    return json.loads(json.dumps(obj, default=default))


def _retable(out: Path) -> int:
    res = json.loads(out.read_text())
    for c in (res.get("categories") or {}).values():
        c.setdefault("summary", {})
    md = out.with_suffix(".md")
    md.write_text(table(res))
    print(f"wrote {md}")
    return 0


def main(argv: Sequence[str] | None = None) -> int:
    """Run the benchmark (or rewrite the table with --retable)."""
    from hark.forge.engine import refsets

    a = _parser().parse_args(argv)
    out = Path(a.out)
    if a.retable:
        return _retable(out)
    reg_path = a.registry or refsets.registry_path()
    reg = refsets.load(reg_path)
    _check_phrases()
    made = {} if a.real_only else gather(a.made, a.files)
    cats = _categories(a, made, reg)
    t0 = time.time()
    models = _load_models()
    res: dict = {"tag": a.tag or out.stem.removeprefix("results_"), "made_from": a.made, "registry": str(reg_path),
                 "created": time.strftime("%Y-%m-%d %H:%M"), "phrases": {c: PHRASES.get(c) for c in cats},
                 "confusions": {c: CONFUSIONS.get(c) for c in cats}, "as_labels": {c: AS_LABELS.get(c) for c in cats},
                 "categories": {c: bench_category(c, made.get(c, []), reg, models, real=not a.no_real) for c in cats}}
    res["seconds"] = round(time.time() - t0, 1)
    res = scrub_paths.scrub(_json_round(res), registry=reg)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(res, indent=1))
    out.with_suffix(".md").write_text(table(res))
    print(f"wrote {out} and {out.with_suffix('.md')} ({res['seconds']} s)")
    return 0


# ---------------------------------------------------------------- table

_COLS = (("typ z", "typ_z"), ("tone dB", "tone_db"), ("env oct", "env_oct"), ("curve dB", "curve_db"),
         ("t40 x", "t40_ratio"), ("phrase z", "phrase_z"), ("margin", "margin"), ("AS names", "as_names"),
         ("artifacts", "artifacts"), ("var cos", "var_cos"))
_SETS = (("made", "made"), ("real x", "real_cross"), ("real loo", "real_loo"))
LEGEND = ("Each category has up to 3 rows: made (our sounds against the bench refs), real x (the forge refs against "
          "the bench refs, the main yardstick) and real loo (each bench ref against the other bench refs). Model "
          "checks (typ z, phrase z, margin) saturate; read progress from tone, envelope, t40 x, artifacts and "
          "variety. Inside = how many checks fall inside the real 80% limits.")


def _cell(v: Any) -> str:
    return "-" if v is None else f"{v:g}" if isinstance(v, (int, float)) else str(v)


def _inside_cell(c: dict) -> str:
    ins = c.get("inside")
    if not ins:
        return "-"
    return f"{np.median(ins['per_sound']):g} / {ins['n_checks']} (real {np.median(c['real_per_sound']):g})"


def _cat_rows(cat: str, c: dict) -> list[str]:
    rows = []
    for label, key in _SETS:
        s = (c.get("summary") or {}).get(key)
        if not s:
            continue
        cells = [cat, label, _cell(s.get("n"))] + [_cell(s.get(k)) for _, k in _COLS]
        cells.append(_inside_cell(c) if key == "made" else "")
        rows.append("| " + " | ".join(cells) + " |")
    return rows


def _texture_rows(res: dict) -> list[str]:
    out = ["", "## Texture", "", "| category | set | floor dB | stereo corr | darken |", "|---|---|---|---|---|"]
    for cat, c in (res.get("categories") or {}).items():
        for label, key in _SETS:
            s = (c.get("summary") or {}).get(key)
            if s:
                out.append(f"| {cat} | {label} | {_cell(s.get('floor_db'))} | {_cell(s.get('stereo_corr'))} | "
                           f"{_cell(s.get('darken'))} |")
    return out


def _div_text(d: dict) -> str:
    if not d or d.get("n", 0) < 2:
        return "-"
    return (f"cos {d['cos_mean']} / {d['cos_max']}, tone min {d['tone_min_db']} dB, {len(d['near_duplicates'])} "
            "near duplicate pair(s)")


def table(res: dict) -> str:
    """Markdown table of a results dict."""
    head = ["category", "set", "n"] + [h for h, _ in _COLS] + ["inside"]
    out = [f"# forge benchmark: {res.get('tag')}", "",
           f"made from {res.get('made_from')}, registry {res.get('registry')}, created {res.get('created')}", "",
           LEGEND, "", "| " + " | ".join(head) + " |", "|" + "---|" * len(head)]
    notes = []
    for cat, c in (res.get("categories") or {}).items():
        rows = _cat_rows(cat, c)
        out += rows or [f"| {cat} | no rows |" + " |" * (len(head) - 2)]
        if c.get("note"):
            notes.append(f"- {cat} note: {c['note']}")
    out += [""] + notes + _texture_rows(res) + ["", "## Diversity", ""]
    for cat, c in (res.get("categories") or {}).items():
        out.append(f"- {cat}: {_div_text(c.get('diversity') or {})} [bench: {_div_text(c.get('diversity_bench'))}]")
    return "\n".join(out) + "\n"


if __name__ == "__main__":
    raise SystemExit(main())
