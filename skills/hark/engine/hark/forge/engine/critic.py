"""The critic: scores made sounds against a text target, confusions and real refs.

Model terms (text-audio match, realness, typicality, feature spread) come from
the listen unit through one services object (`services()`), so the model-free
terms (tone, envelope, artifacts, polish, study targets) and the tests do not
need the ML extras. Each sound is embedded once and every model term reuses it.
"""

from __future__ import annotations

import copy
import hashlib
import json
import math
import os
import threading
from collections.abc import Callable, Sequence
from pathlib import Path
from typing import Any

import numpy as np

from hark.forge.engine import polish as pol
from hark.forge.engine import tone as tn

SR = 48000
FEATS = ("attack_ms", "decay_ms", "brightness_hz", "length_s", "low_end_db", "bandwidth_hz")
LOG = ("attack_ms", "decay_ms", "brightness_hz", "length_s", "bandwidth_hz")
W2 = {"match": 1.0, "real": 2.0, "typ": 0.5, "feat": 0.7, "alt": 1.0, "tone": 1.0, "env": 1.0, "art": 1.0,
      "polish": 0.75}
DESIGNED_WORDS = ("glitch", "stutter", "sub drop", "sub-drop", "subdrop", "blip", "beep", "bleep", "synth", "riser",
                  "downlifter", "uplifter", "stinger", "logo sting", "braam", "laser", "zap", "notification",
                  "shimmer", "bitcrush", "digital")
ENV_PASS, POLISH_PASS, TONE_PASS = 1.5, 1.5, 1.25
GATED = ("glitch", "pro_glitch", "riser", "pro_riser", "zip", "pro_zip", "pro_impact")
SHARE_KEYS = {"sh150": "<150", "sh1k": "150-1k", "sh4k": "1-4k", "sh12k": "4-12k", "shhi": ">12k"}
_NEG = -200.0

STUDY_TARGETS: dict[str, dict[str, tuple[float, float]]] = {
    "pro_whoosh_big": {"length": (0.5, 1.2), "attack": (100, 350), "t20": (60, 160), "t40": (180, 400),
                       "sh150": (-1.5, 0.0), "corr": (0.8, 1.0)},
    "riser": {"length": (1.0, 6.0), "peak_frac": (0.7, 1.0), "t40": (0, 400), "corr": (0.5, 1.0)},
    "pro_riser": {"length": (0.8, 2.5), "peak_frac": (0.75, 1.0), "t40": (0, 300), "corr": (0.8, 1.0)},
    "impact": {"length": (1.0, 8.0), "t40": (1000, 3000), "sh150": (-2.5, -0.2)},
    "pro_impact": {"length": (0.6, 1.3), "attack": (40, 300), "t40": (250, 1100), "sh150": (-1.25, 0.0),
                   "corr": (-0.1, 0.6)},
    "sub_drop": {"length": (0.8, 3.0), "attack": (0, 450), "sh150": (-1.0, 0.0), "corr": (0.8, 1.0)},
    "pro_sub_drop": {"length": (0.8, 1.6), "attack": (250, 460), "t40": (600, 1600), "sh150": (-0.7, 0.0),
                     "corr": (0.8, 1.0)},
    "logo_sting": {"length": (0.8, 2.5), "t40": (250, 1200), "corr": (0.6, 1.0)},
    "pro_logo_sting": {"length": (0.5, 1.5), "attack": (0, 90), "t20": (30, 110), "t40": (150, 450),
                       "corr": (0.9, 1.0)},
    "glitch": {"length": (0.2, 1.5), "sh150": (_NEG, -10), "corr": (-0.3, 0.6)},
    "pro_glitch": {"length": (0.3, 1.0), "sh150": (_NEG, -12), "sh12k": (-8, 0), "corr": (-0.3, 0.4)},
    "pro_shimmer": {"length": (1.4, 2.6), "attack": (150, 800), "sh150": (_NEG, -30), "corr": (-0.2, 0.45)},
    "pro_pop": {"length": (0.05, 0.2), "attack": (0, 60), "t20": (3, 30), "corr": (0.9, 1.0)},
    "pro_tap": {"length": (0.06, 0.2), "attack": (2, 15), "t20": (10, 30), "t40": (18, 50), "corr": (0.85, 1.0)},
    "pro_click": {"length": (0.02, 0.08), "attack": (0, 4), "t20": (4, 20), "sh150": (_NEG, -20),
                  "sh12k": (-10, 0), "corr": (0.9, 1.0)},
    "pro_typing_run": {"sh150": (_NEG, -25), "sh1k": (_NEG, -20), "corr": (0.9, 1.0)},
    "pro_zip": {"length": (0.12, 0.4), "attack": (10, 50), "corr": (0.85, 1.0)},
    "zip": {"length": (0.12, 0.4), "attack": (10, 50), "corr": (0.85, 1.0)},
    "downlifter": {"length": (1.0, 4.0), "attack": (0, 80), "t40": (500, 3500), "corr": (0.3, 1.0)},
}

