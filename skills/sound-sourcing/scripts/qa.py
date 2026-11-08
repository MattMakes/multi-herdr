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


def tfmt(tsec: float | None) -> str:
    if tsec is None:
        return "n.a."
    return f"{tsec:.2f}s ({int(tsec // 60)}:{tsec % 60:05.2f})"


def dbp(pw: float) -> float:
    return 10 * math.log10(pw) if pw > 0 else -200.0


def _clean_scalar(opts):
    if isinstance(opts, np.generic):
        opts = opts.item()
    return None if isinstance(opts, float) and not math.isfinite(opts) else opts


def clean(opts):
    if isinstance(opts, dict):
        return {str(key_): clean(value) for key_, value in opts.items()}
    if isinstance(opts, (list, tuple)):
        return [clean(value) for value in opts]
    return _clean_scalar(opts)


def sos(order, hz, kind):
    return signal.butter(order, hz, btype=kind, fs=SR, output="sos")


# ---------------------------------------------------------------- inputs

def read(src_path: str) -> np.ndarray:
    import soundfile as sf
    return mst.as_stereo(sf.read(src_path, dtype="float32", always_2d=True)[0]).astype(np.float32)


def _mix_paths(cli, rp) -> tuple[str, str]:
    """(report path, stems folder); a solo QA (--only) reads the solo render's files."""
    if cli.only is None:
        return rp(cli.report or "out/qa/mix_report.json"), rp(cli.stems or "out/audio/stems")
    tag = mix.safe(cli.only, "A-Za-z0-9._-")
    return (rp(cli.report or f"out/audio-review/solo/{tag}_report.json"),
            rp(cli.stems or f"out/audio-review/solo/{tag}_stems"))


def _read_stems(stems_p: str) -> dict:
    stems = {}
    for lane in mix.BUSES:
        fpath = os.path.join(stems_p, f"{lane}.wav")
        if not os.path.isfile(fpath):
            raise InputError(f"stems not found: {fpath} (--stems)")
        stems[lane] = read(fpath)
    if len({len(sig) for sig in stems.values()}) != 1:
        raise InputError(f"stems of one length not found: {stems_p} (--stems)")
    return stems


def _master_path(cli, rp, solo: bool):
    if cli.master:
        return rp(cli.master)
    return None if solo else rp("out/audio/soundtrack.wav")


def _fit_master(master: np.ndarray, count: int, mp: str) -> np.ndarray:
    if abs(len(master) - count) > 1:
        raise InputError(f"master of the stems' length not found: {mp} (--master)")
    return np.pad(master, ((0, max(0, count - len(master))), (0, 0)))[:count]


def _read_master(cli, rp, solo: bool, count: int):
    mp = None if cli.master == "none" else _master_path(cli, rp, solo)
    if mp and os.path.isfile(mp):
        return _fit_master(read(mp), count, mp)
    if cli.master and cli.master != "none":
        raise InputError(f"master not found: {mp} (--master)")
    return None


def _side_inputs(cli, design: dict, root: str, rp) -> dict:
    """Cues, trace and cache, with their paths."""
    cp = mix.cue_path(cli.cues, design, root)
    tp = rp(cli.trace)
    trace = mix.load_json(tp) if os.path.isfile(tp) else None
    return {"cues": mix.load_cues(cp) if os.path.isfile(cp) else {"fps": 60, "events": []}, "cues_path": cp,
            "trace": trace, "trace_path": tp if trace else None,
            "cache": rp(cli.cache) if cli.cache else os.path.join(root, "out/audio/cache")}


def load_inputs(cli) -> dict:
    root = os.path.abspath(cli.root)
    rp = lambda pw: mix.rpath(root, pw)
    dpath = rp(cli.design)
    if not os.path.isfile(dpath):
        raise InputError(f"design not found: {dpath} (--design)")
    design = mix.load_design(cli.design, root, cli.only)
    report_p, stems_p = _mix_paths(cli, rp)
    if not os.path.isfile(report_p):
        raise InputError(f"report not found: {report_p} (--report)")
    stems = _read_stems(stems_p)
    solo = cli.only is not None
    side = _side_inputs(cli, design, root, rp)
    return {"root": root, "design": design, "report": mix.load_json(report_p), "stems": stems,
            "master": _read_master(cli, rp, solo, len(stems["sfx"])),
            "cues": side["cues"], "trace": side["trace"], "cache": side["cache"], "solo": solo,
            "paths": {"design": dpath, "report": report_p, "stems": stems_p, "trace": side["trace_path"],
                      "cues": side["cues_path"], "master": cli.master}}


# ---------------------------------------------------------------- layers and groups

def _layer_meta(pl: dict, rules: dict, places) -> dict:
    """pitch, offset, align, cue time, rule name and qa_ok of 1 placement."""
    if pl["kind"] == "place":
        item = next(places, {})
        return {"pitch": float(item.get("pitch", 0)), "off": float(item.get("offset", 0)),
                "align": item.get("align", "attack"), "cue": float(item.get("at", pl["t"])), "rule": "place",
                "qok": item.get("qa_ok")}
    rec = rules.get(str(pl["rule"]), {})
    off = float(rec.get("offset", 0))
    return {"pitch": float(pl.get("pitch", 0)), "off": off, "align": pl.get("align", "attack"),
            "cue": pl["t"] - off, "rule": str(pl["rule"]), "qok": rec.get("qa_ok")}


def _get(lib, label: str, pitch: float):
    try:
        return lib.get(label, pitch)
    except mix.DesignError:
        return None


def _layer_span(sig: np.ndarray, rec: dict, dsg: dict, N: int):
    """(a, b, signal in the film, attack sample), or None for a sound wholly outside the film."""
    val = rec["start"]
    lo_i, hi_i = max(0, val), min(N, val + len(sig))
    if not len(sig):
        # an empty trim stays as a silent layer, so audibility can name it
        lo_i = min(max(0, val), N)
        return lo_i, lo_i, np.zeros((0, 2), np.float32), lo_i
    if lo_i >= hi_i:
        return None
    gl, gr = mix.pan_gains(rec["pan"])
    wet = sig[lo_i - val:hi_i - val].astype(np.float32) * np.float32(rec["gain"])
    wet = wet * np.array([gl, gr], np.float32) * mix.bus_curve(dsg, "sfx", lo_i, hi_i - lo_i)[:, None]
    return lo_i, hi_i, wet, val + mix.align_index(sig, "attack")


_BAD = "bad"


def _layer(state: dict, rec: dict, pl: dict):
    """The rebuilt sfx layer of 1 trace record, None when it is not an sfx layer in the film, or _BAD."""
    meas = _layer_meta(pl, state["rules"], state["places"])
    sig = _get(state["lib"], pl["sample"], meas["pitch"])
    if sig is None or len(sig) != rec["n"]:
        return _BAD
    span = _layer_span(sig, rec, state["design"], state["N"]) if rec["bus"] == "sfx" else None
    if span is None:
        return None
    lo_i, hi_i, wet, att = span
    return {"sample": pl["sample"], "rule": meas["rule"], "kind": pl["kind"], "event": pl.get("event"),
            "cue": meas["cue"], "offset": meas["off"], "align": meas["align"], "start": rec["start"], "n": len(sig), "a": lo_i,
            "b": hi_i, "sig": wet, "qa_ok": meas["qok"], "att": att, "peak_db": mst.db(_peak(wet))}


def rebuild(job: dict) -> tuple[list, list[str]]:
    dsg, rep, trace = job["design"], job["report"], job["trace"]
    recs = [rec for rec in trace if "sample" in rec]
    pls = rep["placements"]
    if len(recs) != len(pls):
        return None, [f"the trace ({len(recs)} sample adds) and the report ({len(pls)} placements) disagree: "
                      "run the mix again with --trace"]
    lib = mix.Library(dsg, job["root"], job["cache"])
    job["lib"] = lib
    state = {"lib": lib, "design": dsg, "N": len(job["stems"]["sfx"]), "places": iter(dsg["place"]),
          "rules": {mix.rule_key(rec, ix): rec for ix, rec in enumerate(dsg["rules"])}}
    return _sort_built([_layer(state, rec, pl) for rec, pl in zip(recs, pls)])


def _sort_built(built: list) -> tuple[list, list[str]]:
    """(the layers, a note on the placements that could not be rebuilt)."""
    bad = sum(1 for lay in built if lay is _BAD)
    remarks = [f"{bad} placement(s) could not be rebuilt: the design changed after the mix"] if bad else []
    return [lay for lay in built if isinstance(lay, dict)], remarks


def _peak(sig: np.ndarray) -> float:
    return float(np.max(np.abs(sig))) if sig.size else 0.0


def _free_group(clusters: list, lay: dict) -> dict:
    """A hand placement joins a free group that started up to 140 ms earlier, or starts one."""
    grp = next((cum for cum in reversed(clusters[-12:])
              if cum["free"] and lay["att"] - cum["layers"][0]["att"] <= int(0.14 * SR)), None)
    if grp is None:
        grp = {"free": True, "layers": []}
        clusters.append(grp)
    return grp


def _cue_group(clusters: list, index: dict, lay: dict) -> dict:
    key_ = (lay["event"], round(lay["cue"], 4))
    if key_ not in index:
        index[key_] = {"free": False, "layers": []}
        clusters.append(index[key_])
    return index[key_]


