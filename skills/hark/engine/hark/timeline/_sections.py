"""Sections: CLAP windows, Foote novelty bounds, neighbour merging, tags, AST crops, labels (analysis 2.9)."""

from __future__ import annotations

import math

import numpy as np
from scipy import ndimage

from hark.timeline._dsp import SR, logmel, mono_of, robust_z

CATS = ("music_genre", "music_mood", "instruments", "ambience", "voice")
MERGE_COS = 0.84
MIN_LEN = 4.0
GENERIC_AST = {"Music", "Musical instrument", "Speech", "Silence", "Sound effect", "Inside, small room",
               "Outside, rural or natural", "Outside, urban or manmade", "Inside, large room or hall", "Animal"}


# ---------------------------------------------------------------- windows and novelty

def window_starts(dur: float, win: float, hop: float) -> list[float]:
    starts, s = [], 0.0
    while s <= max(dur - win, 0) + 1e-9:
        starts.append(round(s, 3))
        s += hop
    if not starts or starts[-1] + win < dur - 0.5:
        starts.append(round(max(0.0, dur - win), 3))
    return starts


def _frames(m: np.ndarray) -> np.ndarray:
    return logmel(m, n_fft=2048, hop=2400, n_mels=40, fmin=30, fmax=16000)


def novelty_bounds(m: np.ndarray, kernel_s: float = 4.0, zthr: float = 3.0) -> list[tuple[float, float]]:
    """Strong timbre or level changes as (time, z) pairs."""
    L = _frames(np.asarray(m, dtype=np.float64))
    n = L.shape[1] // 10
    w = int(kernel_s / 0.5)
    if n < 4 * w:
        return []
    F = L[:, : n * 10].reshape(L.shape[0], n, 10).mean(axis=2)
    F = (F - F.mean(axis=1, keepdims=True)) / (F.std(axis=1, keepdims=True) + 1e-6)
    F = F / (np.linalg.norm(F, axis=0, keepdims=True) + 1e-12)
    S = F.T @ F
    g = np.exp(-0.5 * ((np.arange(-w, w) + 0.5) / (0.5 * w)) ** 2)
    sign = np.ones(2 * w)
    sign[:w] = -1
    K = np.outer(g, g) * np.outer(sign, sign)
    nov = np.zeros(n)
    for i in range(w, n - w):
        nov[i] = float((K * S[i - w: i + w, i - w: i + w]).sum())
    z = robust_z(nov, n)
    peak = nov == ndimage.maximum_filter1d(nov, size=2 * w + 1, mode="nearest")
    return [(round(i * 0.5, 2), round(float(z[i]), 1)) for i in range(w, n - w) if peak[i] and z[i] >= zthr]


def refine_bound(m: np.ndarray, t: float, radius: float) -> float:
    a = max(0.0, t - radius - 1)
    b = min(m.size / SR, t + radius + 1)
    if b - a < 2:
        return t
    L = _frames(m[int(a * SR): int(b * SR)])
    best, best_t = -1.0, t
    for k in range(10, L.shape[1] - 10 + 1):
        tk = a + 0.05 * k
        if abs(tk - t) > radius:
            continue
        d = float(np.linalg.norm(L[:, k - 10: k].mean(axis=1) - L[:, k: k + 10].mean(axis=1)))
        if d > best:
            best, best_t = d, tk
    return round(best_t, 2)


# ---------------------------------------------------------------- merging

def _unit(v: np.ndarray) -> np.ndarray:
    return v / (np.linalg.norm(v) + 1e-12)


class _Seg:
    def __init__(self, idx: list[int], spans, emb):
        self.idx, self.spans, self.emb = idx, spans, emb

    @property
    def vec(self):
        return _unit(self.emb[self.idx].mean(axis=0))

    @property
    def first(self):
        return self.spans[self.idx[0]]

    @property
    def last(self):
        return self.spans[self.idx[-1]]

    @property
    def length(self):
        return self.last[1] - self.first[0]


def _junction(a: _Seg, b: _Seg, win: float) -> float:
    h = win if b.first[0] >= a.last[1] else a.last[1] - b.first[0]
    return b.first[0] + h / 2


def _blocked(t: float, bounds, hop: float) -> bool:
    return any(abs(bt - t) <= hop for bt, _ in bounds)