_CAPS = {"tone": 3.0, "env": 2.0, "polish": 1.5}


# ---------------------------------------------------------------- listen services

class ListenServices:
    """The listen-unit calls the critic makes. Loads the models on construction."""

    name = "listen"

    def __init__(self) -> None:
        from hark.models import require_ml

        require_ml("forge critic")
        from hark.listen import compare, realness, vocabulary

        self._voc_mod, self._real, self._cmp = vocabulary, realness, compare
        self.voc = vocabulary.get()
        self.model = realness.load_model()

    def embed_text(self, texts: Sequence[str]) -> np.ndarray:
        return self.voc.embed_text(list(texts))

    def calibrate_z(self, E: np.ndarray, Q: np.ndarray) -> np.ndarray:
        return np.asarray(self.voc.calibrate(E, Q).z_q)

    def top_terms(self, e: np.ndarray, k: int = 3) -> list[str]:
        return [t[0] for t in self.voc.top_terms(e, k)]

    def embed_files(self, paths: Sequence[str]) -> np.ndarray:
        return np.asarray(self._voc_mod.embed_files(list(paths), pad=self._voc_mod.PAD))

    def embed_audio(self, arrays: Sequence[np.ndarray]) -> np.ndarray:
        return np.asarray(self._voc_mod.embed_audio(list(arrays), pad=self._voc_mod.PAD))

    def audible(self, m: np.ndarray) -> bool:
        return bool(self._real.audible(m))

    def pure_tone(self, m: np.ndarray) -> bool:
        return bool(self._real.pure_tone(m))

    def p_real(self, m: np.ndarray, e: np.ndarray) -> float:
        heads = self.model["heads"]
        sf = self._real.signal_features(m)
        lc = self._real.head_logit(heads["clap"], e)
        ls = self._real.head_logit(heads["signal"], [sf[k] for k in heads["signal"]["inputs"]]) if sf else 0.0
        return float(self._real.fuse(self.model, {"clap": lc, "signal": ls}))

    def feats(self, x: np.ndarray) -> dict:
        from hark.core.features import measure

        return dict(self._cmp.feats(measure(x, light=True), x))

    def typicality(self, e: np.ndarray, A: np.ndarray, k: int) -> float | None:
        return self._cmp.typicality(e, A, k).get("z")

    def tagger_name(self) -> str:
        from hark.models import ast

        return str(getattr(ast.get(), "model_id", "ast")).replace("/", "_")

    def tags(self, m: np.ndarray) -> tuple[list, float]:
        from hark.models import ast

        T = ast.get()
        p = np.asarray(T.probs(m)) if hasattr(T, "probs") else self._probs_via_file(T, m)
        synth = [T.index[c] for c in self._real.SYNTH_CLASSES if c in T.index] \
            if hasattr(self._real, "SYNTH_CLASSES") else []
        return list(T.top(p, 3)), round(float(max((p[i] for i in synth), default=0.0)), 3)

    @staticmethod
    def _probs_via_file(T: Any, m: np.ndarray) -> np.ndarray:
        import tempfile

        from hark.core.audio import write_wav

        with tempfile.TemporaryDirectory() as d:
            path = write_wav(os.path.join(d, "x.wav"), m)
            return np.asarray(T.probs_file(path))


_SERVICES: Any = None
_SVC_LOCK = threading.Lock()


def services() -> Any:
    """The process-wide listen services (built on first use)."""
    global _SERVICES
    with _SVC_LOCK:
        if _SERVICES is None:
            _SERVICES = ListenServices()
        return _SERVICES


