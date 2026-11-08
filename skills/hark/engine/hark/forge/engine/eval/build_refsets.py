"""Cut the reference recordings into <root>/<category>/{forge,bench}/ and write the refsets registry.

Dev machine only. The cut WAVs, the manifest and the registry stay local; nothing here is committed.
Run: `python -m hark.forge.engine.eval.build_refsets [--cats CAT ...] [--plan] [--registry PATH]`.
"""

from __future__ import annotations

import argparse
import json
import math
import os
from collections.abc import Callable, Sequence
from pathlib import Path
from typing import Any

import numpy as np

from hark.forge.engine.eval import frozen_tone as ft
from hark.forge.engine.eval.scrub_paths import REPO

SR = 48000
OUT = REPO / "out" / "refs"
SOURCES_DEFAULT = OUT / "sources.json"
KIT_DEFAULT = OUT / "kit_manifest.json"
ZMIN, ZMIN_TOGGLE = 2.0, 1.5
EDGE_S, PEAK_DROP_DB = 0.03, 18.0


def _kits(stem: str, nums: Sequence[int]) -> list[str]:
    return [f"kit:{stem}_{n:02d}" for n in nums]


def _finds(ids: Sequence[str], k: int) -> list[tuple]:
    return [(i, k, "find") for i in ids]


def _wholes(ids: Sequence[str]) -> list[tuple]:
    return [(i, 1, "whole") for i in ids]


