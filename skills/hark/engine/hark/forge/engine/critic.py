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
_NO_FLOOR = -200.0

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
    "glitch": {"length": (0.2, 1.5), "sh150": (_NO_FLOOR, -10), "corr": (-0.3, 0.6)},
    "pro_glitch": {"length": (0.3, 1.0), "sh150": (_NO_FLOOR, -12), "sh12k": (-8, 0), "corr": (-0.3, 0.4)},
    "pro_shimmer": {"length": (1.4, 2.6), "attack": (150, 800), "sh150": (_NO_FLOOR, -30), "corr": (-0.2, 0.45)},
    "pro_pop": {"length": (0.05, 0.2), "attack": (0, 60), "t20": (3, 30), "corr": (0.9, 1.0)},
    "pro_tap": {"length": (0.06, 0.2), "attack": (2, 15), "t20": (10, 30), "t40": (18, 50), "corr": (0.85, 1.0)},
    "pro_click": {"length": (0.02, 0.08), "attack": (0, 4), "t20": (4, 20), "sh150": (_NO_FLOOR, -20),
                  "sh12k": (-10, 0), "corr": (0.9, 1.0)},
    "pro_typing_run": {"sh150": (_NO_FLOOR, -25), "sh1k": (_NO_FLOOR, -20), "corr": (0.9, 1.0)},
    "pro_zip": {"length": (0.12, 0.4), "attack": (10, 50), "corr": (0.85, 1.0)},
    "zip": {"length": (0.12, 0.4), "attack": (10, 50), "corr": (0.85, 1.0)},
    "downlifter": {"length": (1.0, 4.0), "attack": (0, 80), "t40": (500, 3500), "corr": (0.3, 1.0)},
}

_PART_CAP = {"tone": 3.0, "env": 2.0, "polish": 1.5}


# ---------------------------------------------------------------- listen services

class ListenServices:
    """The listen-unit calls the critic makes. Loads the models on construction."""

    name = "listen"

    def __init__(self) -> None:
        from hark.models import require_ml

        require_ml("forge critic")
        from hark.listen import compare, realness, vocabulary

        self._vocab, self._realmod, self._cmpmod = vocabulary, realness, compare
        self.vocab_obj = vocabulary.get()
        self.real_model = realness.load_model()

    def embed_text(self, texts: Sequence[str]) -> np.ndarray:
        return self.vocab_obj.embed_text(list(texts))

    def calibrate_z(self, E: np.ndarray, Q: np.ndarray) -> np.ndarray:
        return np.asarray(self.vocab_obj.calibrate(E, Q).z_q)

    def top_terms(self, e: np.ndarray, k: int = 3) -> list[str]:
        return [tv[0] for tv in self.vocab_obj.top_terms(e, k)]

    def embed_files(self, paths: Sequence[str]) -> np.ndarray:
        return np.asarray(self._vocab.embed_files(list(paths), pad=self._vocab.PAD))

    def embed_audio(self, arrays: Sequence[np.ndarray]) -> np.ndarray:
        return np.asarray(self._vocab.embed_audio(list(arrays), pad=self._vocab.PAD))

    def audible(self, m: np.ndarray) -> bool:
        return bool(self._realmod.audible(m))

    def pure_tone(self, m: np.ndarray) -> bool:
        return bool(self._realmod.pure_tone(m))

    def p_real(self, m: np.ndarray, e: np.ndarray) -> float:
        head_map = self.real_model["heads"]
        sig_f = self._realmod.signal_features(m)
        logit_c = self._realmod.head_logit(head_map["clap"], e)
        logit_s = self._realmod.head_logit(head_map["signal"], [sig_f[ky] for ky in head_map["signal"]["inputs"]]) if sig_f else 0.0
        return float(self._realmod.fuse(self.real_model, {"clap": logit_c, "signal": logit_s}))

    def feats(self, x: np.ndarray) -> dict:
        from hark.core.features import measure

        return dict(self._cmpmod.feats(measure(x, light=True), x))

    def typicality(self, e: np.ndarray, A: np.ndarray, k: int) -> float | None:
        return self._cmpmod.typicality(e, A, k).get("z")

    def tagger_name(self) -> str:
        from hark.models import ast

        return str(getattr(ast.get(), "model_id", "ast")).replace("/", "_")

    def tags(self, m: np.ndarray) -> tuple[list, float]:
        from hark.models import ast

        tagger = ast.get()
        pv = np.asarray(tagger.probs(m)) if hasattr(tagger, "probs") else self._probs_from_wav(tagger, m)
        synth_p = [tagger.index[chk] for chk in self._realmod.SYNTH_CLASSES if chk in tagger.index] \
            if hasattr(self._realmod, "SYNTH_CLASSES") else []
        return list(tagger.top(pv, 3)), round(float(max((pv[ix] for ix in synth_p), default=0.0)), 3)

    @staticmethod
    def _probs_from_wav(tagger: Any, mono_x: np.ndarray) -> np.ndarray:
        import tempfile

        from hark.core.audio import write_wav

        with tempfile.TemporaryDirectory() as dpath:
            fpath = write_wav(os.path.join(dpath, "x.wav"), mono_x)
            return np.asarray(tagger.probs_file(fpath))