def _pick_pair(segs, win, hop, bounds, max_n):
    sims = [float(segs[k].vec @ segs[k + 1].vec) - (1.0 if _blocked(_junction(segs[k], segs[k + 1], win), bounds, hop)
                                                     else 0.0) for k in range(len(segs) - 1)]
    j = int(np.argmax(sims))
    if sims[j] >= MERGE_COS or len(segs) > max_n:
        return j
    short = [k for k in range(len(sims)) if segs[k].length < MIN_LEN or segs[k + 1].length < MIN_LEN]
    return max(short, key=lambda k: sims[k]) if short else None


def merge_windows(spans, emb, dur, win, hop, bounds) -> list[_Seg]:
    segs = [_Seg([i], spans, emb) for i in range(len(spans))]
    max_n = max(2, math.ceil(dur / 8))
    while len(segs) > 1:
        j = _pick_pair(segs, win, hop, bounds, max_n)
        if j is None:
            break
        segs[j: j + 2] = [_Seg(segs[j].idx + segs[j + 1].idx, spans, emb)]
    return segs


def boundaries(segs, m, dur, win, hop) -> list[float]:
    out = [0.0]
    for a, b in zip(segs, segs[1:]):
        coarse = (a.last[0] + b.first[0] + win) / 2 if b.first[0] < a.last[1] else b.first[0]
        out.append(refine_bound(m, coarse, hop))
    out.append(round(dur, 2))
    return out


def merge_slivers(segs, bounds) -> tuple[list, list[float]]:
    segs, bounds = list(segs), list(bounds)
    k = 1
    while k < len(bounds) - 1:
        if bounds[k] - bounds[k - 1] < MIN_LEN / 2 or bounds[k + 1] - bounds[k] < MIN_LEN / 2:
            spans, emb = segs[k].spans, segs[k].emb
            segs[k - 1: k + 1] = [_Seg(segs[k - 1].idx + segs[k].idx, spans, emb)]
            del bounds[k]
        else:
            k += 1
    return segs, bounds


# ---------------------------------------------------------------- fields and labels

def _level(x: np.ndarray, a: float, b: float) -> tuple[float | None, bool]:
    from hark.core.features import integrated_lufs

    seg = x[int(a * SR): int(b * SR)]
    silent = not np.any(seg)
    if seg.shape[0] / SR <= 0.4 or silent:
        return None, silent
    v = float(integrated_lufs(seg))
    return (round(v, 1) if v > -70 else None), silent


def tag_fields(vec_z: np.ndarray, vec_c: np.ndarray, vocab) -> dict:
    out = {}
    cats = np.asarray(vocab.term_category)
    for cat in CATS:
        idx = np.flatnonzero(cats == cat)
        top = idx[np.argsort(-vec_z[idx])][:2]
        out[cat] = [{"tag": vocab.terms[k], "cat": cat, "z": round(float(vec_z[k]), 2),
                     "cos": round(float(vec_c[k]), 3)} for k in top]
    return out


def _top(sec: dict, cat: str):
    return sec[cat][0] if sec.get(cat) else None


def _zof(item) -> float:
    return item["z"] if item else -math.inf


def short_term(tag: str) -> str:
    return tag[:-6] if tag.endswith(" music") else tag


def _music_part(sec: dict) -> str:
    parts = []
    g, ins, mood = _top(sec, "music_genre"), _top(sec, "instruments"), _top(sec, "music_mood")
    if g:
        parts.append(short_term(g["tag"]) + ("?" if g["z"] < 2.5 else ""))
    if ins and ins["z"] >= 1.5:
        parts.append(ins["tag"])
    if mood and mood["z"] >= 2.5:
        parts.append(" ".join(w for w in mood["tag"].split() if w != "music"))
    return ", ".join(parts)


def _extras(sec: dict, music: bool) -> str | None:
    extra = [lab.lower() for lab, p in sec.get("ast_top", []) if lab not in GENERIC_AST and p >= 0.08][:3]
    if extra:
        return "also: " + ", ".join(extra) + " (AST)"
    amb = _top(sec, "ambience")
    if amb and amb["z"] >= (2.5 if music else 1.5):
        return "amb: " + amb["tag"]
    return None


def label(sec: dict) -> str:
    if sec.get("digital_silence"):
        return "digital silence"
    if sec.get("lufs") is None or sec["lufs"] < -55:
        return "near silence"
    ast = sec.get("ast")
    music = ast["Music"] >= 0.3 if ast else _zof(_top(sec, "music_genre")) >= _zof(_top(sec, "ambience"))
    parts = [p for p in (_music_part(sec) if music else "", _extras(sec, music)) if p]
    voice = _top(sec, "voice")
    if voice and ((ast and ast["Speech"] >= 0.3) or (not ast and voice["z"] >= 2.5)):
        parts.append("voice: " + voice["tag"])
    return " | ".join(parts) if parts else "no confident music, ambience or voice tag"