# category -> (query, (min s, max s), forge entries, bench entries)
CATS: dict[str, tuple[str, tuple[float, float], list, list]] = {
    "whoosh_fast": ("a fast crisp whoosh of a stick swung through the air", (0.2, 0.9),
                    _kits("whoosh_fast", range(1, 5)) + _wholes(["fs-719636"]),
                    _wholes(["fs-352719", "fs-216675"]) + _finds(["fs-463763"], 3)),
    "whoosh_big": ("a big deep heavy whoosh passing by", (0.7, 3.2),
                   ["kit:whoosh_big_01", ("mk-1167", 1, "whole"), ("mk-1168", 1, "whole", {"start": 0.9})],
                   ["kit:whoosh_big_02", ("fs-711122", 3, "find", {"start": 13.0, "end": 19.5})]),
    "swish": ("a quick light swish of something thrown", (0.12, 0.55),
              _finds(["fs-427979"], 4) + _wholes(["fs-719637"]) + _finds(["fs-425856"], 2),
              _finds(["fs-463763"], 4) + _wholes(["fs-216675", "fs-352719"])),
    "swipe": ("a soft cloth swipe, fabric swished quickly", (0.25, 1.0),
              ["kit:swipe_02", "kit:whoosh_med_03"] + _finds(["fs-425856"], 2),
              ["kit:swipe_01"] + _finds(["fs-495390"], 3)),
    "riser": ("a rising swell building up to a loud peak", (1.5, 6.0),
              ["kit:riser_01"], ["kit:riser_02", ("fs-425434", 1, "rise")]),
    "downlifter": ("a falling downlifter sweep sinking away", (1.0, 4.0), [], []),
    "impact": ("a deep cinematic boom impact hit", (1.0, 8.0), ["kit:impact_01", "kit:impact_02"], ["kit:impact_03"]),
    "sub_drop": ("a deep sub bass drop", (1.0, 4.0), [], _wholes(["fs-428073"])),
    "click": ("a small plastic button click", (0.03, 0.35),
              _kits("ui_click", range(1, 5)) + _finds(["fs-167829", "fs-383863"], 2),
              _wholes(["mk-1124", "mk-2364"]) + _finds(["fs-610524", "fs-461799", "fs-431419"], 2)),
    "tap": ("a soft finger tap on a hard surface", (0.03, 0.3),
            ["kit:ui_click_01"] + _finds(["fs-704862", "fs-452621"], 3), _finds(["fs-631822", "fs-707827"], 3)),
    "toggle": ("a toggle switch flipped with a crisp click", (0.05, 0.45),
               _kits("ui_toggle", (1, 2)) + _wholes(["fs-364200", "fs-448393"]),
               _wholes(["fs-278205", "mk-2585"]) + [("fs-401803", 2, "find", {"end": 5.0})]),
    "hover_tick": ("a tiny soft tick", (0.012, 0.15),
                   _kits("ui_tick", (1, 2)) + _finds(["fs-376195", "fs-329098"], 2),
                   [("fs-401803", 3, "find", {"start": 72.0}), ("fs-873306", 2, "find", {"end": 5.2})]
                   + _finds(["fs-326470"], 2)),
    "pop": ("a short round pop", (0.04, 0.5),
            _kits("ui_pop", range(1, 5)) + _wholes(["mk-3000"]) + _finds(["fs-543200"], 2),
            _finds(["fs-202230", "fs-33369", "fs-545201"], 1) + _finds(["fs-573152"], 2)
            + _wholes(["mk-3192", "mk-3070"])),
    "notify": ("a short bright notification ding", (0.4, 2.0),
               ["kit:ui_notify_01"] + _wholes(["fs-681787", "fs-681789", "fs-681792", "fs-549899"]),
               _wholes(["fs-410636", "fs-410649", "fs-410664"]) + [("fs-170237", 1, "whole", {"end": 1.3})]
               + _wholes(["vsco-Triangle6-Hit_v1_rr1"])),
    "shimmer": ("a magical shimmering chime sweep", (1.0, 6.0),
                ["kit:shimmer_02"] + _finds(["fs-271355"], 1) + _wholes(["mk-2014"]) + _finds(["fs-495327"], 2),
                ["kit:shimmer_01"] + _finds(["fs-257558"], 1)
                + _wholes(["vsco-BellTree_Stroke1", "vsco-glock_fx_up_chromatic_slow_01"])),
    "typing_key": ("a single mechanical keyboard key press", (0.06, 0.45),
                   _kits("kb_thock", range(1, 5)) + ["kit:kb_enter_01", "kit:kb_space_01"]
                   + _kits("kb_click", (1, 2)) + ["kit:kb_enter_02"] + _finds(["fs-561697", "fs-561699"], 1)
                   + _finds(["fs-709460", "fs-665075"], 2),
                   _finds(["fs-378083", "fs-378084", "fs-378085", "fs-570754", "fs-766639", "fs-766640",
                           "fs-515810"], 1) + _finds(["fs-609534"], 3) + _finds(["fs-815614"], 2)),
    "glitch": ("a digital glitch stutter", (0.1, 1.0), [], []),
    "logo_sting": ("a struck chime ringing out, a logo sting", (1.0, 6.0),
                   ["kit:sting_tail_01"] + _wholes(["mk-3108"]),
                   _wholes(["vsco-Triangle6-Hit_v1_rr1"]) + [("fs-170237", 1, "whole", {"end": 1.3})]),
}

NOTES = {
    "whoosh_big": "forge: designed library whooshes; bench: real air and cape swoops",
    "swipe": "cloth only",
    "riser": "thin: one designed riser in forge; bench: two real cymbal swells",
    "downlifter": "empty: trims and fades only, so no reversed or pitched stand-ins",
    "glitch": "empty: trims and fades only, so no reversed or pitched stand-ins",
    "impact": "thin: 3 hits, all first-version refs, one held out",
    "sub_drop": "one designed source, held out as bench",
    "notify": "struck bells, glass, kalimba, celesta and triangle, capped at 2 s",
    "shimmer": "shimmer_01 is bench because its author also recorded a notify bench ref",
    "logo_sting": "struck rings: tail shape and tone only, since real stings are composed",
}


# ---------------------------------------------------------------- families and the plan

def family(src: dict) -> str:
    """The split unit of a source: one library, one Freesound author, or the file itself."""
    sid = str(src.get("id", ""))
    if sid.startswith("vsco-"):
        return "vsco"
    if sid.startswith("fs-") and src.get("author"):
        return f"fs-author:{src['author']}"
    return sid