_SHARED_SVC: Any = None
_BUILD_GUARD = threading.Lock()


def services() -> Any:
    """The process-wide listen services (built on first use)."""
    global _SHARED_SVC
    with _BUILD_GUARD:
        if _SHARED_SVC is None:
            _SHARED_SVC = ListenServices()
        return _SHARED_SVC


# ---------------------------------------------------------------- study targets

def _lr_correlation(sig: np.ndarray) -> float:
    if sig.ndim != 2:
        return 1.0
    arr, rhs = sig[:, 0] - sig[:, 0].mean(), sig[:, 1] - sig[:, 1].mean()
    denom = math.sqrt(float(np.sum(arr * arr) * np.sum(rhs * rhs)))
    return round(float(np.sum(arr * rhs) / denom), 3) if denom > 0 else 1.0


def _extent(mono_x: np.ndarray) -> dict:
    ky = len(mono_x) // 480
    if ky < 2:
        return {}
    emb = 10 * np.log10(np.mean(mono_x[: ky * 480].reshape(ky, 480) ** 2, axis=1) + 1e-20)
    busy = np.flatnonzero(emb >= emb.max() - 50)
    width = busy[-1] - busy[0] + 1
    return {"length": round(width * 0.01, 3), "peak_frac": round((int(np.argmax(emb)) - busy[0] + 0.5) / width, 3)}


def study_measures(x: Any) -> dict:
    """Length, peak position, attack and decay times, region shares and stereo correlation."""
    from hark.forge.engine.search import envelope_stats

    arr = np.asarray(x, dtype=float) if not isinstance(x, (str, os.PathLike)) else tn.as_mono(x)
    mono_x = arr.mean(axis=1) if arr.ndim == 2 else arr
    res = _extent(mono_x)
    envd = envelope_stats(mono_x)
    res.update(attack=round(envd["attack_ms"], 1), t20=round(envd["t20_ms"], 1), t40=round(envd["t40_ms"], 1))
    shr = tn.shares(mono_x)
    res.update({ky: shr[rw] for ky, rw in SHARE_KEYS.items()})
    res["corr"] = _lr_correlation(arr)
    return res


def widen_targets(targets: dict, ref_measures: Sequence[dict]) -> dict:
    """Each range grows to include every ref's value."""
    res = {}
    for ky, (low, high) in targets.items():
        seen = [mono_x[ky] for mono_x in ref_measures if mono_x.get(ky) is not None]
        res[ky] = (min([low] + seen), max([high] + seen))
    return res


def _outside_by(val: float, low: float, high: float) -> float:
    return max(low - val, val - high, 0.0)


def _share_cost(name: str, val: float, low: float, high: float) -> tuple[str, float, str]:
    return "tone", min(2.0, _outside_by(val, low, high) / 6), f"{SHARE_KEYS[name]} share {val:+.1f} dB (target {low}..{high})"


def _stereo_cost(name: str, val: float, low: float, high: float) -> tuple[str, float, str]:
    return "polish", min(1.5, _outside_by(val, low, high) / 0.3), f"stereo corr {val:.2f} (target {low}..{high})"


def _peak_cost(name: str, val: float, low: float, high: float) -> tuple[str, float, str]:
    return "env", min(1.5, _outside_by(val, low, high) / 0.25), f"peak at {val:.0%} of the sound (target {low:.0%}..{high:.0%})"