def _key(lab: str):
    parts = lab.split(" | ")
    return parts[0].split(",")[0].strip(), parts[1:]


def merge_alike(secs: list[dict], bounds, x: np.ndarray) -> list[dict]:
    out: list[dict] = []
    for s in secs:
        if out and _key(out[-1]["label"]) == _key(s["label"]) and not any(abs(bt - s["start"]) <= 1.0
                                                                           for bt, _ in bounds):
            a = out[-1]
            keep = dict(a if (a["end"] - a["start"]) >= (s["end"] - s["start"]) else s)
            keep.update(start=a["start"], end=s["end"], windows=a["windows"] + s["windows"])
            keep["lufs"], keep["digital_silence"] = _level(x, keep["start"], keep["end"])
            out[-1] = keep
        else:
            out.append(s)
    return out


# ---------------------------------------------------------------- AST crops

def plan_crops(secs: list[dict], budget: int) -> dict[int, int]:
    order = sorted(range(len(secs)), key=lambda k: -(secs[k]["end"] - secs[k]["start"]))
    plan = {k: 1 for k in order[:budget]}
    for k in order:
        if k in plan and secs[k]["end"] - secs[k]["start"] > 15 and sum(plan.values()) < budget:
            plan[k] = 2
    return plan


def crops(sec: dict, count: int) -> list[tuple[float, float]]:
    d = sec["end"] - sec["start"]
    w = min(d, 10.24)
    if count == 1:
        a = sec["start"] + (d - w) / 2
        return [(round(a, 3), round(a + w, 3))]
    starts = [sec["start"] + (d - w) * (j + 0.5) / 2 for j in range(2)]
    return [(round(a, 3), round(a + w, 3)) for a in starts]


def section_ast(secs, budget, m, offset, cache) -> int:
    if budget <= 0 or not secs:
        return 0
    from hark.models import ast as ast_mod
    from hark.timeline._tags import cached_vectors

    ast = ast_mod.get()
    total = 0
    for k, count in plan_crops(secs, budget).items():
        spans = crops(secs[k], count)
        P = cached_vectors(spans, m, "ast", cache, lambda xs: ast.probs(xs), offset).mean(axis=0)
        secs[k]["ast"] = {lab: round(float(ast.prob_of(P, lab)), 3) for lab in ("Music", "Speech", "Silence")}
        secs[k]["ast_top"] = [[lab, float(p)] for lab, p in ast.top(P, k=5, min_p=0.05)]
        total += len(spans)
    return total


# ---------------------------------------------------------------- the pass

def sections(x: np.ndarray, t0: float, win: float, hop: float, ast_budget: int, cache) -> tuple[list[dict], dict]:
    """Sections of the window x (times relative to x; t0 is its file time for the cache keys)."""
    m = mono_of(x)
    dur = m.size / SR
    if dur < 1.0:
        return [], {"windows": 0, "ast": 0}
    from hark.listen import vocabulary
    from hark.models import clap as clap_mod
    from hark.timeline._tags import cached_vectors

    clap = clap_mod.get()
    spans = [(s, round(min(dur, s + win), 3)) for s in window_starts(dur, win, hop)]
    emb = cached_vectors(spans, m, "clap-repeat", cache, lambda xs: clap.embed_audio(xs, pad="repeatpad"), t0)
    emb = emb / (np.linalg.norm(emb, axis=1, keepdims=True) + 1e-12)
    bounds = novelty_bounds(m)
    segs = merge_windows(spans, emb, dur, win, hop, bounds)
    segs, edges = merge_slivers(segs, boundaries(segs, m, dur, win, hop))
    vocab = vocabulary.get()
    vecs = np.vstack([s.vec for s in segs])
    cal = vocab.calibrate(vecs)
    secs = []
    for k, s in enumerate(segs):
        sec = {"start": round(edges[k], 2), "end": round(edges[k + 1], 2), "windows": len(s.idx),
               "coherence": round(float((emb[s.idx] @ s.vec).mean()), 3)}
        sec.update(tag_fields(np.asarray(cal.Z[k]), np.asarray(cal.C[k]), vocab))
        sec["lufs"], sec["digital_silence"] = _level(x, sec["start"], sec["end"])
        secs.append(sec)
    n_ast = section_ast(secs, ast_budget, m, t0, cache)
    for sec in secs:
        sec["label"] = label(sec)
    return merge_alike(secs, bounds, x), {"windows": len(spans), "ast": n_ast}
