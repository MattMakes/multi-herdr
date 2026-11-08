#!/usr/bin/env python3
"""Measured QA of a render. Each finding names the placement that caused it.

Sections: clicks, loudness, coverage, edges, audibility, harshness, sync,
density, listen, mix. No models are needed except for --listen/--realness.
Run with the hark venv Python and -I. --self-test runs the built-in checks.
"""
from __future__ import annotations

import argparse
import json
import math
import os
import re
import sys
from collections import Counter

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import numpy as np  # noqa: E402
from scipy import signal  # noqa: E402
from scipy.ndimage import uniform_filter1d  # noqa: E402

import master as mst  # noqa: E402
import mix  # noqa: E402

SR = mix.SR
ORDER = {"fail": 3, "warn": 2, "ok": 1, "skipped": 0}
LONG_KINDS = {"whoosh", "riser", "swell", "suck"}


class InputError(Exception):
    pass


def ms(samples) -> float:
    return round(float(samples) / SR * 1000, 1)


def tfmt(t: float | None) -> str:
    if t is None:
        return "n.a."
    return f"{t:.2f}s ({int(t // 60)}:{t % 60:05.2f})"


def dbp(p: float) -> float:
    return 10 * math.log10(p) if p > 0 else -200.0


def _clean_scalar(o):
    if isinstance(o, np.generic):
        o = o.item()
    return None if isinstance(o, float) and not math.isfinite(o) else o


def clean(o):
    if isinstance(o, dict):
        return {str(k): clean(v) for k, v in o.items()}
    if isinstance(o, (list, tuple)):
        return [clean(v) for v in o]
    return _clean_scalar(o)


def sos(order, hz, kind):
    return signal.butter(order, hz, btype=kind, fs=SR, output="sos")


# ---------------------------------------------------------------- inputs

def read(path: str) -> np.ndarray:
    import soundfile as sf
    return mst.as_stereo(sf.read(path, dtype="float32", always_2d=True)[0]).astype(np.float32)


def _mix_paths(a, rp) -> tuple[str, str]:
    """(report path, stems folder); a solo QA (--only) reads the solo render's files."""
    if a.only is None:
        return rp(a.report or "out/qa/mix_report.json"), rp(a.stems or "out/audio/stems")
    tag = mix.safe(a.only, "A-Za-z0-9._-")
    return (rp(a.report or f"out/audio-review/solo/{tag}_report.json"),
            rp(a.stems or f"out/audio-review/solo/{tag}_stems"))


def _read_stems(stems_p: str) -> dict:
    stems = {}
    for b in mix.BUSES:
        f = os.path.join(stems_p, f"{b}.wav")
        if not os.path.isfile(f):
            raise InputError(f"stems not found: {f} (--stems)")
        stems[b] = read(f)
    if len({len(x) for x in stems.values()}) != 1:
        raise InputError(f"stems of one length not found: {stems_p} (--stems)")
    return stems


def _master_path(a, rp, solo: bool):
    if a.master:
        return rp(a.master)
    return None if solo else rp("out/audio/soundtrack.wav")


def _fit_master(master: np.ndarray, n: int, mp: str) -> np.ndarray:
    if abs(len(master) - n) > 1:
        raise InputError(f"master of the stems' length not found: {mp} (--master)")
    return np.pad(master, ((0, max(0, n - len(master))), (0, 0)))[:n]


def _read_master(a, rp, solo: bool, n: int):
    mp = None if a.master == "none" else _master_path(a, rp, solo)
    if mp and os.path.isfile(mp):
        return _fit_master(read(mp), n, mp)
    if a.master and a.master != "none":
        raise InputError(f"master not found: {mp} (--master)")
    return None


def _side_inputs(a, design: dict, root: str, rp) -> dict:
    """Cues, trace and cache, with their paths."""
    cp = mix.cue_path(a.cues, design, root)
    tp = rp(a.trace)
    trace = mix.load_json(tp) if os.path.isfile(tp) else None
    return {"cues": mix.load_cues(cp) if os.path.isfile(cp) else {"fps": 60, "events": []}, "cues_path": cp,
            "trace": trace, "trace_path": tp if trace else None,
            "cache": rp(a.cache) if a.cache else os.path.join(root, "out/audio/cache")}


def load_inputs(a) -> dict:
    root = os.path.abspath(a.root)
    rp = lambda p: mix.rpath(root, p)
    dpath = rp(a.design)
    if not os.path.isfile(dpath):
        raise InputError(f"design not found: {dpath} (--design)")
    design = mix.load_design(a.design, root, a.only)
    report_p, stems_p = _mix_paths(a, rp)
    if not os.path.isfile(report_p):
        raise InputError(f"report not found: {report_p} (--report)")
    stems = _read_stems(stems_p)
    solo = a.only is not None
    side = _side_inputs(a, design, root, rp)
    return {"root": root, "design": design, "report": mix.load_json(report_p), "stems": stems,
            "master": _read_master(a, rp, solo, len(stems["sfx"])),
            "cues": side["cues"], "trace": side["trace"], "cache": side["cache"], "solo": solo,
            "paths": {"design": dpath, "report": report_p, "stems": stems_p, "trace": side["trace_path"],
                      "cues": side["cues_path"], "master": a.master}}


# ---------------------------------------------------------------- layers and groups

def _layer_meta(pl: dict, rules: dict, places) -> dict:
    """pitch, offset, align, cue time, rule name and qa_ok of 1 placement."""
    if pl["kind"] == "place":
        item = next(places, {})
        return {"pitch": float(item.get("pitch", 0)), "off": float(item.get("offset", 0)),
                "align": item.get("align", "attack"), "cue": float(item.get("at", pl["t"])), "rule": "place",
                "qok": item.get("qa_ok")}
    r = rules.get(str(pl["rule"]), {})
    off = float(r.get("offset", 0))
    return {"pitch": float(pl.get("pitch", 0)), "off": off, "align": pl.get("align", "attack"),
            "cue": pl["t"] - off, "rule": str(pl["rule"]), "qok": r.get("qa_ok")}


def _get(lib, name: str, pitch: float):
    try:
        return lib.get(name, pitch)
    except mix.DesignError:
        return None


def _layer_span(x: np.ndarray, rec: dict, d: dict, N: int):
    """(a, b, signal in the film, attack sample), or None for a sound wholly outside the film."""
    s = rec["start"]
    a, b = max(0, s), min(N, s + len(x))
    if not len(x):
        # an empty trim stays as a silent layer, so audibility can name it
        a = min(max(0, s), N)
        return a, a, np.zeros((0, 2), np.float32), a
    if a >= b:
        return None
    gl, gr = mix.pan_gains(rec["pan"])
    y = x[a - s:b - s].astype(np.float32) * np.float32(rec["gain"])
    y = y * np.array([gl, gr], np.float32) * mix.bus_curve(d, "sfx", a, b - a)[:, None]
    return a, b, y, s + mix.align_index(x, "attack")


_BAD = "bad"


def _layer(st: dict, rec: dict, pl: dict):
    """The rebuilt sfx layer of 1 trace record, None when it is not an sfx layer in the film, or _BAD."""
    m = _layer_meta(pl, st["rules"], st["places"])
    x = _get(st["lib"], pl["sample"], m["pitch"])
    if x is None or len(x) != rec["n"]:
        return _BAD
    span = _layer_span(x, rec, st["design"], st["N"]) if rec["bus"] == "sfx" else None
    if span is None:
        return None
    a, b, y, att = span
    return {"sample": pl["sample"], "rule": m["rule"], "kind": pl["kind"], "event": pl.get("event"),
            "cue": m["cue"], "offset": m["off"], "align": m["align"], "start": rec["start"], "n": len(x), "a": a,
            "b": b, "sig": y, "qa_ok": m["qok"], "att": att, "peak_db": mst.db(_peak(y))}


def rebuild(ctx: dict) -> tuple[list, list[str]]:
    d, rep, trace = ctx["design"], ctx["report"], ctx["trace"]
    recs = [r for r in trace if "sample" in r]
    pls = rep["placements"]
    if len(recs) != len(pls):
        return None, [f"the trace ({len(recs)} sample adds) and the report ({len(pls)} placements) disagree: "
                      "run the mix again with --trace"]
    lib = mix.Library(d, ctx["root"], ctx["cache"])
    ctx["lib"] = lib
    st = {"lib": lib, "design": d, "N": len(ctx["stems"]["sfx"]), "places": iter(d["place"]),
          "rules": {mix.rule_key(r, i): r for i, r in enumerate(d["rules"])}}
    return _sort_built([_layer(st, rec, pl) for rec, pl in zip(recs, pls)])


