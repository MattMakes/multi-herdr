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

import numpy

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


def softmax(sig: Sequence[float], sv: float) -> numpy.ndarray:
    """Forced choice over raw cosines with the CLAP logit scale ``s``."""
    vec = numpy.asarray(sig, dtype=numpy.float64)
    ex = numpy.exp(sv * (vec - vec.max()))
    return ex / ex.sum()


def pct_text(pth: float) -> str:
    """A forced-choice probability as a percentage text."""
    if pth > 0.995:
        return ">99%"
    if pth < 0.005:
        return "<1%"
    return f"{int(round(pth * 100))}%"


def unit(vec: numpy.ndarray) -> numpy.ndarray:
    """v divided by its norm (+1e-12), row-wise for 2-D input."""
    vec = numpy.asarray(vec, dtype=numpy.float64)
    count = numpy.linalg.norm(vec, axis=-1, keepdims=True)
    return vec / (count + NORM_FLOOR)


# ---------------------------------------------------------------- ranges and batches


def raise_first(checks: Sequence[tuple[bool, str | None]]) -> None:
    """Raise ValueError with the message of the first (failed, message) pair whose message is set."""
    message = next((m for failed, m in checks if failed and m), None)
    if message:
        raise ValueError(message)


def check_range(fpath: str, t_from: float | None = None, t_to: float | None = None) -> float:
    """Validate a range against the file; return the file duration in seconds."""
    duration = audio.duration(fpath)
    known, st = t_from is not None, "" if t_from is None else format(t_from, "g")
    raise_first([(known and t_from < 0, f"start {st} s is negative"),
                 (known and t_from >= duration, f"start {st} s is at or past the end of {fpath} ({duration:.3f} s long)"),
                 (t_to is not None and t_to <= (t_from or 0.0), "end must be after start")])
    return duration


def clamp_range(duration: float, t_from: float | None, t_to: float | None) -> Span:
    """The judged span [s, e) of a checked range (end clamped to the duration)."""
    sv = max(0.0, t_from or 0.0)
    ex = duration if t_to is None else min(duration, t_to)
    return sv, ex


def first_line(exc: BaseException, limit: int = 200) -> str:
    """The first line of an exception message, at most ``limit`` characters."""
    joined = str(exc).strip().splitlines()
    return (joined[0] if joined else type(exc).__name__)[:limit]


def split_readable(files: Sequence[str]) -> tuple[list[str], list[dict]]:
    """Split paths into decodable ones and ``{path, error}`` rows."""
    good: list[str] = []
    bad: list[dict] = []
    for pth in files:
        try:
            audio.duration(pth)
            good.append(pth)
        except Exception as exc:  # one bad file never fails a batch
            bad.append({"path": pth, "error": first_line(exc)})
    return good, bad


def peak_db(fpath: str, t_from: float | None = None, t_to: float | None = None, mono: bool = False) -> float:
    """Sample peak of a decoded range in dB (an empty decode reads as silence)."""
    sig = audio.load(fpath, mono=mono, start=t_from, end=t_to)
    return array_peak_db(sig)


def array_peak_db(sig: numpy.ndarray) -> float:
    """Sample peak of an array in dB."""
    return float(audio.db(float(numpy.max(numpy.abs(sig))) if sig.size else 0.0))


def same_path(lhs: str, b: str) -> bool:
    """True when 2 paths name one file (absolute, case-normalised)."""
    return os.path.normcase(os.path.abspath(lhs)) == os.path.normcase(os.path.abspath(b))


# ---------------------------------------------------------------- windows


def spans(t_from: float, t_to: float, win_cap: int = MAX_WIN) -> list[Span]:
    """CLAP windows of [start, end): 10 s windows, 5 s hop, at most ``max_win`` evenly spaced."""
    win_n, hop_n = int(WIN_S * SR), int(HOP_S * SR)
    count = int(round((t_to - t_from) * SR))
    if count <= win_n:
        return [(t_from, t_to)]
    starts = list(range(0, count - win_n + 1, hop_n))
    if starts[-1] + win_n != count:
        starts.append(count - win_n)
    if len(starts) > win_cap:
        picks = numpy.linspace(0, len(starts) - 1, win_cap)
        starts = [starts[int(round(ix))] for ix in picks]
    return [(t_from + sv / SR, t_from + (sv + win_n) / SR) for sv in starts]