def _bucket(layers: list) -> list[dict]:
    clusters, index = [], {}
    for lay in sorted(layers, key=lambda lyr: lyr["att"]):
        grp = _free_group(clusters, lay) if lay["kind"] == "place" else _cue_group(clusters, index, lay)
        grp["layers"].append(lay)
    return clusters


def _sum_layers(ls: list) -> tuple[int, int, np.ndarray]:
    lo_i, hi_i = min(lyr["a"] for lyr in ls), max(lyr["b"] for lyr in ls)
    sig = np.zeros((hi_i - lo_i, 2), np.float32)
    for lyr in ls:
        sig[lyr["a"] - lo_i:lyr["b"] - lo_i] += lyr["sig"]
    return lo_i, hi_i, sig


def _flags(ls: list) -> dict:
    return {"long": any(lyr["kind"] in LONG_KINDS or lyr["align"] == "peak" for lyr in ls),
            "builds": any(lyr["kind"] in ("riser", "swell") for lyr in ls),
            "qa_ok": next((lyr["qa_ok"] for lyr in ls if lyr.get("qa_ok")), None)}


def _group_row(grp: dict, master_gain: float) -> dict:
    ls = grp["layers"]
    lo_i, hi_i, sig = _sum_layers(ls)
    loud = max(ls, key=lambda lyr: lyr["peak_db"])
    pk = mst.db(_peak(sig)) + master_gain
    cue = min(lyr["cue"] for lyr in ls)
    label = f"place:{ls[0]['sample']}" if grp["free"] else ls[0]["event"]
    row = {"a": lo_i, "b": hi_i, "sig": sig, "layers": ls, "cue": cue, "loud": loud, "kind": loud["kind"], "label": label}
    row.update(_flags(ls))
    row.update({"att": min(lyr["att"] for lyr in ls), "peak_db": round(pk, 2), "silent": pk < -70,
                "rules": sorted({lyr["rule"] for lyr in ls}), "t": round(cue, 4)})
    return row


def group(layers: list, master_gain: float) -> list[dict]:
    return [_group_row(grp, master_gain) for grp in _bucket(layers)]


# ---------------------------------------------------------------- clicks

def _window_energy(pw: np.ndarray, half: int) -> np.ndarray:
    cum = np.concatenate([[0.0], np.cumsum(pw)])
    ix = np.arange(len(pw))
    return cum[np.minimum(len(pw), ix + half + 1)] - cum[np.maximum(0, ix - half)]


def detect_clicks(sig: np.ndarray) -> list[dict]:
    found = []
    hp = sos(4, 14000, "highpass")
    for ch in range(2):
        hh = signal.sosfilt(hp, sig[:, ch].astype(np.float64))
        ev, pw = np.abs(hh), hh * hh
        loc = np.sqrt(np.maximum(uniform_filter1d(pw, 960), 0.0)) + 1e-5
        near = np.sqrt(np.maximum(uniform_filter1d(pw, 192), 0.0)) + 1e-5
        cand = np.nonzero((ev / loc > 9) & (ev / near > 6) & (ev > 0.01))[0]
        if not len(cand):
            continue
        e24, e240 = _window_energy(pw, 24), _window_energy(pw, 240)
        prev = -10 ** 9
        for ix in cand:
            if ix - prev > 480:
                conc = e24[ix] / max(e240[ix], 1e-30)
                found.append({"i": int(ix), "ch": ch, "level_db": round(mst.db(float(ev[ix])), 2), "conc": round(float(conc), 3),
                              "kind": "click" if conc >= 0.8 else "attack"})
            prev = ix
    found.sort(key=lambda clk: (clk["i"], clk["ch"]))
    acc = []
    for clk in found:
        if acc and clk["i"] - acc[-1]["i"] <= 48:
            continue
        acc.append(clk)
    return acc


def _where(ix: int, start: int, count: int) -> str:
    if abs(ix - start) <= 96:
        return "start"
    if abs(ix - (start + count)) <= 96:
        return "end"
    return "body"


def _layer_hf(lay: dict, ix: int, ch: int, hp) -> float | None:
    """The high-passed peak of layer L within 0.5 ms of sample i, or None when L is not near i."""
    if lay["b"] < ix - 480 or lay["a"] > ix + 480:
        return None
    low, high = max(lay["a"], ix - 480), min(lay["b"], ix + 480)
    if high - low < 64:
        low, high = max(lay["a"], high - 64), min(lay["b"], low + 64)
    seg = lay["sig"][low - lay["a"]:high - lay["a"], ch].astype(np.float64)
    if len(seg) < 8:
        return None
    hh = np.abs(signal.sosfilt(hp, seg))
    k0, k1 = max(0, ix - 24 - low), max(0, min(len(hh), ix + 25 - low))
    return float(hh[k0:k1].max()) if k1 > k0 else 0.0


def _layer_owner(clk: dict, label: str, cx: dict) -> list[dict]:
    """The sfx layer that carries click c, when its high band explains at least 30 % of it."""
    ix = clk["i"]
    lvl = 10 ** (clk["level_db"] / 20) * (10 ** (-cx["G"] / 20) if label == "master" else 1)
    best, bv = None, 0.0
    for lay in cx["layers"]:
        value = _layer_hf(lay, ix, clk["ch"], cx["hp"])
        if value is not None and value > bv:
            best, bv = lay, value
    if best is None or bv < 0.3 * lvl:
        return []
    return [{"bus": "sfx", "what": best["sample"], "rule": best["rule"], "where": _where(ix, best["start"], best["n"]),
             "from_start_ms": ms(ix - best["start"]), "from_end_ms": ms(best["start"] + best["n"] - ix)}]


def _trace_owners(clk: dict, label: str, trace: list) -> list[dict]:
    """Up to 3 trace records near click c, edges first."""
    ix = clk["i"]
    recs = [rec for rec in trace if rec["start"] - 960 <= ix <= rec["start"] + rec["n"] + 960
            and (label == "master" or rec["bus"] == label)]
    dist = lambda rec: min(abs(ix - rec["start"]), abs(ix - rec["start"] - rec["n"]))
    recs.sort(key=lambda rec: (_where(ix, rec["start"], rec["n"]) == "body", dist(rec)))
    return [{"bus": rec["bus"], "what": rec.get("sample") or rec.get("name") or rec.get("label"),
             "rule": rec.get("rule"), "where": _where(ix, rec["start"], rec["n"]),
             "from_start_ms": ms(ix - rec["start"]), "level_db": clk["level_db"]} for rec in recs[:3]]


def _click_item(label: str, clk: dict, cx: dict) -> dict:
    owners = _layer_owner(clk, label, cx) if cx["layers"] and label in ("master", "sfx") else []
    owners = owners or _trace_owners(clk, label, cx["trace"])
    return {"in": label, "t": round(clk["i"] / SR, 4), "ch": clk["ch"], "level_db": clk["level_db"], "conc": clk["conc"],
            "kind": "click", "owners": owners, "at_edge": bool(owners) and owners[0]["where"] in ("start", "end")}


def _click_counts(cl: list) -> dict:
    return {"clicks": sum(clk["kind"] == "click" for clk in cl), "attacks": sum(clk["kind"] == "attack" for clk in cl)}


def _clicks_result(items: list, counts: dict) -> dict:
    edge_master = [clk for clk in items if clk["in"] == "master" and clk["at_edge"]]
    status = "fail" if edge_master else ("warn" if items else "ok")
    count = len(items)
    summ = (f"{count} click(s), {len(edge_master)} at a placement edge on the master" if count else "no clicks")
    if count:
        summ += "; a click inside a recording is its own transient or a defect of the source: a critic decides"
    return {"status": status, "summary": summ, "counts": counts, "items": items}


def section_clicks(job, layers, master_gain) -> dict:
    sigs = ([("master", job["master"])] if job["master"] is not None else []) + [(lane, job["stems"][lane]) for lane in mix.BUSES]
    cx = {"layers": layers, "G": master_gain, "hp": sos(4, 14000, "highpass"), "trace": job["trace"] or []}
    items, counts = [], {}
    for label, sig in sigs:
        cl = detect_clicks(sig)
        counts[label] = _click_counts(cl)
        items += [_click_item(label, clk, cx) for clk in cl if clk["kind"] == "click"]
    return _clicks_result(items, counts)


# ---------------------------------------------------------------- edges

def _rms_db(sig: np.ndarray) -> float:
    return mst.db(float(np.sqrt(np.mean(sig.astype(np.float64) ** 2)))) if len(sig) else -200.0


def late_onset(sig: np.ndarray) -> float | None:
    meas = np.abs(sig).max(axis=1)
    nb = len(meas) // 240
    if nb < 3:
        return None
    ev = meas[: nb * 240].reshape(nb, 240).max(axis=1)
    pk = ev.max()
    for key_ in range(max(1, int(0.6 * nb)), nb):
        if ev[key_] > 0.12 * pk and ev[key_] > 4 * ev[key_ - 1]:
            return round(key_ * 240 / SR, 4)
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