# ---------------------------------------------------------------- study targets

def _corr(x: np.ndarray) -> float:
    if x.ndim != 2:
        return 1.0
    a, b = x[:, 0] - x[:, 0].mean(), x[:, 1] - x[:, 1].mean()
    den = math.sqrt(float(np.sum(a * a) * np.sum(b * b)))
    return round(float(np.sum(a * b) / den), 3) if den > 0 else 1.0


def _span_measures(m: np.ndarray) -> dict:
    k = len(m) // 480
    if k < 2:
        return {}
    e = 10 * np.log10(np.mean(m[: k * 480].reshape(k, 480) ** 2, axis=1) + 1e-20)
    act = np.flatnonzero(e >= e.max() - 50)
    span = act[-1] - act[0] + 1
    return {"length": round(span * 0.01, 3), "peak_frac": round((int(np.argmax(e)) - act[0] + 0.5) / span, 3)}


def study_measures(x: Any) -> dict:
    """Length, peak position, attack and decay times, region shares and stereo correlation."""
    from hark.forge.engine.search import envelope_stats

    a = np.asarray(x, dtype=float) if not isinstance(x, (str, os.PathLike)) else tn.as_mono(x)
    m = a.mean(axis=1) if a.ndim == 2 else a
    out = _span_measures(m)
    env = envelope_stats(m)
    out.update(attack=round(env["attack_ms"], 1), t20=round(env["t20_ms"], 1), t40=round(env["t40_ms"], 1))
    sh = tn.shares(m)
    out.update({k: sh[r] for k, r in SHARE_KEYS.items()})
    out["corr"] = _corr(a)
    return out


def widen_targets(targets: dict, ref_measures: Sequence[dict]) -> dict:
    """Each range grows to include every ref's value."""
    out = {}
    for k, (lo, hi) in targets.items():
        vals = [m[k] for m in ref_measures if m.get(k) is not None]
        out[k] = (min([lo] + vals), max([hi] + vals))
    return out


def _dist(v: float, lo: float, hi: float) -> float:
    return max(lo - v, v - hi, 0.0)


def _study_one(key: str, v: float, lo: float, hi: float) -> tuple[str, float, str]:
    if key in SHARE_KEYS:
        return "tone", min(2.0, _dist(v, lo, hi) / 6), f"{SHARE_KEYS[key]} share {v:+.1f} dB (target {lo}..{hi})"
    if key == "corr":
        return "polish", min(1.5, _dist(v, lo, hi) / 0.3), f"stereo corr {v:.2f} (target {lo}..{hi})"
    if key == "peak_frac":
        return "env", min(1.5, _dist(v, lo, hi) / 0.25), \
            f"peak at {v:.0%} of the sound (target {lo:.0%}..{hi:.0%})"
    fl, unit = (1e-3, "s") if key == "length" else (1.0, "ms")
    vv, lo2, hi2 = max(v, fl), max(lo, fl), max(hi, lo, fl)
    p = min(1.5, max(math.log2(lo2 / vv), math.log2(vv / hi2), 0.0))
    return "env", p, f"{key} {v}{unit} (target {lo}..{hi})"


def study_penalties(sm: dict, targets: dict) -> tuple[dict, dict, str]:
    """(penalty per part, off-target keys, words) of study measures against targets."""
    pen = {"tone": 0.0, "env": 0.0, "polish": 0.0}
    off, texts = {}, []
    for key, (lo, hi) in targets.items():
        if sm.get(key) is None:
            continue
        part, p, text = _study_one(key, float(sm[key]), lo, hi)
        if p > 0:
            off[key] = round(p, 3)
            pen[part] += p
            texts.append(text)
    pen = {k: min(_CAPS[k], v) for k, v in pen.items()}
    return pen, off, ("study targets: " + "; ".join(texts)) if texts else "study targets met"


# ---------------------------------------------------------------- pure helpers

def holdout_key(mono: np.ndarray) -> str:
    """sha1 of the mono float32 samples."""
    return hashlib.sha1(np.ascontiguousarray(np.asarray(mono, dtype=np.float32)).tobytes()).hexdigest()