def _time_cost(name: str, val: float, low: float, high: float) -> tuple[str, float, str]:
    lowest, unit_s = (1e-3, "s") if name == "length" else (1.0, "ms")
    vc, lo_c, hi_c = max(val, lowest), max(low, lowest), max(high, low, lowest)
    pv = min(1.5, max(math.log2(lo_c / vc), math.log2(vc / hi_c), 0.0))
    return "env", pv, f"{name} {val}{unit_s} (target {low}..{high})"


_COST_FOR = {"corr": _stereo_cost, "peak_frac": _peak_cost}


def _target_cost(name: str, val: float, low: float, high: float) -> tuple[str, float, str]:
    cost_fn = _share_cost if name in SHARE_KEYS else _COST_FOR.get(name, _time_cost)
    return cost_fn(name, val, low, high)


def study_penalties(sm: dict, targets: dict) -> tuple[dict, dict, str]:
    """(penalty per part, off-target keys, words) of study measures against targets."""
    cost = {"tone": 0.0, "env": 0.0, "polish": 0.0}
    miss, notes = {}, []
    for name, (low, high) in targets.items():
        if sm.get(name) is None:
            continue
        slot, pv, phrase = _target_cost(name, float(sm[name]), low, high)
        if pv > 0:
            miss[name] = round(pv, 3)
            cost[slot] += pv
            notes.append(phrase)
    cost = {ky: min(_PART_CAP[ky], val) for ky, val in cost.items()}
    return cost, miss, ("study targets: " + "; ".join(notes)) if notes else "study targets met"


# ---------------------------------------------------------------- pure helpers

def holdout_key(mono: np.ndarray) -> str:
    """sha1 of the mono float32 samples."""
    return hashlib.sha1(np.ascontiguousarray(np.asarray(mono, dtype=np.float32)).tobytes()).hexdigest()


def resolve_mode(mode: str, target: str, refs: Sequence[Any] | None) -> str:
    """'real' or 'designed'; auto is designed with no refs or a designed target word."""
    if mode in ("real", "designed"):
        return mode
    lower = (target or "").lower()
    if not refs or any(wd in lower for wd in DESIGNED_WORDS):
        return "designed"
    return "real"


def _v2_sum(rw: dict, wts: dict, mode_s: str) -> float:
    def num(ky: str) -> float:
        return float(rw.get(ky) or 0.0)

    typ_z = rw.get("ref_typ_z")
    plus = {"match": min(num("match_z"), 6), "real": 2 * num("p_real") - 1 if mode_s == "real" else 0.0,
            "typ": float(np.clip(typ_z, -3, 2)) if typ_z is not None else 0.0}
    minus = {"feat": num("feat_pen"), "alt": max(0.0, 0.5 - num("alt_margin")), "tone": num("tone_pen"),
            "env": num("env_pen"), "art": num("art_pen"), "polish": num("polish_pen")}
    return sum(wts[ky] * val for ky, val in plus.items()) - sum(wts[ky] * val for ky, val in minus.items())


def _v2_ok(rw: dict, mode_s: str) -> bool:
    def num(ky: str) -> float:
        return float(rw.get(ky) or 0.0)

    typ_z = rw.get("ref_typ_z")
    ok_list = [num("match_z") >= 3, num("alt_margin") > 0, mode_s != "real" or num("p_real") >= 0.77,
              typ_z is None or typ_z >= -2, num("feat_pen") <= 1, not rw.get("hard"), num("env_pen") <= ENV_PASS,
              num("polish_pen") <= POLISH_PASS, not rw.get("tone_far")]
    return all(ok_list)


def total_v2(r: dict, W: dict | None = None, mode: str = "real") -> tuple[float, bool]:
    """The version-2 total and pass of a critic row."""
    W = {**W2, **(W or {})}
    return float(_v2_sum(r, W, mode)), _v2_ok(r, mode)


def _flat32(sig: Any) -> np.ndarray:
    arr = np.asarray(sig, dtype=np.float32)
    return arr.mean(axis=1).astype(np.float32) if arr.ndim == 2 else arr


def _two_ch(sig: Any) -> np.ndarray:
    arr = np.asarray(sig, dtype=np.float32)
    return np.column_stack([arr, arr]) if arr.ndim == 1 else arr


def _audio_paths(ref_paths: Any) -> list[str]:
    if not ref_paths:
        return []
    entries = [ref_paths] if isinstance(ref_paths, (str, os.PathLike)) else list(ref_paths)
    try:
        from hark.core.audio import list_media

        return list(list_media([str(pv) for pv in entries]))
    except ImportError:
        return [str(pv) for pv in entries if os.path.isfile(str(pv))]