def _norm(p: Any) -> str:
    return os.path.normcase(os.path.normpath(str(p).replace("\\", "/")))


def entry_source(e: Any, sources: dict, kit: dict) -> dict:
    """The source record of a plan entry (a kit sound resolves through its source path)."""
    if isinstance(e, str) and e.startswith("kit:"):
        snd = kit.get(e[4:]) or {}
        path = _norm((snd.get("source") or {}).get("path", ""))
        return next((s for s in sources.values() if _norm(s.get("path", "")) == path), {"id": e})
    return sources.get(e[0], {"id": e[0]})


def plan(cats: dict, sources: dict, kit: dict, log: Callable = print) -> list[str]:
    """Print the source ids per category and split; return the family leaks."""
    seen: dict[str, set] = {"forge": set(), "bench": set()}
    for cat, (_, _, forge, bench) in cats.items():
        for split, entries in (("forge", forge), ("bench", bench)):
            srcs = [entry_source(e, sources, kit) for e in entries]
            log(f"{cat} {split}: {', '.join(dict.fromkeys(s['id'] for s in srcs)) or '-'}")
            seen[split] |= {family(s) for s in srcs}
    leaks = sorted(seen["forge"] & seen["bench"])
    log(f"family leaks: {', '.join(leaks) if leaks else 'none'}")
    return leaks


# ---------------------------------------------------------------- cuts