def applied_pad(duration: float, padding: str) -> str:
    """The padding that a span of ``duration`` seconds really gets (part of its cache key)."""
    if duration >= WIN_S - 0.001:
        return "-"
    if padding == "spaced" and duration >= SPACED_UNIT_S:
        return "repeatpad"
    return padding


def prepare(seg: numpy.ndarray, padding: str) -> numpy.ndarray:
    """One mono span made ready for a CLAP window under padding mode ``pad``."""
    win_n = int(WIN_S * SR)
    sig = numpy.asarray(seg, dtype=numpy.float32)[:win_n]
    if len(sig) < MIN_SAMPLES:
        sig = numpy.pad(sig, (0, MIN_SAMPLES - len(sig)))
    if padding == "zero" and len(sig) < win_n:
        return numpy.pad(sig, (0, win_n - len(sig)))
    if padding == "spaced" and len(sig) < SPACED_UNIT_S * SR:
        cell = numpy.pad(sig, (0, int(SPACED_UNIT_S * SR) - len(sig)))
        return numpy.tile(cell, int(math.ceil(win_n / len(cell))))[:win_n]
    return sig


# ---------------------------------------------------------------- cache


def _slug(joined: str) -> str:
    return re.sub(r"[^0-9A-Za-z._-]", "_", str(joined))


def cache_dir(kind: str = "clap") -> Path:
    """The listen cache folder of the configured model (no model load needed)."""
    model = config.CLAP_MODEL if kind == "clap" else getattr(config, "AST_MODEL", "ast")
    rev = getattr(config, "CLAP_REVISION" if kind == "clap" else "AST_REVISION", None)
    name = _slug(model) + (f"--{_slug(rev)}" if rev else "") + f"--{_slug(config.PRECISION)}"
    return Path(config.EMB_CACHE) / "listen" / kind / name


def key_of(*parts: Any) -> str:
    """A 24-hex cache key of the given parts."""
    joined = "|".join(str(pth) for pth in (CACHE_VERSION, *parts))
    return hashlib.sha1(joined.encode("utf-8")).hexdigest()[:24]


def load_array(fpath: Path, shape: tuple[int, ...] | None = None) -> numpy.ndarray | None:
    """A cached array, or None when it is absent, unreadable or of the wrong shape."""
    try:
        stored = numpy.load(fpath, allow_pickle=False)
    except Exception:
        return None
    if shape is not None and stored.shape != shape:
        return None
    return stored


def save_array(fpath: Path, stored: numpy.ndarray) -> None:
    """Write an array atomically. A failed write never fails the call."""
    try:
        fpath.parent.mkdir(parents=True, exist_ok=True)
        tmp = fpath.with_name(f"{fpath.stem}.{os.getpid()}.{threading.get_ident()}.tmp.npy")
        numpy.save(tmp, stored)
        audio.replace_quietly(str(tmp), str(fpath))
    except Exception:
        pass


def load_json(fpath: Path) -> Any:
    """A cached JSON value, or None."""
    try:
        return json.loads(fpath.read_text(encoding="utf-8"))
    except Exception:
        return None


def save_json(fpath: Path, value: Any) -> None:
    """Write JSON atomically. A failed write never fails the call."""
    try:
        fpath.parent.mkdir(parents=True, exist_ok=True)
        tmp = fpath.with_name(f"{fpath.name}.{os.getpid()}.{threading.get_ident()}.tmp")
        tmp.write_text(json.dumps(value), encoding="utf-8")
        audio.replace_quietly(str(tmp), str(fpath))
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


