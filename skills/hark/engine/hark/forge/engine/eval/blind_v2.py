"""Build a clean A/B blind listening test: forge finals against held-out real recordings.

The script makes no new sound. It cuts, fades, changes gain and places existing files on digital silence.
Run: `python -m hark.forge.engine.eval.blind_v2 [--out DIR] [--n 20] [--seed S] [--dry-run] [--allow-fail]`.
After listening: `python <out>/score_answers.py A B B A ...`.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import re
import time
from collections.abc import Sequence
from pathlib import Path
from typing import Any

import numpy as np

from hark.forge.engine.eval import blind_trim as bt
from hark.forge.engine.eval.scrub_paths import REPO

SR = bt.SR
FINALS = REPO / "out" / "forge_v2"
MANIFEST = REPO / "out" / "refs" / "manifest.json"
OUT_DEFAULT = REPO / "out" / "showcase" / "blind_v2"
PAD_S = 0.25

# (category, answer-sheet name, trials at N = 20, tier)
PLAN = [("whoosh_big", "big whoosh", 2, "strong"), ("whoosh_fast", "fast whoosh", 2, "strong"),
        ("click", "UI click", 2, "strong"), ("typing_key", "key press", 2, "strong"),
        ("shimmer", "shimmer", 2, "strong"), ("pop", "pop", 2, "strong"), ("swipe", "cloth swipe", 1, "strong"),
        ("swish", "swish", 1, "strong"), ("impact", "impact", 1, "strong"), ("hover_tick", "hover tick", 1, "middle"),
        ("tap", "tap", 1, "weak"), ("toggle", "toggle", 1, "middle"), ("notify", "notification ding", 1, "weak"),
        ("riser", "riser", 1, "weak")]
EXCLUDED = {
    "logo_sting": "its bench refs are the notify bench files (a triangle hit and a struck ring), so a pair would test "
                  "form, not realism",
    "downlifter": "no bench refs", "glitch": "no bench refs",
    "sub_drop": "its only bench ref is a tom lowered by 24 semitones: designed, not a real event",
    "pro_*": "the refs are trims of launch films, mostly under music or voice, so no clean A/B is possible",
}
NOTES = {"impact": "the bench ref is a library cinematic hit, designed by a person from recordings",
         "riser": "the bench refs are real cymbal swells; the made risers are designed",
         "whoosh_fast": "three of its bench refs are also swish bench refs; the test uses each file once",
         "swish": "three of its bench refs are also whoosh_fast bench refs; the test uses each file once"}


# ---------------------------------------------------------------- inputs

def finals(cat: str) -> list[str]:
    """The category's delivered finals <cat>_NN.wav."""
    d = Path(FINALS) / cat / "final"
    return sorted(str(p) for p in d.glob(f"{cat}_[0-9][0-9].wav")) if d.is_dir() else []


def bench_refs(cat: str) -> list[str]:
    from hark.forge.engine import refsets

    try:
        return refsets.refset(cat, "bench")
    except (refsets.RefsetsMissing, KeyError):
        return []


def _digest(p: str) -> str:
    return hashlib.md5(bt.load_audio(p).astype(np.float32).tobytes()).hexdigest()


def source_of(p: Any) -> str:
    """The file stem without a trailing _<digits> (sister trims share a source)."""
    return re.sub(r"_\d+$", "", Path(str(p)).stem)


def _caps(cat: str) -> int:
    return min(len(finals(cat)), len(bench_refs(cat)))


def _first_rounds(n: int) -> tuple[dict[str, int], int]:
    """1 trial per category, then up to min(2, plan); (allocation, trials left)."""
    alloc = {c: 0 for c, *_ in PLAN}
    left = n
    for c, *_ in PLAN:
        if left:
            alloc[c], left = 1, left - 1
    for c, _, t, _ in PLAN:
        if left and alloc[c] < min(2, t):
            alloc[c], left = alloc[c] + 1, left - 1
    return alloc, left


def _round_robin(alloc: dict[str, int], left: int) -> tuple[int, bool]:
    grew = False
    for c, *_ in PLAN:
        if left and alloc[c] < _caps(c):
            alloc[c], left, grew = alloc[c] + 1, left - 1, True
    return left, grew