def judge_trim(opts: dict, proc: np.ndarray, ar: dict) -> dict:
    """Edge findings of 1 trim. ar: pre20, post20 (dB around the trim), starts_late, fi, fo (fade lengths)."""
    pk = float(np.max(np.abs(proc))) or 1e-12
    head = _rms_db(proc[:48]) - mst.db(pk)
    tail = _rms_db(proc[-48:]) - mst.db(pk)
    att = mix.align_index(proc, "attack") / SR * 1000
    defects, remarks = _entry_findings(ar, head, att)
    d2, n2 = _exit_findings(ar, tail)
    defects, remarks = defects + d2, remarks + n2
    low = late_onset(proc)
    if low is not None:
        remarks.append(f"a second hit at {low:.3f} s inside the sample (fine for a rattle, wrong for a single hit)")
    if opts.get("qa_ok") and defects:
        remarks += [f"acknowledged ({opts['qa_ok']}): {meas}" for meas in defects]
        defects = []
    return {"head_db": round(head, 2), "tail_db": round(tail, 2), "attack_ms": round(att, 2), "late_onset": low,
            "defects": defects, "notes": remarks}


def _silent_trim(trim: np.ndarray, rpk: float, raw: np.ndarray) -> list[str]:
    why = ("empty (start >= end or past the file end)" if len(trim) == 0 else
           "digital silence" if rpk == 0 else "near-silent")
    return [f"the trim is {why}; the file is {len(raw) / SR:.3f} s long"]


def _sample_edges(job: dict, label: str) -> dict:
    opts = job["design"]["samples"][label]
    raw = mix.decode(mix.rpath(job["root"], opts["file"]), job["cache"])
    lo_i = int(float(opts.get("start", 0)) * SR)
    hi_i = int(float(opts["end"]) * SR) if opts.get("end") is not None else len(raw)
    trim = raw[lo_i:hi_i]
    rpk = float(np.max(np.abs(trim))) if len(trim) else 0.0
    entry = {"sample": label, "file": opts["file"]}
    if mst.db(rpk) < -60:
        entry.update(defects=_silent_trim(trim, rpk, raw), notes=[])
        return entry
    pre20 = _rms_db(raw[max(0, lo_i - 960):lo_i]) - mst.db(rpk) if lo_i > 0 else -200.0
    post20 = _rms_db(raw[hi_i:hi_i + 960]) - mst.db(rpk) if hi_i < len(raw) else -200.0
    entry.update(pre20_db=round(pre20, 2), post20_db=round(post20, 2))
    ar = {"pre20": pre20, "post20": post20, "starts_late": lo_i > 0, "fi": float(opts.get("fade_in", 0.002)),
          "fo": float(opts.get("fade_out", 0.012))}
    entry.update(judge_trim(opts, mix.shape(trim, opts, with_fades=False), ar))
    if opts.get("qa_ok"):
        entry["qa_ok"] = opts["qa_ok"]
    return entry


def _strip_numbers(meas: str) -> str:
    return re.sub(r"\s*\([^)]*\d[^)]*\)|\d+(\.\d+)?\s*s\b", "", meas).strip()


def _slice_edges(job: dict, grp: str) -> dict:
    opts = job["design"]["slices"].get(grp, {})
    entry = {"slices": grp, "file": opts.get("file")}
    try:
        sl = mix.cut_slices(mix.decode(mix.rpath(job["root"], opts["file"]), job["cache"]), opts, opts["file"], with_fades=False)
    except Exception as ev:  # noqa: BLE001
        entry.update(defects=[f"cannot slice: {ev}"], notes=[])
        return entry
    defs, nts = set(), set()
    for _, sig in sl:
        ar = {"pre20": -200.0, "post20": _rms_db(sig[-48:]) - mst.db(float(np.max(np.abs(sig))) or 1e-12),
              "starts_late": False, "fi": float(opts.get("fade_in", 0.002)), "fo": float(opts.get("fade_out", 0.02))}
        rec = judge_trim(opts, sig, ar)
        defs |= {_strip_numbers(meas) for meas in rec["defects"]}
        nts |= {_strip_numbers(meas) for meas in rec["notes"]}
    entry.update(n=len(sl), defects=sorted(defs), notes=sorted(nts))
    return entry


def _music_source(job: dict, mu: dict) -> tuple[np.ndarray, float]:
    """The (stretched) music source and its time factor."""
    ratio = 1.0 if mu.get("stretch") == "none" else float(mu.get("target_bpm", 120)) / float(mu["src_bpm"])
    src_path = mix.rpath(job["root"], mu["file"])
    src = mix.stretched(src_path, ratio, job["cache"]) if abs(ratio - 1) >= 1e-6 else mix.decode(src_path, job["cache"])
    return src, 1 / ratio


def _chop(row: dict, val: dict, end_t: float, film: float) -> None:
    chops = (not val.get("tail_fade") and float(val.get("fade_out", 0.06)) < 0.1 and end_t < film - 0.05
             and row["cut_db"] > row["out_db"] - 6 and row["cut_db"] > row["in_db"] + 6)
    if not chops:
        return
    msg = "chops the music: the source keeps playing past src_to and the next part is quieter"
    if val.get("qa_ok"):
        row["notes"].append(f"acknowledged ({val['qa_ok']}): {msg}")
    else:
        row["defects"].append(msg)


def _gap_note(nxt, end_t: float) -> list[str]:
    gap = float(nxt["at"]) - end_t if nxt is not None else 0.0
    if gap > 0.02:
        return [f"{gap * 1000:.0f} ms of no music before the next segment"]
    return [f"overlaps the next segment by {-gap * 1000:.0f} ms"] if gap < -0.1 else []


def _splice_row(sp: dict, val: dict, nxt) -> dict:
    """1 music splice. sp: lv (level at a sample), k (time factor), w (window), film (length)."""
    lev, key_, wgt = sp["lv"], sp["k"], sp["w"]
    end = int(round(float(val["src_to"]) * key_ * SR))
    start = int(round(float(val["src_from"]) * key_ * SR))
    end_t = float(val["at"]) + (float(val["src_to"]) - float(val["src_from"])) * key_
    in_db = lev(int(round(float(nxt["src_from"]) * key_ * SR))) if nxt else -120.0
    row = {"label": val.get("label"), "at": val["at"], "end_t": round(end_t, 4), "out_db": round(lev(end - wgt), 2),
           "cut_db": round(lev(end), 2), "in_db": round(in_db, 2), "defects": [], "notes": []}
    _chop(row, val, end_t, sp["film"])
    row["notes"] += _gap_note(nxt, end_t)
    if start > wgt and lev(start - wgt) > lev(start) - 3:
        row["notes"].append("begins inside a sustained sound (no fresh attack on the splice)")
    return row


def _music_splices(job: dict) -> list[dict]:
    mu = job["design"].get("music")
    if not (mu and mu.get("segments")):
        return []
    src, key_ = _music_source(job, mu)
    wgt = int(0.05 * SR)
    sp = {"lv": lambda ix: _rms_db(src[max(0, ix):max(0, ix) + wgt]), "k": key_, "w": wgt,
          "film": len(job["stems"]["sfx"]) / SR}
    segs = sorted(mu["segments"], key=lambda val: float(val["at"]))
    return [_splice_row(sp, val, segs[j + 1] if j + 1 < len(segs) else None) for j, val in enumerate(segs)]


def _cut_by_ends(job: dict) -> list[dict]:
    N = len(job["stems"]["sfx"])
    return [{"bus": rec["bus"], "what": rec.get("sample") or rec.get("name") or rec.get("label"), "t": round(rec["start"] / SR, 4),
             "before_0": rec["start"] < 0, "after_end": rec["start"] + rec["n"] > N}
            for rec in job["trace"] or [] if rec["bus"] != "music" and (rec["start"] < 0 or rec["start"] + rec["n"] > N)]


def _trim_items(job: dict) -> list[dict]:
    """Edge findings of every used sample trim and slice group."""
    dsg = job["design"]
    used = {pw["sample"] for pw in job["report"]["placements"]}
    items = [_sample_edges(job, count) for count in sorted(u for u in used if "#" not in u and u in dsg["samples"])]
    return items + [_slice_edges(job, grp) for grp in sorted({u.split("#")[0] for u in used if "#" in u})]


def section_edges(job) -> dict:
    items = _trim_items(job)
    splices = _music_splices(job)
    cut = _cut_by_ends(job)
    nd = sum(bool(ix["defects"]) for ix in items) + sum(bool(val["defects"]) for val in splices)
    summ = f"{nd} edge defect(s) in {len(items)} trim(s) and {len(splices)} music splice(s)"
    return {"status": "warn" if nd else "ok", "summary": summ + (f"; {len(cut)} placement(s) cut by the film ends" if cut else ""),
            "items": items, "music_splices": splices, "cut_by_film_ends": cut}


# ---------------------------------------------------------------- audibility

_BANDS = [10 ** (count / 10) for count in range(20, 42)]
_MASKS: dict[int, list] = {}


def band_energy(sig: np.ndarray) -> np.ndarray:
    count = len(sig)
    if count not in _MASKS:
        freqs = np.fft.rfftfreq(count, 1 / SR)
        _MASKS[count] = [(freqs >= cum * 2 ** (-1 / 6)) & (freqs < cum * 2 ** (1 / 6)) for cum in _BANDS]
    wgt = signal.windows.tukey(count, 0.25)[:, None]
    pw = (np.abs(np.fft.rfft(sig * wgt, axis=0)) ** 2).sum(axis=1)
    return np.array([pw[meas].sum() for meas in _MASKS[count]]) + 1e-30