def _embed_with_templates(phrases: Sequence[str], phrasings: Sequence[str]) -> numpy.ndarray:
    texts = [fill(tx, pth) for pth in phrases for tx in phrasings]
    raw = numpy.asarray(clap_model().embed_text(texts), dtype=numpy.float64)
    mean = raw.reshape(len(phrases), len(phrasings), -1).mean(axis=1)
    return unit(mean).astype(numpy.float32)


def _text_key(phrasings: Sequence[str], phrases: Sequence[str]) -> str:
    return key_of("text", "\x1f".join(phrasings), "\x1e".join(phrases))


def embed_phrases(phrases: Sequence[str], phrasings: Sequence[str]) -> numpy.ndarray:
    """Template-averaged unit text embeddings, (k, 512) float32, cached on disk."""
    phrases = list(phrases)
    if not phrases:
        return numpy.zeros((0, DIM), numpy.float32)
    if len(phrases) > TEXT_SPLIT:
        return _embed_list_cached(phrases, phrasings)
    return numpy.stack([_embed_one_cached(pth, phrasings) for pth in phrases])


def _embed_list_cached(phrases: list[str], phrasings: Sequence[str]) -> numpy.ndarray:
    fpath = cache_dir() / f"tl_{_text_key(phrasings, phrases)}.npy"
    stored = load_array(fpath, (len(phrases), DIM))
    if stored is None:
        stored = _embed_with_templates(phrases, phrasings)
        save_array(fpath, stored)
    return stored.astype(numpy.float32)


def _embed_one_cached(phrase: str, phrasings: Sequence[str]) -> numpy.ndarray:
    fpath = cache_dir() / f"t_{_text_key(phrasings, [phrase])}.npy"
    stored = load_array(fpath, (DIM,))
    if stored is None:
        stored = _embed_with_templates([phrase], phrasings)[0]
        save_array(fpath, stored)
    return stored.astype(numpy.float32)


def embed_plain_text(texts: Sequence[str]) -> numpy.ndarray:
    """Plain CLAP text embeddings (no templates), cached per text."""
    result = []
    for tx in texts:
        fpath = cache_dir() / f"p_{key_of('plain', tx)}.npy"
        stored = load_array(fpath, (DIM,))
        if stored is None:
            stored = unit(numpy.asarray(clap_model().embed_text([tx]), dtype=numpy.float64)[0]).astype(numpy.float32)
            save_array(fpath, stored)
        result.append(stored)
    return numpy.stack(result) if result else numpy.zeros((0, DIM), numpy.float32)


# ---------------------------------------------------------------- audio embedding


def _span_key(fid: str, span: Span, padding: str) -> str:
    sv, ex = span
    return key_of("span", fid, round(sv, 4), round(ex, 4), applied_pad(ex - sv, padding))


def _slice(sig: numpy.ndarray, span: Span) -> numpy.ndarray:
    lhs = max(0, int(round(span[0] * SR)))
    b = max(lhs, int(round(span[1] * SR)))
    return sig[lhs:b]


def _embed_arrays(segs: list[numpy.ndarray]) -> numpy.ndarray:
    result = []
    model = clap_model()
    for ix in range(0, len(segs), SPAN_BATCH):
        emb = numpy.asarray(model.embed_audio(segs[ix:ix + SPAN_BATCH], pad=PAD), dtype=numpy.float64)
        result.append(unit(numpy.atleast_2d(emb)))
    return numpy.concatenate(result).astype(numpy.float32)


def _missing_spans(entries: Sequence[tuple[str, Sequence[Span]]], padding: str, result: list[list]) -> list:
    todo = []
    for ix, (fpath, sp) in enumerate(entries):
        fid = audio.file_id(fpath)
        for jj, span in enumerate(sp):
            pth = cache_dir() / f"s_{_span_key(fid, span, padding)}.npy"
            stored = load_array(pth, (DIM,))
            if stored is None:
                todo.append((ix, jj, pth))
            else:
                result[ix][jj] = stored
    return todo


