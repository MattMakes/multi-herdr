"""The find checks (listen-eval E12) and the timings (E15).

``<assets>/find/checks.json`` names the recordings (paths relative to that folder):

    {"K1": {"file": "k1.wav", "slices": [[s, e], ...], "skips": [[9.3, 10.45]]},
     "K2": {"file": "typing.wav", "slices": [...], "unlabelled": [[17.3, 1e9]]},
     "K3": {"file": "k3.wav", "slices": [...]},
     "room_tone": "room.wav",
     "s1_shots": ["ting.wav", "whoosh.wav", "pop.wav", "lego.wav", "plop.wav", "switch.wav", "space.wav", "bell.wav"],
     "s2_shots": {"ting": "ting.wav", "pop": "pop.wav", "whoosh": "whoosh.wav"},
     "timing": {"short_key": "key.wav", "rain_31s": "rain.wav", "foley_dir": "foley20"}}

Queries and top counts come from the check table. A missing recording stops only its own check.
"""

from __future__ import annotations

import contextlib
import json
import time
from collections.abc import Callable, Iterator, Mapping
from pathlib import Path

import numpy as np

from hark.core import audio, config
from hark.listen import vocabulary as voc
from hark.listen.eval import metrics as mt
from hark.listen.eval import scenes

KEY_QUERY = "a single mechanical keyboard key press"
FAST_QUERY = "fast continuous typing on a keyboard"
KEY_CHECKS = {"K1": ("K1", KEY_QUERY, 17), "K2": ("K2", KEY_QUERY, 5), "K2b": ("K2", FAST_QUERY, 5),
              "K3": ("K3", KEY_QUERY, 10)}


@contextlib.contextmanager
def padding(pad: str) -> Iterator[None]:
    """Make every span embedding of find use ``pad`` while the check runs."""
    orig = voc.span_embeddings
    voc.span_embeddings = lambda items, pad_=None: orig(items, pad)
    try:
        yield
    finally:
        voc.span_embeddings = orig


def _find(path: str, query: str, top: int) -> tuple[dict, float]:
    from hark.listen import search

    t0 = time.perf_counter()
    r = search.find(path, query, top=top)
    return r, time.perf_counter() - t0


def attacks_of(path: str, slices) -> list[float]:
    x = np.asarray(audio.load(path, mono=True), np.float64)
    sr = voc.SR
    return [mt.attack_time(x[int(s * sr):int(e * sr)], s, sr) for s, e in slices]


def key_check(root: Path, cfg: Mapping, name: str) -> dict:
    """K1, K2, K2b or K3: hits, leads, unlabelled and skip-range segments, cold and cached time."""
    src, query, top = KEY_CHECKS[name]
    rec = cfg[src]
    path = str(root / rec["file"])
    first, cold = _find(path, query, top)
    _, cached = _find(path, query + " ", top)
    segs = [(s["start"], s["end"]) for s in first["segments"]]
    out = mt.key_report(segs, attacks_of(path, rec["slices"]), rec.get("unlabelled", ()), rec.get("skips", ()))
    out.update({"query": query, "top": top, "cold_s": round(cold, 2), "cached_s": round(cached, 2),
                "duration": first["duration"], "n_candidates": first["n_candidates"],
                "kinds": [s["kind"] for s in first["segments"]]})
    if name == "K2b":
        out["in_fast_part"] = sum(s >= 17.3 for s, _ in segs)
    return out


def scene_check(path: Path, events: Mapping[str, list[float]], queries: Mapping[str, str], top: int = 3) -> dict:
    """Query each planted event with its own text; found, top-1 and tight trims per query."""
    rows = {}
    for key, text in queries.items():
        r, sec = _find(str(path), text, top)
        rows[key] = {"query": text, "seconds": round(sec, 2), **mt.scene_found(r["segments"], events[key])}
    return rows


def _load(root: Path, name: str) -> np.ndarray:
    p = root / name
    if not p.exists():
        raise FileNotFoundError(f"missing recording {p.name}")
    return np.asarray(audio.load(str(p), mono=True), np.float64)


def s1_check(root: Path, cfg: Mapping, out: Path) -> dict:
    shots = [_load(root, f) for f in cfg["s1_shots"]]
    x, starts = scenes.build_s1(_load(root, cfg["room_tone"]), shots)
    path = scenes.write_scene(out / "s1.wav", x)
    rows = scene_check(path, {q: [t] for q, t in zip(scenes.S1_QUERIES, starts)},
                       {q: q for q in scenes.S1_QUERIES})
    return {"top1": sum(r["top1"] for r in rows.values()), "tight": sum(r["tight"] > 0 for r in rows.values()),
            "queries": rows}