def margin(E: np.ndarray, X: np.ndarray) -> tuple[float, float]:
    keep = E >= 1e-3 * E.max()
    meas = np.clip(10 * np.log10(E / X), -60, 60)
    meas = np.where(keep, meas, -np.inf)
    ix = int(np.argmax(meas))
    return round(float(meas[ix]), 2), round(_BANDS[ix])


def _take(sig: np.ndarray, start: int, count: int, base: int = 0) -> np.ndarray:
    """Samples [start, start + n) of x (x begins at absolute sample base), zero-padded."""
    acc = np.zeros((count, 2), np.float32)
    lo_i, hi_i = max(start, base), min(start + count, base + len(sig))
    if lo_i < hi_i:
        acc[lo_i - start:hi_i - start] = sig[lo_i - base:hi_i - base]
    return acc


def best_window(E: np.ndarray, base: int, low: int, high: int, count: int) -> int:
    pw = (E.astype(np.float64) ** 2).sum(axis=1)
    cum = np.concatenate([[0.0], np.cumsum(pw)])
    best, bv = low, -1.0
    for val in range(low, max(low, high) + 1, 48):
        lo_i, hi_i = min(max(0, val - base), len(pw)), min(max(0, val + count - base), len(pw))
        value = cum[hi_i] - cum[lo_i]
        if value > bv:
            best, bv = val, value
    return best


def judge_audibility(E, lo_i, hi_i, att, long, job) -> dict:
    state = job["stems"]
    w60, w20 = int(0.06 * SR), int(0.02 * SR)
    hi60 = (hi_i - w60) if long else att + int(0.3 * SR)
    s60 = best_window(E, lo_i, att - int(0.005 * SR), max(att - int(0.005 * SR), hi60), w60)
    s20 = s60 if long else best_window(E, lo_i, att - int(0.003 * SR), att + int(0.08 * SR), w20)
    bed = lambda val, count: _take(state["music"], val, count) + _take(state["amb"], val, count)
    rest = lambda val, count: _take(state["sfx"], val, count) - _take(E, val, count, lo_i) + bed(val, count)
    e60, e20 = band_energy(_take(E, s60, w60, lo_i)), band_energy(_take(E, s20, w20, lo_i))
    vb, vbh = margin(e60, band_energy(bed(s60, w60)))
    va, _ = margin(e60, band_energy(rest(s60, w60)))
    f20, f20h = margin(e20, band_energy(rest(s20, w20)))
    rd = max(va, f20)
    verdict = "clear" if rd >= 6 else "ok" if rd >= 3 else "barely" if rd >= 0 else "masked"
    return {"vs_bed_db": vb, "vs_bed_hz": vbh, "vs_all_db": va, "first20_db": f20, "first20_hz": f20h, "read_db": rd,
            "verdict": verdict, "covered_by": "music/amb" if vb < 3 else "other effects"}


def _audibility_row(grp: dict, job: dict) -> dict:
    row = {"t": grp["t"], "label": grp["label"], "kind": grp["kind"], "n_layers": len(grp["layers"]), "rules": grp["rules"],
           "peak_db": grp["peak_db"]}
    if grp["qa_ok"]:
        row["qa_ok"] = grp["qa_ok"]
    if grp["silent"]:
        row.update(verdict="silent", samples=sorted({lyr["sample"] for lyr in grp["layers"]}))
    elif job["no_bed"]:
        row["verdict"] = "not judged"
    else:
        row.update(judge_audibility(grp["sig"], grp["a"], grp["b"], grp["att"], grp["long"], job))
    return row


def _audibility_status(silent: list, masked: list, no_bed: bool) -> str:
    if any(not rec.get("qa_ok") for rec in silent):
        return "fail"
    return "skipped" if no_bed else ("warn" if masked else "ok")


def _dead_layer(grp: dict, lay: dict, master_gain: float, job: dict):
    """A layer that adds nothing to its group: silent, or masked by the rest. None when it reads."""
    if lay["peak_db"] + master_gain < -70:
        return {"t": grp["t"], "rule": lay["rule"], "sample": lay["sample"], "read_db": None, "silent": True}
    rec = None if job["no_bed"] else judge_audibility(lay["sig"], lay["a"], lay["b"], lay["att"], grp["long"], job)
    if rec is None or rec["read_db"] >= 0:
        return None
    return {"t": grp["t"], "rule": lay["rule"], "sample": lay["sample"], "read_db": rec["read_db"]}


def _dead_layers(clusters: list, master_gain: float, job: dict) -> list[dict]:
    dead = []
    for grp in (grp for grp in clusters if not grp["silent"] and len(grp["layers"]) >= 2):
        dead += [sig for sig in (_dead_layer(grp, lay, master_gain, job) for lay in grp["layers"]) if sig is not None]
    return dead


def _partition(table: list) -> tuple[list, list, list]:
    """(silent rows, judged rows quietest first, masked rows without qa_ok)."""
    silent = [rec for rec in table if rec["verdict"] == "silent"]
    judged = sorted([rec for rec in table if "read_db" in rec], key=lambda rec: rec["read_db"])
    return silent, judged, [rec for rec in judged if rec["verdict"] == "masked" and not rec.get("qa_ok")]


def section_audibility(job, clusters, master_gain, layers_flag) -> dict:
    table = [_audibility_row(grp, job) for grp in clusters]
    silent, judged, masked = _partition(table)
    summ = (f"{len(silent)} silent, {len(masked)} masked of {len(table)} event(s)" if not job["no_bed"]
            else f"no bed (solo render): {len(silent)} silent of {len(table)} event(s), the rest not judged")
    acc = {"status": _audibility_status(silent, masked, job["no_bed"]), "items": (silent + judged)[:40],
           "events": table, "summary": summ}
    if layers_flag:
        acc["dead_layers"] = _dead_layers(clusters, master_gain, job)
        acc["summary"] += f"; {len(acc['dead_layers'])} dead layer(s)"
    return acc


# ---------------------------------------------------------------- harshness

def _band_peaks(grp: dict, cx: dict):
    """(2-6 kHz power peak, 80-400 Hz power peak, centroid) over the attack window, or None when it is short."""
    pad = cx["pad"]
    meas = np.concatenate([np.zeros(pad), grp["sig"].mean(axis=1).astype(np.float64)])
    hi_b, lo_b = signal.sosfilt(cx["bp"], meas)[pad:], signal.sosfilt(cx["lowp"], meas)[pad:]
    a0 = max(0, grp["att"] - grp["a"] - pad)
    a1 = len(hi_b) if grp["long"] else min(len(hi_b), grp["att"] - grp["a"] + int(0.08 * SR))
    if a1 - a0 < 240:
        return None
    ph = uniform_filter1d(hi_b[a0:a1] ** 2, 240).max()
    pl = uniform_filter1d(lo_b[a0:a1] ** 2, 240).max()
    seg = meas[pad + a0:pad + a1]
    spec = np.abs(np.fft.rfft(seg * np.hanning(len(seg)))) ** 2
    freqs = np.fft.rfftfreq(len(seg), 1 / SR)
    return ph, pl, float((spec * freqs).sum() / max(spec.sum(), 1e-30))


def _vs_music(row: dict, grp: dict, pres: float, cx: dict) -> None:
    cum, rec = grp["att"], int(0.25 * SR)
    row["music_pres_db"] = round(dbp(float(np.mean(cx["mus"][max(0, cum - rec):cum + rec] ** 2))) + cx["G"], 2)
    row["over_music_max_db"] = round(pres - cx["ref"], 2)
    if pres - cx["ref"] > 6 and not grp["qa_ok"]:
        row["issues"].append(f"2-6 kHz {pres - cx['ref']:.1f} dB above the music's loudest presence")


def _harsh_row(grp: dict, cx: dict):
    bands = None if grp["silent"] else _band_peaks(grp, cx)
    if bands is None:
        return None
    ph, pl, cen = bands
    pres = dbp(float(ph)) + cx["G"]
    row = {"t": grp["t"], "label": grp["label"], "rules": grp["rules"], "pres_db": round(pres, 2),
           "pres_minus_low_db": round(pres - dbp(float(pl)) - cx["G"], 2), "centroid_hz": round(cen),
           "music_pres_db": None, "over_music_max_db": None, "issues": [], "bright": cen > 6500}
    if cx["has_music"]:
        _vs_music(row, grp, pres, cx)
    return row


def _harsh_result(table: list, bright: list, has_music: bool, ref) -> dict:
    hot = [rec for rec in table if rec["issues"]]
    items = hot[:20] + [rec for rec in table if not rec["issues"]][: max(0, 10 - len(hot[:20]))]
    summ = (f"{len(hot)} event(s) over the music's 2-6 kHz peak by more than 6 dB" if has_music
            else "no music: the presence check is not judged") + f"; {len(bright)} bright event(s)"
    return {"status": "warn" if hot else "ok", "summary": summ, "items": items, "bright": bright, "music_ref_db": ref}