def _fill_missing(entries: Sequence[tuple[str, Sequence[Span]]], todo: list, padding: str, result: list[list]) -> None:
    decoded: dict[int, numpy.ndarray] = {}
    for b in range(0, len(todo), SPAN_BATCH):
        chunk = todo[b:b + SPAN_BATCH]
        segs = []
        for ix, jj, _ in chunk:
            if ix not in decoded:
                decoded.clear()
                decoded[ix] = audio.load(entries[ix][0], mono=True)
            segs.append(prepare(_slice(decoded[ix], entries[ix][1][jj]), padding))
        for (ix, jj, pth), emb in zip(chunk, _embed_arrays(segs)):
            save_array(pth, emb)
            result[ix][jj] = emb


def span_embeddings(entries: Sequence[tuple[str, Sequence[Span]]], padding: str = PAD) -> list[numpy.ndarray]:
    """Unit embeddings of every span of every (path, spans) item, read from the cache when present."""
    result: list[list] = [[None] * len(sp) for _, sp in entries]
    todo = _missing_spans(entries, padding, result)
    if todo:
        _fill_missing(entries, todo, padding, result)
    return [numpy.stack(row).astype(numpy.float32) if row else numpy.zeros((0, DIM), numpy.float32) for row in result]


def file_spans(fpath: str, t_from: float | None = None, t_to: float | None = None,
               win_cap: int | None = None) -> list[Span]:
    """The CLAP spans of a file range (A4 steps 1 and 2)."""
    d = audio.duration(fpath)
    sv, ex = clamp_range(d, t_from, t_to)
    return spans(sv, max(ex, sv + 0.01), win_cap or MAX_WIN)


def embed_files(files: Sequence[str], pad: str = PAD, max_win: int | None = None,
                ranges: Sequence[tuple[float | None, float | None]] | None = None) -> numpy.ndarray:
    """One unit embedding per file (mean of its span embeddings), (n, 512) float32."""
    if not files:
        return numpy.zeros((0, DIM), numpy.float32)
    rng = list(ranges) if ranges is not None else [(None, None)] * len(files)
    entries = [(pth, file_spans(pth, sv, ex, max_win)) for pth, (sv, ex) in zip(files, rng)]
    embs = span_embeddings(entries, pad)
    return numpy.stack([unit(E.mean(axis=0)) for E in embs]).astype(numpy.float32)


def embed_file(fpath: str, t_from: float | None = None, t_to: float | None = None, padding: str = PAD) -> numpy.ndarray:
    """The unit embedding of one file range, (512,)."""
    return embed_files([fpath], padding, ranges=[(t_from, t_to)])[0]


def embed_audio(arrays: Sequence[numpy.ndarray], pad: str = PAD) -> numpy.ndarray:
    """One unit embedding per in-memory 48 kHz array (same windows and padding as files)."""
    result = []
    for sig in arrays:
        m = audio.to_mono(numpy.asarray(sig, dtype=numpy.float32))
        sp = spans(0.0, max(len(m) / SR, 0.01))
        segs = [prepare(_slice(m, sv), pad) for sv in sp]
        result.append(unit(_embed_arrays(segs).mean(axis=0)))
    return numpy.stack(result).astype(numpy.float32) if result else numpy.zeros((0, DIM), numpy.float32)


# ---------------------------------------------------------------- calibration


class Calibration(NamedTuple):
    """The calibration of n sounds against T vocabulary terms and k queries."""

    C: numpy.ndarray
    U: numpy.ndarray
    Z: numpy.ndarray
    mu: numpy.ndarray
    sigma: numpy.ndarray
    Cq: numpy.ndarray | None = None
    z_q: numpy.ndarray | None = None
    percentile: numpy.ndarray | None = None
    rank: numpy.ndarray | None = None


def read_vocab(fpath: Path = VOCAB_PATH) -> dict:
    """The vocabulary file as a dict."""
    return json.loads(Path(fpath).read_text(encoding="utf-8"))


