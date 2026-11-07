"""The listening vocabulary, CLAP embeddings with prompt templates, and the z calibration.

Every text-scoring command of hark listen shares this module. A raw CLAP cosine is not comparable
across sounds or texts. The calibration turns it into a z: how far one description stands out, for
this sound, among the vocabulary terms, after the description's own bias is removed.

Run ``python -m hark.listen.vocabulary`` to check the vocabulary file, or with
``--build-stats REF ...`` to rebuild the reference statistics file.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import re
import sys
import threading
from collections.abc import Sequence
from pathlib import Path
from typing import Any, NamedTuple

import numpy as np

from hark.core import audio, config

SR = 48000
DIM = 512
PAD = "repeatpad"
PADS = ("repeatpad", "zero", "spaced")
WIN_S = 10.0
HOP_S = 5.0
MAX_WIN = 6
REF_MAX_WIN = 3
MIN_SAMPLES = 480
SPACED_UNIT_S = 1.0
SPAN_BATCH = 8
TEXT_SPLIT = 16
SIGMA_FLOOR = 1e-9
NORM_FLOOR = 1e-12
TIE_EPS = 1e-6
NEAR_SILENT_DB = -70.0
DEFAULT_LOGIT_SCALE = 38.75
CACHE_VERSION = 1

UNVALIDATED = ("music_genre", "music_mood", "instruments", "voice")
NOT_SOUNDS_LIKE = ("quality", "production")
CATEGORY_NAMES = (
    "transitions", "impacts", "ui", "keyboard", "foley", "water", "nature", "chimes", "ambience",
    "everyday", "music_genre", "music_mood", "instruments", "voice", "quality", "production",
)

VERDICT_THRESHOLDS_Z: dict[str, float] = {"strong": 3.0, "plausible": 2.5, "weak": 2.0}
NO_MATCH = "no match"

VERDICT_EVIDENCE: dict[str, dict] = {
    "strong": {"z": ">= 3", "lr": 1289, "precision_library": 0.98, "precision_even_odds": 1.0,
               "lr_strict": 51, "precision_library_strict": 0.61, "recall_at_or_above": 0.51},
    "plausible": {"z": "2.5 - 3", "lr": 21, "precision_library": 0.41, "precision_even_odds": 0.95,
                  "lr_strict": 9.1, "precision_library_strict": 0.22, "recall_at_or_above": 0.63},
    "weak": {"z": "2 - 2.5", "lr": 12, "precision_library": 0.28, "precision_even_odds": 0.92,
             "lr_strict": 6.6, "precision_library_strict": 0.17, "recall_at_or_above": 0.76},
    "no match": {"z": "< 2", "lr": 0.25, "precision_library": 0.008, "precision_even_odds": 0.2,
                 "lr_strict": 0.25, "precision_library_strict": 0.008},
}

DATA_DIR = Path(__file__).resolve().parent.parent / "data" / "listen"
VOCAB_PATH = DATA_DIR / "vocab.json"
STATS_PATH = DATA_DIR / "vocab_stats.json"

Span = tuple[float, float]


# ---------------------------------------------------------------- verdicts and text


def verdict(z: float) -> str:
    """The verdict band of a calibrated z (bands are not cumulative)."""
    for name, lo in VERDICT_THRESHOLDS_Z.items():
        if z >= lo:
            return name
    return NO_MATCH


def zf(z: float) -> str:
    """z as text, floored to 0.1 with a sign, so it never crosses a verdict threshold."""
    return format(math.floor(z * 10 + 1e-9) / 10, "+.1f")


def softmax(x: Sequence[float], s: float) -> np.ndarray:
    """Forced choice over raw cosines with the CLAP logit scale ``s``."""
    v = np.asarray(x, dtype=np.float64)
    e = np.exp(s * (v - v.max()))
    return e / e.sum()


def pct_text(p: float) -> str:
    """A forced-choice probability as a percentage text."""
    if p > 0.995:
        return ">99%"
    if p < 0.005:
        return "<1%"
    return f"{int(round(p * 100))}%"


def unit(v: np.ndarray) -> np.ndarray:
    """v divided by its norm (+1e-12), row-wise for 2-D input."""
    v = np.asarray(v, dtype=np.float64)
    n = np.linalg.norm(v, axis=-1, keepdims=True)
    return v / (n + NORM_FLOOR)


# ---------------------------------------------------------------- ranges and batches


def check_range(path: str, start: float | None = None, end: float | None = None) -> float:
    """Validate a range against the file; return the file duration in seconds."""
    duration = audio.duration(path)
    if start is not None and start < 0:
        raise ValueError(f"start {start:g} s is negative")
    if start is not None and start >= duration:
        raise ValueError(f"start {start:g} s is at or past the end of {path} ({duration:.3f} s long)")
    if end is not None and end <= (start or 0.0):
        raise ValueError("end must be after start")
    return duration


def clamp_range(duration: float, start: float | None, end: float | None) -> Span:
    """The judged span [s, e) of a checked range (end clamped to the duration)."""
    s = max(0.0, start or 0.0)
    e = duration if end is None else min(duration, end)
    return s, e


def first_line(exc: BaseException, limit: int = 200) -> str:
    """The first line of an exception message, at most ``limit`` characters."""
    text = str(exc).strip().splitlines()
    return (text[0] if text else type(exc).__name__)[:limit]


def split_readable(paths: Sequence[str]) -> tuple[list[str], list[dict]]:
    """Split paths into decodable ones and ``{path, error}`` rows."""
    ok: list[str] = []
    skipped: list[dict] = []
    for p in paths:
        try:
            audio.duration(p)
            ok.append(p)
        except Exception as exc:  # one bad file never fails a batch
            skipped.append({"path": p, "error": first_line(exc)})
    return ok, skipped


def peak_db(path: str, start: float | None = None, end: float | None = None, mono: bool = False) -> float:
    """Sample peak of a decoded range in dB (an empty decode reads as silence)."""
    x = audio.load(path, mono=mono, start=start, end=end)
    return array_peak_db(x)


def array_peak_db(x: np.ndarray) -> float:
    """Sample peak of an array in dB."""
    return float(audio.db(float(np.max(np.abs(x))) if x.size else 0.0))


def same_path(a: str, b: str) -> bool:
    """True when 2 paths name one file (absolute, case-normalised)."""
    return os.path.normcase(os.path.abspath(a)) == os.path.normcase(os.path.abspath(b))


# ---------------------------------------------------------------- windows


def spans(start: float, end: float, max_win: int = MAX_WIN) -> list[Span]:
    """CLAP windows of [start, end): 10 s windows, 5 s hop, at most ``max_win`` evenly spaced."""
    w, h = int(WIN_S * SR), int(HOP_S * SR)
    n = int(round((end - start) * SR))
    if n <= w:
        return [(start, end)]
    starts = list(range(0, n - w + 1, h))
    if starts[-1] + w != n:
        starts.append(n - w)
    if len(starts) > max_win:
        picks = np.linspace(0, len(starts) - 1, max_win)
        starts = [starts[int(round(i))] for i in picks]
    return [(start + s / SR, start + (s + w) / SR) for s in starts]


def applied_pad(duration: float, pad: str) -> str:
    """The padding that a span of ``duration`` seconds really gets (part of its cache key)."""
    if duration >= WIN_S - 0.001:
        return "-"
    if pad == "spaced" and duration >= SPACED_UNIT_S:
        return "repeatpad"
    return pad


def prepare(seg: np.ndarray, pad: str) -> np.ndarray:
    """One mono span made ready for a CLAP window under padding mode ``pad``."""
    w = int(WIN_S * SR)
    x = np.asarray(seg, dtype=np.float32)[:w]
    if len(x) < MIN_SAMPLES:
        x = np.pad(x, (0, MIN_SAMPLES - len(x)))
    if pad == "zero" and len(x) < w:
        return np.pad(x, (0, w - len(x)))
    if pad == "spaced" and len(x) < SPACED_UNIT_S * SR:
        cell = np.pad(x, (0, int(SPACED_UNIT_S * SR) - len(x)))
        return np.tile(cell, int(math.ceil(w / len(cell))))[:w]
    return x


# ---------------------------------------------------------------- cache


def _slug(text: str) -> str:
    return re.sub(r"[^0-9A-Za-z._-]", "_", str(text))


def cache_dir(kind: str = "clap") -> Path:
    """The listen cache folder of the configured model (no model load needed)."""
    model = config.CLAP_MODEL if kind == "clap" else getattr(config, "AST_MODEL", "ast")
    rev = getattr(config, "CLAP_REVISION" if kind == "clap" else "AST_REVISION", None)
    name = _slug(model) + (f"--{_slug(rev)}" if rev else "") + f"--{_slug(config.PRECISION)}"
    return Path(config.EMB_CACHE) / "listen" / kind / name


def key_of(*parts: Any) -> str:
    """A 24-hex cache key of the given parts."""
    text = "|".join(str(p) for p in (CACHE_VERSION, *parts))
    return hashlib.sha1(text.encode("utf-8")).hexdigest()[:24]


def load_array(path: Path, shape: tuple[int, ...] | None = None) -> np.ndarray | None:
    """A cached array, or None when it is absent, unreadable or of the wrong shape."""
    try:
        arr = np.load(path, allow_pickle=False)
    except Exception:
        return None
    if shape is not None and arr.shape != shape:
        return None
    return arr


def save_array(path: Path, arr: np.ndarray) -> None:
    """Write an array atomically. A failed write never fails the call."""
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        tmp = path.with_name(f"{path.stem}.{os.getpid()}.{threading.get_ident()}.tmp.npy")
        np.save(tmp, arr)
        audio.replace_quietly(str(tmp), str(path))
    except Exception:
        pass


def load_json(path: Path) -> Any:
    """A cached JSON value, or None."""
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return None


def save_json(path: Path, value: Any) -> None:
    """Write JSON atomically. A failed write never fails the call."""
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        tmp = path.with_name(f"{path.name}.{os.getpid()}.{threading.get_ident()}.tmp")
        tmp.write_text(json.dumps(value), encoding="utf-8")
        audio.replace_quietly(str(tmp), str(path))
    except Exception:
        pass


# ---------------------------------------------------------------- text embedding


def clap_model():
    """The process CLAP model (loads it on first use)."""
    from hark.models import clap

    return clap.get()


def fill(template: str, phrase: str) -> str:
    """Put a phrase into a template at its ``{}``."""
    return template.replace("{}", phrase, 1)


def _embed_with_templates(phrases: Sequence[str], templates: Sequence[str]) -> np.ndarray:
    texts = [fill(t, p) for p in phrases for t in templates]
    raw = np.asarray(clap_model().embed_text(texts), dtype=np.float64)
    mean = raw.reshape(len(phrases), len(templates), -1).mean(axis=1)
    return unit(mean).astype(np.float32)


def _text_key(templates: Sequence[str], phrases: Sequence[str]) -> str:
    return key_of("text", "\x1f".join(templates), "\x1e".join(phrases))


def embed_phrases(phrases: Sequence[str], templates: Sequence[str]) -> np.ndarray:
    """Template-averaged unit text embeddings, (k, 512) float32, cached on disk."""
    phrases = list(phrases)
    if not phrases:
        return np.zeros((0, DIM), np.float32)
    if len(phrases) > TEXT_SPLIT:
        return _embed_list_cached(phrases, templates)
    return np.stack([_embed_one_cached(p, templates) for p in phrases])


def _embed_list_cached(phrases: list[str], templates: Sequence[str]) -> np.ndarray:
    path = cache_dir() / f"tl_{_text_key(templates, phrases)}.npy"
    arr = load_array(path, (len(phrases), DIM))
    if arr is None:
        arr = _embed_with_templates(phrases, templates)
        save_array(path, arr)
    return arr.astype(np.float32)


def _embed_one_cached(phrase: str, templates: Sequence[str]) -> np.ndarray:
    path = cache_dir() / f"t_{_text_key(templates, [phrase])}.npy"
    arr = load_array(path, (DIM,))
    if arr is None:
        arr = _embed_with_templates([phrase], templates)[0]
        save_array(path, arr)
    return arr.astype(np.float32)


def embed_plain_text(texts: Sequence[str]) -> np.ndarray:
    """Plain CLAP text embeddings (no templates), cached per text."""
    out = []
    for t in texts:
        path = cache_dir() / f"p_{key_of('plain', t)}.npy"
        arr = load_array(path, (DIM,))
        if arr is None:
            arr = unit(np.asarray(clap_model().embed_text([t]), dtype=np.float64)[0]).astype(np.float32)
            save_array(path, arr)
        out.append(arr)
    return np.stack(out) if out else np.zeros((0, DIM), np.float32)


# ---------------------------------------------------------------- audio embedding


def _span_key(fid: str, span: Span, pad: str) -> str:
    s, e = span
    return key_of("span", fid, round(s, 4), round(e, 4), applied_pad(e - s, pad))


def _slice(x: np.ndarray, span: Span) -> np.ndarray:
    a = max(0, int(round(span[0] * SR)))
    b = max(a, int(round(span[1] * SR)))
    return x[a:b]


def _embed_arrays(segs: list[np.ndarray]) -> np.ndarray:
    out = []
    model = clap_model()
    for i in range(0, len(segs), SPAN_BATCH):
        emb = np.asarray(model.embed_audio(segs[i:i + SPAN_BATCH], pad=PAD), dtype=np.float64)
        out.append(unit(np.atleast_2d(emb)))
    return np.concatenate(out).astype(np.float32)


def _missing_spans(items: Sequence[tuple[str, Sequence[Span]]], pad: str, out: list[list]) -> list:
    todo = []
    for i, (path, sp) in enumerate(items):
        fid = audio.file_id(path)
        for j, span in enumerate(sp):
            p = cache_dir() / f"s_{_span_key(fid, span, pad)}.npy"
            arr = load_array(p, (DIM,))
            if arr is None:
                todo.append((i, j, p))
            else:
                out[i][j] = arr
    return todo


def _fill_missing(items: Sequence[tuple[str, Sequence[Span]]], todo: list, pad: str, out: list[list]) -> None:
    decoded: dict[int, np.ndarray] = {}
    for b in range(0, len(todo), SPAN_BATCH):
        chunk = todo[b:b + SPAN_BATCH]
        segs = []
        for i, j, _ in chunk:
            if i not in decoded:
                decoded.clear()
                decoded[i] = audio.load(items[i][0], mono=True)
            segs.append(prepare(_slice(decoded[i], items[i][1][j]), pad))
        for (i, j, p), emb in zip(chunk, _embed_arrays(segs)):
            save_array(p, emb)
            out[i][j] = emb


def span_embeddings(items: Sequence[tuple[str, Sequence[Span]]], pad: str = PAD) -> list[np.ndarray]:
    """Unit embeddings of every span of every (path, spans) item, read from the cache when present."""
    out: list[list] = [[None] * len(sp) for _, sp in items]
    todo = _missing_spans(items, pad, out)
    if todo:
        _fill_missing(items, todo, pad, out)
    return [np.stack(row).astype(np.float32) if row else np.zeros((0, DIM), np.float32) for row in out]


def file_spans(path: str, start: float | None = None, end: float | None = None,
               max_win: int | None = None) -> list[Span]:
    """The CLAP spans of a file range (A4 steps 1 and 2)."""
    d = audio.duration(path)
    s, e = clamp_range(d, start, end)
    return spans(s, max(e, s + 0.01), max_win or MAX_WIN)


def embed_files(paths: Sequence[str], pad: str = PAD, max_win: int | None = None,
                ranges: Sequence[tuple[float | None, float | None]] | None = None) -> np.ndarray:
    """One unit embedding per file (mean of its span embeddings), (n, 512) float32."""
    if not paths:
        return np.zeros((0, DIM), np.float32)
    rng = list(ranges) if ranges is not None else [(None, None)] * len(paths)
    items = [(p, file_spans(p, s, e, max_win)) for p, (s, e) in zip(paths, rng)]
    embs = span_embeddings(items, pad)
    return np.stack([unit(E.mean(axis=0)) for E in embs]).astype(np.float32)


def embed_file(path: str, start: float | None = None, end: float | None = None, pad: str = PAD) -> np.ndarray:
    """The unit embedding of one file range, (512,)."""
    return embed_files([path], pad, ranges=[(start, end)])[0]


def embed_audio(arrays: Sequence[np.ndarray], pad: str = PAD) -> np.ndarray:
    """One unit embedding per in-memory 48 kHz array (same windows and padding as files)."""
    out = []
    for x in arrays:
        m = audio.to_mono(np.asarray(x, dtype=np.float32))
        sp = spans(0.0, max(len(m) / SR, 0.01))
        segs = [prepare(_slice(m, s), pad) for s in sp]
        out.append(unit(_embed_arrays(segs).mean(axis=0)))
    return np.stack(out).astype(np.float32) if out else np.zeros((0, DIM), np.float32)


# ---------------------------------------------------------------- calibration


class Calibration(NamedTuple):
    """The calibration of n sounds against T vocabulary terms and k queries."""

    C: np.ndarray
    U: np.ndarray
    Z: np.ndarray
    mu: np.ndarray
    sigma: np.ndarray
    Cq: np.ndarray | None = None
    z_q: np.ndarray | None = None
    percentile: np.ndarray | None = None
    rank: np.ndarray | None = None


def read_vocab(path: Path = VOCAB_PATH) -> dict:
    """The vocabulary file as a dict."""
    return json.loads(Path(path).read_text(encoding="utf-8"))


def check_vocab(data: dict) -> list[str]:
    """Problems of a vocabulary file (empty when it is valid)."""
    problems = [f"template {t!r} must hold exactly one {{}}" for t in data.get("templates", [])
                if t.count("{}") != 1]
    seen: set[str] = set()
    for cat, phrases in data.get("categories", {}).items():
        for p in phrases:
            if p in seen:
                problems.append(f"phrase {p!r} appears twice")
            seen.add(p)
    problems += [f"category {c!r} is missing" for c in CATEGORY_NAMES if c not in data.get("categories", {})]
    return problems


def read_stats(path: Path, model_id: str) -> dict | None:
    """The reference statistics of ``model_id``, or None (missing file or another model)."""
    stats = load_json(Path(path))
    if not isinstance(stats, dict) or stats.get("model") != model_id or len(stats.get("mean", [])) != DIM:
        return None
    return stats


class Vocab:
    """The vocabulary terms, their embeddings, and the calibration against them."""

    def __init__(self, path: Path = VOCAB_PATH, stats_path: Path = STATS_PATH):
        data = read_vocab(path)
        self.templates: list[str] = list(data["templates"])
        self.categories: list[str] = list(data["categories"])
        self.terms: list[str] = [p for c in self.categories for p in data["categories"][c]]
        self.term_category: list[str] = [c for c in self.categories for _ in data["categories"][c]]
        self.model_id: str = config.CLAP_MODEL
        self.E: np.ndarray = embed_phrases(self.terms, self.templates)
        self.stats = read_stats(stats_path, self.model_id)
        self.mean = np.asarray(self.stats["mean"], np.float64) if self.stats else None
        self.bE = self.bias(self.E)
        self.index = {t: i for i, t in enumerate(self.terms)}
        self._scale: float | None = float(self.stats["logit_scale"]) if self.stats else None

    @property
    def logit_scale(self) -> float:
        """The CLAP softmax temperature: from the statistics, else from the model (cached)."""
        if self._scale is None:
            self._scale = _model_logit_scale()
        return self._scale

    def embed_text(self, texts: Sequence[str]) -> np.ndarray:
        """Template-averaged unit embeddings; a vocabulary term reuses its own row (exact ties)."""
        texts = list(texts)
        if not texts:
            return np.zeros((0, DIM), np.float32)
        new = [t for t in texts if t not in self.index]
        emb = dict(zip(new, embed_phrases(new, self.templates))) if new else {}
        return np.stack([self.E[self.index[t]] if t in self.index else emb[t] for t in texts])

    def bias(self, Q: np.ndarray) -> np.ndarray:
        """The term bias b(q) = q . m of every row (0 without statistics)."""
        Q = np.atleast_2d(np.asarray(Q, np.float64))
        return np.zeros(len(Q)) if self.mean is None else Q @ self.mean

    def calibrate(self, A: np.ndarray, Q: np.ndarray | None = None, method: str = "both") -> Calibration:
        """Cosines, bias-free scores, per-sound z, percentile and rank (A6)."""
        A = np.atleast_2d(np.asarray(A, np.float64))
        center, per_sound = method in ("center", "both"), method in ("sound", "both")
        C = A @ np.asarray(self.E, np.float64).T
        U = C - self.bE[None, :] if center else C
        mu = U.mean(axis=1) if per_sound else np.zeros(len(A))
        sigma = U.std(axis=1) + SIGMA_FLOOR if per_sound else np.ones(len(A))
        Z = (U - mu[:, None]) / sigma[:, None]
        if Q is None:
            return Calibration(C, U, Z, mu, sigma)
        return self._with_queries(A, np.atleast_2d(np.asarray(Q, np.float64)), Calibration(C, U, Z, mu, sigma),
                                  center)

    def _with_queries(self, A: np.ndarray, Q: np.ndarray, cal: Calibration, center: bool) -> Calibration:
        Cq = A @ Q.T
        Uq = Cq - self.bias(Q)[None, :] if center else Cq
        zq = (Uq - cal.mu[:, None]) / cal.sigma[:, None]
        diff = cal.U[:, :, None] - Uq[:, None, :]
        pct = (diff < -TIE_EPS).mean(axis=1)
        rank = 1 + (diff > TIE_EPS).sum(axis=1)
        return cal._replace(Cq=Cq, z_q=zq, percentile=pct, rank=rank)

    def scores(self, A: np.ndarray, queries: Sequence[str]) -> list[list[dict]]:
        """Per sound and per query: {query, cosine, z, percentile, rank_in_vocab, vocab_size} (A7)."""
        cal = self.calibrate(A, self.embed_text(queries))
        return [[self._record(cal, i, j, q) for j, q in enumerate(queries)] for i in range(len(cal.C))]

    def _record(self, cal: Calibration, i: int, j: int, query: str) -> dict:
        return {"query": query, "cosine": round(float(cal.Cq[i, j]), 3), "z": round(float(cal.z_q[i, j]), 2),
                "percentile": round(100 * float(cal.percentile[i, j]), 1),
                "rank_in_vocab": int(cal.rank[i, j]), "vocab_size": len(self.terms)}

    def top_terms(self, a: np.ndarray, k: int, cats: Sequence[str] | None = None,
                  exclude: Sequence[str] = ()) -> list[tuple[str, str, float, float]]:
        """The k best terms of one sound by z, filtered by category (A8)."""
        cal = self.calibrate(np.asarray(a)[None, :] if np.ndim(a) == 1 else a)
        return self.ranked_terms(cal.Z[0], cal.C[0], k, cats, exclude)

    def ranked_terms(self, z: np.ndarray, c: np.ndarray, k: int, cats: Sequence[str] | None = None,
                     exclude: Sequence[str] = ()) -> list[tuple[str, str, float, float]]:
        """The k best terms of one calibrated row."""
        out = []
        for t in np.argsort(-z, kind="stable"):
            cat = self.term_category[t]
            if (cats is None or cat in cats) and cat not in exclude:
                out.append((self.terms[t], cat, round(float(z[t]), 2), round(float(c[t]), 3)))
                if len(out) >= k:
                    break
        return out

    def nearest_category(self, query: str) -> str:
        """The category of the vocabulary term nearest to the query text (A9)."""
        q = np.asarray(self.embed_text([query])[0], np.float64)
        return self.term_category[int(np.argmax(np.asarray(self.E, np.float64) @ q))]

    def validated(self, query: str) -> bool:
        """False when the query reads as an unvalidated category."""
        return self.nearest_category(query) not in UNVALIDATED


def _model_logit_scale() -> float:
    path = cache_dir() / "logit_scale.json"
    cached = load_json(path)
    if isinstance(cached, (int, float)):
        return float(cached)
    scale = float(getattr(clap_model(), "logit_scale", DEFAULT_LOGIT_SCALE))
    save_json(path, scale)
    return scale


_LOCK = threading.Lock()
_VOCAB: Vocab | None = None


def get() -> Vocab:
    """The process-wide vocabulary (built once, thread-safe)."""
    global _VOCAB
    with _LOCK:
        if _VOCAB is None:
            _VOCAB = Vocab()
        return _VOCAB


def reset() -> None:
    """Forget the process vocabulary (tests and model changes)."""
    global _VOCAB
    with _LOCK:
        _VOCAB = None


# ---------------------------------------------------------------- reference statistics


def folder_weights(paths: Sequence[str], normalise: bool = True) -> np.ndarray:
    """1 / (files in the same parent folder) per file; normalised to sum 1 when asked."""
    folders = [os.path.normcase(os.path.dirname(os.path.abspath(p))) for p in paths]
    counts = {f: folders.count(f) for f in set(folders)}
    w = np.array([1.0 / counts[f] for f in folders], dtype=np.float64)
    return w / w.sum() if normalise and len(w) else w


def reference_stats(R: np.ndarray, paths: Sequence[str], E: np.ndarray) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    """Folder-balanced mean embedding and per-term weighted mean and std of the cosine (A10)."""
    w = folder_weights(paths)
    R = np.asarray(R, np.float64)
    m = (w[:, None] * R).sum(axis=0)
    cos = R @ np.asarray(E, np.float64).T
    mu = (w[:, None] * cos).sum(axis=0)
    sd = np.sqrt((w[:, None] * (cos - mu) ** 2).sum(axis=0))
    return m, mu, sd


def stats_text(stats: dict) -> str:
    """The statistics file text: one key per line, one term per line in term_cos."""
    lines = ["{"]
    keys = [k for k in stats if k != "term_cos"] + ["term_cos"]
    for n, k in enumerate(keys):
        end = "," if n < len(keys) - 1 else ""
        if k == "term_cos":
            terms = list(stats[k].items())
            body = [f"  {json.dumps(t)}: {json.dumps(v)}" + ("," if i < len(terms) - 1 else "")
                    for i, (t, v) in enumerate(terms)]
            lines += [' "term_cos": {'] + body + [" }" + end]
        else:
            lines.append(f" {json.dumps(k)}: {json.dumps(stats[k])}{end}")
    return "\n".join(lines + ["}"]) + "\n"


def build_stats(refs: Sequence[str], out: Path = STATS_PATH, max_win: int = REF_MAX_WIN) -> dict:
    """Embed the references, compute the statistics and write the statistics file."""
    paths = audio.list_media(list(refs))
    if not paths:
        raise ValueError("no reference media files given")
    v = get()
    R = embed_files(paths, max_win=max_win)
    m, mu, sd = reference_stats(R, paths, v.E)
    folders = {os.path.normcase(os.path.dirname(os.path.abspath(p))) for p in paths}
    stats = {"model": v.model_id, "precision": str(config.PRECISION), "pad": PAD, "templates": v.templates,
             "logit_scale": round(_model_logit_scale(), 4), "n_ref": len(paths), "n_folders": len(folders),
             "max_win": max_win, "reference_set": f"{len(paths)} files in {len(folders)} folders",
             "mean": [round(float(x), 5) for x in m],
             "term_cos": {t: [round(float(a), 4), round(float(b), 4)] for t, a, b in zip(v.terms, mu, sd)}}
    Path(out).parent.mkdir(parents=True, exist_ok=True)
    Path(out).write_text(stats_text(stats), encoding="utf-8")
    return stats


def info() -> dict:
    """Term count, category count and the number of reference recordings in the statistics."""
    data = read_vocab()
    stats = load_json(STATS_PATH)
    n_ref = stats.get("n_ref") if isinstance(stats, dict) else None
    return {"terms": sum(len(v) for v in data["categories"].values()), "categories": len(data["categories"]),
            "n_ref": n_ref, "problems": check_vocab(data)}


def main(argv: Sequence[str] | None = None) -> int:
    """``python -m hark.listen.vocabulary [--build-stats REF ...] [-o PATH]``."""
    ap = argparse.ArgumentParser(prog="python -m hark.listen.vocabulary")
    ap.add_argument("--build-stats", nargs="+", metavar="REF", help="rebuild the reference statistics")
    ap.add_argument("-o", "--out", default=str(STATS_PATH), help="statistics file to write")
    args = ap.parse_args(argv)
    if args.build_stats:
        stats = build_stats(args.build_stats, Path(args.out))
        print(f"{stats['n_ref']} reference recordings in {stats['n_folders']} folders -> {args.out}")
        return 0
    i = info()
    print(f"{i['terms']} terms in {i['categories']} categories; reference recordings: {i['n_ref'] or 'none'}")
    for p in i["problems"]:
        print(f"problem: {p}")
    return 1 if i["problems"] else 0


if __name__ == "__main__":
    sys.exit(main())