def _sort_built(built: list) -> tuple[list, list[str]]:
    """(the layers, a note on the placements that could not be rebuilt)."""
    bad = sum(1 for L in built if L is _BAD)
    notes = [f"{bad} placement(s) could not be rebuilt: the design changed after the mix"] if bad else []
    return [L for L in built if isinstance(L, dict)], notes


def _peak(x: np.ndarray) -> float:
    return float(np.max(np.abs(x))) if x.size else 0.0


def _free_group(groups: list, L: dict) -> dict:
    """A hand placement joins a free group that started up to 140 ms earlier, or starts one."""
    g = next((c for c in reversed(groups[-12:])
              if c["free"] and L["att"] - c["layers"][0]["att"] <= int(0.14 * SR)), None)
    if g is None:
        g = {"free": True, "layers": []}
        groups.append(g)
    return g


def _cue_group(groups: list, index: dict, L: dict) -> dict:
    k = (L["event"], round(L["cue"], 4))
    if k not in index:
        index[k] = {"free": False, "layers": []}
        groups.append(index[k])
    return index[k]


def _bucket(layers: list) -> list[dict]:
    groups, index = [], {}
    for L in sorted(layers, key=lambda q: q["att"]):
        g = _free_group(groups, L) if L["kind"] == "place" else _cue_group(groups, index, L)
        g["layers"].append(L)
    return groups


def _sum_layers(ls: list) -> tuple[int, int, np.ndarray]:
    a, b = min(q["a"] for q in ls), max(q["b"] for q in ls)
    sig = np.zeros((b - a, 2), np.float32)
    for q in ls:
        sig[q["a"] - a:q["b"] - a] += q["sig"]
    return a, b, sig


def _flags(ls: list) -> dict:
    return {"long": any(q["kind"] in LONG_KINDS or q["align"] == "peak" for q in ls),
            "builds": any(q["kind"] in ("riser", "swell") for q in ls),
            "qa_ok": next((q["qa_ok"] for q in ls if q.get("qa_ok")), None)}


def _group_row(g: dict, G: float) -> dict:
    ls = g["layers"]
    a, b, sig = _sum_layers(ls)
    loud = max(ls, key=lambda q: q["peak_db"])
    pk = mst.db(_peak(sig)) + G
    cue = min(q["cue"] for q in ls)
    label = f"place:{ls[0]['sample']}" if g["free"] else ls[0]["event"]
    row = {"a": a, "b": b, "sig": sig, "layers": ls, "cue": cue, "loud": loud, "kind": loud["kind"], "label": label}
    row.update(_flags(ls))
    row.update({"att": min(q["att"] for q in ls), "peak_db": round(pk, 2), "silent": pk < -70,
                "rules": sorted({q["rule"] for q in ls}), "t": round(cue, 4)})
    return row


def group(layers: list, G: float) -> list[dict]:
    return [_group_row(g, G) for g in _bucket(layers)]


# ---------------------------------------------------------------- clicks

def _window_energy(p: np.ndarray, half: int) -> np.ndarray:
    c = np.concatenate([[0.0], np.cumsum(p)])
    i = np.arange(len(p))
    return c[np.minimum(len(p), i + half + 1)] - c[np.maximum(0, i - half)]


def detect_clicks(x: np.ndarray) -> list[dict]:
    found = []
    hp = sos(4, 14000, "highpass")
    for ch in range(2):
        hh = signal.sosfilt(hp, x[:, ch].astype(np.float64))
        e, p = np.abs(hh), hh * hh
        loc = np.sqrt(np.maximum(uniform_filter1d(p, 960), 0.0)) + 1e-5
        near = np.sqrt(np.maximum(uniform_filter1d(p, 192), 0.0)) + 1e-5
        cand = np.nonzero((e / loc > 9) & (e / near > 6) & (e > 0.01))[0]
        if not len(cand):
            continue
        e24, e240 = _window_energy(p, 24), _window_energy(p, 240)
        prev = -10 ** 9
        for i in cand:
            if i - prev > 480:
                conc = e24[i] / max(e240[i], 1e-30)
                found.append({"i": int(i), "ch": ch, "level_db": round(mst.db(float(e[i])), 2), "conc": round(float(conc), 3),
                              "kind": "click" if conc >= 0.8 else "attack"})
            prev = i
    found.sort(key=lambda c: (c["i"], c["ch"]))
    out = []
    for c in found:
        if out and c["i"] - out[-1]["i"] <= 48:
            continue
        out.append(c)
    return out


def _where(i: int, start: int, n: int) -> str:
    if abs(i - start) <= 96:
        return "start"
    if abs(i - (start + n)) <= 96:
        return "end"
    return "body"


def _layer_hf(L: dict, i: int, ch: int, hp) -> float | None:
    """The high-passed peak of layer L within 0.5 ms of sample i, or None when L is not near i."""
    if L["b"] < i - 480 or L["a"] > i + 480:
        return None
    lo, hi = max(L["a"], i - 480), min(L["b"], i + 480)
    if hi - lo < 64:
        lo, hi = max(L["a"], hi - 64), min(L["b"], lo + 64)
    seg = L["sig"][lo - L["a"]:hi - L["a"], ch].astype(np.float64)
    if len(seg) < 8:
        return None
    hh = np.abs(signal.sosfilt(hp, seg))
    k0, k1 = max(0, i - 24 - lo), max(0, min(len(hh), i + 25 - lo))
    return float(hh[k0:k1].max()) if k1 > k0 else 0.0


def _layer_owner(c: dict, name: str, cx: dict) -> list[dict]:
    """The sfx layer that carries click c, when its high band explains at least 30 % of it."""
    i = c["i"]
    lvl = 10 ** (c["level_db"] / 20) * (10 ** (-cx["G"] / 20) if name == "master" else 1)
    best, bv = None, 0.0
    for L in cx["layers"]:
        v = _layer_hf(L, i, c["ch"], cx["hp"])
        if v is not None and v > bv:
            best, bv = L, v
    if best is None or bv < 0.3 * lvl:
        return []
    return [{"bus": "sfx", "what": best["sample"], "rule": best["rule"], "where": _where(i, best["start"], best["n"]),
             "from_start_ms": ms(i - best["start"]), "from_end_ms": ms(best["start"] + best["n"] - i)}]


def _trace_owners(c: dict, name: str, trace: list) -> list[dict]:
    """Up to 3 trace records near click c, edges first."""
    i = c["i"]
    recs = [r for r in trace if r["start"] - 960 <= i <= r["start"] + r["n"] + 960
            and (name == "master" or r["bus"] == name)]
    dist = lambda r: min(abs(i - r["start"]), abs(i - r["start"] - r["n"]))
    recs.sort(key=lambda r: (_where(i, r["start"], r["n"]) == "body", dist(r)))
    return [{"bus": r["bus"], "what": r.get("sample") or r.get("name") or r.get("label"),
             "rule": r.get("rule"), "where": _where(i, r["start"], r["n"]),
             "from_start_ms": ms(i - r["start"]), "level_db": c["level_db"]} for r in recs[:3]]


def _click_item(name: str, c: dict, cx: dict) -> dict:
    owners = _layer_owner(c, name, cx) if cx["layers"] and name in ("master", "sfx") else []
    owners = owners or _trace_owners(c, name, cx["trace"])
    return {"in": name, "t": round(c["i"] / SR, 4), "ch": c["ch"], "level_db": c["level_db"], "conc": c["conc"],
            "kind": "click", "owners": owners, "at_edge": bool(owners) and owners[0]["where"] in ("start", "end")}


def _click_counts(cl: list) -> dict:
    return {"clicks": sum(c["kind"] == "click" for c in cl), "attacks": sum(c["kind"] == "attack" for c in cl)}


def _clicks_result(items: list, counts: dict) -> dict:
    edge_master = [c for c in items if c["in"] == "master" and c["at_edge"]]
    status = "fail" if edge_master else ("warn" if items else "ok")
    n = len(items)
    summ = (f"{n} click(s), {len(edge_master)} at a placement edge on the master" if n else "no clicks")
    if n:
        summ += "; a click inside a recording is its own transient or a defect of the source: a critic decides"
    return {"status": status, "summary": summ, "counts": counts, "items": items}


def section_clicks(ctx, layers, G) -> dict:
    sigs = ([("master", ctx["master"])] if ctx["master"] is not None else []) + [(b, ctx["stems"][b]) for b in mix.BUSES]
    cx = {"layers": layers, "G": G, "hp": sos(4, 14000, "highpass"), "trace": ctx["trace"] or []}
    items, counts = [], {}
    for name, x in sigs:
        cl = detect_clicks(x)
        counts[name] = _click_counts(cl)
        items += [_click_item(name, c, cx) for c in cl if c["kind"] == "click"]
    return _clicks_result(items, counts)