def section_harshness(job, clusters, master_gain) -> dict:
    bp = sos(4, [2000, 6000], "bandpass")
    mus = signal.sosfilt(bp, job["stems"]["music"].mean(axis=1).astype(np.float64))
    mp = uniform_filter1d(mus * mus, 240)
    has_music = mp.max() > 1e-10
    ref = dbp(float(mp.max())) + master_gain if has_music else None
    cx = {"bp": bp, "lowp": sos(4, [80, 400], "bandpass"), "mus": mus, "has_music": has_music, "ref": ref, "G": master_gain,
          "pad": int(0.01 * SR)}
    table = [rec for rec in (_harsh_row(grp, cx) for grp in clusters) if rec is not None]
    bright = [{"t": rec["t"], "label": rec["label"], "centroid_hz": rec["centroid_hz"]} for rec in table if rec["bright"]]
    table.sort(key=lambda rec: -rec["pres_db"])
    return _harsh_result(table, bright, has_music, ref)


# ---------------------------------------------------------------- sync

def _build_issue(test: str, meas: dict, val: float):
    if test == "end":
        value = meas["end_ms"] - val
        return None if -300 <= value <= 60 else f"ends {value:.0f} ms from the hit (allowed -300..+60)"
    value = meas["apex_ms"] - val
    return None if abs(value) <= 45 else f"apex {value:+.0f} ms off the cue (allowed +-45)"


def _impact_issue(meas: dict, val: float):
    if meas["attack_ms"] - val < -45:
        return f"sound before picture: attack {meas['attack_ms'] - val:+.0f} ms"
    return f"lands late: peak {meas['peak_ms'] - val:+.0f} ms" if meas["peak_ms"] - val > 75 else None


def _sync_issue(test, meas, val):
    return _build_issue(test, meas, val) if test in ("end", "apex") else _impact_issue(meas, val)


def _sync_test(grp: dict):
    """end, apex or impact; None for a group aligned by its start or end."""
    lay = grp["loud"]
    if grp["builds"]:
        return "apex" if lay["align"] == "peak" else "end"
    if lay["align"] in ("start", "end"):
        return None
    return "apex" if grp["long"] else "impact"


def _sync_marks(grp: dict, sig: np.ndarray, cue: float) -> dict:
    nb = len(sig) // 240
    pk = (int(np.argmax(sig[: nb * 240].reshape(nb, 240).max(axis=1))) * 240 + 120) if nb else 0
    pw = uniform_filter1d((grp["sig"].astype(np.float64) ** 2).sum(axis=1), 2400)
    return {"attack_ms": ms(grp["loud"]["att"] - cue), "peak_ms": ms(grp["a"] + pk - cue),
            "apex_ms": ms(grp["a"] + int(np.argmax(pw)) - cue)}


def _explain(row: dict, grp: dict, test: str, meas: dict, issue):
    """Clear an issue that a rule offset or a qa_ok explains. Returns the issue left."""
    lyr = next((lyr for lyr in grp["layers"] if abs(lyr["offset"]) >= 0.03 and _sync_issue(test, meas, lyr["offset"] * 1000) is None),
             None)
    if lyr is not None:
        row.update(deliberate=True, offset_s=lyr["offset"], offset_rule=lyr["rule"],
                   explanation=f"sits {lyr['offset'] * 1000:+.0f} ms off the cue by the offset of rule {lyr['rule']}")
        return None
    if grp["qa_ok"]:
        row.update(deliberate=True, explanation=f"acknowledged: {grp['qa_ok']}")
        return None
    tried = [lyr["offset"] for lyr in grp["layers"] if abs(lyr["offset"]) >= 0.03]
    if tried:
        row["offsets_tried_s"] = tried
    return issue


def _sync_row(grp: dict):
    test = None if grp["silent"] else _sync_test(grp)
    if test is None:
        return None
    sig = np.abs(grp["sig"]).max(axis=1)
    cue = grp["cue"] * SR
    meas = _sync_marks(grp, sig, cue)
    row = {"t_cue": grp["t"], "label": grp["label"], "kind": grp["kind"], "test": test, "align": grp["loud"]["align"], **meas,
           "long": grp["long"], "issue": None}
    if test == "end":
        idx = np.nonzero(sig > 0.1 * sig.max())[0]
        row["end_ms"] = meas["end_ms"] = ms(grp["a"] + (idx[-1] if len(idx) else 0) - cue)
    issue = _sync_issue(test, meas, 0)
    row["issue"] = _explain(row, grp, test, meas, issue) if issue else None
    return row


def section_sync(clusters) -> dict:
    table = [rec for rec in (_sync_row(grp) for grp in clusters) if rec is not None]
    issues = [rec for rec in table if rec["issue"]]
    deliberate = [rec for rec in table if rec.get("deliberate")]
    return {"status": "warn" if issues else "ok", "items": issues + deliberate, "events": table,
            "summary": f"{len(issues)} sync issue(s) in {len(table)} judged event(s), {len(deliberate)} deliberate "
                       "(check the deliberate ones on the frames)"}


# ---------------------------------------------------------------- density, loudness, coverage

