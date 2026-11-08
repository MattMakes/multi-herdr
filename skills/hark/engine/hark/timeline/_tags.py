"""Event tags: which events to tag, CLAP phrase scoring with background damping, AST labels (analysis 2.8)."""

from __future__ import annotations

import hashlib
import os
from typing import Any

import numpy as np

from hark.timeline._dsp import SR

TEMPLATES = ("{}", "the sound of {}", "a recording of {}", "{} sound effect", "an audio clip of {}")

PHRASES: dict[str, tuple[str, ...]] = {
    "WHOOSH": ("a fast air whoosh", "a swoosh passing by", "a short swish", "a whip pan whoosh transition"),
    "RISER": ("a rising riser building tension", "a reverse cymbal swell", "an uplifter sweep"),
    "HIT": ("a deep cinematic boom", "a punchy impact hit", "a heavy thud", "a sub bass drop", "a soft thump"),
    "KEY": ("a mechanical keyboard key press", "typing on a computer keyboard", "a laptop key click",
            "a spacebar press"),
    "UI": ("a mouse click", "a soft UI button tap", "a notification ding", "a chat message pop",
           "a toggle switch click", "a camera shutter click"),
    "STEP": ("footsteps on grass", "footsteps on wood", "a small soft footstep"),
    "FOLEY": ("a plastic click", "lego bricks clicking", "a wooden block hit", "glass clinking", "paper rustling",
              "leaves rustling", "a cork pop", "a bubble pop", "a water drop", "a small object dropping on a table"),
    "NOTE": ("a piano note", "a bell chime", "a plucked string note", "an orchestral stab", "a celesta note",
             "a glockenspiel note"),
    "DRUM": ("a kick drum", "a snare drum hit", "a hi-hat", "a drum hit"),
    "VOICE": ("a spoken word", "a person talking", "a breath", "a short vocal sound"),
}

# Mean CLAP cosine of each phrase over real background events (data asset A2; regenerate for another checkpoint).
BACKGROUND: dict[str, float] = {
    "a fast air whoosh": -0.0137, "a swoosh passing by": -0.0271, "a short swish": 0.0538,
    "a whip pan whoosh transition": 0.0173, "a rising riser building tension": -0.0403,
    "a reverse cymbal swell": -0.0354, "an uplifter sweep": -0.0522, "a deep cinematic boom": -0.0294,
    "a punchy impact hit": 0.0361, "a heavy thud": 0.0383, "a sub bass drop": -0.0194, "a soft thump": -0.0344,
    "a mechanical keyboard key press": 0.1109, "typing on a computer keyboard": -0.0065,
    "a laptop key click": 0.1327, "a spacebar press": 0.1456, "a mouse click": 0.0743,
    "a soft UI button tap": 0.0578, "a notification ding": 0.0487, "a chat message pop": 0.1634,
    "a toggle switch click": 0.1030, "a camera shutter click": 0.0964, "footsteps on grass": -0.0284,
    "footsteps on wood": -0.0025, "a small soft footstep": 0.0282, "a plastic click": 0.0729,
    "lego bricks clicking": -0.0168, "a wooden block hit": 0.0335, "glass clinking": -0.0419,
    "paper rustling": -0.0469, "leaves rustling": -0.0228, "a cork pop": 0.0633, "a bubble pop": 0.0977,
    "a water drop": 0.1012, "a small object dropping on a table": 0.0316, "a piano note": 0.0143,
    "a bell chime": -0.0081, "a plucked string note": 0.0687, "an orchestral stab": 0.0640,
    "a celesta note": 0.0378, "a glockenspiel note": 0.0350, "a kick drum": 0.0659, "a snare drum hit": 0.0322,
    "a hi-hat": 0.0719, "a drum hit": 0.0758, "a spoken word": -0.0140, "a person talking": -0.1026,
    "a breath": 0.0520, "a short vocal sound": 0.0318,
}

KINDS = tuple(PHRASES)
PHRASE_LIST = [p for ps in PHRASES.values() for p in ps]
PHRASE_KIND = {p: k for k, ps in PHRASES.items() for p in ps}
SPAN_VERSION = 1
BUDGETS = {  # detail -> (tag, ast, win, hop, max_win, ast_sec)
    "low": (12, 0, 4.0, 4.0, 30, 0),
    "normal": (20, 1, 3.0, 2.0, 32, 3),
    "high": (96, 16, 3.0, 1.5, 200, 12),
}