def check_vocab(data: dict) -> list[str]:
    """Problems of a vocabulary file (empty when it is valid)."""
    problems = [f"template {tx!r} must hold exactly one {{}}" for tx in data.get("templates", [])
                if tx.count("{}") != 1]
    seen: set[str] = set()
    for cat, phrases in data.get("categories", {}).items():
        for pth in phrases:
            if pth in seen:
                problems.append(f"phrase {pth!r} appears twice")
            seen.add(pth)
    problems += [f"category {c!r} is missing" for c in CATEGORY_NAMES if c not in data.get("categories", {})]
    return problems


def read_stats(fpath: Path, model_id: str) -> dict | None:
    """The reference statistics of ``model_id``, or None (missing file or another model)."""
    ref_stats = load_json(Path(fpath))
    if not isinstance(ref_stats, dict) or ref_stats.get("model") != model_id or len(ref_stats.get("mean", [])) != DIM:
        return None
    return ref_stats


class Vocab:
    """The vocabulary terms, their embeddings, and the calibration against them."""

    def __init__(self, path: Path = VOCAB_PATH, stats_path: Path = STATS_PATH):
        data = read_vocab(path)
        self.templates: list[str] = list(data["templates"])
        self.categories: list[str] = list(data["categories"])
        self.terms: list[str] = [pth for c in self.categories for pth in data["categories"][c]]
        self.term_category: list[str] = [c for c in self.categories for _ in data["categories"][c]]
        self.model_id: str = config.CLAP_MODEL
        self.E: numpy.ndarray = embed_phrases(self.terms, self.templates)
        self.stats = read_stats(stats_path, self.model_id)
        self.mean = numpy.asarray(self.stats["mean"], numpy.float64) if self.stats else None
        self.bE = self.bias(self.E)
        self.index = {tx: ix for ix, tx in enumerate(self.terms)}
        self._scale: float | None = float(self.stats["logit_scale"]) if self.stats else None

    @property
    def logit_scale(self) -> float:
        """The CLAP softmax temperature: from the statistics, else from the model (cached)."""
        if self._scale is None:
            self._scale = _model_logit_scale()
        return self._scale

    def embed_text(self, texts: Sequence[str]) -> numpy.ndarray:
        """Template-averaged unit embeddings; a vocabulary term reuses its own row (exact ties)."""
        texts = list(texts)
        if not texts:
            return numpy.zeros((0, DIM), numpy.float32)
        new = [tx for tx in texts if tx not in self.index]
        emb = dict(zip(new, embed_phrases(new, self.templates))) if new else {}
        return numpy.stack([self.E[self.index[tx]] if tx in self.index else emb[tx] for tx in texts])

    def bias(self, qmat: numpy.ndarray) -> numpy.ndarray:
        """The term bias b(q) = q . m of every row (0 without statistics)."""
        qmat = numpy.atleast_2d(numpy.asarray(qmat, numpy.float64))
        return numpy.zeros(len(qmat)) if self.mean is None else qmat @ self.mean

    def calibrate(self, A: numpy.ndarray, Q: numpy.ndarray | None = None, method: str = "both") -> Calibration:
        """Cosines, bias-free scores, per-sound z, percentile and rank (A6)."""
        A = numpy.atleast_2d(numpy.asarray(A, numpy.float64))
        center, per_sound = method in ("center", "both"), method in ("sound", "both")
        C = A @ numpy.asarray(self.E, numpy.float64).T
        U = C - self.bE[None, :] if center else C
        centre = U.mean(axis=1) if per_sound else numpy.zeros(len(A))
        sigma = U.std(axis=1) + SIGMA_FLOOR if per_sound else numpy.ones(len(A))
        Z = (U - centre[:, None]) / sigma[:, None]
        if Q is None:
            return Calibration(C, U, Z, centre, sigma)
        return self._with_queries(A, numpy.atleast_2d(numpy.asarray(Q, numpy.float64)), Calibration(C, U, Z, centre, sigma),
                                  center)

    def _with_queries(self, A: numpy.ndarray, qmat: numpy.ndarray, cal: Calibration, center: bool) -> Calibration:
        Cq = A @ qmat.T
        Uq = Cq - self.bias(qmat)[None, :] if center else Cq
        zq = (Uq - cal.mu[:, None]) / cal.sigma[:, None]
        diff = cal.U[:, :, None] - Uq[:, None, :]
        pct = (diff < -TIE_EPS).mean(axis=1)
        rank = 1 + (diff > TIE_EPS).sum(axis=1)
        return cal._replace(Cq=Cq, z_q=zq, percentile=pct, rank=rank)

    def scores(self, A: numpy.ndarray, queries: Sequence[str]) -> list[list[dict]]:
        """Per sound and per query: {query, cosine, z, percentile, rank_in_vocab, vocab_size} (A7)."""
        cal = self.calibrate(A, self.embed_text(queries))
        return [[self._record(cal, ix, jj, q) for jj, q in enumerate(queries)] for ix in range(len(cal.C))]

    def _record(self, cal: Calibration, ix: int, jj: int, query: str) -> dict:
        return {"query": query, "cosine": round(float(cal.Cq[ix, jj]), 3), "z": round(float(cal.z_q[ix, jj]), 2),
                "percentile": round(100 * float(cal.percentile[ix, jj]), 1),
                "rank_in_vocab": int(cal.rank[ix, jj]), "vocab_size": len(self.terms)}

    def top_terms(self, lhs: numpy.ndarray, k: int, cats: Sequence[str] | None = None,
                  exclude: Sequence[str] = ()) -> list[tuple[str, str, float, float]]:
        """The k best terms of one sound by z, filtered by category (A8)."""
        cal = self.calibrate(numpy.asarray(lhs)[None, :] if numpy.ndim(lhs) == 1 else lhs)
        return self.ranked_terms(cal.Z[0], cal.C[0], k, cats, exclude)

    def ranked_terms(self, z: numpy.ndarray, c: numpy.ndarray, kk: int, cats: Sequence[str] | None = None,
                     exclude: Sequence[str] = ()) -> list[tuple[str, str, float, float]]:
        """The k best terms of one calibrated row."""
        result = []
        for tx in numpy.argsort(-z, kind="stable"):
            cat = self.term_category[tx]
            if (cats is None or cat in cats) and cat not in exclude:
                result.append((self.terms[tx], cat, round(float(z[tx]), 2), round(float(c[tx]), 3)))
                if len(result) >= kk:
                    break
        return result

    def nearest_category(self, query: str) -> str:
        """The category of the vocabulary term nearest to the query text (A9)."""
        q = numpy.asarray(self.embed_text([query])[0], numpy.float64)
        return self.term_category[int(numpy.argmax(numpy.asarray(self.E, numpy.float64) @ q))]

    def validated(self, query: str) -> bool:
        """False when the query reads as an unvalidated category."""
        return self.nearest_category(query) not in UNVALIDATED