def allocate(n: int) -> dict[str, int]:
    """Trials per PLAN category: 1 each, then up to min(2, plan), then round-robin up to the inputs."""
    alloc, left = _first_rounds(n)
    while left:
        left, grew = _round_robin(alloc, left)
        if not grew:
            raise SystemExit(f"cannot build {n} trials: not enough finals / bench refs")
    return {c: k for c, k in alloc.items() if k}


# ---------------------------------------------------------------- picks

class _Picker:
    """The trial picks: length and edge facts only, never a score."""

    def __init__(self, n: int, seed: int):
        self.alloc = allocate(n)
        self.refs = {c: bench_refs(c) for c in self.alloc}
        self.dig = {p: _digest(p) for ps in self.refs.values() for p in ps}
        self.used_dig: set[str] = set()
        self.used_src: set[str] = set()
        self.rng = np.random.default_rng([seed, 1])

    def _order(self) -> list[str]:
        def key(c: str) -> tuple:
            t = self.alloc[c]
            clean = {self.dig[p] for p in self.refs[c] if bt.facts(p, True)[1]}
            return (len(clean) / t, len({source_of(p) for p in self.refs[c]}) / t, c)

        return sorted(self.alloc, key=key)

    def _free(self, c: str) -> list[str]:
        return [p for p in self.refs[c] if self.dig[p] not in self.used_dig]

    def available(self, c: str) -> list[str]:
        free = self._free(c)
        clean = [p for p in free if bt.facts(p, True)[1]]
        fresh = [p for p in free if source_of(p) not in self.used_src]
        both = [p for p in clean if p in fresh]
        return next((pool for pool in (both, clean, fresh, free) if pool), [])

    def _gap(self, m: str, refs: list[str]) -> float:
        em = max(bt.facts(m, False)[0], 1e-4)
        return min((abs(math.log(max(bt.facts(r, True)[0], 1e-4) / em)) for r in refs), default=math.inf)

    def _pool(self, c: str, k: int) -> list[str]:
        fs = finals(c)
        if len(fs) < k:
            raise SystemExit(f"{c}: {len(fs)} finals for {k} trials")
        av = self.available(c)
        ok = [m for m in fs if self._gap(m, av) <= math.log(bt.COMPAT)]
        clean = [m for m in ok if bt.facts(m, False)[1]]
        if len(clean) >= k:
            pool = clean
        elif len(ok) >= k:
            pool = ok
        else:
            pool = sorted(fs, key=lambda m: (self._gap(m, av), m))[:k]
        return pool

    def _real_for(self, c: str, m: str) -> str:
        av = self.available(c)
        if not av:
            raise SystemExit(f"{c}: no unused bench ref left")
        em = max(bt.facts(m, False)[0], 1e-4)
        return min(av, key=lambda r: (abs(math.log(max(bt.facts(r, True)[0], 1e-4) / em)), r))

    def category(self, c: str) -> list[dict]:
        k = self.alloc[c]
        pool = self._pool(c, k)
        idx = sorted(self.rng.choice(len(pool), size=k, replace=False))
        out = []
        name, tier = next((nm, tr) for cc, nm, _, tr in PLAN if cc == c)
        for m in (pool[i] for i in idx):
            real = self._real_for(c, m)
            reused = source_of(real) in self.used_src
            self.used_dig.add(self.dig[real])
            self.used_src.add(source_of(real))
            out.append({"category": c, "sound": name, "tier": tier, "made": m, "real": real,
                        "source_reused": reused, "pool": [os.path.basename(p) for p in pool],
                        "clean": {"made": bt.facts(m, False)[1], "real": bt.facts(real, True)[1]}})
        return out


def _no_repeats(trials: list[dict], seed: int) -> list[dict]:
    rng = np.random.default_rng([seed, 2])
    order = list(range(len(trials)))
    for _ in range(5000):
        order = list(rng.permutation(len(trials)))
        if all(trials[a]["category"] != trials[b]["category"] for a, b in zip(order, order[1:])):
            break
    return [trials[i] for i in order]