def s2_check(root: Path, cfg: Mapping, out: Path) -> dict:
    k1 = cfg["K1"]
    x = _load(root, k1["file"])
    keys = [x[int(s * voc.SR):int(e * voc.SR)] for s, e in k1["slices"]]
    shots = {k: _load(root, f) for k, f in cfg["s2_shots"].items()}
    scene, _, planted = scenes.build_s2(_load(root, cfg["room_tone"]), keys, shots)
    path = scenes.write_scene(out / "s2.wav", scene)
    rows = scene_check(path, planted, scenes.S2_QUERIES)
    return {"found": sum(r["n_found"] for r in rows.values()), "tight": sum(r["tight"] for r in rows.values()),
            "queries": rows}


def run_all(root: Path, out: Path, pad: str = voc.PAD) -> dict:
    """Every find check; a failing check records its error line and the others still run."""
    cfg = json.loads((root / "checks.json").read_text(encoding="utf-8"))
    checks: dict[str, Callable[[], dict]] = {n: (lambda n=n: key_check(root, cfg, n)) for n in KEY_CHECKS}
    checks["S1"] = lambda: s1_check(root, cfg, out)
    checks["S2"] = lambda: s2_check(root, cfg, out)
    result = {}
    with padding(pad):
        for name, run in checks.items():
            try:
                result[name] = run()
            except (FileNotFoundError, KeyError, ValueError, audio.AudioError) as exc:
                result[name] = {"error": f"{type(exc).__name__}: {exc}"}
    return result


def row_text(r: Mapping) -> str:
    if "hits" in r:
        return f"{r['hits']}/{r['segments']} hits on {r['presses_hit']} presses, lead {r['lead_range_ms']} ms"
    if "top1" in r and "queries" in r:
        return f"top-1 {r['top1']}/{len(r['queries'])}, tight {r['tight']}"
    return f"found {r.get('found')}, tight {r.get('tight')}"


def _timed(fn: Callable[[], object]) -> float:
    t0 = time.perf_counter()
    fn()
    return round(time.perf_counter() - t0, 2)


def timings(assets: Path, out: Path) -> dict:
    """E15: wall-clock seconds per call, models warm, every input a fresh copy (no cache hit)."""
    from hark.listen import describe, match, search
    from hark.listen.eval.run_eval import fresh_copy
    from hark.models import ast, clap

    root = assets / "find"
    cfg = json.loads((root / "checks.json").read_text(encoding="utf-8"))
    tm, tmp = cfg["timing"], out / "timing"
    res: dict[str, object] = {"threads": config.threads()}
    res["model_load_s"] = _timed(lambda: (voc.get(), clap.get(), ast.get()))
    key = fresh_copy(str(root / tm["short_key"]), tmp)
    res["describe_short_cold"] = _timed(lambda: describe.describe(key))
    res["match_cached_new_query"] = _timed(lambda: match.match(key, "a soft plastic tap"))
    rain = fresh_copy(str(root / tm["rain_31s"]), tmp)
    res["describe_31s_cold"] = _timed(lambda: describe.describe(rain))
    k1 = fresh_copy(str(root / cfg["K1"]["file"]), tmp)
    res["find_18s_cold"] = _timed(lambda: search.find(k1, KEY_QUERY, top=17))
    res["find_18s_cached_new_query"] = _timed(lambda: search.find(k1, "a spacebar press", top=17))
    s2 = out / "s2.wav"
    if s2.exists():
        res["find_300s_cold"] = _timed(lambda: search.find(fresh_copy(str(s2), tmp), "a glass ting", top=3))
    foley = [fresh_copy(p, tmp / "foley") for p in audio.list_media([str(root / tm["foley_dir"])])[:20]]
    cold = _timed(lambda: search.rank(foley, "a cloth rustle"))
    res["rank_20_foley_files_cold"] = {"seconds": cold, "per_file": round(cold / max(1, len(foley)), 2),
                                       "audio_seconds": round(sum(audio.duration(p) for p in foley), 1)}
    res["rank_20_cached_new_query"] = _timed(lambda: search.rank(foley, "a paper crumple"))
    return res