def _model_logit_scale() -> float:
    fpath = cache_dir() / "logit_scale.json"
    cached = load_json(fpath)
    if isinstance(cached, (int, float)):
        return float(cached)
    scale = float(getattr(clap_model(), "logit_scale", DEFAULT_LOGIT_SCALE))
    save_json(fpath, scale)
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


def folder_weights(files: Sequence[str], normalise: bool = True) -> numpy.ndarray:
    """1 / (files in the same parent folder) per file; normalised to sum 1 when asked."""
    folders = [os.path.normcase(os.path.dirname(os.path.abspath(pth))) for pth in files]
    counts = {f: folders.count(f) for f in set(folders)}
    win_n = numpy.array([1.0 / counts[f] for f in folders], dtype=numpy.float64)
    return win_n / win_n.sum() if normalise and len(win_n) else win_n


def reference_stats(R: numpy.ndarray, files: Sequence[str], E: numpy.ndarray) -> tuple[numpy.ndarray, numpy.ndarray, numpy.ndarray]:
    """Folder-balanced mean embedding and per-term weighted mean and std of the cosine (A10)."""
    win_n = folder_weights(files)
    R = numpy.asarray(R, numpy.float64)
    m = (win_n[:, None] * R).sum(axis=0)
    cos = R @ numpy.asarray(E, numpy.float64).T
    centre = (win_n[:, None] * cos).sum(axis=0)
    sd = numpy.sqrt((win_n[:, None] * (cos - centre) ** 2).sum(axis=0))
    return m, centre, sd