def _feature_spread(fvs: Sequence[dict]) -> dict:
    spread = {}
    for ky in FEATS:
        seen = [fd[ky] for fd in fvs if fd.get(ky) is not None]
        if len(seen) < 2:
            continue
        val = np.log(np.maximum(seen, 1e-3)) if ky in LOG else np.asarray(seen, dtype=float)
        min_sd = 0.15 if ky in LOG else 1.5
        quart = float(np.percentile(val, 75) - np.percentile(val, 25))
        spread[ky] = (float(np.median(val)), max(quart / 1.35, min_sd))
    return spread


def _file_label(pv: Any) -> str:
    return os.path.basename(str(pv).replace("\\", "/"))


_TONE_FIELDS = ("tone_db", "tone_ratio", "outside", "t40_ms", "t40_ref", "length_s", "length_ref", "shares",
              "shares_ref_med", "words")
# (penalty part, the row key that, when set, means another check already judged this part)
_TARGET_SLOTS = (("tone", "tone_ratio"), ("env", "env_ratio"), ("polish", None))


def _merge_weights(wt_in: dict | None, tone_w: float) -> dict:
    wts = {**W2, **(wt_in or {})}
    if tone_w > 0 and "tone" not in (wt_in or {}):
        wts["tone"] = tone_w
    return wts


def _targets_and_gate(cat: str | None, tg_in: dict | None, gate: bool | None) -> tuple[dict, bool]:
    tg_d = copy.deepcopy(STUDY_TARGETS.get(cat or "", {}) if tg_in is None else tg_in)
    return tg_d, (cat in GATED) if gate is None else bool(gate)


def _note_list(rw: dict) -> list[str]:
    """The words of the tone, envelope, polish and study checks, in that order."""
    note = [(rw.get("tone") or {}).get("words"), (rw.get("env") or {}).get("words"), rw.get("polish_words")]
    tg_in = rw.get("study") or {}
    note.append(tg_in.get("words") if tg_in.get("off") else None)
    return [wd for wd in note if wd]


def _v1_sum(mz: float, pv: float, typ_z: float | None, fpen: float, gap: float) -> tuple[float, bool]:
    tv = float(np.clip(typ_z, -3, 2)) if typ_z is not None else 0.0
    tot = min(mz, 6) + 2 * (2 * pv - 1) + 0.5 * tv - 0.7 * fpen - max(0.0, 0.5 - gap)
    ok_list = [mz >= 3, gap > 0, pv >= 0.77, typ_z is None or typ_z >= -2, fpen <= 1]
    return float(tot), all(ok_list)


def _add_target_costs(rec: dict, cost: dict) -> dict:
    """Add study penalties to parts no other check judged; return the parts used."""
    applied = {}
    for slot, seen_by in _TARGET_SLOTS:
        if cost[slot] > 0 and (seen_by is None or rec.get(seen_by) is None):
            rec[f"{slot}_pen"] = rec.get(f"{slot}_pen", 0.0) + cost[slot]
            applied[slot] = cost[slot]
    return applied


# ---------------------------------------------------------------- Critic