# ---------------------------------------------------------------- edges

def _rms_db(x: np.ndarray) -> float:
    return mst.db(float(np.sqrt(np.mean(x.astype(np.float64) ** 2)))) if len(x) else -200.0


def late_onset(x: np.ndarray) -> float | None:
    m = np.abs(x).max(axis=1)
    nb = len(m) // 240
    if nb < 3:
        return None
    e = m[: nb * 240].reshape(nb, 240).max(axis=1)
    pk = e.max()
    for k in range(max(1, int(0.6 * nb)), nb):
        if e[k] > 0.12 * pk and e[k] > 4 * e[k - 1]:
            return round(k * 240 / SR, 4)
    return None


def _entry_findings(ar: dict, head: float, att: float) -> tuple[list, list]:
    inside = ar["starts_late"] and ar["pre20"] > -20
    abrupt = inside and ar["fi"] < 0.005 and (att > 10 or ar["pre20"] > -10)
    if abrupt:
        return [f"abrupt entry: the trim cuts into a sound already playing (pre20 {ar['pre20']:.1f} dB) "
                "and no attack covers it"], []
    if inside:
        return [], ["the trim starts inside a sound; the fade-in softens it"]
    return [], ([f"the attack starts in the first ms (head {head:.1f} dB): the fade-in dulls it; start the trim "
                 "3-5 ms earlier"] if head > -30 else [])


def _exit_findings(ar: dict, tail: float) -> tuple[list, list]:
    if not (tail > -35 and ar["post20"] > -35):
        return [], []
    chopped = ar["fo"] < 0.03
    msg = f"{'chopped' if chopped else 'decay faded early'}: the sound continues after the trim (tail {tail:.1f} dB)"
    return ([msg], []) if chopped else ([], [msg])


def judge_trim(o: dict, proc: np.ndarray, ar: dict) -> dict:
    """Edge findings of 1 trim. ar: pre20, post20 (dB around the trim), starts_late, fi, fo (fade lengths)."""
    pk = float(np.max(np.abs(proc))) or 1e-12
    head = _rms_db(proc[:48]) - mst.db(pk)
    tail = _rms_db(proc[-48:]) - mst.db(pk)
    att = mix.align_index(proc, "attack") / SR * 1000
    defects, notes = _entry_findings(ar, head, att)
    d2, n2 = _exit_findings(ar, tail)
    defects, notes = defects + d2, notes + n2
    lo = late_onset(proc)
    if lo is not None:
        notes.append(f"a second hit at {lo:.3f} s inside the sample (fine for a rattle, wrong for a single hit)")
    if o.get("qa_ok") and defects:
        notes += [f"acknowledged ({o['qa_ok']}): {m}" for m in defects]
        defects = []
    return {"head_db": round(head, 2), "tail_db": round(tail, 2), "attack_ms": round(att, 2), "late_onset": lo,
            "defects": defects, "notes": notes}


def _silent_trim(trim: np.ndarray, rpk: float, raw: np.ndarray) -> list[str]:
    why = ("empty (start >= end or past the file end)" if len(trim) == 0 else
           "digital silence" if rpk == 0 else "near-silent")
    return [f"the trim is {why}; the file is {len(raw) / SR:.3f} s long"]


def _sample_edges(ctx: dict, name: str) -> dict:
    o = ctx["design"]["samples"][name]
    raw = mix.decode(mix.rpath(ctx["root"], o["file"]), ctx["cache"])
    a = int(float(o.get("start", 0)) * SR)
    b = int(float(o["end"]) * SR) if o.get("end") is not None else len(raw)
    trim = raw[a:b]
    rpk = float(np.max(np.abs(trim))) if len(trim) else 0.0
    it = {"sample": name, "file": o["file"]}
    if mst.db(rpk) < -60:
        it.update(defects=_silent_trim(trim, rpk, raw), notes=[])
        return it
    pre20 = _rms_db(raw[max(0, a - 960):a]) - mst.db(rpk) if a > 0 else -200.0
    post20 = _rms_db(raw[b:b + 960]) - mst.db(rpk) if b < len(raw) else -200.0
    it.update(pre20_db=round(pre20, 2), post20_db=round(post20, 2))
    ar = {"pre20": pre20, "post20": post20, "starts_late": a > 0, "fi": float(o.get("fade_in", 0.002)),
          "fo": float(o.get("fade_out", 0.012))}
    it.update(judge_trim(o, mix.shape(trim, o, with_fades=False), ar))
    if o.get("qa_ok"):
        it["qa_ok"] = o["qa_ok"]
    return it


def _strip_numbers(m: str) -> str:
    return re.sub(r"\s*\([^)]*\d[^)]*\)|\d+(\.\d+)?\s*s\b", "", m).strip()


def _slice_edges(ctx: dict, g: str) -> dict:
    o = ctx["design"]["slices"].get(g, {})
    it = {"slices": g, "file": o.get("file")}
    try:
        sl = mix.cut_slices(mix.decode(mix.rpath(ctx["root"], o["file"]), ctx["cache"]), o, o["file"], with_fades=False)
    except Exception as e:  # noqa: BLE001
        it.update(defects=[f"cannot slice: {e}"], notes=[])
        return it
    defs, nts = set(), set()
    for _, x in sl:
        ar = {"pre20": -200.0, "post20": _rms_db(x[-48:]) - mst.db(float(np.max(np.abs(x))) or 1e-12),
              "starts_late": False, "fi": float(o.get("fade_in", 0.002)), "fo": float(o.get("fade_out", 0.02))}
        r = judge_trim(o, x, ar)
        defs |= {_strip_numbers(m) for m in r["defects"]}
        nts |= {_strip_numbers(m) for m in r["notes"]}
    it.update(n=len(sl), defects=sorted(defs), notes=sorted(nts))
    return it


def _music_source(ctx: dict, mu: dict) -> tuple[np.ndarray, float]:
    """The (stretched) music source and its time factor."""
    ratio = 1.0 if mu.get("stretch") == "none" else float(mu.get("target_bpm", 120)) / float(mu["src_bpm"])
    path = mix.rpath(ctx["root"], mu["file"])
    src = mix.stretched(path, ratio, ctx["cache"]) if abs(ratio - 1) >= 1e-6 else mix.decode(path, ctx["cache"])
    return src, 1 / ratio


def _chop(row: dict, s: dict, end_t: float, film: float) -> None:
    chops = (not s.get("tail_fade") and float(s.get("fade_out", 0.06)) < 0.1 and end_t < film - 0.05
             and row["cut_db"] > row["out_db"] - 6 and row["cut_db"] > row["in_db"] + 6)
    if not chops:
        return
    msg = "chops the music: the source keeps playing past src_to and the next part is quieter"
    if s.get("qa_ok"):
        row["notes"].append(f"acknowledged ({s['qa_ok']}): {msg}")
    else:
        row["defects"].append(msg)


def _gap_note(nxt, end_t: float) -> list[str]:
    gap = float(nxt["at"]) - end_t if nxt is not None else 0.0
    if gap > 0.02:
        return [f"{gap * 1000:.0f} ms of no music before the next segment"]
    return [f"overlaps the next segment by {-gap * 1000:.0f} ms"] if gap < -0.1 else []


def _splice_row(sp: dict, s: dict, nxt) -> dict:
    """1 music splice. sp: lv (level at a sample), k (time factor), w (window), film (length)."""
    lv, k, w = sp["lv"], sp["k"], sp["w"]
    end = int(round(float(s["src_to"]) * k * SR))
    start = int(round(float(s["src_from"]) * k * SR))
    end_t = float(s["at"]) + (float(s["src_to"]) - float(s["src_from"])) * k
    in_db = lv(int(round(float(nxt["src_from"]) * k * SR))) if nxt else -120.0
    row = {"label": s.get("label"), "at": s["at"], "end_t": round(end_t, 4), "out_db": round(lv(end - w), 2),
           "cut_db": round(lv(end), 2), "in_db": round(in_db, 2), "defects": [], "notes": []}
    _chop(row, s, end_t, sp["film"])
    row["notes"] += _gap_note(nxt, end_t)
    if start > w and lv(start - w) > lv(start) - 3:
        row["notes"].append("begins inside a sustained sound (no fresh attack on the splice)")
    return row