def _sides(trials: list[dict], seed: int) -> None:
    rng = np.random.default_rng([seed, 3])
    n = len(trials)
    n_a = n // 2 + (1 if n % 2 and rng.random() < 0.5 else 0)
    sides = rng.permutation(["A"] * n_a + ["B"] * (n - n_a))
    for t, s in zip(trials, sides):
        t["real_is"] = str(s)


def pick_trials(n: int, seed: int) -> list[dict]:
    """The trials: made finals, their closest-extent bench refs, the order and the real side."""
    pk = _Picker(n, seed)
    trials = [t for c in pk._order() for t in pk.category(c)]
    trials = _no_repeats(trials, seed)
    _sides(trials, seed)
    return trials


# ---------------------------------------------------------------- one pair

def _analyse_pair(t: dict, real: np.ndarray, made: np.ndarray, cls: str) -> tuple[dict, dict, tuple]:
    ir = bt.analyse(real, cls, bt.end_fade_s(t["real"]), bt.source_floor(t["real"]), cut_file=True)
    im = bt.analyse(made, cls, bt.MADE_FADE_GUARD, None, cut_file=False)
    return ir, im, bt.pair_levels(ir, im)


def _cut_pair(real: np.ndarray, made: np.ndarray, ir: dict, im: dict, lv: tuple) -> tuple:
    keep = ir["peak_late"] and im["peak_late"]
    cr, fr = bt.cut(real, ir, lv[0], lv[1], keep)
    cm, fm = bt.cut(made, im, lv[0], lv[1], keep)
    return cr, cm, (fr, fm)


def _level_pair(cr: np.ndarray, cm: np.ndarray, cls: str) -> tuple:
    g, gc, over = bt.match_pair(cr, cm, cls)
    return cr * 10 ** (gc / 20), cm * 10 ** ((g + gc) / 20), (g, gc, over)


def _played(cr: np.ndarray, cm: np.ndarray, cls: str) -> tuple[dict, dict]:
    nf = int(bt.FADE_IN_S * SR)
    return bt.measure_clip(cr, cls, nf), bt.measure_clip(cm, cls, nf)


def _pair(t: dict) -> dict:
    real, made = bt.load_audio(t["real"]), bt.load_audio(t["made"])
    cls = bt._cls(bt.extent_s(real), bt.extent_s(made))
    ir, im, lv = _analyse_pair(t, real, made, cls)
    cr, cm, facts = _cut_pair(real, made, ir, im, lv)
    cr, cm, gains = _level_pair(cr, cm, cls)
    played = _played(cr, cm, cls)
    fails, warns, row = bt.check_pair(played, facts, lv, cls, gains[1])
    return {"real": cr, "made": cm, "cls": cls, "levels": lv, "facts": facts, "gains": gains,
            "played": played, "fails": fails, "warns": warns, "row": row,
            "files": (len(real) / SR, len(made) / SR), "extents": (bt.extent_s(real), bt.extent_s(made))}


def _rel(p: Any) -> str:
    try:
        return str(Path(p).resolve().relative_to(REPO)).replace("\\", "/")
    except ValueError:
        return str(p).replace("\\", "/")


def _md5(p: str) -> str:
    return hashlib.md5(Path(p).read_bytes()).hexdigest()


def _key_head(i: int, t: dict, a: tuple, b: tuple) -> dict:
    return {"trial": i, "category": t["category"], "sound": t["sound"], "tier": t["tier"], "A_start": a[0],
            "B_start": b[0], "A_length_s": round(a[1], 6), "B_length_s": round(b[1], 6), "real": t["real_is"],
            "real_file": _rel(t["real"]), "made_file": _rel(t["made"]), "source_reused": t["source_reused"],
            "clean_trim": t["clean"]}


def _key_trim(pr: dict) -> dict:
    return {"file_lengths_s": {"real": round(pr["files"][0], 3), "made": round(pr["files"][1], 3)},
            "extents_s": {"real": round(pr["extents"][0], 3), "made": round(pr["extents"][1], 3)},
            "class": pr["cls"], "L_on": round(pr["levels"][0], 2), "L_end": round(pr["levels"][1], 2),
            "trim_real": pr["facts"][0], "trim_made": pr["facts"][1]}