class Critic:
    """Scores sounds against a target text, confusions and real references."""

    def __init__(self, target: str, alts: Sequence[str] = (), refs: Any = None, k: int = 3,
                 extra: Callable | None = None, tone: float = 0.0, tone_target: float | None = None,
                 mode: str = "auto", version: int = 2, weights: dict | None = None, category: str | None = None,
                 study: dict | None = None, gated: bool | None = None):
        self.target, self.alts, self.nn_k, self.extra = target, list(alts or []), k, extra
        self.tone, self.tone_target, self.version, self.category = tone, tone_target, version, category
        self.W = _merge_weights(weights, tone)
        self.listen = services()
        self.text_emb = self.listen.embed_text([target] + self.alts)
        self.refs = _audio_paths(refs)
        self.mode = resolve_mode(mode, target, self.refs)
        self.study, self.is_gated = _targets_and_gate(category, study, gated)
        self._ref_rows: dict = {}
        self._loo_spread: dict = {}
        self._ref_targets: list | None = None
        self._prepare_refs()

    def _prepare_refs(self) -> None:
        self.ref_audio = [self._read_audio(pv) for pv in self.refs]
        if self.refs:
            self.A = np.asarray(self.listen.embed_files(self.refs))
            self.ref_fv = [self.listen.feats(_two_ch(sig)) for sig in self.ref_audio]
            self.spread_tbl = _feature_spread(self.ref_fv)
            self.tone_chk = tn.ToneRef(self.ref_audio)
            self.env_chk = tn.EnvRef(self.ref_audio, names=self.refs)
        else:
            self.A, self.ref_fv, self.spread_tbl, self.tone_chk, self.env_chk = None, [], {}, None, None
        self.polish_ref = pol.PolishRef(self.ref_audio, designed=self.mode == "designed",
                                        gated=self.is_gated and self.mode == "designed")

    @staticmethod
    def _read_audio(pv: str) -> np.ndarray:
        from hark.core.audio import load

        return np.asarray(load(pv), dtype=np.float32)

    # -- embedding and ref sets

    def embed(self, xs: Sequence[Any], batch: int = 16) -> np.ndarray:
        """Text-audio embeddings of the mono mixes."""
        res = [self.listen.embed_audio([_flat32(sig) for sig in xs[ix:ix + batch]]) for ix in range(0, len(xs), batch)]
        return np.concatenate(res, axis=0) if res else np.zeros((0, 512))

    def _kept_refs(self, left: int | None) -> tuple[list[int], dict]:
        kept = [ix for ix in range(len(self.refs)) if ix != left]
        if left is None:
            return kept, self.spread_tbl
        if left not in self._loo_spread:
            self._loo_spread[left] = _feature_spread([self.ref_fv[ix] for ix in kept])
        return kept, self._loo_spread[left]

    # -- scoring

    def score(self, xs: Sequence[Any], E: np.ndarray | None = None, exclude: Sequence[int | None] | None = None
              ) -> list[dict]:
        """One critic row per sound."""
        xs = list(xs)
        if not xs:
            return []
        E = self.embed(xs) if E is None else E
        zs = self.listen.calibrate_z(E, self.text_emb)
        exclude = list(exclude) if exclude is not None else [None] * len(xs)
        return [self._judge_one(ix, sig, E[ix], zs[ix], exclude[ix]) for ix, sig in enumerate(xs)]

    def _judge_one(self, ix: int, sig: Any, emb: np.ndarray, zrow: np.ndarray, left: int | None) -> dict:
        mono_x = _flat32(sig)
        if not len(mono_x) or not self.listen.audible(mono_x):
            return {"i": ix, "total": -math.inf, "passes": False, "summary": "silent"}
        kept, spread = self._kept_refs(left)
        rec = {"i": ix, "mode": self.mode}
        self._add_model(rec, mono_x, sig, emb, zrow, kept, spread)
        if left is not None:
            rec["left_out"] = _file_label(self.refs[left])
        self._add_tone(rec, sig, left)
        self._add_measures(rec, sig, left, kept)
        rec["total_v1"] = rec["total"]
        if self.version >= 2:
            rec["total"], rec["passes"] = total_v2(rec, self.W, self.mode)
        if self.extra is not None:
            rec["extra"] = float(self.extra(sig, rec))
            rec["total"] += rec["extra"]
        rec["summary"] = self._headline(rec)
        return rec

    def _add_model(self, rec: dict, mono_x: np.ndarray, sig: Any, emb: np.ndarray, zrow: np.ndarray, kept: list[int],
                     spread: dict) -> None:
        mz, other_z = float(zrow[0]), [float(val) for val in zrow[1:]]
        gap = mz - max(other_z) if other_z else 9.0
        pv = self._realness(mono_x, emb)
        fv = self.listen.feats(_two_ch(sig))
        fpen, devs = self._feature_cost(fv, spread)
        typ_z, closest = self._typical(emb, kept)
        tot, ok = _v1_sum(mz, pv, typ_z, fpen, gap)
        rec.update(match_z=mz, alt_z=max(other_z) if other_z else None, alt_zs=other_z, alt_margin=gap,
                   p_real=round(pv, 3), ref_typ_z=typ_z, nearest_ref=closest, feat_pen=fpen, feat_dev=devs,
                   feats=fv, total=tot, passes=ok, sounds_like=self.listen.top_terms(emb, 3))

    def _realness(self, mono_x: np.ndarray, emb: np.ndarray) -> float:
        pv = self.listen.p_real(mono_x, emb)
        return min(pv, 0.03) if self.listen.pure_tone(mono_x) else pv

    @staticmethod
    def _feature_cost(fv: dict, spread: dict) -> tuple[float, dict]:
        devs, cost, cnt = {}, 0.0, 0
        for ky, (mid, sdev) in spread.items():
            if fv.get(ky) is None:
                continue
            val = math.log(max(fv[ky], 1e-3)) if ky in LOG else float(fv[ky])
            zv = (val - mid) / sdev
            devs[ky] = round(zv, 2)
            cost += max(0.0, abs(zv) - 1.5)
            cnt += 1
        return cost / max(1, cnt), devs

    def _typical(self, emb: np.ndarray, kept: list[int]) -> tuple[float | None, str | None]:
        if self.A is None or len(kept) < 3:
            return None, None
        embs_ref = self.A[kept]
        typ_z = self.listen.typicality(emb, embs_ref, self.nn_k)
        closest = _file_label(self.refs[kept[int(np.argmax(embs_ref @ emb))]])
        return (None if typ_z is None else float(typ_z)), closest

    def _add_tone(self, rec: dict, sig: Any, left: int | None) -> None:
        tv = self.tone_chk.check(sig, exclude=left) if self.tone_chk is not None and self.tone_chk.n else None
        if not tv:
            return
        if self.tone_target:
            tv["tone_ratio"] = round(tv["tone_db"] / self.tone_target, 2)
        rec["tone"] = {ky: tv.get(ky) for ky in _TONE_FIELDS}
        rec["tone_db"], rec["tone_ratio"] = tv["tone_db"], tv["tone_ratio"]
        rec["tone_pen"] = self.tone_chk.penalty(tv)
        limit = self._tone_limit(tv)
        if self.version < 2 and self.tone > 0:
            self._apply_tone_v1(rec, tv, limit)
        rec["tone_far"] = bool(limit is not None and tv["tone_db"] > TONE_PASS * limit)

    def _tone_limit(self, tv: dict) -> float | None:
        if self.tone_target:
            return 1.6 * self.tone_target
        return max(tv["tone_ref"]) if tv.get("tone_ref") else None

    def _apply_tone_v1(self, rec: dict, tv: dict, limit: float | None) -> None:
        rec["total"] -= self.tone * rec["tone_pen"]
        if limit is not None and tv["tone_db"] > limit:
            rec["passes"] = False

    def _add_measures(self, rec: dict, sig: Any, left: int | None, kept: list[int]) -> None:
        bandg = tn.bandgram(sig)
        if self.env_chk is not None and self.env_chk.n:
            env_chk = self.env_chk.check(sig, exclude=left, G=bandg)
            if env_chk:
                fields = ("env_db", "env_ratio", "env_ref", "env_out", "env_out_ref", "nearest", "anchor", "stretch",
                        "shift_ms", "outside", "words")
                rec["env"] = {ky: env_chk.get(ky) for ky in fields}
                rec.update(env_db=env_chk["env_db"], env_ratio=env_chk["env_ratio"], env_out=env_chk["env_out"],
                           env_pen=tn.EnvRef.penalty(env_chk))
        self._add_polish(rec, sig, left, bandg)
        if self.study and len(kept) < 3:
            self._add_targets(rec, sig, left)

    def _add_polish(self, rec: dict, sig: Any, left: int | None, bandg: np.ndarray) -> None:
        chk = self.polish_ref.check(sig, exclude=left, G=bandg)
        lims = chk["limits"]
        rec["artifacts"] = {"pen": chk["art_pen"], "hard": chk["hard"], **{ky: lims.get(ky) for ky in chk["art"]}}
        rec["polish"] = {"pen": chk["polish_pen"], **{ky: lims.get(ky) for ky in chk["polish"]}}
        rec["measures"] = {ky: val for ky, val in chk["measures"].items() if ky != "res_peaks"}
        rec.update(art_pen=chk["art_pen"], polish_pen=chk["polish_pen"], hard=chk["hard"], polish_words=chk["words"])

    def _add_targets(self, rec: dict, sig: Any, left: int | None) -> None:
        if self._ref_targets is None:
            self._ref_targets = [study_measures(arr) for arr in self.ref_audio]
        rest = [mono_x for jx, mono_x in enumerate(self._ref_targets) if jx != left]
        goals = widen_targets(self.study, rest)
        sm = study_measures(sig)
        cost, miss, note = study_penalties(sm, goals)
        applied = _add_target_costs(rec, cost)
        rec["study"] = {"measures": sm, "targets": goals, "off": miss, "pen": applied, "words": note}

    def _headline(self, rw: dict) -> str:
        return ("PASS " if rw["passes"] else "") + f"total {rw['total']:.2f}: " + "; ".join(self._headline_bits(rw))

    def _headline_bits(self, rw: dict) -> list[str]:
        return self._model_bits(rw) + self._check_bits(rw)

    def _model_bits(self, rw: dict) -> list[str]:
        alt_s = f" (best alternative {rw['alt_z']:+.2f})" if rw.get("alt_z") is not None else ""
        des_s = " (not required: designed)" if self.mode == "designed" else ""
        bits = [f"match z {rw['match_z']:+.2f}" + alt_s, f"p_real {rw['p_real']:.2f}" + des_s]
        if rw.get("ref_typ_z") is not None:
            bits.append(f"typicality vs refs z {rw['ref_typ_z']:+.2f} (nearest {rw.get('nearest_ref')})")
        return bits

    @staticmethod
    def _check_bits(rw: dict) -> list[str]:
        limit = [f"{ky} {val:+.1f}" for ky, val in (rw.get("feat_dev") or {}).items() if abs(val) > 1.5]
        bits = ["outside the refs: " + ", ".join(limit)] if limit else []
        bits.append("sounds like: " + ", ".join(rw.get("sounds_like") or []))
        return bits + _note_list(rw)

    # -- refs and checks

    def score_files(self, paths: Sequence[str]) -> list[dict]:
        """Score audio files."""
        return self.score([self._read_audio(pv) for pv in paths])

    def score_refs(self, loo: bool = True) -> list[dict]:
        """Score every ref with this critic; with loo each ref is judged against the others."""
        name = (loo, tuple(sorted(self.W.items())), self.mode, self.version, id(self.extra), tuple(self.refs))
        if name not in self._ref_rows:
            if not self.refs:
                self._ref_rows[name] = []
            else:
                left = list(range(len(self.refs))) if loo else None
                tbl = self.score(self.ref_audio, E=self.A, exclude=left)
                for rw, pv in zip(tbl, self.refs):
                    rw["file"] = _file_label(pv)
                self._ref_rows[name] = tbl
        return copy.deepcopy(self._ref_rows[name])

    def consistency(self, xs: Sequence[Any]) -> dict:
        """Loudness spread of the sounds, and of the refs when there are refs."""
        res = pol.loudness_spread(list(xs))
        if self.refs:
            res["refs"] = pol.loudness_spread(self.ref_audio)
        return res

    def holdout(self, xs: Sequence[Any], cache: bool = True) -> list[dict]:
        """Held-out AudioSet tags (never part of the objective)."""
        return [copy.deepcopy(_tags_for(self.listen, _flat32(sig), cache)) for sig in xs]