def _frames5(m: np.ndarray) -> np.ndarray:
    n = int(0.005 * SR)
    k = max(1, len(m) // n)
    seg = m[: k * n] if len(m) >= n else np.pad(m, (0, n - len(m)))
    return 10 * np.log10(np.mean(seg.reshape(k, -1) ** 2, axis=1) + 1e-20)


def _mono(x: np.ndarray) -> np.ndarray:
    return x.mean(axis=1) if x.ndim == 2 else x


def _range(x: np.ndarray, start: float | None, end: float | None) -> tuple[int, int]:
    s0 = int(round((start or 0.0) * SR))
    s1 = len(x) if end is None else min(len(x), int(round(end * SR)))
    return max(0, s0), max(s0, s1)


def cut_whole(x: np.ndarray, dur: tuple[float, float], start: float | None = None,
              end: float | None = None) -> dict:
    """The one sound in the range: {'a', 'b', 'fade_out', 'capped'} in samples."""
    s0, s1 = _range(x, start, end)
    e = _frames5(_mono(x[s0:s1]))
    n = int(0.005 * SR)
    p = int(np.argmax(e))
    onset = int(np.argmax(e >= e[p] - 30))
    last = int(np.flatnonzero(e >= e[p] - 45)[-1])
    a = s0 + max(0, onset * n - n)
    b = min(s1, s0 + (last + 1) * n + int(0.02 * SR))
    mx = int(round(dur[1] * SR))
    if b - a > mx:
        return {"a": a, "b": min(s1, a + mx), "fade_out": 0.25, "capped": True}
    return {"a": a, "b": b, "fade_out": 0.03, "capped": False}


def cut_rise(x: np.ndarray, dur: tuple[float, float], start: float | None = None,
             end: float | None = None) -> dict:
    """A crescendo up to 0.3 s past its loudest frame."""
    s0, s1 = _range(x, start, end)
    e = _frames5(_mono(x[s0:s1]))
    n = int(0.005 * SR)
    p = int(np.argmax(e))
    i = p
    while i > 0 and e[i - 1] >= e[p] - 35 and (p - i + 1) * n < (dur[1] - 0.3) * SR:
        i -= 1
    return {"a": s0 + i * n, "b": min(s1, s0 + p * n + int(0.3 * SR)), "fade_out": 0.25, "capped": False}


def export(x: np.ndarray, a: int, b: int, path: Any, fade_out: float) -> float:
    """Write samples [a, b) with short linear fades at -1 dBFS peak; return the gain in dB."""
    from hark.core.audio import write_wav

    y = np.array(x[a:b], dtype=float)
    y = np.column_stack([y, y]) if y.ndim == 1 else y[:, :2]
    n = len(y)
    fi = max(1, int(min(n / 4, 0.002 * SR)))
    fo = max(1, int(min(n / 3, fade_out * SR)))
    y[:fi] *= np.linspace(0, 1, fi)[:, None]
    y[-fo:] *= np.linspace(1, 0, fo)[:, None]
    pk = float(np.max(np.abs(y))) + 1e-12
    g = 10 ** (-1 / 20) / pk
    write_wav(str(path), y * g, SR, "PCM_24")
    return round(20 * math.log10(g), 2)


# ---------------------------------------------------------------- find (listen unit)

def _find(path: str, query: str, top: int, start: float | None, end: float | None, cache: dict) -> list[dict]:
    key = f"{path}|{query}|{top}|{start}|{end}"
    if key not in cache:
        from hark.listen.search import find

        got = find(path, query, top=top, start=start, end=end)
        cache[key] = list(got.get("segments", got) if isinstance(got, dict) else got)
    return cache[key]


def _edge_cut(seg: dict, r0: float, r1: float) -> bool:
    flags = seg.get("flags") or []
    head = "head cut" in flags and seg["start"] - r0 > EDGE_S
    tail = "tail cut" in flags and r1 - seg["end"] > EDGE_S
    return head or tail


def _reject(seg: dict, dur: tuple, zmin: float, loudest: float, kept: list) -> str | None:
    d = seg["end"] - seg["start"]
    z = seg.get("z", 0.0)
    drop = loudest - seg.get("peak_db", loudest)
    in_range = dur[0] <= d <= dur[1]
    overlap = any(seg["start"] < b and a < seg["end"] for a, b in kept)
    if not in_range:
        res = f"length {d:.2f} s"
    elif z < zmin:
        res = f"z {z:.2f} < {zmin}"
    elif drop > PEAK_DROP_DB:
        res = f"{drop:.0f} dB under the loudest"
    elif overlap:
        res = "overlaps a kept trim"
    else:
        res = None
    return res


def pick_find(segs: list[dict], k: int, dur: tuple, zmin: float, r0: float, r1: float,
              kept: list) -> tuple[list[dict], list[dict]]:
    """The best k segments that pass every test, and the rejections with their reason."""
    loudest = max((s.get("peak_db", 0.0) for s in segs), default=0.0)
    picks, rejected = [], []
    for s in sorted(segs, key=lambda s: -s.get("score", s.get("z", 0.0))):
        why = "edge cut" if _edge_cut(s, r0, r1) else _reject(s, dur, zmin, loudest, kept)
        if why is None and len(picks) >= k:
            why = "enough kept"
        if why:
            rejected.append({**s, "why": why})
        else:
            picks.append(s)
            kept.append((s["start"], s["end"]))
    return picks, rejected


# ---------------------------------------------------------------- one category

class _Build:
    """Cuts one category's splits and collects its manifest records."""

    def __init__(self, cat: str, sources: dict, kit: dict, root: Path, cache: dict, log: Callable):
        self.cat, self.sources, self.kit, self.root, self.cache, self.log = cat, sources, kit, root, cache, log
        self.query, self.dur, self.forge, self.bench = CATS[cat]
        self.zmin = ZMIN_TOGGLE if cat == "toggle" else ZMIN
        self.rec: dict = {"query": self.query, "dur": list(self.dur), "note": NOTES.get(cat, ""), "forge": [],
                          "bench": [], "rejected": []}

    def _write(self, split: str, src: dict, x: np.ndarray, cut: dict, name: str, how: str, ev: dict) -> None:
        path = self.root / self.cat / split / name
        g = export(x, cut["a"], cut["b"], path, cut["fade_out"])
        y = x[cut["a"]:cut["b"]]
        self.rec[split].append({"file": str(path), "source_id": src.get("id"), "family": family(src),
                                "title": src.get("title"), "author": src.get("author"), "licence": src.get("license"),
                                "page_url": src.get("page_url"), "source_path": src.get("path"),
                                "trim_s": [round(cut["a"] / SR, 3), round(cut["b"] / SR, 3)], "fade_in_s": 0.002,
                                "fade_out_s": cut["fade_out"], "gain_db": g,
                                "evidence": {"how": how + (" (capped)" if cut.get("capped") else ""), **ev,
                                             "t40_ms": ft.t40_ms(y), "length_s": round(len(y) / SR, 3)}})

    def _kit(self, split: str, name: str) -> None:
        snd = self.kit.get(name)
        if not snd:
            self.log(f"{self.cat} {split}: kit sound {name} not in the kit manifest")
            return
        src = entry_source(f"kit:{name}", self.sources, self.kit)
        x = _load(snd["source"]["path"])
        t0, t1 = snd["source"]["trim_s"]
        a, b = int(round(t0 * SR)), int(round(t1 * SR))
        mx = int(round(self.dur[1] * SR))
        fade = (snd.get("processing") or {}).get("fade_out_s") or 0.03
        cut = {"a": a, "b": a + mx, "fade_out": 0.25, "capped": True} if b - a > mx else \
            {"a": a, "b": b, "fade_out": fade, "capped": False}
        ev = {"kit_sound": name, **{k: (snd.get("evidence") or {}).get(k) for k in ("match_query", "match_z",
                                                                                   "realness_p")}}
        self._write(split, src, x, cut, f"kit-{name}.wav", "kit trim", ev)

    def _shape(self, split: str, src: dict, x: np.ndarray, mode: str, opts: dict) -> None:
        fn = cut_whole if mode == "whole" else cut_rise
        cut = fn(x, self.dur, opts.get("start"), opts.get("end"))
        d = (cut["b"] - cut["a"]) / SR
        if d < self.dur[0]:
            self.rec["rejected"].append({"source_id": src["id"], "split": split, "start": cut["a"] / SR,
                                         "end": cut["b"] / SR, "z": None,
                                         "why": f"{mode} cut {d:.2f} s shorter than the range"})
            return
        self._write(split, src, x, cut, f"{src['id']}_1.wav", mode, {})

    def _find_cut(self, split: str, src: dict, x: np.ndarray, k: int, opts: dict, kept: list) -> None:
        r0, r1 = (opts.get("start") or 0.0), (opts.get("end") or len(x) / SR)
        segs = _find(src["path"], self.query, max(8, 3 * k), opts.get("start"), opts.get("end"), self.cache)
        picks, rej = pick_find(segs, k, self.dur, self.zmin, r0, r1, kept)
        self.rec["rejected"] += _rejected_rows(src, split, rej)
        if not picks:
            self.log(f"{self.cat} {split} {src['id']}: nothing usable")
        for q, s in enumerate(picks, start=1):
            cut, ev = _find_trim(s)
            self._write(split, src, x, cut, f"{src['id']}_{q}.wav", "find", ev)

    def split(self, split: str, entries: list) -> None:
        kept: dict[str, list] = {}
        for e in entries:
            if isinstance(e, str):
                self._kit(split, e[4:])
                continue
            sid, k, mode = e[0], e[1], e[2]
            opts = e[3] if len(e) > 3 else {}
            src = self.sources.get(sid)
            if not src or not os.path.isfile(src.get("path", "")):
                self.log(f"{self.cat} {split}: source {sid} is not on this machine")
                continue
            x = _load(src["path"])
            if mode == "find":
                self._find_cut(split, src, x, k, opts, kept.setdefault(sid, []))
            else:
                self._shape(split, src, x, mode, opts)

    def run(self) -> dict:
        for split in ("forge", "bench"):
            d = self.root / self.cat / split
            d.mkdir(parents=True, exist_ok=True)
            for f in d.glob("*.wav"):
                f.unlink()
        self.split("forge", self.forge)
        self.split("bench", self.bench)
        self.log(f"{self.cat}: forge {len(self.rec['forge'])}, bench {len(self.rec['bench'])}, "
                 f"rejected {len(self.rec['rejected'])}")
        return self.rec


def _rejected_rows(src: dict, split: str, rej: list[dict]) -> list[dict]:
    return [{"source_id": src["id"], "split": split, "start": s["start"], "end": s["end"], "z": s.get("z"),
             "why": s["why"]} for s in rej]


def _find_trim(s: dict) -> tuple[dict, dict]:
    d = s["end"] - s["start"]
    cut = {"a": int(round(s["start"] * SR)), "b": int(round(s["end"] * SR)), "fade_out": min(0.03, d / 4)}
    ev = {"find_z": s.get("z"), "verdict": s.get("verdict"), "kind": s.get("kind"),
          "head_db": s.get("head_db"), "tail_db": s.get("tail_db")}
    return cut, ev


def _load(path: str) -> np.ndarray:
    from hark.core.audio import load

    return np.asarray(load(str(path), sr=SR), dtype=float)


# ---------------------------------------------------------------- run

def _read_json(path: Path, default: Any) -> Any:
    return json.loads(path.read_text()) if path.is_file() else default


def _kit_index(manifest: dict) -> dict:
    return {s["name"]: s for a in manifest.get("areas") or [] for s in a.get("sounds") or []}


def _registry(manifest: dict, root: Path) -> dict:
    cats = {c: {"forge": [r["file"] for r in m["forge"]], "bench": [r["file"] for r in m["bench"]],
                "note": m.get("note", "")} for c, m in manifest.items()}
    return {"version": 1, "built_by": "hark.forge.engine.eval.build_refsets", "root": str(root), "categories": cats}


def _write_registry(manifest: dict, root: Path, reg_path: Any, log: Callable) -> int:
    from hark.forge.engine import refsets

    reg = _registry(manifest, root)
    fams = {r["file"]: r["family"] for m in manifest.values() for s in ("forge", "bench") for r in m[s]}
    problems = refsets.check_split(reg, fams)
    if problems:
        log(f"split leak, registry not written: {'; '.join(problems)}")
        return 1
    log(f"registry: {refsets.write(reg, reg_path)}")
    return 0


def _parser() -> argparse.ArgumentParser:
    ap = argparse.ArgumentParser(prog="python -m hark.forge.engine.eval.build_refsets")
    ap.add_argument("--cats", nargs="+", default=None)
    ap.add_argument("--plan", action="store_true")
    ap.add_argument("--registry", default=None)
    ap.add_argument("--sources", default=str(SOURCES_DEFAULT))
    ap.add_argument("--kit", default=str(KIT_DEFAULT))
    ap.add_argument("--root", default=str(OUT))
    return ap


def _save(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)


def _cut_cats(cats: Sequence[str], sources: dict, kit: dict, root: Path) -> dict:
    mpath = root / "manifest.json"
    manifest = _read_json(mpath, {})
    cache_path = root / "_work" / "find_cache.json"
    cache = _read_json(cache_path, {})
    for cat in cats:
        manifest[cat] = _Build(cat, sources, kit, root, cache, print).run()
        _save(mpath, json.dumps(manifest, indent=1))
        _save(cache_path, json.dumps(cache))
    manifest = {c: manifest[c] for c in CATS if c in manifest}
    mpath.write_text(json.dumps(manifest, indent=1))
    return manifest


def main(argv: Sequence[str] | None = None) -> int:
    """Plan, cut every category, write the manifest and the registry."""
    a = _parser().parse_args(argv)
    sources = {s["id"]: s for s in _read_json(Path(a.sources), [])}
    kit = _kit_index(_read_json(Path(a.kit), {}))
    if plan(CATS, sources, kit):
        res = 1
    elif a.plan:
        res = 0
    else:
        root = Path(a.root)
        res = _write_registry(_cut_cats(a.cats or list(CATS), sources, kit, root), root, a.registry, print)
    return res


if __name__ == "__main__":
    raise SystemExit(main())