def _key_level(pr: dict) -> dict:
    g, gc, over = pr["gains"]
    return {"match": f"K-weighted loudest {bt.WIN[pr['cls']] * 1000:.0f} ms matched; real at "
                     f"{bt.TARGET[pr['cls']]:g} LUFS on that window unless a true peak would pass -1 dBTP",
            "made_gain_db": round(g, 3), "pair_gain_db": round(gc, 3), "peak_cap_drop_db": round(over, 3),
            "played": {"real": _round(pr["played"][0]), "made": _round(pr["played"][1])},
            "fails": pr["fails"], "warnings": pr["warns"]}


def _key_entry(i: int, t: dict, pr: dict, a: tuple, b: tuple) -> dict:
    return {**_key_head(i, t, a, b), **_key_trim(pr), "pool": t["pool"], **_key_level(pr),
            "note": NOTES.get(t["category"]), "md5": {"real": _md5(t["real"]), "made": _md5(t["made"])}}


def _round(d: dict) -> dict:
    return {k: round(v, 3) if isinstance(v, float) else v for k, v in d.items()}


# ---------------------------------------------------------------- build

def _fingerprint() -> dict:
    files = [p for c, *_ in PLAN for p in finals(c) + bench_refs(c)]
    return {p: (os.path.getsize(p), os.stat(p).st_mtime_ns) for p in files if os.path.isfile(p)}


def _place(trials: list[dict], pairs: list[dict]) -> tuple[list[tuple], float]:
    t, clips = bt.LEAD, []
    for tr, pr in zip(trials, pairs):
        a, b = (pr["real"], pr["made"]) if tr["real_is"] == "A" else (pr["made"], pr["real"])
        sa = round(t, 2)
        sb = round(sa + len(a) / SR + bt.GAP, 2)
        clips.append(((sa, a), (sb, b)))
        t = round(sb + len(b) / SR + bt.BETWEEN, 2)
    return clips, t - bt.BETWEEN + bt.TAIL


def _reel(clips: list[tuple], total: float) -> tuple[np.ndarray, list[str]]:
    buf = np.zeros((int(round(total * SR)) + 1, 2))
    mask = np.zeros(len(buf), dtype=bool)
    for pair in clips:
        for s, x in pair:
            i = int(round(s * SR))
            buf[i:i + len(x)] += x
            mask[i:i + len(x)] = True
    fails = []
    if np.any(buf[~mask] != 0):
        fails.append("gaps are not digital silence")
    if np.max(np.abs(buf)) > 10 ** (bt.TP_MAX / 20) + 1e-6:
        fails.append("a sample of the reel is above the true-peak ceiling")
    return buf, fails


def _trial_msgs(trials: list[dict], pairs: list[dict], key: str) -> list[str]:
    return [f"trial {i} ({t['sound']}): {f}" for i, (t, p) in enumerate(zip(trials, pairs), 1) for f in p[key]]


def _maxima(rows: list[dict]) -> dict:
    def mx(k: str) -> float | None:
        v = [r[k] for r in rows if r.get(k) is not None]
        return round(max(v), 3) if v else None

    margins = [r["floor_margin_db"] for r in rows if r.get("floor_margin_db") is not None]
    return {"max_loud_diff_lu": mx("loud_diff"), "max_meter_diff_lu": mx("meter_diff"),
            "max_true_peak_dbtp": mx("tp_max"), "max_tp_diff_db": mx("tp_diff"),
            "max_preroll_dbfs": mx("preroll_max_dbfs"), "max_floor_played_dbfs": mx("floor_played_dbfs"),
            "max_length_ratio": mx("length_ratio"), "min_floor_margin_db": round(min(margins), 2) if margins else None}


def _summary(pairs: list[dict], trials: list[dict], reel_fails: list[str]) -> dict:
    fails = _trial_msgs(trials, pairs, "fails") + reel_fails
    warns = _trial_msgs(trials, pairs, "warns")
    return {"status": "FAIL" if fails else "PASS", "trials": len(pairs),
            "pairs_with_fails": sum(1 for p in pairs if p["fails"]),
            "pairs_with_warnings": sum(1 for p in pairs if p["warns"]), **_maxima([p["row"] for p in pairs]),
            "gaps_silent": "gaps are not digital silence" not in reel_fails, "fails": fails, "warnings": warns}