def _per_second(live: list, dur: float) -> list[int]:
    per = [0] * max(1, math.ceil(dur))
    for val in (int(grp["att"] // SR) for grp in live):
        if 0 <= val < len(per):
            per[val] += 1
    return per


def _rule_row(rec: str, ps: list) -> dict:
    ioi = np.diff(sorted(pw["t"] for pw in ps))
    return {"rule": rec, "n": len(ps), "distinct_samples": len({pw["sample"] for pw in ps}),
            "ioi_s": round(float(ioi.mean()), 4),
            "ioi_cv": round(float(ioi.std() / ioi.mean()), 3) if ioi.mean() > 0 else None,
            "gain_sd_db": round(float(np.std([pw["gain_db"] for pw in ps])), 3),
            "pitch_sd_st": round(float(np.std([pw["pitch"] for pw in ps])), 3)}


def _rule_rows(pls: list) -> tuple[list, list]:
    """(a row per rule with 4 or more placements, the machine-gun rules)."""
    by = {}
    for pw in (pw for pw in pls if pw["kind"] != "place"):
        by.setdefault(str(pw["rule"]), []).append(pw)
    rules = [_rule_row(rec, ps) for rec, ps in by.items() if len(ps) >= 4]
    return rules, _machine_gun(rules)


def _machine_gun(rules: list) -> list[str]:
    """Rules that repeat 1 take at 1 pitch and 1 level."""
    return [row["rule"] for row in rules
            if row["distinct_samples"] == 1 and row["pitch_sd_st"] < 0.1 and row["gain_sd_db"] < 0.5]


def _busiest(per: list) -> list[dict]:
    return sorted(({"s": ix, "n": count} for ix, count in enumerate(per) if count), key=lambda rec: (-rec["n"], rec["s"]))[:5]


def section_density(job, clusters) -> dict:
    dur = len(job["stems"]["sfx"]) / SR
    live = [grp for grp in clusters if not grp["silent"]]
    per = _per_second(live, dur)
    busiest = _busiest(per)
    pls = job["report"]["placements"]
    used = Counter(pw["sample"] for pw in pls).most_common(8)
    rules, mg = _rule_rows(pls)
    p10 = len(live) / dur * 10 if dur else 0
    summ = (f"{len(live)} event(s), {p10:.1f} per 10 s (norm about 5); busiest seconds "
            + ", ".join(f"{lane['s']}s x{lane['n']}" for lane in busiest[:3]) + (f"; machine gun: {', '.join(mg)}" if mg else ""))
    return {"status": "warn" if mg else "ok", "events_per_s": per, "busiest_s": busiest,
            "most_used_samples": [{"sample": val, "n": count} for val, count in used], "rules": rules, "machine_gun": mg,
            "summary": summ}


def _loudness_skip(sig):
    """The section result when the master cannot be measured, else None."""
    if sig is None:
        return {"status": "skipped", "summary": "no master"}
    if len(sig) < int(0.4 * SR):
        return {"status": "skipped", "summary": "the master is shorter than 400 ms"}
    return None if np.any(sig) else {"status": "fail", "summary": "the master is all zeros"}


def _loudness_numbers(sig: np.ndarray, target: float, pre_lufs) -> tuple[dict, np.ndarray, np.ndarray]:
    kw = mst.kweight(mst.as_stereo(sig))
    mom, mt = mst.block_loudness(sig, 0.4, 0.1, kw)
    state, _ = mst.block_loudness(sig, 3.0, 1.0, kw)
    ix = int(np.argmax(mom))
    dur = len(sig) / SR
    rec = {"lufs": round(mst.integrated(sig, kw), 2), "target": target, "true_peak_dbtp": round(mst.true_peak(sig), 2),
         "lra_lu": round(mst.lra(sig, kw), 2), "premaster_lufs": pre_lufs, "loudest_momentary": round(float(mom[ix]), 2),
         "loudest_at": round(float(mt[ix]), 2), "loudest_frac": round(float(mt[ix]) / dur, 3),
         "short_term_per_s": [round(float(value), 2) for value in state],
         "head_5ms_peak_dbfs": round(mst.db(float(np.abs(sig[:240]).max())), 2),
         "tail_50ms_rms_dbfs": round(_rms_db(sig[-2400:]), 2), "tail_10ms_peak_dbfs": round(mst.db(float(np.abs(sig[-480:]).max())), 2)}
    return rec, mom, mt


def _act_row(sel: np.ndarray, low: float) -> dict:
    if not len(sel):
        return {"from": round(low, 3), "momentary_median": None, "momentary_max": None}
    return {"from": round(low, 3), "momentary_median": round(float(np.median(sel)), 2),
            "momentary_max": round(float(sel.max()), 2)}


def _acts(cues: dict, mom: np.ndarray, mt: np.ndarray) -> dict:
    fps = float(cues.get("fps", 60))
    acts = {}
    for label, lo_i in (cues.get("acts") or {}).items():
        low, high = float(lo_i["from"]) / fps, (float(lo_i["from"]) + float(lo_i["dur"])) / fps
        acts[label] = _act_row(mom[(mt >= low) & (mt < high)], low)
    return acts


def _loudness_findings(rec: dict, target: float) -> tuple[list, list]:
    off = not math.isfinite(rec["lufs"]) or abs(rec["lufs"] - target) > 0.5
    loud_end = rec["tail_50ms_rms_dbfs"] > -45 or rec["tail_10ms_peak_dbfs"] > -60
    fails = [(off, f"integrated {rec['lufs']} LUFS is off the target {target} by more than 0.5 LU"),
             (rec["true_peak_dbtp"] > -1.0, f"true peak {rec['true_peak_dbtp']} dBTP > -1.0 (AAC would clip)"),
             (loud_end, "the end is not silent")]
    warns = [(rec["head_5ms_peak_dbfs"] > -40, "the first 5 ms are loud: starts mid-sound or clicks on sample 0"),
             (rec["lra_lu"] < 4, f"LRA {rec['lra_lu']} LU: flat"),
             (rec["lra_lu"] > 10, f"LRA {rec['lra_lu']} LU: quiet passages vanish on phones"),
             (rec["loudest_frac"] > 0.85, "the loudest moment is in the last 15 %: the peak belongs on the drop")]
    return [meas for bad, meas in fails if bad], [meas for bad, meas in warns if bad]


def section_loudness(job, target, pre_lufs) -> dict:
    skip = _loudness_skip(job["master"])
    if skip is not None:
        return skip
    rec, mom, mt = _loudness_numbers(job["master"], target, pre_lufs)
    rec["acts"] = _acts(job["cues"], mom, mt)
    fails, warns = _loudness_findings(rec, target)
    status = "fail" if fails else "warn" if warns else "ok"
    summ = (f"{rec['lufs']} LUFS, TP {rec['true_peak_dbtp']} dBTP, LRA {rec['lra_lu']} LU, loudest {rec['loudest_momentary']} "
            f"LUFS-M at {tfmt(rec['loudest_at'])}" + ("; " + "; ".join(fails + warns) if fails or warns else ""))
    rec.update(fails=fails, warnings=warns, status=status, summary=summ)
    return rec


def _placed_silent(clusters) -> list[dict]:
    return [{"t": grp["t"], "kind": grp["kind"], "id": grp["label"]} for grp in clusters or []
            if grp["silent"] and not grp["qa_ok"] and not grp["label"].startswith("place:")]


def _event_class(kind, ix) -> tuple:
    return kind, re.sub(r"(:\d+)+$", "", str(ix or ""))


def _class_counts(job: dict, none: set, placed_silent: list) -> tuple[Counter, Counter]:
    """(events per class, unsounded events per class)."""
    fps = float(job["cues"].get("fps", 60))
    total, missing = Counter(), Counter()
    for ev in job["cues"].get("events", []):
        clk = _event_class(ev.get("kind"), ev.get("id"))
        total[clk] += 1
        gone = (round(float(ev["f"]) / fps, 4), ev.get("kind"), ev.get("id")) in none
        if gone or any(ev.get("id") == pw["id"] and ev.get("kind") == pw["kind"] for pw in placed_silent):
            missing[clk] += 1
    return total, missing


def _class_rows(total: Counter, missing: Counter) -> tuple[list, list]:
    """(partly sounded classes, never sounded classes)."""
    partial = [{"kind": key_, "class": clk, "unsounded": missing[(key_, clk)], "of": total[(key_, clk)]}
               for (key_, clk) in total if 0 < missing[(key_, clk)] < total[(key_, clk)]]
    never = [{"kind": key_, "class": clk, "n": total[(key_, clk)]} for (key_, clk) in total if missing[(key_, clk)] == total[(key_, clk)]]
    return partial, never


def section_coverage(job, clusters) -> dict:
    none = {(round(ev["t"], 4), ev.get("kind"), ev.get("id")) for ev in job["report"].get("unsounded_events", [])}
    placed_silent = _placed_silent(clusters)
    none |= {(round(pw["t"], 4), pw["kind"], pw["id"]) for pw in placed_silent}
    partial, never = _class_rows(*_class_counts(job, none, placed_silent))
    summ = f"{len(partial)} partial class(es), {len(never)} class(es) never sounded, {len(placed_silent)} placed but silent"
    if job["solo"]:
        summ += "; solo render: events of other fragments are expected to have no sound"
    return {"status": "warn" if partial or placed_silent else "ok", "summary": summ, "partial": partial,
            "unsounded_classes": never, "placed_but_silent": placed_silent}


def _load_api():
    import importlib
    return importlib.import_module("hark.core.api")  # the flat API that `import hark` forwards to


def _try_api():
    try:
        return _load_api()
    except ImportError:
        return None


def _use_engine(eng: str) -> None:
    """Put the sibling engine folder first on the path and forget any hark already imported."""
    if eng not in sys.path:
        sys.path.insert(0, eng)
    for label in [meas for meas in sys.modules if meas == "hark" or meas.startswith("hark.")]:
        del sys.modules[label]


def import_hark():
    """The hark API, from the venv or from the sibling skill's engine folder; None when absent."""
    api = _try_api()
    eng = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "../../hark/engine"))
    if api is None and os.path.isdir(eng):
        _use_engine(eng)
        api = _try_api()
    return api


def _sheet(lx: dict, tl: dict, outcome: dict, remarks: list) -> None:
    try:
        lx["api"].sheet(lx["mp"], os.path.join(lx["outdir"], "qa_sheet.png"), video=lx["video"], cues=lx["cues"], tl=tl)
        outcome["sheet"] = os.path.join(lx["outdir"], "qa_sheet.png")
    except Exception as ev:  # noqa: BLE001
        remarks.append(f"sheet failed: {ev}")


def _timeline(lx: dict, outcome: dict, remarks: list) -> None:
    try:
        tl = lx["api"].timeline(lx["mp"], video=lx["video"], cues=lx["cues"])
        with open(os.path.join(lx["outdir"], "qa_timeline.txt"), "w") as fh:
            fh.write(tl.get("text", ""))
        val = tl.get("summary", "")
        outcome["timeline"] = val if len(val) <= 400 else val[:360] + " ... (see qa_timeline.txt)"
        _sheet(lx, tl, outcome, remarks)
    except Exception as ev:  # noqa: BLE001
        remarks.append(f"timeline failed: {ev}")


def _critique(lx: dict, outcome: dict, remarks: list) -> None:
    try:
        cr = lx["api"].critique(lx["mp"], video=lx["video"], use_ast=False, use_realness=False)
        outcome["critique"] = {"summary": cr.get("summary"), "verdict": cr.get("verdict"), "score": cr.get("score"),
                           "actions": cr.get("actions"),
                           "metrics": [meas for meas in cr.get("metrics", []) if meas.get("status") in ("low", "high")]}
    except Exception as ev:  # noqa: BLE001
        remarks.append(f"critique failed: {ev}")


def _realness_row(api, job: dict, fpath: str, remarks: list) -> dict:
    try:
        rec = api.realness(mix.rpath(job["root"], fpath))
        return {"file": fpath, "p_real": rec.get("p_real"), "verdict": rec.get("verdict")}
    except Exception as ev:  # noqa: BLE001
        remarks.append(f"realness failed on {fpath}: {ev}")
        return {"file": fpath, "p_real": None, "verdict": None}


def _not_real(rec: dict) -> bool:
    if rec["p_real"] is not None:
        return rec["p_real"] < 0.5
    return "synth" in str(rec["verdict"] or "")


def _realness(api, job: dict, outcome: dict, remarks: list) -> list[str]:
    table = [_realness_row(api, job, fpath, remarks) for fpath in job["report"].get("files_used", [])]
    table.sort(key=lambda rec: (rec["p_real"] is None, rec["p_real"] or 0))
    outcome["realness"] = table
    outcome["not_real"] = [rec["file"] for rec in table if _not_real(rec)]
    return outcome["not_real"]


def _listen_paths(job: dict, cli) -> dict:
    mp = None if cli.master == "none" else mix.rpath(job["root"], cli.master or "out/audio/soundtrack.wav")
    cs = mix.rpath(job["root"], cli.cues_sounded)
    return {"mp": mp, "video": mix.rpath(job["root"], cli.video) if cli.video else None,
            "cues": cs if os.path.isfile(cs) else None}


def section_listen(job, cli, outdir) -> dict:
    api = import_hark()
    if api is None:
        return {"status": "skipped", "summary": "hark cannot be imported: run QA without --listen and call the hark "
                "launcher (skills/hark/scripts/hark timeline ...) directly, or repair the venv"}
    outcome, remarks = {}, []
    lx = dict(_listen_paths(job, cli), api=api, outdir=outdir)
    if cli.listen and lx["mp"] and os.path.isfile(lx["mp"]):
        _timeline(lx, outcome, remarks)
        _critique(lx, outcome, remarks)
    not_real = _realness(api, job, outcome, remarks) if cli.realness else []
    status = "warn" if not_real or remarks else "ok"
    return {"status": status, "summary": f"{len(not_real)} recording(s) not real; {len(remarks)} failed call(s)", "notes": remarks, **outcome}