def stats_text(ref_stats: dict) -> str:
    """The statistics file text: one key per line, one term per line in term_cos."""
    lines = ["{"]
    keys = [kk for kk in ref_stats if kk != "term_cos"] + ["term_cos"]
    for count, kk in enumerate(keys):
        t_to = "," if count < len(keys) - 1 else ""
        if kk == "term_cos":
            terms = list(ref_stats[kk].items())
            body = [f"  {json.dumps(tx)}: {json.dumps(vec)}" + ("," if ix < len(terms) - 1 else "")
                    for ix, (tx, vec) in enumerate(terms)]
            lines += [' "term_cos": {'] + body + [" }" + t_to]
        else:
            lines.append(f" {json.dumps(kk)}: {json.dumps(ref_stats[kk])}{t_to}")
    return "\n".join(lines + ["}"]) + "\n"


def _stats_dict(vec: Vocab, files: Sequence[str], R: numpy.ndarray, win_cap: int) -> dict:
    m, centre, sd = reference_stats(R, files, vec.E)
    n_folders = len({os.path.normcase(os.path.dirname(os.path.abspath(pth))) for pth in files})
    return {"model": vec.model_id, "precision": str(config.PRECISION), "pad": PAD, "templates": vec.templates,
            "logit_scale": round(_model_logit_scale(), 4), "n_ref": len(files), "n_folders": n_folders,
            "max_win": win_cap, "reference_set": f"{len(files)} files in {n_folders} folders",
            "mean": [round(float(sig), 5) for sig in m],
            "term_cos": {tx: [round(float(lhs), 4), round(float(b), 4)] for tx, lhs, b in zip(vec.terms, centre, sd)}}


def build_stats(refs: Sequence[str], result: Path = STATS_PATH, win_cap: int = REF_MAX_WIN) -> dict:
    """Embed the references, compute the statistics and write the statistics file."""
    files = audio.list_media(list(refs))
    if not files:
        raise ValueError("no reference media files given")
    ref_stats = _stats_dict(get(), files, embed_files(files, max_win=win_cap), win_cap)
    Path(result).parent.mkdir(parents=True, exist_ok=True)
    Path(result).write_text(stats_text(ref_stats), encoding="utf-8")
    return ref_stats


def info() -> dict:
    """Term count, category count and the number of reference recordings in the statistics."""
    data = read_vocab()
    ref_stats = load_json(STATS_PATH)
    n_ref = ref_stats.get("n_ref") if isinstance(ref_stats, dict) else None
    return {"terms": sum(len(vec) for vec in data["categories"].values()), "categories": len(data["categories"]),
            "n_ref": n_ref, "problems": check_vocab(data)}


def main(argv: Sequence[str] | None = None) -> int:
    """``python -m hark.listen.vocabulary [--build-stats REF ...] [-o PATH]``."""
    ap = argparse.ArgumentParser(prog="python -m hark.listen.vocabulary")
    ap.add_argument("--build-stats", nargs="+", metavar="REF", help="rebuild the reference statistics")
    ap.add_argument("-o", "--out", default=str(STATS_PATH), help="statistics file to write")
    args = ap.parse_args(argv)
    if args.build_stats:
        ref_stats = build_stats(args.build_stats, Path(args.out))
        print(f"{ref_stats['n_ref']} reference recordings in {ref_stats['n_folders']} folders -> {args.out}")
        return 0
    ix = info()
    print(f"{ix['terms']} terms in {ix['categories']} categories; reference recordings: {ix['n_ref'] or 'none'}")
    for pth in ix["problems"]:
        print(f"problem: {pth}")
    return 1 if ix["problems"] else 0


if __name__ == "__main__":
    sys.exit(main())