def _write_trials(out: Path, clips: list[tuple]) -> None:
    from hark.core.audio import write_wav

    for f in out.glob("t*_[AB].wav"):
        f.unlink()
    pad = np.zeros((int(PAD_S * SR), 2))
    for i, pair in enumerate(clips, 1):
        for side, (_, x) in zip("AB", pair):
            write_wav(str(out / f"t{i:02d}_{side}.wav"), np.concatenate([pad, x, pad]), SR, "PCM_24")


def _verify(out: Path, pairs: list[dict], summary: dict) -> None:
    diffs, tps = [], []
    n = int(PAD_S * SR)
    for i, pr in enumerate(pairs, 1):
        xs = [bt.load_audio(out / f"t{i:02d}_{s}.wav")[n:-n] for s in "AB"]
        w = bt.WIN[pr["cls"]]
        diffs.append(abs(bt.loud_max(xs[0], w) - bt.loud_max(xs[1], w)))
        tps += [bt.true_peak(x) for x in xs]
    v = {"max_loud_diff_lu": round(max(diffs), 3), "max_true_peak_dbtp": round(max(tps), 3)}
    summary["verified_from_written_files"] = v
    if v["max_loud_diff_lu"] > bt.FAIL["loud_lu"] or v["max_true_peak_dbtp"] > bt.FAIL["true_peak"]:
        summary["status"] = "FAIL"
        summary["fails"].append("the written trial files do not hold the matched levels")


def _residual(pairs: list[dict]) -> dict:
    def count(get: Any, eps: float) -> dict:
        c = {"made_more": 0, "made_less": 0, "same": 0}
        for p in pairs:
            d = get(p["played"][1]) - get(p["played"][0])
            c["same" if abs(d) <= eps else "made_more" if d > 0 else "made_less"] += 1
        return c

    return {"other_window": count(lambda m: m["loud_other_window"], 0.1), "meter": count(lambda m: m["meter_m400"], 0.1),
            "true_peak": count(lambda m: m["true_peak_dbtp"], 0.5),
            "length": count(lambda m: math.log(max(m["length_s"], 1e-6)), math.log(1.1))}


def _cleanliness(summary: dict, pairs: list[dict]) -> dict:
    return {"about": "Cleanliness of the blind test. No answers are inside: only differences, maxima and sorted pairs.",
            "fail_limits": bt.FAIL, "warn_limits": bt.WARN,
            "method": {"trim": "both clips trimmed by one rule: 4 ms raised-cosine fade in before the onset, a fade "
                                "over the last 6 dB of decay above a shared end level",
                       "level": "made gain matches the K-weighted loudest window; one common gain sets the pair "
                                "level, lowered together when a true peak would pass -1 dBTP",
                       "floor": "onset and end levels sit at least 10 dB above each ref's recording floor"},
            "summary": summary, "pairs": [p["row"] for p in pairs]}


def _status_line(summary: dict) -> str:
    return (f"blind test {summary['status']}: {summary['trials']} trials, {len(summary['fails'])} fails, "
            f"{len(summary['warnings'])} warnings" + "".join(f"\n  {x}" for x in summary["fails"] + summary["warnings"]))


def _same_inputs(before: dict, after: dict) -> None:
    if after != before:
        changed = sorted(os.path.basename(p) for p in set(before) | set(after) if before.get(p) != after.get(p))
        raise SystemExit(f"input files changed while building ({', '.join(changed[:5])})")


def _assemble(n: int, seed: int) -> tuple[dict, tuple]:
    """(summary, (clips, reel, pairs, keys, total)); stops when an input file changes during the build."""
    before = _fingerprint()
    trials = pick_trials(n, seed)
    pairs = [_pair(t) for t in trials]
    clips, total = _place(trials, pairs)
    keys = [_key_entry(i, t, pr, (a[0], len(a[1]) / SR), (b[0], len(b[1]) / SR))
            for i, (t, pr, (a, b)) in enumerate(zip(trials, pairs, clips), 1)]
    _same_inputs(before, _fingerprint())
    reel, reel_fails = _reel(clips, total)
    return _summary(pairs, trials, reel_fails), (clips, reel, pairs, keys, total)