def _music_splices(ctx: dict) -> list[dict]:
    mu = ctx["design"].get("music")
    if not (mu and mu.get("segments")):
        return []
    src, k = _music_source(ctx, mu)
    w = int(0.05 * SR)
    sp = {"lv": lambda i: _rms_db(src[max(0, i):max(0, i) + w]), "k": k, "w": w,
          "film": len(ctx["stems"]["sfx"]) / SR}
    segs = sorted(mu["segments"], key=lambda s: float(s["at"]))
    return [_splice_row(sp, s, segs[j + 1] if j + 1 < len(segs) else None) for j, s in enumerate(segs)]


def _cut_by_ends(ctx: dict) -> list[dict]:
    N = len(ctx["stems"]["sfx"])
    return [{"bus": r["bus"], "what": r.get("sample") or r.get("name") or r.get("label"), "t": round(r["start"] / SR, 4),
             "before_0": r["start"] < 0, "after_end": r["start"] + r["n"] > N}
            for r in ctx["trace"] or [] if r["bus"] != "music" and (r["start"] < 0 or r["start"] + r["n"] > N)]


def _trim_items(ctx: dict) -> list[dict]:
    """Edge findings of every used sample trim and slice group."""
    d = ctx["design"]
    used = {p["sample"] for p in ctx["report"]["placements"]}
    items = [_sample_edges(ctx, n) for n in sorted(u for u in used if "#" not in u and u in d["samples"])]
    return items + [_slice_edges(ctx, g) for g in sorted({u.split("#")[0] for u in used if "#" in u})]


def section_edges(ctx) -> dict:
    items = _trim_items(ctx)
    splices = _music_splices(ctx)
    cut = _cut_by_ends(ctx)
    nd = sum(bool(i["defects"]) for i in items) + sum(bool(s["defects"]) for s in splices)
    summ = f"{nd} edge defect(s) in {len(items)} trim(s) and {len(splices)} music splice(s)"
    return {"status": "warn" if nd else "ok", "summary": summ + (f"; {len(cut)} placement(s) cut by the film ends" if cut else ""),
            "items": items, "music_splices": splices, "cut_by_film_ends": cut}


# ---------------------------------------------------------------- audibility

_BANDS = [10 ** (n / 10) for n in range(20, 42)]
_MASKS: dict[int, list] = {}


def band_energy(x: np.ndarray) -> np.ndarray:
    n = len(x)
    if n not in _MASKS:
        f = np.fft.rfftfreq(n, 1 / SR)
        _MASKS[n] = [(f >= c * 2 ** (-1 / 6)) & (f < c * 2 ** (1 / 6)) for c in _BANDS]
    w = signal.windows.tukey(n, 0.25)[:, None]
    p = (np.abs(np.fft.rfft(x * w, axis=0)) ** 2).sum(axis=1)
    return np.array([p[m].sum() for m in _MASKS[n]]) + 1e-30


def margin(E: np.ndarray, X: np.ndarray) -> tuple[float, float]:
    keep = E >= 1e-3 * E.max()
    m = np.clip(10 * np.log10(E / X), -60, 60)
    m = np.where(keep, m, -np.inf)
    i = int(np.argmax(m))
    return round(float(m[i]), 2), round(_BANDS[i])


def _take(x: np.ndarray, start: int, n: int, base: int = 0) -> np.ndarray:
    """Samples [start, start + n) of x (x begins at absolute sample base), zero-padded."""
    out = np.zeros((n, 2), np.float32)
    a, b = max(start, base), min(start + n, base + len(x))
    if a < b:
        out[a - start:b - start] = x[a - base:b - base]
    return out


def best_window(E: np.ndarray, base: int, lo: int, hi: int, n: int) -> int:
    p = (E.astype(np.float64) ** 2).sum(axis=1)
    c = np.concatenate([[0.0], np.cumsum(p)])
    best, bv = lo, -1.0
    for s in range(lo, max(lo, hi) + 1, 48):
        a, b = min(max(0, s - base), len(p)), min(max(0, s + n - base), len(p))
        v = c[b] - c[a]
        if v > bv:
            best, bv = s, v
    return best


def judge_audibility(E, a, b, att, long, ctx) -> dict:
    st = ctx["stems"]
    w60, w20 = int(0.06 * SR), int(0.02 * SR)
    hi60 = (b - w60) if long else att + int(0.3 * SR)
    s60 = best_window(E, a, att - int(0.005 * SR), max(att - int(0.005 * SR), hi60), w60)
    s20 = s60 if long else best_window(E, a, att - int(0.003 * SR), att + int(0.08 * SR), w20)
    bed = lambda s, n: _take(st["music"], s, n) + _take(st["amb"], s, n)
    rest = lambda s, n: _take(st["sfx"], s, n) - _take(E, s, n, a) + bed(s, n)
    e60, e20 = band_energy(_take(E, s60, w60, a)), band_energy(_take(E, s20, w20, a))
    vb, vbh = margin(e60, band_energy(bed(s60, w60)))
    va, _ = margin(e60, band_energy(rest(s60, w60)))
    f20, f20h = margin(e20, band_energy(rest(s20, w20)))
    rd = max(va, f20)
    verdict = "clear" if rd >= 6 else "ok" if rd >= 3 else "barely" if rd >= 0 else "masked"
    return {"vs_bed_db": vb, "vs_bed_hz": vbh, "vs_all_db": va, "first20_db": f20, "first20_hz": f20h, "read_db": rd,
            "verdict": verdict, "covered_by": "music/amb" if vb < 3 else "other effects"}


def _audibility_row(g: dict, ctx: dict) -> dict:
    row = {"t": g["t"], "label": g["label"], "kind": g["kind"], "n_layers": len(g["layers"]), "rules": g["rules"],
           "peak_db": g["peak_db"]}
    if g["qa_ok"]:
        row["qa_ok"] = g["qa_ok"]
    if g["silent"]:
        row.update(verdict="silent", samples=sorted({q["sample"] for q in g["layers"]}))
    elif ctx["no_bed"]:
        row["verdict"] = "not judged"
    else:
        row.update(judge_audibility(g["sig"], g["a"], g["b"], g["att"], g["long"], ctx))
    return row


def _audibility_status(silent: list, masked: list, no_bed: bool) -> str:
    if any(not r.get("qa_ok") for r in silent):
        return "fail"
    return "skipped" if no_bed else ("warn" if masked else "ok")


def _dead_layer(g: dict, L: dict, G: float, ctx: dict):
    """A layer that adds nothing to its group: silent, or masked by the rest. None when it reads."""
    if L["peak_db"] + G < -70:
        return {"t": g["t"], "rule": L["rule"], "sample": L["sample"], "read_db": None, "silent": True}
    r = None if ctx["no_bed"] else judge_audibility(L["sig"], L["a"], L["b"], L["att"], g["long"], ctx)
    if r is None or r["read_db"] >= 0:
        return None
    return {"t": g["t"], "rule": L["rule"], "sample": L["sample"], "read_db": r["read_db"]}


def _dead_layers(groups: list, G: float, ctx: dict) -> list[dict]:
    dead = []
    for g in (g for g in groups if not g["silent"] and len(g["layers"]) >= 2):
        dead += [x for x in (_dead_layer(g, L, G, ctx) for L in g["layers"]) if x is not None]
    return dead


def _partition(rows: list) -> tuple[list, list, list]:
    """(silent rows, judged rows quietest first, masked rows without qa_ok)."""
    silent = [r for r in rows if r["verdict"] == "silent"]
    judged = sorted([r for r in rows if "read_db" in r], key=lambda r: r["read_db"])
    return silent, judged, [r for r in judged if r["verdict"] == "masked" and not r.get("qa_ok")]


def section_audibility(ctx, groups, G, layers_flag) -> dict:
    rows = [_audibility_row(g, ctx) for g in groups]
    silent, judged, masked = _partition(rows)
    summ = (f"{len(silent)} silent, {len(masked)} masked of {len(rows)} event(s)" if not ctx["no_bed"]
            else f"no bed (solo render): {len(silent)} silent of {len(rows)} event(s), the rest not judged")
    out = {"status": _audibility_status(silent, masked, ctx["no_bed"]), "items": (silent + judged)[:40],
           "events": rows, "summary": summ}
    if layers_flag:
        out["dead_layers"] = _dead_layers(groups, G, ctx)
        out["summary"] += f"; {len(out['dead_layers'])} dead layer(s)"
    return out


# ---------------------------------------------------------------- harshness