def short_label(phrase: str) -> str:
    for art in ("a ", "an ", "the "):
        if phrase.startswith(art):
            return phrase[len(art):]
    return phrase


# ---------------------------------------------------------------- which events

def _priority(events: list[dict], near_times) -> list[int]:
    near = np.asarray(sorted(near_times), dtype=float)

    def is_near(e):
        return near.size > 0 and bool(np.min(np.abs(near - e["t"])) <= 0.1 + 1e-9)

    return sorted(range(len(events)), key=lambda i: (not is_near(events[i]), -events[i].get("prominence_db", 0),
                                                     -events[i].get("contrast_db", 0)))


def _burst_order(events: list[dict], order: list[int]) -> list[int]:
    kept, followers = [], []
    for i in order:
        e = events[i]
        if any(events[k]["band"] == e["band"] and abs(events[k]["t"] - e["t"]) < 0.5 for k in kept):
            followers.append(i)
        else:
            kept.append(i)
    return kept + followers


def _first_per_bin(events: list[dict], combined: list[int], bins: int, lo: float, hi: float) -> list[int]:
    """The first index in priority order that falls in each of `bins` equal time bins."""
    width = (hi - lo) / bins if hi > lo else 1.0
    seen, first = set(), []
    for i in combined:
        b = min(bins - 1, max(0, int((events[i]["t"] - lo) / width)))
        if b not in seen:
            seen.add(b)
            first.append(i)
    return first