# ---------------------------------------------------------------- main

def _lufs(sig) -> float:
    return mst.integrated(sig) if sig is not None and len(sig) >= int(0.4 * SR) else float("-inf")


def _layers_groups(job: dict, master_gain: float) -> tuple:
    """(layers, groups, notes); layers and groups are None without a usable trace."""
    if job["trace"] is None:
        return None, None, []
    layers, remarks = rebuild(job)
    return layers, (group(layers, master_gain) if layers is not None else None), remarks


def _skip(job: dict) -> dict:
    return {"status": "skipped", "summary": "no trace: run the mix with --trace" if job["trace"] is None else
            "the trace and the report disagree: run the mix again with --trace"}


def _group_sections(job: dict, cli, clusters, master_gain: float) -> dict:
    fns = (("audibility", lambda: section_audibility(job, clusters, master_gain, cli.layers)),
           ("harshness", lambda: section_harshness(job, clusters, master_gain)),
           ("sync", lambda: section_sync(clusters)), ("density", lambda: section_density(job, clusters)))
    return {label: fn() if clusters is not None else dict(_skip(job)) for label, fn in fns}


def _listen_section(job: dict, lo_i) -> dict:
    outdir = os.path.dirname(mix.rpath(job["root"], lo_i.out)) or "."
    os.makedirs(outdir, exist_ok=True)
    if lo_i.listen or lo_i.realness:
        return section_listen(job, lo_i, outdir)
    return {"status": "skipped", "summary": "not requested (--listen, --realness)"}


def _overall(sections: dict, master_gain: float, master_scale: bool) -> tuple[str, str]:
    """(summary line, worst status)."""
    worst = max((val["status"] for val in sections.values()), key=lambda val: ORDER[val])
    named = lambda status: ", ".join(key_ for key_, val in sections.items() if val["status"] == status) or "none"
    scale = f"levels at master scale (master {master_gain:+.2f} dB over the stems)" if master_scale else "levels at stem scale"
    return (f"QA {worst.upper()}: fail {named('fail')}; warn {named('warn')}; not run {named('skipped')}; {scale}"), worst


def _gain(job: dict) -> tuple[float, bool, float]:
    """(premaster LUFS, both measurable, master minus premaster in dB or 0)."""
    state = job["stems"]
    pre, mas = _lufs(state["sfx"] + state["amb"] + state["music"]), _lufs(job["master"])
    scaled = math.isfinite(pre) and math.isfinite(mas)
    return pre, scaled, (mas - pre if scaled else 0.0)


def analyse(cli) -> dict:
    job = load_inputs(cli)
    state = job["stems"]
    job["no_bed"] = not np.any(state["music"]) and not np.any(state["amb"])
    target = float((job["design"].get("master") or {}).get("lufs", -14))
    pre, scaled, master_gain = _gain(job)
    layers, clusters, remarks = _layers_groups(job, master_gain)
    sections = {"clicks": section_clicks(job, layers, master_gain),
                "loudness": section_loudness(job, target, round(pre, 2) if math.isfinite(pre) else None),
                "coverage": section_coverage(job, clusters),
                "edges": section_edges(job) if job["trace"] is not None else dict(_skip(job))}
    sections.update(_group_sections(job, cli, clusters, master_gain))
    sections["listen"] = _listen_section(job, cli)
    if all(not np.any(sig) for sig in state.values()):
        sections["mix"] = {"status": "fail", "summary": "every stem is silent: check the trims (--dump), gains and cue matches"}
    summary, worst = _overall(sections, master_gain, scaled and job["master"] is not None)
    return {"summary": summary, "status": worst, "note": "; ".join(remarks) or None, "sections": sections,
            "inputs": job["paths"]}


def _owner_text(clk: dict) -> str:
    return f"{clk['owners'][0]['what']} ({clk['owners'][0]['where']})" if clk["owners"] else "no owner"


def _click_rows(val: dict) -> list[str]:
    return [f"{clk['in']} {tfmt(clk['t'])} {clk['level_db']} dB " + _owner_text(clk) for clk in val.get("items", [])[:8]]


def _edge_rows(val: dict) -> list[str]:
    table = [f"{ix.get('sample') or ix.get('slices')}: {'; '.join(ix['defects'])}" for ix in val.get("items", []) if ix.get("defects")][:8]
    return table + [f"music {rec['label']}: {'; '.join(rec['defects'])}" for rec in val.get("music_splices", []) if rec["defects"]][:4]


def _read_text(rec: dict) -> str:
    return f" read {rec['read_db']} dB ({rec['covered_by']})" if "read_db" in rec else ""


def _audibility_rows(val: dict) -> list[str]:
    return [f"{tfmt(rec['t'])} {rec['label']}: {rec['verdict']}" + _read_text(rec) for rec in val.get("items", [])[:6]]


def _harshness_rows(val: dict) -> list[str]:
    return [f"{tfmt(rec['t'])} {rec['label']}: {'; '.join(rec['issues'])}" for rec in val.get("items", []) if rec["issues"]][:5]


def _sync_rows(val: dict) -> list[str]:
    return [f"{tfmt(rec['t_cue'])} {rec['label']}: {rec['issue'] or rec.get('explanation')}" for rec in val.get("items", [])[:8]]


_DIGEST_ROWS = {"clicks": _click_rows, "edges": _edge_rows, "audibility": _audibility_rows,
                "harshness": _harshness_rows, "sync": _sync_rows}


def digest(outcome: dict) -> str:
    acc = [outcome["summary"]] + (["note: " + outcome["note"]] if outcome.get("note") else [])
    for label, val in outcome["sections"].items():
        acc.append(f"{label.upper():11s} {val['status'].upper():7s} {val.get('summary', '')}")
        acc += ["    " + rec for rec in _DIGEST_ROWS.get(label, lambda _: [])(val)]
    return "\n".join(acc)


def parse(argv):
    ap = argparse.ArgumentParser(prog="qa.py", description="Measured QA of a render.")
    ap.add_argument("--root", default=".")
    ap.add_argument("--design", default="audio/sounddesign.json")
    ap.add_argument("--report")
    ap.add_argument("--trace", default="out/qa/adds.json")
    ap.add_argument("--stems")
    ap.add_argument("--master")
    ap.add_argument("--cues")
    ap.add_argument("--cues-sounded", default="out/qa/cues_sounded.json")
    ap.add_argument("--only")
    ap.add_argument("--cache")
    ap.add_argument("--video")
    ap.add_argument("--out", default="out/qa/qa.json")
    ap.add_argument("--layers", action="store_true")
    ap.add_argument("--listen", action="store_true")
    ap.add_argument("--realness", action="store_true")
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--self-test", action="store_true")
    return ap.parse_args(argv)


def _run(cli) -> int:
    try:
        outcome = clean(analyse(cli))
    except (InputError, mix.DesignError) as ev:
        mix.ascii_print(f"qa.py: {ev}", file=sys.stderr)
        return 2
    with open(mix.rpath(os.path.abspath(cli.root), cli.out), "w") as fh:
        json.dump(outcome, fh, indent=1)
    mix.ascii_print(json.dumps(outcome, indent=1) if cli.json else digest(outcome))
    return 1 if outcome["status"] == "fail" else 0


def main(argv=None) -> int:
    cli = parse(sys.argv[1:] if argv is None else argv)
    return self_test() if cli.self_test else _run(cli)


def _qa_design(fade_in, offset=0.0) -> dict:
    return {"duration": 3.0, "samples": {"q.x": {"file": "x.wav", "fade_in": fade_in}}, "pools": {"q.x": ["q.x"]},
            "rules": [{"name": "q.r", "pool": "q.x", "align": "start", "offset": offset}],
            "ambience": [{"name": "q.bed", "file": "bed.wav", "at": 0, "until": 3, "fade_in": 0.2, "fade_out": 0.2}]}


def _mix_qa(tmp: str, design: str, cues: str = "c.json", master: bool = True, qa_args=()) -> tuple[int, dict]:
    """Mix design with a trace, then run QA. Returns (QA exit code, qa.json)."""
    flags = [] if master else ["--no-master"]
    mix.main(["--root", tmp, "--cues", cues, "--trace", "out/qa/adds.json", "--cache", "cache", "--design", design] + flags)
    code = main(["--root", tmp, "--design", design, "--cues", cues, "--cache", "cache"]
                + ([] if master else ["--master", "none"]) + list(qa_args))
    return code, mix.load_json(os.path.join(tmp, "out/qa/qa.json"))