def _band_peaks(g: dict, cx: dict):
    """(2-6 kHz power peak, 80-400 Hz power peak, centroid) over the attack window, or None when it is short."""
    pad = cx["pad"]
    m = np.concatenate([np.zeros(pad), g["sig"].mean(axis=1).astype(np.float64)])
    hi_b, lo_b = signal.sosfilt(cx["bp"], m)[pad:], signal.sosfilt(cx["lowp"], m)[pad:]
    a0 = max(0, g["att"] - g["a"] - pad)
    a1 = len(hi_b) if g["long"] else min(len(hi_b), g["att"] - g["a"] + int(0.08 * SR))
    if a1 - a0 < 240:
        return None
    ph = uniform_filter1d(hi_b[a0:a1] ** 2, 240).max()
    pl = uniform_filter1d(lo_b[a0:a1] ** 2, 240).max()
    seg = m[pad + a0:pad + a1]
    spec = np.abs(np.fft.rfft(seg * np.hanning(len(seg)))) ** 2
    f = np.fft.rfftfreq(len(seg), 1 / SR)
    return ph, pl, float((spec * f).sum() / max(spec.sum(), 1e-30))


def _vs_music(row: dict, g: dict, pres: float, cx: dict) -> None:
    c, r = g["att"], int(0.25 * SR)
    row["music_pres_db"] = round(dbp(float(np.mean(cx["mus"][max(0, c - r):c + r] ** 2))) + cx["G"], 2)
    row["over_music_max_db"] = round(pres - cx["ref"], 2)
    if pres - cx["ref"] > 6 and not g["qa_ok"]:
        row["issues"].append(f"2-6 kHz {pres - cx['ref']:.1f} dB above the music's loudest presence")


def _harsh_row(g: dict, cx: dict):
    bands = None if g["silent"] else _band_peaks(g, cx)
    if bands is None:
        return None
    ph, pl, cen = bands
    pres = dbp(float(ph)) + cx["G"]
    row = {"t": g["t"], "label": g["label"], "rules": g["rules"], "pres_db": round(pres, 2),
           "pres_minus_low_db": round(pres - dbp(float(pl)) - cx["G"], 2), "centroid_hz": round(cen),
           "music_pres_db": None, "over_music_max_db": None, "issues": [], "bright": cen > 6500}
    if cx["has_music"]:
        _vs_music(row, g, pres, cx)
    return row


def _harsh_result(rows: list, bright: list, has_music: bool, ref) -> dict:
    hot = [r for r in rows if r["issues"]]
    items = hot[:20] + [r for r in rows if not r["issues"]][: max(0, 10 - len(hot[:20]))]
    summ = (f"{len(hot)} event(s) over the music's 2-6 kHz peak by more than 6 dB" if has_music
            else "no music: the presence check is not judged") + f"; {len(bright)} bright event(s)"
    return {"status": "warn" if hot else "ok", "summary": summ, "items": items, "bright": bright, "music_ref_db": ref}


def section_harshness(ctx, groups, G) -> dict:
    bp = sos(4, [2000, 6000], "bandpass")
    mus = signal.sosfilt(bp, ctx["stems"]["music"].mean(axis=1).astype(np.float64))
    mp = uniform_filter1d(mus * mus, 240)
    has_music = mp.max() > 1e-10
    ref = dbp(float(mp.max())) + G if has_music else None
    cx = {"bp": bp, "lowp": sos(4, [80, 400], "bandpass"), "mus": mus, "has_music": has_music, "ref": ref, "G": G,
          "pad": int(0.01 * SR)}
    rows = [r for r in (_harsh_row(g, cx) for g in groups) if r is not None]
    bright = [{"t": r["t"], "label": r["label"], "centroid_hz": r["centroid_hz"]} for r in rows if r["bright"]]
    rows.sort(key=lambda r: -r["pres_db"])
    return _harsh_result(rows, bright, has_music, ref)


# ---------------------------------------------------------------- sync

def _build_issue(test: str, m: dict, s: float):
    if test == "end":
        v = m["end_ms"] - s
        return None if -300 <= v <= 60 else f"ends {v:.0f} ms from the hit (allowed -300..+60)"
    v = m["apex_ms"] - s
    return None if abs(v) <= 45 else f"apex {v:+.0f} ms off the cue (allowed +-45)"


def _impact_issue(m: dict, s: float):
    if m["attack_ms"] - s < -45:
        return f"sound before picture: attack {m['attack_ms'] - s:+.0f} ms"
    return f"lands late: peak {m['peak_ms'] - s:+.0f} ms" if m["peak_ms"] - s > 75 else None


def _sync_issue(test, m, s):
    return _build_issue(test, m, s) if test in ("end", "apex") else _impact_issue(m, s)


def _sync_test(g: dict):
    """end, apex or impact; None for a group aligned by its start or end."""
    L = g["loud"]
    if g["builds"]:
        return "apex" if L["align"] == "peak" else "end"
    if L["align"] in ("start", "end"):
        return None
    return "apex" if g["long"] else "impact"


def _sync_marks(g: dict, x: np.ndarray, cue: float) -> dict:
    nb = len(x) // 240
    pk = (int(np.argmax(x[: nb * 240].reshape(nb, 240).max(axis=1))) * 240 + 120) if nb else 0
    pw = uniform_filter1d((g["sig"].astype(np.float64) ** 2).sum(axis=1), 2400)
    return {"attack_ms": ms(g["loud"]["att"] - cue), "peak_ms": ms(g["a"] + pk - cue),
            "apex_ms": ms(g["a"] + int(np.argmax(pw)) - cue)}


def _explain(row: dict, g: dict, test: str, m: dict, issue):
    """Clear an issue that a rule offset or a qa_ok explains. Returns the issue left."""
    q = next((q for q in g["layers"] if abs(q["offset"]) >= 0.03 and _sync_issue(test, m, q["offset"] * 1000) is None),
             None)
    if q is not None:
        row.update(deliberate=True, offset_s=q["offset"], offset_rule=q["rule"],
                   explanation=f"sits {q['offset'] * 1000:+.0f} ms off the cue by the offset of rule {q['rule']}")
        return None
    if g["qa_ok"]:
        row.update(deliberate=True, explanation=f"acknowledged: {g['qa_ok']}")
        return None
    tried = [q["offset"] for q in g["layers"] if abs(q["offset"]) >= 0.03]
    if tried:
        row["offsets_tried_s"] = tried
    return issue


def _sync_row(g: dict):
    test = None if g["silent"] else _sync_test(g)
    if test is None:
        return None
    x = np.abs(g["sig"]).max(axis=1)
    cue = g["cue"] * SR
    m = _sync_marks(g, x, cue)
    row = {"t_cue": g["t"], "label": g["label"], "kind": g["kind"], "test": test, "align": g["loud"]["align"], **m,
           "long": g["long"], "issue": None}
    if test == "end":
        idx = np.nonzero(x > 0.1 * x.max())[0]
        row["end_ms"] = m["end_ms"] = ms(g["a"] + (idx[-1] if len(idx) else 0) - cue)
    issue = _sync_issue(test, m, 0)
    row["issue"] = _explain(row, g, test, m, issue) if issue else None
    return row


def section_sync(groups) -> dict:
    rows = [r for r in (_sync_row(g) for g in groups) if r is not None]
    issues = [r for r in rows if r["issue"]]
    deliberate = [r for r in rows if r.get("deliberate")]
    return {"status": "warn" if issues else "ok", "items": issues + deliberate, "events": rows,
            "summary": f"{len(issues)} sync issue(s) in {len(rows)} judged event(s), {len(deliberate)} deliberate "
                       "(check the deliberate ones on the frames)"}


# ---------------------------------------------------------------- density, loudness, coverage