def _print_fail(summary: dict) -> None:
    print(f"CLEANLINESS FAIL (nothing written; {len(summary['fails'])} fails)")
    for f in summary["fails"]:
        print(f"  {f}")


def build_blind(out: Any, n: int = 20, seed: int = 2026, quiet: bool = False, allow_fail: bool = False,
                dry_run: bool = False) -> dict:
    """Build the blind test folder; returns {'code', 'summary'}."""
    bt.MANIFEST = Path(MANIFEST)
    bt._manifest_index.cache_clear()
    summary, parts = _assemble(n, seed)
    if dry_run:
        print(_status_line(summary))
    elif summary["status"] == "FAIL" and not allow_fail:
        _print_fail(summary)
    else:
        _write(Path(out), parts, summary, (seed, n, quiet))
    return {"code": 0 if summary["status"] == "PASS" else 2, "summary": summary}


def _key_doc(seed: int, keys: list, summary: dict, pairs: list) -> dict:
    return {"seed": seed, "built": time.strftime("%Y-%m-%d %H:%M"), "trials": keys, "excluded": EXCLUDED,
            "cleanliness": summary, "how": "real = the side (A or B) that plays the real recording",
            "residual_signs_made_vs_real": _residual(pairs)}


def _write_docs(out: Path, clean: dict, key: dict, summary: dict) -> None:
    from hark.forge.engine.eval.blind_sheet import SCORE_SCRIPT

    (out / "cleanliness.json").write_text(json.dumps(clean, indent=1))
    key["cleanliness"] = summary
    (out / "KEY_open_after.json").write_text(json.dumps(key, indent=1))
    (out / "score_answers.py").write_text(SCORE_SCRIPT)


def _write(out: Path, parts: tuple, summary: dict, run: tuple) -> None:
    from hark.core.audio import write_wav

    from hark.forge.engine.eval.blind_sheet import answer_sheet

    clips, reel, pairs, keys, total = parts
    clean = _cleanliness(summary, pairs)
    key = _key_doc(run[0], keys, summary, pairs)
    json.dumps(clean)
    json.dumps(key)
    if not np.all(np.isfinite(reel)):
        raise SystemExit("the reel has non-finite samples")
    out.mkdir(parents=True, exist_ok=True)
    _write_trials(out, clips)
    write_wav(str(out / "blind_test.wav"), reel, SR, "PCM_24")
    _verify(out, pairs, summary)
    _write_docs(out, clean, key, summary)
    sheet = answer_sheet(keys, summary, total, run[1])
    (out / "answer_sheet.txt").write_text(sheet)
    if not run[2]:
        print(sheet)


# ---------------------------------------------------------------- CLI

def _path(p: str) -> Path:
    q = Path(p)
    return q if q.is_absolute() else REPO / q


def _parser() -> argparse.ArgumentParser:
    ap = argparse.ArgumentParser(prog="python -m hark.forge.engine.eval.blind_v2")
    ap.add_argument("--out", default=str(OUT_DEFAULT))
    ap.add_argument("--progress", default=None)
    ap.add_argument("--seed", type=int, default=2026)
    ap.add_argument("--n", type=int, default=20)
    ap.add_argument("--results", default=None)
    ap.add_argument("--only", choices=("blind", "progress"), default=None)
    ap.add_argument("--allow-fail", action="store_true")
    ap.add_argument("--dry-run", action="store_true")
    return ap


def main(argv: Sequence[str] | None = None) -> int:
    """Build the blind test and, unless skipped, the progress file."""
    a = _parser().parse_args(argv)
    out = _path(a.out)
    if a.only != "progress":
        res = build_blind(out, a.n, a.seed, allow_fail=a.allow_fail, dry_run=a.dry_run)
        if res["code"]:
            return res["code"]
    if a.dry_run or a.only == "blind":
        return 0
    from hark.forge.engine.eval.blind_progress import build_progress

    build_progress(_path(a.progress) if a.progress else out.parent / "progress",
                   _path(a.results) if a.results else None)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