def pick(events: list[dict], n: int, near_times, lo: float, hi: float) -> tuple[list[int], list[int]]:
    """Chosen event indices (sorted) and the combined priority order."""
    if n <= 0 or not events:
        return [], []
    combined = _burst_order(events, _priority(events, near_times))
    first = _first_per_bin(events, combined, max(1, n // 2), lo, hi)
    taken = set(first)
    rest = [i for i in combined if i not in taken]
    return sorted((first + rest)[:n]), combined


def span(ev: dict, duration: float) -> tuple[float, float]:
    t = ev["t"]
    if ev.get("shape") == "swell":
        a, b = t - (ev["rise_ms"] / 1000 if ev.get("rise_ms") else 0.5), t + 0.3
    else:
        a, b = t - 0.02, t + min(max(ev.get("length_ms", 0) / 1000, 0.3), 1.5)
    return round(max(0.0, a), 3), round(min(duration, b), 3)


def excerpt(m: np.ndarray, a: float, b: float) -> np.ndarray:
    i = int(round(a * SR))
    j = max(i + 480, int(round(b * SR)))
    return np.asarray(m[i:j], dtype=np.float32)


# ---------------------------------------------------------------- per-span model cache

def cache_dir() -> str:
    home = os.environ.get("HARK_HOME")
    if not home:
        from hark.core import config

        home = str(config.HOME)
    return os.path.join(home, "emb", "timeline")


class SpanCache:
    """Disk cache of CLAP embeddings and AST probabilities per file-time span."""

    def __init__(self, source_id: str | None, reuse: bool):
        self.source_id, self.reuse = source_id, reuse

    def _path(self, a: float, b: float, kind: str) -> str | None:
        if not self.source_id:
            return None
        key = f"{self.source_id}|mono|{a:.3f}|{b:.3f}|{kind}|v{SPAN_VERSION}"
        return os.path.join(cache_dir(), hashlib.sha1(key.encode()).hexdigest()[:24] + ".npy")

    def get(self, a: float, b: float, kind: str) -> np.ndarray | None:
        p = self._path(a, b, kind)
        if p is None or not self.reuse or not os.path.isfile(p):
            return None
        try:
            return np.load(p)
        except (OSError, ValueError):
            return None

    def put(self, a: float, b: float, kind: str, v: np.ndarray) -> None:
        p = self._path(a, b, kind)
        if p is None:
            return
        try:
            os.makedirs(os.path.dirname(p), exist_ok=True)
            tmp = f"{p}.{os.getpid()}.tmp.npy"
            np.save(tmp, np.asarray(v, dtype=np.float32))
            os.replace(tmp, p)
        except OSError:
            pass


def cached_vectors(spans, m: np.ndarray, kind: str, cache: SpanCache, compute, offset: float = 0.0) -> np.ndarray:
    """One vector per span of m; the cache key uses file time (span + offset).

    Misses are computed in one batch with compute(list of arrays).
    """
    keys = [(round(a + offset, 3), round(b + offset, 3)) for a, b in spans]
    out: list[Any] = [cache.get(a, b, kind) for a, b in keys]
    miss = [k for k, v in enumerate(out) if v is None]
    if miss:
        fresh = np.asarray(compute([excerpt(m, *spans[k]) for k in miss]))
        for k, v in zip(miss, fresh):
            out[k] = v
            cache.put(*keys[k], kind, v)
    return np.vstack(out) if out else np.zeros((0, 0))


# ---------------------------------------------------------------- scoring

def phrase_matrix(clap) -> np.ndarray:
    texts = [tpl.format(p) for p in PHRASE_LIST for tpl in TEMPLATES]
    E = np.asarray(clap.embed_text(texts), dtype=np.float64).reshape(len(PHRASE_LIST), len(TEMPLATES), -1).mean(1)
    return E / (np.linalg.norm(E, axis=1, keepdims=True) + 1e-12)


def phrase_z(A: np.ndarray, E: np.ndarray) -> np.ndarray:
    A = A / (np.linalg.norm(A, axis=1, keepdims=True) + 1e-12)
    bg = np.array([BACKGROUND[p] for p in PHRASE_LIST])
    d = A @ E.T - 0.5 * bg
    return (d - d.mean(axis=1, keepdims=True)) / (d.std(axis=1, keepdims=True) + 1e-9)


def apply_tags(ev: dict, zrow: np.ndarray) -> None:
    top = np.argsort(-zrow)[:3]
    ev["tags"] = [{"tag": PHRASE_LIST[k], "kind": PHRASE_KIND[PHRASE_LIST[k]], "z": round(float(zrow[k]), 2)}
                  for k in top]
    ev["kind"] = ev["tags"][0]["kind"]
    ev["label"] = short_label(ev["tags"][0]["tag"])
    ev["exposed"] = ev.get("prominence_db", 0) >= 12


def apply_ast(ev: dict, p: np.ndarray, ast) -> None:
    ev["ast"] = [[lab, float(q)] for lab, q in ast.top(p, k=4, min_p=0.05)]
    speech = float(ast.prob_of(p, "Speech"))
    if speech >= 0.5 and ev.get("kind") != "VOICE":
        ev["kind"], ev["label"], ev["exposed"] = "VOICE", f"speech (AST {speech:.2f})", True


def tag_events(events: list[dict], m: np.ndarray, duration: float, detail: str, near_times, lo: float,
               hi: float, cache: SpanCache, offset: float = 0.0) -> dict:
    """Tag chosen events in place. Spans are file time (context time + offset)."""
    n_tag, n_ast = BUDGETS[detail][:2]
    chosen, order = pick(events, n_tag, near_times, lo, hi)
    if not chosen:
        return {"tagged": 0, "ast": 0, "clap_windows": 0}
    from hark.models import clap as clap_mod

    clap = clap_mod.get()
    spans = [span(events[i], duration) for i in chosen]
    A = cached_vectors(spans, m, "clap-repeat", cache, lambda xs: clap.embed_audio(xs, pad="repeatpad"), offset)
    Z = phrase_z(np.asarray(A, dtype=np.float64), phrase_matrix(clap))
    for row, i in enumerate(chosen):
        apply_tags(events[i], Z[row])
    n_ast_done = _ast_pass(events, [i for i in order if i in set(chosen)][:n_ast], duration, offset, m, cache)
    return {"tagged": len(chosen), "ast": n_ast_done, "clap_windows": len(chosen)}


def _ast_pass(events, idx, duration, offset, m, cache) -> int:
    if not idx:
        return 0
    from hark.models import ast as ast_mod

    ast = ast_mod.get()
    spans = [span(events[i], duration) for i in idx]
    P = cached_vectors(spans, m, "ast", cache, lambda xs: ast.probs(xs), offset)
    for row, i in enumerate(idx):
        apply_ast(events[i], P[row], ast)
    return len(idx)