_HOLD: dict[str, dict] = {}


def _tag_cache_dir() -> Path:
    base_dir = os.environ.get("HARK_HOME") or os.path.join("~", ".cache", "hark")
    return Path(base_dir).expanduser() / "emb" / "forge_holdout"


def _load_tags(fpath: Path) -> dict | None:
    try:
        cached = json.loads(fpath.read_text())
    except (OSError, ValueError):
        return None
    cached["audioset_top"] = [tuple(tv) for tv in cached.get("audioset_top") or []]
    return cached


def _tags_for(srv: Any, mono_x: np.ndarray, use_cache: bool) -> dict:
    name = f"{srv.tagger_name()}/{holdout_key(mono_x)}"
    fpath = _tag_cache_dir() / (name.replace("/", "__") + ".json")
    cached = (_HOLD.get(name) or (_load_tags(fpath) if fpath.is_file() else None)) if use_cache else None
    if cached is None:
        best, synth_p = srv.tags(mono_x)
        cached = {"audioset_top": [tuple(tv) for tv in best], "audioset_synth_max": synth_p}
        if use_cache:
            _save_tags(fpath, cached)
    _HOLD[name] = cached
    return cached


def _save_tags(fpath: Path, data: dict) -> None:
    try:
        fpath.parent.mkdir(parents=True, exist_ok=True)
        tmp_p = fpath.with_name(f"{fpath.name}.tmp{os.getpid()}")
        tmp_p.write_text(json.dumps(data))
        os.replace(tmp_p, fpath)
    except OSError:
        pass