def section_density(ctx, groups) -> dict:
    dur = len(ctx["stems"]["sfx"]) / SR
    live = [g for g in groups if not g["silent"]]
    per = [0] * max(1, math.ceil(dur))
    for g in live:
        s = int(g["att"] // SR)
        if 0 <= s < len(per):
            per[s] += 1
    busiest = sorted(({"s": i, "n": n} for i, n in enumerate(per) if n), key=lambda r: (-r["n"], r["s"]))[:5]
    pls = ctx["report"]["placements"]
    used = Counter(p["sample"] for p in pls).most_common(8)
    rules, mg = [], []
    by = {}
    for p in pls:
        if p["kind"] != "place":
            by.setdefault(str(p["rule"]), []).append(p)
    for r, ps in by.items():
        if len(ps) < 4:
            continue
        ts = sorted(p["t"] for p in ps)
        ioi = np.diff(ts)
        row = {"rule": r, "n": len(ps), "distinct_samples": len({p["sample"] for p in ps}),
               "ioi_s": round(float(ioi.mean()), 4), "ioi_cv": round(float(ioi.std() / ioi.mean()), 3) if ioi.mean() > 0 else None,
               "gain_sd_db": round(float(np.std([p["gain_db"] for p in ps])), 3),
               "pitch_sd_st": round(float(np.std([p["pitch"] for p in ps])), 3)}
        rules.append(row)
        if row["distinct_samples"] == 1 and row["pitch_sd_st"] < 0.1 and row["gain_sd_db"] < 0.5:
            mg.append(r)
    p10 = len(live) / dur * 10 if dur else 0
    return {"status": "warn" if mg else "ok", "events_per_s": per, "busiest_s": busiest,
            "most_used_samples": [{"sample": s, "n": n} for s, n in used], "rules": rules, "machine_gun": mg,
            "summary": f"{len(live)} event(s), {p10:.1f} per 10 s (norm about 5); busiest seconds "
                       + ", ".join(f"{b['s']}s x{b['n']}" for b in busiest[:3]) + (f"; machine gun: {', '.join(mg)}" if mg else "")}


def section_loudness(ctx, target, pre_lufs) -> dict:
    x = ctx["master"]
    if x is None:
        return {"status": "skipped", "summary": "no master"}
    if len(x) < int(0.4 * SR):
        return {"status": "skipped", "summary": "the master is shorter than 400 ms"}
    if not np.any(x):
        return {"status": "fail", "summary": "the master is all zeros"}
    kw = mst.kweight(mst.as_stereo(x))
    mom, mt = mst.block_loudness(x, 0.4, 0.1, kw)
    st, _ = mst.block_loudness(x, 3.0, 1.0, kw)
    i = int(np.argmax(mom))
    dur = len(x) / SR
    r = {"lufs": round(mst.integrated(x, kw), 2), "target": target, "true_peak_dbtp": round(mst.true_peak(x), 2),
         "lra_lu": round(mst.lra(x, kw), 2), "premaster_lufs": pre_lufs, "loudest_momentary": round(float(mom[i]), 2),
         "loudest_at": round(float(mt[i]), 2), "loudest_frac": round(float(mt[i]) / dur, 3),
         "short_term_per_s": [round(float(v), 2) for v in st],
         "head_5ms_peak_dbfs": round(mst.db(float(np.abs(x[:240]).max())), 2),
         "tail_50ms_rms_dbfs": round(_rms_db(x[-2400:]), 2), "tail_10ms_peak_dbfs": round(mst.db(float(np.abs(x[-480:]).max())), 2)}
    fps = float(ctx["cues"].get("fps", 60))
    acts = {}
    for name, a in (ctx["cues"].get("acts") or {}).items():
        lo, hi = float(a["from"]) / fps, (float(a["from"]) + float(a["dur"])) / fps
        sel = mom[(mt >= lo) & (mt < hi)]
        acts[name] = {"from": round(lo, 3), "momentary_median": round(float(np.median(sel)), 2) if len(sel) else None,
                      "momentary_max": round(float(sel.max()), 2) if len(sel) else None}
    r["acts"] = acts
    fails, warns = [], []
    if not math.isfinite(r["lufs"]) or abs(r["lufs"] - target) > 0.5:
        fails.append(f"integrated {r['lufs']} LUFS is off the target {target} by more than 0.5 LU")
    if r["true_peak_dbtp"] > -1.0:
        fails.append(f"true peak {r['true_peak_dbtp']} dBTP > -1.0 (AAC would clip)")
    if r["tail_50ms_rms_dbfs"] > -45 or r["tail_10ms_peak_dbfs"] > -60:
        fails.append("the end is not silent")
    if r["head_5ms_peak_dbfs"] > -40:
        warns.append("the first 5 ms are loud: starts mid-sound or clicks on sample 0")
    if r["lra_lu"] < 4:
        warns.append(f"LRA {r['lra_lu']} LU: flat")
    if r["lra_lu"] > 10:
        warns.append(f"LRA {r['lra_lu']} LU: quiet passages vanish on phones")
    if r["loudest_frac"] > 0.85:
        warns.append("the loudest moment is in the last 15 %: the peak belongs on the drop")
    r.update(fails=fails, warnings=warns, status="fail" if fails else "warn" if warns else "ok",
             summary=f"{r['lufs']} LUFS, TP {r['true_peak_dbtp']} dBTP, LRA {r['lra_lu']} LU, loudest {r['loudest_momentary']} "
                     f"LUFS-M at {tfmt(r['loudest_at'])}" + ("; " + "; ".join(fails + warns) if fails or warns else ""))
    return r


def section_coverage(ctx, groups) -> dict:
    fps = float(ctx["cues"].get("fps", 60))
    none = {(round(e["t"], 4), e.get("kind"), e.get("id")) for e in ctx["report"].get("unsounded_events", [])}
    placed_silent = []
    for g in groups or []:
        if g["silent"] and not g["qa_ok"] and not g["label"].startswith("place:"):
            placed_silent.append({"t": g["t"], "kind": g["kind"], "id": g["label"]})
            none.add((round(g["t"], 4), g["kind"], g["label"]))
    cls = lambda kind, i: (kind, re.sub(r"(:\d+)+$", "", str(i or "")))
    total, missing = Counter(), Counter()
    for e in ctx["cues"].get("events", []):
        c = cls(e.get("kind"), e.get("id"))
        total[c] += 1
        if (round(float(e["f"]) / fps, 4), e.get("kind"), e.get("id")) in none or any(
                e.get("id") == p["id"] and e.get("kind") == p["kind"] for p in placed_silent):
            missing[c] += 1
    partial = [{"kind": k, "class": c, "unsounded": missing[(k, c)], "of": total[(k, c)]}
               for (k, c) in total if 0 < missing[(k, c)] < total[(k, c)]]
    never = [{"kind": k, "class": c, "n": total[(k, c)]} for (k, c) in total if missing[(k, c)] == total[(k, c)]]
    summ = f"{len(partial)} partial class(es), {len(never)} class(es) never sounded, {len(placed_silent)} placed but silent"
    if ctx["solo"]:
        summ += "; solo render: events of other fragments are expected to have no sound"
    return {"status": "warn" if partial or placed_silent else "ok", "summary": summ, "partial": partial,
            "unsounded_classes": never, "placed_but_silent": placed_silent}


def _load_api():
    import importlib
    return importlib.import_module("hark.core.api")  # the flat API that `import hark` forwards to


def import_hark():
    """The hark API, from the venv or from the sibling skill's engine folder; None when absent."""
    try:
        return _load_api()
    except ImportError:
        pass
    eng = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "../../hark/engine"))
    if not os.path.isdir(eng):
        return None
    if eng not in sys.path:
        sys.path.insert(0, eng)
    for name in [m for m in sys.modules if m == "hark" or m.startswith("hark.")]:
        del sys.modules[name]
    try:
        return _load_api()
    except ImportError:
        return None


def section_listen(ctx, a, outdir) -> dict:
    api = import_hark()
    if api is None:
        return {"status": "skipped", "summary": "hark cannot be imported: run QA without --listen and call the hark "
                "launcher (skills/hark/scripts/hark timeline ...) directly, or repair the venv"}
    res, notes = {}, []
    mp = None if a.master == "none" else mix.rpath(ctx["root"], a.master or "out/audio/soundtrack.wav")
    video = mix.rpath(ctx["root"], a.video) if a.video else None
    cs = mix.rpath(ctx["root"], a.cues_sounded)
    if a.listen and mp and os.path.isfile(mp):
        try:
            tl = api.timeline(mp, video=video, cues=cs if os.path.isfile(cs) else None)
            txt = tl.get("text", "")
            with open(os.path.join(outdir, "qa_timeline.txt"), "w") as fh:
                fh.write(txt)
            s = tl.get("summary", "")
            res["timeline"] = s if len(s) <= 400 else s[:360] + " ... (see qa_timeline.txt)"
            try:
                api.sheet(mp, os.path.join(outdir, "qa_sheet.png"), video=video, cues=cs if os.path.isfile(cs) else None, tl=tl)
                res["sheet"] = os.path.join(outdir, "qa_sheet.png")
            except Exception as e:  # noqa: BLE001
                notes.append(f"sheet failed: {e}")
        except Exception as e:  # noqa: BLE001
            notes.append(f"timeline failed: {e}")
        try:
            cr = api.critique(mp, video=video, use_ast=False, use_realness=False)
            res["critique"] = {"summary": cr.get("summary"), "verdict": cr.get("verdict"), "score": cr.get("score"),
                               "actions": cr.get("actions"),
                               "metrics": [m for m in cr.get("metrics", []) if m.get("status") in ("low", "high")]}
        except Exception as e:  # noqa: BLE001
            notes.append(f"critique failed: {e}")
    not_real = []
    if a.realness:
        rows = []
        for f in ctx["report"].get("files_used", []):
            try:
                r = api.realness(mix.rpath(ctx["root"], f))
                rows.append({"file": f, "p_real": r.get("p_real"), "verdict": r.get("verdict")})
            except Exception as e:  # noqa: BLE001
                rows.append({"file": f, "p_real": None, "verdict": None})
                notes.append(f"realness failed on {f}: {e}")
        rows.sort(key=lambda r: (r["p_real"] is None, r["p_real"] or 0))
        not_real = [r["file"] for r in rows if (r["p_real"] is not None and r["p_real"] < 0.5)
                    or (r["p_real"] is None and "synth" in str(r["verdict"] or ""))]
        res["realness"] = rows
        res["not_real"] = not_real
    status = "warn" if not_real or notes else "ok"
    return {"status": status, "summary": f"{len(not_real)} recording(s) not real; {len(notes)} failed call(s)", "notes": notes, **res}