def _test_clicks(ck, fx, tmp: str) -> None:
    """Q1, Q2: a master click at a placement start, and a fade-in that removes it."""
    fx.write(os.path.join(tmp, "bed.wav"), fx.noise_bed(3.0, 0.002, 4))  # quiet: its 14 kHz noise must not hide the click
    tsec = np.arange(int(0.2 * SR)) / SR
    tone = 0.5 * np.sin(2 * np.pi * 300 * tsec + 1.0) * np.exp(-8 * tsec)  # starts on a non-zero sample
    fx.write(os.path.join(tmp, "x.wav"), np.stack([tone, tone], 1), subtype="FLOAT")
    fx.put_json(os.path.join(tmp, "c.json"), {"fps": 60, "events": [{"f": 60, "kind": "hit", "id": "q:h:0"}]})
    fx.put_json(os.path.join(tmp, "d.json"), _qa_design(0))
    code, qa_out = _mix_qa(tmp, "d.json")
    cl = qa_out["sections"]["clicks"]
    edge = [clk for clk in cl["items"] if clk["in"] == "master" and clk["at_edge"]]
    ck("Q1 a master click at a placement start fails QA",
       bool(edge) and edge[0]["owners"][0]["where"] == "start" and (cl["status"], code) == ("fail", 1))
    fx.put_json(os.path.join(tmp, "d.json"), _qa_design(0.004))
    _, qa_out = _mix_qa(tmp, "d.json")
    cl = qa_out["sections"]["clicks"]
    ck("Q2 a 4 ms fade-in leaves no edge click",
       cl["status"] != "fail" and not any(clk["at_edge"] and clk["in"] == "master" for clk in cl["items"]))


def _test_masking(ck, fx, tmp: str) -> None:
    """Q3: an effect 10 dB under a broadband bed."""
    loud = fx.noise_bed(3.0, 0.2, 8)
    fx.write(os.path.join(tmp, "loud.wav"), loud)
    # the effect is the bed's own noise 10 dB down, so it is 10 dB under the bed in every band
    fx.write(os.path.join(tmp, "soft.wav"), loud[SR:SR + int(0.15 * SR)] * 10 ** (-10 / 20))
    fx.put_json(os.path.join(tmp, "d3.json"), {
        "duration": 3.0, "samples": {"q.s": {"file": "soft.wav"}}, "pools": {"q.s": ["q.s"]},
        "rules": [{"name": "q.s", "pool": "q.s"}],
        "ambience": [{"name": "q.loud", "file": "loud.wav", "at": 0, "until": 3, "fade_in": 0.1, "fade_out": 0.1}]})
    _, qa_out = _mix_qa(tmp, "d3.json", master=False)
    au = qa_out["sections"]["audibility"]
    ev = au["events"][0]
    ck("Q3 masked under the bed", (ev["verdict"], ev["covered_by"], au["status"]) == ("masked", "music/amb", "warn"))


def _test_offset(ck, fx, tmp: str) -> None:
    """Q4: a rule offset of 0.2 s on an impact is deliberate."""
    fx.write(os.path.join(tmp, "hit.wav"), fx.burst(0.1, 200, 40, 3))
    fx.put_json(os.path.join(tmp, "d4.json"), {
        "duration": 3.0, "samples": {"q.h": {"file": "hit.wav"}}, "pools": {"q.h": ["q.h"]},
        "rules": [{"name": "q.late", "pool": "q.h", "offset": 0.2}],
        "ambience": [{"name": "q.bed", "file": "bed.wav", "at": 0, "until": 3}]})
    _, qa_out = _mix_qa(tmp, "d4.json", master=False)
    ev = qa_out["sections"]["sync"]["events"][0]
    ck("Q4 the offset is deliberate", bool(ev.get("deliberate")) and ev["offset_rule"] == "q.late" and not ev["issue"])


def _test_density(ck, fx, tmp: str) -> None:
    """Q5, Q6, Q7, Q8: machine gun, partial coverage, no trace, missing stems."""
    evs = [{"f": 20 + 15 * key_, "kind": "tick", "id": f"a:k:{key_}"} for key_ in range(4)]
    evs += [{"f": 120 + 10 * key_, "kind": "tap", "id": f"b:t:{key_}"} for key_ in range(6)]
    fx.put_json(os.path.join(tmp, "c5.json"), {"fps": 60, "events": evs})
    fx.put_json(os.path.join(tmp, "d5.json"), {
        "duration": 3.0, "samples": {"q.h": {"file": "hit.wav"}}, "pools": {"q.h": ["q.h"]},
        "rules": [{"name": "q.mg", "pool": "q.h", "match": {"kind": "tap"}},
                  {"name": "q.part", "pool": "q.h", "match": {"id": "a:k:[01]$"}}]})
    _, qa_out = _mix_qa(tmp, "d5.json", cues="c5.json", master=False)
    sec = qa_out["sections"]
    ck("Q5 machine gun", "q.mg" in sec["density"]["machine_gun"] and sec["density"]["status"] == "warn")
    part = [(proj["class"], proj["unsounded"], proj["of"]) for proj in sec["coverage"]["partial"]]
    ck("Q6 partial class a:k with 2 of 4 unsounded", ("a:k", 2, 4) in part and sec["coverage"]["status"] == "warn")
    os.remove(os.path.join(tmp, "out/qa/adds.json"))
    main(["--root", tmp, "--design", "d5.json", "--cues", "c5.json", "--master", "none", "--cache", "cache"])
    sec = mix.load_json(os.path.join(tmp, "out/qa/qa.json"))["sections"]
    skipped = {val for val in sec if sec[val]["status"] == "skipped"}
    ck("Q7 trace sections skipped, the rest ran",
       {"edges", "audibility", "harshness", "sync", "density"} <= skipped and not {"clicks", "coverage"} & skipped)
    ck("Q8 missing stems exit 2", main(["--root", tmp, "--design", "d5.json", "--stems", "nope"]) == 2)


def _test_empty_trim(ck, fx, tmp: str) -> None:
    """Q9: a trim past the end of its file."""
    fx.put_json(os.path.join(tmp, "d9.json"), {
        "duration": 3.0, "samples": {"q.e": {"file": "hit.wav", "start": 5.0}}, "pools": {"q.e": ["q.e"]},
        "rules": [{"name": "q.e", "pool": "q.e"}],
        "ambience": [{"name": "q.bed", "file": "bed.wav", "at": 0, "until": 3}]})
    code, qa_out = _mix_qa(tmp, "d9.json", master=False)
    sec = qa_out["sections"]
    ck("Q9 empty trim: edges issue, audibility silent, fail",
       "empty" in " ".join(sec["edges"]["items"][0]["defects"]) and sec["audibility"]["events"][0]["verdict"] == "silent"
       and (sec["audibility"]["status"], code) == ("fail", 1))


def _splice(tmp: str, ok_reason) -> dict:
    """The first music splice of a segment that ends inside a held chord, then silence."""
    seg = {"label": "bars 1-2", "src_from": 0.0, "src_to": 2.0, "at": 0.0}
    if ok_reason:
        seg["qa_ok"] = ok_reason
    seg2 = {"label": "breakdown", "src_from": 4.0, "src_to": 5.0, "at": 2.0}
    with open(os.path.join(tmp, "d10.json"), "w") as fh:
        json.dump({"duration": 4.0, "music": {"file": "chord.wav", "src_bpm": 120, "stretch": "none",
                                              "segments": [seg, seg2]}}, fh)
    mix.main(["--root", tmp, "--trace", "out/qa/adds.json", "--cache", "cache", "--design", "d10.json", "--no-master"])
    main(["--root", tmp, "--design", "d10.json", "--master", "none", "--cache", "cache"])
    return mix.load_json(os.path.join(tmp, "out/qa/qa.json"))["sections"]["edges"]["music_splices"][0]


def _test_splice(ck, fx, tmp: str) -> None:
    """Q10: a music segment that ends inside a held chord, then silence."""
    tsec = np.arange(SR * 6) / SR
    chord = 0.3 * (np.sin(2 * np.pi * 220 * tsec) + np.sin(2 * np.pi * 330 * tsec))
    chord[4 * SR:] *= 0.0005
    fx.write(os.path.join(tmp, "chord.wav"), np.stack([chord, chord], 1))
    sp = _splice(tmp, None)
    ck("Q10 the splice chops the music", any("chops the music" in meas for meas in sp["defects"]))
    sp = _splice(tmp, "a hard cut into the breakdown")
    ck("Q10 qa_ok turns the chop into a note", not sp["defects"] and any("acknowledged" in count for count in sp["notes"]))


def _test_project(ck, fx, tmp: str) -> None:
    """W3: the full fixture project, every section present."""
    proj = fx.project(os.path.join(tmp, "film"))
    mix.main(["--root", proj["root"], "--trace", "out/qa/adds.json"])
    code = main(["--root", proj["root"], "--layers"])
    qa_out = mix.load_json(os.path.join(proj["root"], "out/qa/qa.json"))
    need = {"clicks", "loudness", "coverage", "edges", "audibility", "harshness", "sync", "density", "listen"}
    skipped = [key_ for key_, val in qa_out["sections"].items() if val["status"] == "skipped"]
    ck("W3 every section present, only listen skipped", need <= set(qa_out["sections"]) and code in (0, 1) and skipped == ["listen"])


def self_test() -> int:
    import tempfile
    import fixtures as fx
    ck = fx.Checker()
    with tempfile.TemporaryDirectory() as tmp:
        for test in (_test_clicks, _test_masking, _test_offset, _test_density, _test_empty_trim, _test_splice,
                     _test_project):
            test(ck, fx, tmp)
    return ck.code()


if __name__ == "__main__":
    sys.exit(main())