def resolve_mode(mode: str, target: str, refs: Sequence[Any] | None) -> str:
    """'real' or 'designed'; auto is designed with no refs or a designed target word."""
    if mode in ("real", "designed"):
        return mode
    low = (target or "").lower()
    if not refs or any(w in low for w in DESIGNED_WORDS):
        return "designed"
    return "real"


def total_v2(r: dict, W: dict | None = None, mode: str = "real") -> tuple[float, bool]:
    """The version-2 total and pass of a critic row."""
    W = {**W2, **(W or {})}
    real = W["real"] if mode == "real" else 0.0
    typ = r.get("ref_typ_z")
    g = lambda k: float(r.get(k) or 0.0)  # noqa: E731
    total = (W["match"] * min(g("match_z"), 6) + real * (2 * g("p_real") - 1)
             + W["typ"] * (float(np.clip(typ, -3, 2)) if typ is not None else 0.0)
             - W["feat"] * g("feat_pen") - W["alt"] * max(0.0, 0.5 - g("alt_margin"))
             - W["tone"] * g("tone_pen") - W["env"] * g("env_pen") - W["art"] * g("art_pen")
             - W["polish"] * g("polish_pen"))
    passes = bool(g("match_z") >= 3 and g("alt_margin") > 0 and (mode != "real" or g("p_real") >= 0.77)
                  and (typ is None or typ >= -2) and g("feat_pen") <= 1 and not r.get("hard")
                  and g("env_pen") <= ENV_PASS and g("polish_pen") <= POLISH_PASS and not r.get("tone_far"))
    return float(total), passes


def _mono(x: Any) -> np.ndarray:
    a = np.asarray(x, dtype=np.float32)
    return a.mean(axis=1).astype(np.float32) if a.ndim == 2 else a


def _stereo(x: Any) -> np.ndarray:
    a = np.asarray(x, dtype=np.float32)
    return np.column_stack([a, a]) if a.ndim == 1 else a


def _list_audio(refs: Any) -> list[str]:
    if not refs:
        return []
    items = [refs] if isinstance(refs, (str, os.PathLike)) else list(refs)
    try:
        from hark.core.audio import list_media

        return list(list_media([str(p) for p in items]))
    except ImportError:
        return [str(p) for p in items if os.path.isfile(str(p))]


def _feat_stats(F: Sequence[dict]) -> dict:
    stats = {}
    for k in FEATS:
        vals = [f[k] for f in F if f.get(k) is not None]
        if len(vals) < 2:
            continue
        v = np.log(np.maximum(vals, 1e-3)) if k in LOG else np.asarray(vals, dtype=float)
        floor = 0.15 if k in LOG else 1.5
        iqr = float(np.percentile(v, 75) - np.percentile(v, 25))
        stats[k] = (float(np.median(v)), max(iqr / 1.35, floor))
    return stats


def _bname(p: Any) -> str:
    return os.path.basename(str(p).replace("\\", "/"))


# ---------------------------------------------------------------- Critic