# ---------------------------------------------------------------- main

def analyse(a) -> dict:
    ctx = load_inputs(a)
    st = ctx["stems"]
    ctx["no_bed"] = not np.any(st["music"]) and not np.any(st["amb"])
    target = float((ctx["design"].get("master") or {}).get("lufs", -14))
    total = st["sfx"] + st["amb"] + st["music"]
    pre = mst.integrated(total) if len(total) >= int(0.4 * SR) else float("-inf")
    mas = mst.integrated(ctx["master"]) if ctx["master"] is not None and len(ctx["master"]) >= int(0.4 * SR) else float("-inf")
    scaled = math.isfinite(pre) and math.isfinite(mas)
    G = mas - pre if scaled else 0.0
    sections, notes = {}, []
    layers = groups = None
    if ctx["trace"] is not None:
        layers, n = rebuild(ctx)
        notes += n
        if layers is not None:
            groups = group(layers, G)
    sections["clicks"] = section_clicks(ctx, layers, G)
    sections["loudness"] = section_loudness(ctx, target, round(pre, 2) if math.isfinite(pre) else None)
    sections["coverage"] = section_coverage(ctx, groups)
    skip = {"status": "skipped", "summary": "no trace: run the mix with --trace" if ctx["trace"] is None else
            "the trace and the report disagree: run the mix again with --trace"}
    if ctx["trace"] is not None:
        sections["edges"] = section_edges(ctx)
    else:
        sections["edges"] = dict(skip)
    for name, fn in (("audibility", lambda: section_audibility(ctx, groups, G, a.layers)),
                     ("harshness", lambda: section_harshness(ctx, groups, G)),
                     ("sync", lambda: section_sync(groups)), ("density", lambda: section_density(ctx, groups))):
        sections[name] = fn() if groups is not None else dict(skip)
    outdir = os.path.dirname(mix.rpath(ctx["root"], a.out)) or "."
    os.makedirs(outdir, exist_ok=True)
    if a.listen or a.realness:
        sections["listen"] = section_listen(ctx, a, outdir)
    else:
        sections["listen"] = {"status": "skipped", "summary": "not requested (--listen, --realness)"}
    if all(not np.any(x) for x in st.values()):
        sections["mix"] = {"status": "fail", "summary": "every stem is silent: check the trims (--dump), gains and cue matches"}
    worst = max((s["status"] for s in sections.values()), key=lambda s: ORDER[s])
    fails = [k for k, s in sections.items() if s["status"] == "fail"]
    warns = [k for k, s in sections.items() if s["status"] == "warn"]
    skipped = [k for k, s in sections.items() if s["status"] == "skipped"]
    scale = f"levels at master scale (master {G:+.2f} dB over the stems)" if scaled and ctx["master"] is not None else "levels at stem scale"
    summary = (f"QA {worst.upper()}: fail {', '.join(fails) or 'none'}; warn {', '.join(warns) or 'none'}; "
               f"not run {', '.join(skipped) or 'none'}; {scale}")
    return {"summary": summary, "status": worst, "note": "; ".join(notes) or None, "sections": sections,
            "inputs": ctx["paths"]}


def digest(res: dict) -> str:
    out = [res["summary"]]
    if res.get("note"):
        out.append("note: " + res["note"])
    for name, s in res["sections"].items():
        out.append(f"{name.upper():11s} {s['status'].upper():7s} {s.get('summary', '')}")
        rows = []
        if name == "clicks":
            rows = [f"{c['in']} {tfmt(c['t'])} {c['level_db']} dB " + (f"{c['owners'][0]['what']} ({c['owners'][0]['where']})" if c["owners"] else "no owner") for c in s.get("items", [])[:8]]
        elif name == "edges":
            rows = [f"{i.get('sample') or i.get('slices')}: {'; '.join(i['defects'])}" for i in s.get("items", []) if i.get("defects")][:8]
            rows += [f"music {r['label']}: {'; '.join(r['defects'])}" for r in s.get("music_splices", []) if r["defects"]][:4]
        elif name == "audibility":
            rows = [f"{tfmt(r['t'])} {r['label']}: {r['verdict']}" + (f" read {r['read_db']} dB ({r['covered_by']})" if "read_db" in r else "") for r in s.get("items", [])[:6]]
        elif name == "harshness":
            rows = [f"{tfmt(r['t'])} {r['label']}: {'; '.join(r['issues'])}" for r in s.get("items", []) if r["issues"]][:5]
        elif name == "sync":
            rows = [f"{tfmt(r['t_cue'])} {r['label']}: {r['issue'] or r.get('explanation')}" for r in s.get("items", [])[:8]]
        out += ["    " + r for r in rows]
    return "\n".join(out)


def parse(argv):
    p = argparse.ArgumentParser(prog="qa.py", description="Measured QA of a render.")
    p.add_argument("--root", default=".")
    p.add_argument("--design", default="audio/sounddesign.json")
    p.add_argument("--report")
    p.add_argument("--trace", default="out/qa/adds.json")
    p.add_argument("--stems")
    p.add_argument("--master")
    p.add_argument("--cues")
    p.add_argument("--cues-sounded", default="out/qa/cues_sounded.json")
    p.add_argument("--only")
    p.add_argument("--cache")
    p.add_argument("--video")
    p.add_argument("--out", default="out/qa/qa.json")
    p.add_argument("--layers", action="store_true")
    p.add_argument("--listen", action="store_true")
    p.add_argument("--realness", action="store_true")
    p.add_argument("--json", action="store_true")
    p.add_argument("--self-test", action="store_true")
    return p.parse_args(argv)


def main(argv=None) -> int:
    a = parse(sys.argv[1:] if argv is None else argv)
    if a.self_test:
        return self_test()
    try:
        res = clean(analyse(a))
    except InputError as e:
        mix.ascii_print(f"qa.py: {e}", file=sys.stderr)
        return 2
    except mix.DesignError as e:
        mix.ascii_print(f"qa.py: {e}", file=sys.stderr)
        return 2
    out = mix.rpath(os.path.abspath(a.root), a.out)
    with open(out, "w") as fh:
        json.dump(res, fh, indent=1)
    mix.ascii_print(json.dumps(res, indent=1) if a.json else digest(res))
    return 1 if res["status"] == "fail" else 0


def self_test() -> int:
    import tempfile
    import fixtures as fx
    ck = fx.Checker()
    py = lambda *args: mix.main(list(args))
    with tempfile.TemporaryDirectory() as d:
        bed = fx.noise_bed(3.0, 0.002, 4)  # a quiet bed: its 14 kHz noise must not hide the click
        fx.write(os.path.join(d, "bed.wav"), bed)
        t = np.arange(int(0.2 * SR)) / SR
        tone = 0.5 * np.sin(2 * np.pi * 300 * t + 1.0) * np.exp(-8 * t)  # starts on a non-zero sample
        fx.write(os.path.join(d, "x.wav"), np.stack([tone, tone], 1), subtype="FLOAT")
        fx.put_json(os.path.join(d, "c.json"), {"fps": 60, "events": [{"f": 60, "kind": "hit", "id": "q:h:0"}]})

        def design(fade_in, offset=0.0):
            return {"duration": 3.0, "samples": {"q.x": {"file": "x.wav", "fade_in": fade_in}}, "pools": {"q.x": ["q.x"]},
                    "rules": [{"name": "q.r", "pool": "q.x", "align": "start", "offset": offset}],
                    "ambience": [{"name": "q.bed", "file": "bed.wav", "at": 0, "until": 3, "fade_in": 0.2, "fade_out": 0.2}]}
        mixargs = ["--root", d, "--cues", "c.json", "--trace", "out/qa/adds.json", "--cache", "cache"]
        fx.put_json(os.path.join(d, "d.json"), design(0))
        py(*mixargs, "--design", "d.json")
        code = main(["--root", d, "--design", "d.json", "--cues", "c.json", "--cache", "cache"])
        q = mix.load_json(os.path.join(d, "out/qa/qa.json"))
        cl = q["sections"]["clicks"]
        edge = [c for c in cl["items"] if c["in"] == "master" and c["at_edge"]]
        ck("Q1 a master click at a placement start fails QA",
           edge and edge[0]["owners"][0]["where"] == "start" and cl["status"] == "fail" and code == 1)
        fx.put_json(os.path.join(d, "d.json"), design(0.004))
        py(*mixargs, "--design", "d.json")
        main(["--root", d, "--design", "d.json", "--cues", "c.json", "--cache", "cache"])
        q = mix.load_json(os.path.join(d, "out/qa/qa.json"))
        ck("Q2 a 4 ms fade-in leaves no edge click", q["sections"]["clicks"]["status"] != "fail"
           and not any(c["at_edge"] and c["in"] == "master" for c in q["sections"]["clicks"]["items"]))
        # Q3: an effect 10 dB under a broadband bed
        loud = fx.noise_bed(3.0, 0.2, 8)
        fx.write(os.path.join(d, "loud.wav"), loud)
        # the effect is the bed's own noise 10 dB down, so it is 10 dB under the bed in every band
        nb = loud[SR:SR + int(0.15 * SR)] * 10 ** (-10 / 20)
        fx.write(os.path.join(d, "soft.wav"), nb)
        fx.put_json(os.path.join(d, "d3.json"), {
            "duration": 3.0, "samples": {"q.s": {"file": "soft.wav"}}, "pools": {"q.s": ["q.s"]},
            "rules": [{"name": "q.s", "pool": "q.s"}],
            "ambience": [{"name": "q.loud", "file": "loud.wav", "at": 0, "until": 3, "fade_in": 0.1, "fade_out": 0.1}]})
        py(*mixargs, "--design", "d3.json", "--no-master")
        main(["--root", d, "--design", "d3.json", "--cues", "c.json", "--master", "none", "--cache", "cache"])
        au = mix.load_json(os.path.join(d, "out/qa/qa.json"))["sections"]["audibility"]
        ev = au["events"][0]
        ck("Q3 masked under the bed", ev["verdict"] == "masked" and ev["covered_by"] == "music/amb" and au["status"] == "warn")
        # Q4: a rule offset of 0.2 s on an impact is deliberate
        fx.write(os.path.join(d, "hit.wav"), fx.burst(0.1, 200, 40, 3))
        fx.put_json(os.path.join(d, "d4.json"), {
            "duration": 3.0, "samples": {"q.h": {"file": "hit.wav"}}, "pools": {"q.h": ["q.h"]},
            "rules": [{"name": "q.late", "pool": "q.h", "offset": 0.2}],
            "ambience": [{"name": "q.bed", "file": "bed.wav", "at": 0, "until": 3}]})
        py(*mixargs, "--design", "d4.json", "--no-master")
        main(["--root", d, "--design", "d4.json", "--cues", "c.json", "--master", "none", "--cache", "cache"])
        sy = mix.load_json(os.path.join(d, "out/qa/qa.json"))["sections"]["sync"]
        ck("Q4 the offset is deliberate", sy["events"][0].get("deliberate") and sy["events"][0]["offset_rule"] == "q.late"
           and not sy["events"][0]["issue"])
        # Q5, Q6: machine gun and partial coverage
        evs = [{"f": 20 + 15 * k, "kind": "tick", "id": f"a:k:{k}"} for k in range(4)]
        evs += [{"f": 120 + 10 * k, "kind": "tap", "id": f"b:t:{k}"} for k in range(6)]
        fx.put_json(os.path.join(d, "c5.json"), {"fps": 60, "events": evs})
        fx.put_json(os.path.join(d, "d5.json"), {
            "duration": 3.0, "samples": {"q.h": {"file": "hit.wav"}}, "pools": {"q.h": ["q.h"]},
            "rules": [{"name": "q.mg", "pool": "q.h", "match": {"kind": "tap"}},
                      {"name": "q.part", "pool": "q.h", "match": {"id": "a:k:[01]$"}}]})
        py("--root", d, "--cues", "c5.json", "--trace", "out/qa/adds.json", "--cache", "cache", "--design", "d5.json", "--no-master")
        main(["--root", d, "--design", "d5.json", "--cues", "c5.json", "--master", "none", "--cache", "cache"])
        q = mix.load_json(os.path.join(d, "out/qa/qa.json"))["sections"]
        ck("Q5 machine gun", "q.mg" in q["density"]["machine_gun"] and q["density"]["status"] == "warn")
        part = q["coverage"]["partial"]
        ck("Q6 partial class a:k with 2 of 4 unsounded", any(p["class"] == "a:k" and p["unsounded"] == 2 and p["of"] == 4 for p in part)
           and q["coverage"]["status"] == "warn")
        # Q7: no trace
        os.remove(os.path.join(d, "out/qa/adds.json"))
        main(["--root", d, "--design", "d5.json", "--cues", "c5.json", "--master", "none", "--cache", "cache"])
        q = mix.load_json(os.path.join(d, "out/qa/qa.json"))["sections"]
        ck("Q7 trace sections skipped, the rest ran",
           all(q[s]["status"] == "skipped" for s in ("edges", "audibility", "harshness", "sync", "density"))
           and q["clicks"]["status"] != "skipped" and q["coverage"]["status"] != "skipped")
        ck("Q8 missing stems exit 2", main(["--root", d, "--design", "d5.json", "--stems", "nope"]) == 2)
        # Q9: a trim past the end of its file
        fx.put_json(os.path.join(d, "d9.json"), {
            "duration": 3.0, "samples": {"q.e": {"file": "hit.wav", "start": 5.0}}, "pools": {"q.e": ["q.e"]},
            "rules": [{"name": "q.e", "pool": "q.e"}],
            "ambience": [{"name": "q.bed", "file": "bed.wav", "at": 0, "until": 3}]})
        py(*mixargs, "--design", "d9.json", "--no-master")
        code = main(["--root", d, "--design", "d9.json", "--cues", "c.json", "--master", "none", "--cache", "cache"])
        q = mix.load_json(os.path.join(d, "out/qa/qa.json"))["sections"]
        ck("Q9 empty trim: edges issue, audibility silent, fail",
           "empty" in " ".join(q["edges"]["items"][0]["defects"]) and q["audibility"]["events"][0]["verdict"] == "silent"
           and q["audibility"]["status"] == "fail" and code == 1)
        # Q10: a music segment that ends inside a held chord, then silence
        t = np.arange(SR * 6) / SR
        chord = 0.3 * (np.sin(2 * np.pi * 220 * t) + np.sin(2 * np.pi * 330 * t))
        chord[4 * SR:] *= 0.0005
        fx.write(os.path.join(d, "chord.wav"), np.stack([chord, chord], 1))
        for ok_reason in (None, "a hard cut into the breakdown"):
            seg = {"label": "bars 1-2", "src_from": 0.0, "src_to": 2.0, "at": 0.0}
            seg2 = {"label": "breakdown", "src_from": 4.0, "src_to": 5.0, "at": 2.0}
            if ok_reason:
                seg["qa_ok"] = ok_reason
            fx.put_json(os.path.join(d, "d10.json"), {"duration": 4.0, "music": {
                "file": "chord.wav", "src_bpm": 120, "stretch": "none", "segments": [seg, seg2]}})
            py("--root", d, "--trace", "out/qa/adds.json", "--cache", "cache", "--design", "d10.json", "--no-master")
            main(["--root", d, "--design", "d10.json", "--master", "none", "--cache", "cache"])
            sp = mix.load_json(os.path.join(d, "out/qa/qa.json"))["sections"]["edges"]["music_splices"][0]
            if ok_reason:
                ck("Q10 qa_ok turns the chop into a note", not sp["defects"] and any("acknowledged" in n for n in sp["notes"]))
            else:
                ck("Q10 the splice chops the music", any("chops the music" in m for m in sp["defects"]))
        # W2/W3-like: the full fixture project, every section present
        p = fx.project(os.path.join(d, "film"))
        py("--root", p["root"], "--trace", "out/qa/adds.json")
        code = main(["--root", p["root"], "--layers"])
        q = mix.load_json(os.path.join(p["root"], "out/qa/qa.json"))
        need = {"clicks", "loudness", "coverage", "edges", "audibility", "harshness", "sync", "density", "listen"}
        ck("W3 every section present, only listen skipped",
           need <= set(q["sections"]) and code in (0, 1)
           and [k for k, s in q["sections"].items() if s["status"] == "skipped"] == ["listen"])
    return ck.code()


if __name__ == "__main__":
    sys.exit(main())