class Critic:
    """Scores sounds against a target text, confusions and real references."""

    def __init__(self, target: str, alts: Sequence[str] = (), refs: Any = None, k: int = 3,
                 extra: Callable | None = None, tone: float = 0.0, tone_target: float | None = None,
                 mode: str = "auto", version: int = 2, weights: dict | None = None, category: str | None = None,
                 study: dict | None = None, gated: bool | None = None):
        self.target, self.alts, self.k, self.extra = target, list(alts or []), k, extra
        self.tone, self.tone_target, self.version, self.category = tone, tone_target, version, category
        self.W = {**W2, **(weights or {})}
        if tone > 0 and "tone" not in (weights or {}):
            self.W["tone"] = tone
        self.svc = services()
        self.Q = self.svc.embed_text([target] + self.alts)
        self.refs = _list_audio(refs)
        self.mode = resolve_mode(mode, target, self.refs)
        self.study = copy.deepcopy(STUDY_TARGETS.get(category or "", {}) if study is None else study)
        self.gated = (category in GATED) if gated is None else bool(gated)
        self._memo: dict = {}
        self._stats_memo: dict = {}
        self._study_ref: list | None = None
        self._load_refs()

    def _load_refs(self) -> None:
        self.ref_audio = [self._load(p) for p in self.refs]
        if self.refs:
            self.A = np.asarray(self.svc.embed_files(self.refs))
            self.ref_feats = [self.svc.feats(_stereo(x)) for x in self.ref_audio]
            self.stats = _feat_stats(self.ref_feats)
            self.tone_ref = tn.ToneRef(self.ref_audio)
            self.env_ref = tn.EnvRef(self.ref_audio, names=self.refs)
        else:
            self.A, self.ref_feats, self.stats, self.tone_ref, self.env_ref = None, [], {}, None, None
        self.polish_ref = pol.PolishRef(self.ref_audio, designed=self.mode == "designed",
                                        gated=self.gated and self.mode == "designed")

    @staticmethod
    def _load(p: str) -> np.ndarray:
        from hark.core.audio import load

        return np.asarray(load(p), dtype=np.float32)

    # -- embedding and ref sets

    def embed(self, xs: Sequence[Any], batch: int = 16) -> np.ndarray:
        """Text-audio embeddings of the mono mixes."""
        out = [self.svc.embed_audio([_mono(x) for x in xs[i:i + batch]]) for i in range(0, len(xs), batch)]
        return np.concatenate(out, axis=0) if out else np.zeros((0, 512))

    def _ref_set(self, ex: int | None) -> tuple[list[int], dict]:
        keep = [i for i in range(len(self.refs)) if i != ex]
        if ex is None:
            return keep, self.stats
        if ex not in self._stats_memo:
            self._stats_memo[ex] = _feat_stats([self.ref_feats[i] for i in keep])
        return keep, self._stats_memo[ex]

    # -- scoring

    def score(self, xs: Sequence[Any], E: np.ndarray | None = None, exclude: Sequence[int | None] | None = None
              ) -> list[dict]:
        """One critic row per sound."""
        xs = list(xs)
        if not xs:
            return []
        E = self.embed(xs) if E is None else E
        Z = self.svc.calibrate_z(E, self.Q)
        exclude = list(exclude) if exclude is not None else [None] * len(xs)
        return [self._row(i, x, E[i], Z[i], exclude[i]) for i, x in enumerate(xs)]

    def _row(self, i: int, x: Any, e: np.ndarray, z: np.ndarray, ex: int | None) -> dict:
        m = _mono(x)
        if not len(m) or not self.svc.audible(m):
            return {"i": i, "total": -math.inf, "passes": False, "summary": "silent"}
        keep, stats = self._ref_set(ex)
        row = {"i": i, "mode": self.mode}
        self._model_terms(row, m, x, e, z, keep, stats)
        if ex is not None:
            row["left_out"] = _bname(self.refs[ex])
        self._tone_terms(row, x, ex)
        self._free_terms(row, x, ex, keep)
        row["total_v1"] = row["total"]
        if self.version >= 2:
            row["total"], row["passes"] = total_v2(row, self.W, self.mode)
        if self.extra is not None:
            row["extra"] = float(self.extra(x, row))
            row["total"] += row["extra"]
        row["summary"] = self._summary(row)
        return row

    def _model_terms(self, row: dict, m: np.ndarray, x: Any, e: np.ndarray, z: np.ndarray, keep: list[int],
                     stats: dict) -> None:
        match = float(z[0])
        alt_zs = [float(v) for v in z[1:]]
        alt = max(alt_zs) if alt_zs else -math.inf
        margin = match - alt if alt_zs else 9.0
        p = self.svc.p_real(m, e)
        if self.svc.pure_tone(m):
            p = min(p, 0.03)
        feats = self.svc.feats(_stereo(x))
        feat_pen, dev = self._feat_pen(feats, stats)
        typ, near = self._typicality(e, keep)
        total = min(match, 6) + 2 * (2 * p - 1) + 0.5 * (float(np.clip(typ, -3, 2)) if typ is not None else 0.0) \
            - 0.7 * feat_pen - max(0.0, 0.5 - margin)
        passes = match >= 3 and margin > 0 and p >= 0.77 and (typ is None or typ >= -2) and feat_pen <= 1
        row.update(match_z=match, alt_z=alt if alt_zs else None, alt_zs=alt_zs, alt_margin=margin,
                   p_real=round(p, 3), ref_typ_z=typ, nearest_ref=near, feat_pen=feat_pen, feat_dev=dev,
                   feats=feats, total=float(total), passes=bool(passes),
                   sounds_like=self.svc.top_terms(e, 3))

    @staticmethod
    def _feat_pen(feats: dict, stats: dict) -> tuple[float, dict]:
        dev, pen, n = {}, 0.0, 0
        for k, (med, sd) in stats.items():
            if feats.get(k) is None:
                continue
            v = math.log(max(feats[k], 1e-3)) if k in LOG else float(feats[k])
            zk = (v - med) / sd
            dev[k] = round(zk, 2)
            pen += max(0.0, abs(zk) - 1.5)
            n += 1
        return pen / max(1, n), dev

    def _typicality(self, e: np.ndarray, keep: list[int]) -> tuple[float | None, str | None]:
        if self.A is None or len(keep) < 3:
            return None, None
        A = self.A[keep]
        typ = self.svc.typicality(e, A, self.k)
        near = _bname(self.refs[keep[int(np.argmax(A @ e))]])
        return (None if typ is None else float(typ)), near

    def _tone_terms(self, row: dict, x: Any, ex: int | None) -> None:
        if self.tone_ref is None or not self.tone_ref.n:
            return
        t = self.tone_ref.check(x, exclude=ex)
        if not t:
            return
        if self.tone_target:
            t["tone_ratio"] = round(t["tone_db"] / self.tone_target, 2)
        keys = ("tone_db", "tone_ratio", "outside", "t40_ms", "t40_ref", "length_s", "length_ref", "shares",
                "shares_ref_med", "words")
        row["tone"] = {k: t.get(k) for k in keys}
        row["tone_db"], row["tone_ratio"] = t["tone_db"], t["tone_ratio"]
        row["tone_pen"] = self.tone_ref.penalty(t)
        far = max(t["tone_ref"]) if t.get("tone_ref") else None
        if self.tone_target:
            far = 1.6 * self.tone_target
        if self.version < 2 and self.tone > 0:
            row["total"] -= self.tone * row["tone_pen"]
            if far is not None and t["tone_db"] > far:
                row["passes"] = False
        row["tone_far"] = bool(far is not None and t["tone_db"] > TONE_PASS * far)

    def _free_terms(self, row: dict, x: Any, ex: int | None, keep: list[int]) -> None:
        G = tn.bandgram(x)
        if self.env_ref is not None and self.env_ref.n:
            ev = self.env_ref.check(x, exclude=ex, G=G)
            if ev:
                keys = ("env_db", "env_ratio", "env_ref", "env_out", "env_out_ref", "nearest", "anchor", "stretch",
                        "shift_ms", "outside", "words")
                row["env"] = {k: ev.get(k) for k in keys}
                row.update(env_db=ev["env_db"], env_ratio=ev["env_ratio"], env_out=ev["env_out"],
                           env_pen=tn.EnvRef.penalty(ev))
        self._polish_terms(row, x, ex, G)
        if self.study and len(keep) < 3:
            self._study_terms(row, x, ex)

    def _polish_terms(self, row: dict, x: Any, ex: int | None, G: np.ndarray) -> None:
        c = self.polish_ref.check(x, exclude=ex, G=G)
        lim = c["limits"]
        row["artifacts"] = {"pen": c["art_pen"], "hard": c["hard"], **{k: lim.get(k) for k in c["art"]}}
        row["polish"] = {"pen": c["polish_pen"], **{k: lim.get(k) for k in c["polish"]}}
        row["measures"] = {k: v for k, v in c["measures"].items() if k != "res_peaks"}
        row.update(art_pen=c["art_pen"], polish_pen=c["polish_pen"], hard=c["hard"], polish_words=c["words"])

    def _study_terms(self, row: dict, x: Any, ex: int | None) -> None:
        if self._study_ref is None:
            self._study_ref = [study_measures(a) for a in self.ref_audio]
        others = [m for j, m in enumerate(self._study_ref) if j != ex]
        tg = widen_targets(self.study, others)
        sm = study_measures(x)
        pen, off, words = study_penalties(sm, tg)
        used = {}
        if row.get("tone_ratio") is None and pen["tone"] > 0:
            row["tone_pen"] = row.get("tone_pen", 0.0) + pen["tone"]
            used["tone"] = pen["tone"]
        if row.get("env_ratio") is None and pen["env"] > 0:
            row["env_pen"] = row.get("env_pen", 0.0) + pen["env"]
            used["env"] = pen["env"]
        if pen["polish"] > 0:
            row["polish_pen"] = row.get("polish_pen", 0.0) + pen["polish"]
            used["polish"] = pen["polish"]
        row["study"] = {"measures": sm, "targets": tg, "off": off, "pen": used, "words": words}

    def _summary(self, r: dict) -> str:
        parts = [f"match z {r['match_z']:+.2f}" + (f" (best alternative {r['alt_z']:+.2f})"
                                                   if r.get("alt_z") is not None else ""),
                 f"p_real {r['p_real']:.2f}" + (" (not required: designed)" if self.mode == "designed" else "")]
        if r.get("ref_typ_z") is not None:
            parts.append(f"typicality vs refs z {r['ref_typ_z']:+.2f} (nearest {r.get('nearest_ref')})")
        far = [f"{k} {v:+.1f}" for k, v in (r.get("feat_dev") or {}).items() if abs(v) > 1.5]
        if far:
            parts.append("outside the refs: " + ", ".join(far))
        parts.append("sounds like: " + ", ".join(r.get("sounds_like") or []))
        parts += [w for w in ((r.get("tone") or {}).get("words"), (r.get("env") or {}).get("words"),
                              r.get("polish_words")) if w]
        if (r.get("study") or {}).get("off"):
            parts.append(r["study"]["words"])
        return ("PASS " if r["passes"] else "") + f"total {r['total']:.2f}: " + "; ".join(parts)

    # -- refs and checks

    def score_files(self, paths: Sequence[str]) -> list[dict]:
        """Score audio files."""
        return self.score([self._load(p) for p in paths])

    def score_refs(self, loo: bool = True) -> list[dict]:
        """Score every ref with this critic; with loo each ref is judged against the others."""
        key = (loo, tuple(sorted(self.W.items())), self.mode, self.version, id(self.extra), tuple(self.refs))
        if key not in self._memo:
            if not self.refs:
                self._memo[key] = []
            else:
                ex = list(range(len(self.refs))) if loo else None
                rows = self.score(self.ref_audio, E=self.A, exclude=ex)
                for r, p in zip(rows, self.refs):
                    r["file"] = _bname(p)
                self._memo[key] = rows
        return copy.deepcopy(self._memo[key])

    def consistency(self, xs: Sequence[Any]) -> dict:
        """Loudness spread of the sounds, and of the refs when there are refs."""
        out = pol.loudness_spread(list(xs))
        if self.refs:
            out["refs"] = pol.loudness_spread(self.ref_audio)
        return out

    def holdout(self, xs: Sequence[Any], cache: bool = True) -> list[dict]:
        """Held-out AudioSet tags (never part of the objective)."""
        return [copy.deepcopy(_holdout_one(self.svc, _mono(x), cache)) for x in xs]


_HOLD: dict[str, dict] = {}


def _hold_dir() -> Path:
    home = os.environ.get("HARK_HOME") or os.path.join("~", ".cache", "hark")
    return Path(home).expanduser() / "emb" / "forge_holdout"


def _holdout_one(svc: Any, m: np.ndarray, cache: bool) -> dict:
    key = f"{svc.tagger_name()}/{holdout_key(m)}"
    if cache and key in _HOLD:
        return _HOLD[key]
    path = _hold_dir() / (key.replace("/", "__") + ".json")
    if cache and path.is_file():
        try:
            got = json.loads(path.read_text())
            got["audioset_top"] = [tuple(t) for t in got.get("audioset_top") or []]
            _HOLD[key] = got
            return got
        except (OSError, ValueError):
            pass
    top, synth = svc.tags(m)
    got = {"audioset_top": [tuple(t) for t in top], "audioset_synth_max": synth}
    _HOLD[key] = got
    if cache:
        _write_quietly(path, got)
    return got


def _write_quietly(path: Path, obj: dict) -> None:
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        tmp = path.with_name(f"{path.name}.tmp{os.getpid()}")
        tmp.write_text(json.dumps(obj))
        os.replace(tmp, path)
    except OSError:
        pass
