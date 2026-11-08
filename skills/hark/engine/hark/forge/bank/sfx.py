"""``hark sfx``: list the bank, render one take, audit any sound effect file.

Spec: docs/specs/hark/bank-kit-sfx-sfx.md (part 2). The audit measures. It
never listens and loads no model.
"""

from __future__ import annotations

import argparse
import math
import os
from collections.abc import Callable
from typing import Any

import numpy as np

from hark.forge.bank import bank
from hark.forge.bank._measures import SR, mono, rms_env

DESIGNED = frozenset({"glitch", "pro_glitch"})
GATED = frozenset({"glitch", "pro_glitch", "riser", "pro_riser", "pro_zip", "pro_impact"})
HIT_ROLES = frozenset({"hit", "land"})
HIT_CATEGORIES = frozenset({"impact", "pro_impact", "logo_sting", "pro_logo_sting"})
SILENT_PEAK_DB = -60.0
SILENT_LIVE_MS = 5.0
LIVE_DB = -40.0
EDGE_DB = -40.0
DC_DB = -40.0
RING_Z = 15.0
TRUNC_DB = -45.0
SILENCE_MS = 4.0
TAIL_DB = 6.0
GLIDE_ST = 4.0
GLIDE_MIN_S = 0.05
GLIDE_WIN, GLIDE_HOP, GLIDE_NFFT = 1024, 240, 8192
GLIDE_BAND = (80.0, 10000.0)
GLIDE_ACTIVE_DB = -30.0
GLIDE_SPLIT_FRAMES, GLIDE_SPLIT_ST = 2, 3.0
GLIDE_TRACK_MIN, GLIDE_MEDIAN = 6, 5
LAYER_WIN, LAYER_HOP = 1200, 240
LAYER_PEAK_DB, LAYER_DIP_DB, LAYER_SPAN_S = -6.0, 10.0, 0.5
PRE_SWELL_MS = 100.0
ENDS_ON_PEAK = 0.97
DRIP_MAX_ST, DRIP_START_S, DRIP_SPAN_S = 14.0, 0.06, 0.25
MEASURE_KEYS = ("clicks", "edge_db", "clip_runs", "overs", "dc_db", "ring_z", "trunc_db", "silence_ms", "bump_db",
                "attack_ms", "t40_ms", "crest_db", "res_db", "res_hz")
SR_CHOICES = (44100, 48000, 88200, 96000)


# ---------------------------------------------------------------- detectors


def live_ms(y: np.ndarray, sr: int = SR, db: float = LIVE_DB) -> float:
    """Total time (ms) of 1 ms windows within ``db`` of the loudest (part 2, 3.8)."""
    m = mono(y)
    win, hop = int(round(sr / 1000)), max(1, int(round(sr / 4000)))
    if len(m) < win:
        m = np.concatenate([m, np.zeros(win - len(m))])
    ms = rms_env(m, win, hop) ** 2
    if ms.max() <= 0:
        return 0.0
    return round(float(np.count_nonzero(ms >= ms.max() * 10 ** (db / 10))) * hop * 1000 / sr, 2)


def _band_power(m: np.ndarray, sr: int) -> tuple[np.ndarray, np.ndarray]:
    """Frame power spectra inside GLIDE_BAND, and their frequencies."""
    if len(m) < GLIDE_WIN:
        m = np.concatenate([m, np.zeros(GLIDE_WIN - len(m))])
    count = 1 + (len(m) - GLIDE_WIN) // GLIDE_HOP
    idx = np.arange(GLIDE_WIN)[None, :] + GLIDE_HOP * np.arange(count)[:, None]
    power = np.abs(np.fft.rfft(m[idx] * np.hanning(GLIDE_WIN), GLIDE_NFFT, axis=1)) ** 2
    freqs = np.fft.rfftfreq(GLIDE_NFFT, 1.0 / sr)
    keep = (freqs >= GLIDE_BAND[0]) & (freqs <= GLIDE_BAND[1])
    return power[:, keep], freqs[keep]


def _glide_frames(m: np.ndarray, sr: int) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    """Per frame: band power T, strongest frequency fp, and the power near fp."""
    power, freqs = _band_power(m, sr)
    total = power.sum(axis=1)
    fp = freqs[np.argmax(power, axis=1)]
    half = np.maximum(fp * (2 ** (3 / 12) - 1), 2 * sr / GLIDE_WIN)
    near = (np.abs(freqs[None, :] - fp[:, None]) <= half[:, None])
    return total, fp, (power * near).sum(axis=1)


def _tracks(frames: np.ndarray, st: np.ndarray) -> list[list[int]]:
    out: list[list[int]] = []
    for i in frames:
        if out and i - out[-1][-1] <= GLIDE_SPLIT_FRAMES and abs(st[i] - st[out[-1][-1]]) <= GLIDE_SPLIT_ST:
            out[-1].append(int(i))
        else:
            out.append([int(i)])
    return [t for t in out if len(t) >= GLIDE_TRACK_MIN]


def _running_median(v: np.ndarray, width: int) -> np.ndarray:
    h = width // 2
    return np.array([np.median(v[max(0, i - h): i + h + 1]) for i in range(len(v))])


def _track_climb(track: list[int], st: np.ndarray, t: np.ndarray) -> tuple[float, float, float]:
    s = _running_median(st[track], GLIDE_MEDIAN)
    low = np.minimum.accumulate(s)
    climb = s - low
    k = int(np.argmax(climb))
    start = int(np.argmin(s[: k + 1]))
    return float(climb[k]), float(t[track[start]]), float(t[track[k]])


def _tonal_frames(total: np.ndarray, near: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Active frames (within GLIDE_ACTIVE_DB of the loudest) and the tonal ones among them."""
    active = total >= total.max() * 10 ** (GLIDE_ACTIVE_DB / 10) if total.max() > 0 else total > 0
    return active, active & (near >= 0.5 * total)


def _best_climb(tonal: np.ndarray, st: np.ndarray, t: np.ndarray) -> tuple[float, Any, Any]:
    best: tuple[float, Any, Any] = (0.0, None, None)
    for track in _tracks(np.nonzero(tonal)[0], st):
        climb = _track_climb(track, st, t)
        if climb[0] > best[0]:
            best = climb
    return best


def _round_or_none(v: float | None, digits: int) -> float | None:
    return None if v is None else round(v, digits)


def rising_glide(y: np.ndarray, sr: int = SR) -> dict[str, Any]:
    """The largest pitch climb of a tonal track (part 2, 3.6)."""
    total, fp, near = _glide_frames(mono(y), sr)
    active, tonal = _tonal_frames(total, near)
    st = 12 * np.log2(np.maximum(fp, 1e-9) / 440.0)
    t = np.arange(len(total)) * GLIDE_HOP / sr + (GLIDE_WIN / 2) / sr
    rise, fr, to = _best_climb(tonal, st, t)
    share = round(float(tonal.sum()) / max(1, int(active.sum())), 2)
    return {"tonal_share": share, "rise_st": round(rise, 1), "from_s": _round_or_none(fr, 3),
            "to_s": _round_or_none(to, 3)}


def _layer_env(m: np.ndarray) -> np.ndarray:
    count = 1 + (len(m) - LAYER_WIN) // LAYER_HOP
    csum = np.concatenate([[0.0], np.cumsum(m * m)])
    starts = np.arange(count) * LAYER_HOP
    e = 10 * np.log10((csum[starts + LAYER_WIN] - csum[starts]) / LAYER_WIN + 1e-24)
    return e - e.max()


def _is_peak(e: np.ndarray, i: int) -> bool:
    """A local maximum of ``e`` within LAYER_PEAK_DB of the loudest."""
    rises = i == 0 or e[i] >= e[i - 1]
    falls = i == len(e) - 1 or e[i] >= e[i + 1]
    return bool(e[i] >= LAYER_PEAK_DB and rises and falls)


def _no_dip(e: np.ndarray, last: int, c: int) -> bool:
    """True when the level between two peaks never dips LAYER_DIP_DB below the lower one."""
    return bool(e[last: c + 1].min() > min(e[last], e[c]) - LAYER_DIP_DB)


def _attack_frames(e: np.ndarray, sr: int) -> list[int]:
    kept: list[int] = []
    span = LAYER_SPAN_S * sr / LAYER_HOP
    for c in (i for i in range(len(e)) if _is_peak(e, i)):
        if kept and c - kept[0] > span:
            break
        if kept and _no_dip(e, kept[-1], c):
            kept[-1] = c if e[c] > e[kept[-1]] else kept[-1]
        else:
            kept.append(c)
    return kept


def layers(y: np.ndarray, sr: int = SR) -> dict[str, Any]:
    """Separate attacks and the pre-swell of a hit (part 2, 3.7)."""
    m = mono(y)
    if len(m) < LAYER_WIN + 2 * LAYER_HOP:
        return {"attacks": [0.0], "count": 1, "pre_swell_ms": 0.0}
    e = _layer_env(m)
    i30 = int(np.argmax(e >= -30.0))
    i6 = int(np.argmax(e >= LAYER_PEAK_DB))
    kept = _attack_frames(e, sr)
    attacks = [round((i * LAYER_HOP + LAYER_WIN / 2) / sr, 3) for i in kept]
    return {"attacks": attacks, "count": len(attacks), "pre_swell_ms": round((i6 - i30) * LAYER_HOP * 1000 / sr, 1)}


# ---------------------------------------------------------------- the audit


def _polish(x: np.ndarray) -> dict[str, Any]:
    from hark.forge.engine import polish

    m = mono(x)
    got: dict[str, Any] = {}
    for fn in (polish.clicks, polish.dc, polish.ring, polish.truncated, polish.silence, polish.tail,
               polish.attack, polish.body, polish.resonances):
        got.update(fn(m))
    got.update(polish.clipping(np.asarray(x, dtype=np.float64)))
    return got


def _load(x: Any, sr: int | None) -> tuple[np.ndarray, str | None]:
    if isinstance(x, (str, os.PathLike)):
        from hark.core import audio

        return np.asarray(audio.load(str(x), sr=SR), dtype=np.float64), os.path.basename(str(x))
    y = np.asarray(x, dtype=np.float64)
    if sr is not None and int(sr) != SR:
        from hark.core import audio

        y = np.asarray(audio.resample(y, int(sr), SR), dtype=np.float64)
    return y, None


class _Audit:
    """Collects warnings and notes of one audit in rule order."""

    def __init__(self) -> None:
        self.warnings: list[dict] = []
        self.notes: list[dict] = []

    def add(self, code: str, msg: str, warn: bool = True) -> None:
        (self.warnings if warn else self.notes).append({"code": code, "msg": msg})


def _silent(a: _Audit, peak: float, live: float) -> bool:
    peak_db = 20 * math.log10(peak) if peak > 0 else -math.inf
    if peak_db >= SILENT_PEAK_DB and live >= SILENT_LIVE_MS:
        return False
    if peak == 0:
        a.add("silent", "digital silence")
    else:
        a.add("silent", f"silent: peak {peak_db:.1f} dBFS, live for {live:.1f} ms")
    return True


def _above(p: dict, key: str, limit: float) -> bool:
    """True when measure ``key`` exists and is over ``limit``."""
    return p.get(key) is not None and p[key] > limit


def _count(p: dict, key: str) -> float:
    return p.get(key) or 0


def _rule_clicks(a: _Audit, p: dict, designed: bool, gated: bool) -> None:
    if _count(p, "clicks") > 0:
        times = ", ".join(f"{t:.3f}" for t in (p.get("click_times") or [])[:4])
        a.add("clicks", f"{p['clicks']} click(s) inside the sound at {times} s", not designed)


def _rule_edge(a: _Audit, p: dict, designed: bool, gated: bool) -> None:
    if _above(p, "edge_db", EDGE_DB):
        a.add("edge", f"the sound starts or ends at {p['edge_db']:.1f} dB re its peak: it clicks when played")


def _rule_clipping(a: _Audit, p: dict, designed: bool, gated: bool) -> None:
    runs, overs = _count(p, "clip_runs"), _count(p, "overs")
    if runs > 0 or overs > 0:
        a.add("clipping", f"clipped: {runs} flat top(s), {overs} sample(s) over full scale")


def _rule_dc(a: _Audit, p: dict, designed: bool, gated: bool) -> None:
    if _above(p, "dc_db", DC_DB):
        a.add("dc", f"DC offset at {p['dc_db']:.1f} dB re the peak")


def _rule_ring(a: _Audit, p: dict, designed: bool, gated: bool) -> None:
    if _above(p, "ring_z", RING_Z):
        a.add("ring", f"metallic comb ring in the tail ({_count(p, 'ring_ms'):.1f} ms delay, z {p['ring_z']:.1f})")


def _rule_truncated(a: _Audit, p: dict, designed: bool, gated: bool) -> None:
    if not _above(p, "trunc_db", TRUNC_DB):
        return
    if gated:
        a.add("truncated", f"the tail stops at {p['trunc_db']:.1f} dB (the stop is the design)", False)
    else:
        a.add("truncated", f"the tail is cut at {p['trunc_db']:.1f} dB re the peak: fade it out or render it longer")


def _rule_silence(a: _Audit, p: dict, designed: bool, gated: bool) -> None:
    if _count(p, "silence_ms") > SILENCE_MS:
        a.add("silence", f"{p['silence_ms']:.1f} ms of digital silence inside the sound", not designed)


def _rule_tail(a: _Audit, p: dict, designed: bool, gated: bool) -> None:
    if _count(p, "bump_db") > TAIL_DB:
        a.add("tail", f"the level climbs {p['bump_db']:.1f} dB again after the peak (a second event or a roll)", False)


ARTIFACT_RULES = (_rule_clicks, _rule_edge, _rule_clipping, _rule_dc, _rule_ring, _rule_truncated, _rule_silence,
                  _rule_tail)


def _artifact_rules(a: _Audit, p: dict, designed: bool, gated: bool) -> None:
    for rule in ARTIFACT_RULES:
        rule(a, p, designed, gated)


def drip_allowed(glide: dict) -> bool:
    """True when a drip's climb is a bubble's (part 2, 3.9)."""
    if glide["from_s"] is None:
        return True
    return (glide["rise_st"] <= DRIP_MAX_ST and glide["from_s"] <= DRIP_START_S
            and glide["to_s"] - glide["from_s"] <= DRIP_SPAN_S)


def _glide_rule(a: _Audit, glide: dict, category: str | None) -> None:
    if glide["from_s"] is None or glide["rise_st"] <= GLIDE_ST or glide["to_s"] - glide["from_s"] < GLIDE_MIN_S:
        return
    if category == "drip" and drip_allowed(glide):
        a.add("rising_glide", "a bubble rising (drip allowance)", False)
        return
    span = glide["to_s"] - glide["from_s"]
    a.add("rising_glide", f"a rising synth whip reads as cheap: +{glide['rise_st']:.1f} st over {span * 1000:.0f} ms "
                          f"({glide['from_s']:.3f}-{glide['to_s']:.3f} s)")


def _layer_rules(a: _Audit, lay: dict, role: str | None, category: str | None) -> None:
    hit_role = role in HIT_ROLES
    if not hit_role and category not in HIT_CATEGORIES:
        return
    if lay["count"] >= 2:
        at = ", ".join(f"{t:.2f}" for t in lay["attacks"])
        a.add("stacked", f"{lay['count']} separate attacks at {at} s: one hit should have one attack", hit_role)
    if lay["pre_swell_ms"] > PRE_SWELL_MS:
        a.add("pre_swell", f"a {lay['pre_swell_ms']:.0f} ms build before the hit", hit_role)


def _summary(name: str, a: _Audit) -> str:
    if a.warnings:
        n = len(a.warnings)
        return f"{name}: {n} warning{'' if n == 1 else 's'}: " + "; ".join(w["msg"] for w in a.warnings)
    if a.notes:
        return f"{name}: clean (" + "; ".join(w["msg"] for w in a.notes) + ")"
    return f"{name}: clean"


def _measures(p: dict, peak: float, live: float) -> dict[str, Any]:
    out = {k: p.get(k) for k in MEASURE_KEYS}
    out["peak_dbfs"] = None if peak == 0 else round(20 * math.log10(peak), 2)
    out["live_ms"] = live
    return out


def check(x: Any, sr: int | None = None, category: str | None = None, role: str | None = None,
          sound: str | None = None) -> dict[str, Any]:
    """Audit one sound: ``{ok, warnings, notes, summary, measures, glide, layers, context}``."""
    y, base = _load(x, sr)
    context = {k: v for k, v in (("category", category), ("role", role), ("sound", sound)) if v}
    a = _Audit()
    peak = float(np.max(np.abs(y))) if y.size else 0.0
    live = live_ms(y)
    result: dict[str, Any] = {"context": context, "glide": None, "layers": None}
    if _silent(a, peak, live):
        result["measures"] = _measures({}, peak, live)
    else:
        result.update(_audible_rules(a, y, category, role))
        result["measures"] = _measures(result.pop("polish"), peak, live)
    result.update(ok=not a.warnings, warnings=a.warnings, notes=a.notes,
                  summary=_summary(sound or base or "sound", a))
    return result


def _ends_on_peak(y: np.ndarray) -> bool:
    m = mono(y)
    return int(np.argmax(np.abs(m))) >= ENDS_ON_PEAK * len(m)


def _audible_rules(a: _Audit, y: np.ndarray, category: str | None, role: str | None) -> dict[str, Any]:
    """Run the rules after ``silent``; returns ``{polish, glide, layers}``."""
    p = _polish(y)
    gated = category in GATED or _ends_on_peak(y)
    _artifact_rules(a, p, category in DESIGNED, gated)
    glide, lay = rising_glide(y), layers(y)
    _glide_rule(a, glide, category)
    _layer_rules(a, lay, role, category)
    return {"polish": p, "glide": glide, "layers": lay}


# ---------------------------------------------------------------- list, render, check


def sfx_list(category: str | None = None, kind: str | None = None) -> dict[str, Any]:
    """``{summary, version, sounds}``: the bank rows with the filters."""
    rows = bank.list_sounds(category=category, kind=kind)
    version = bank.VERSION
    filters = " ".join(f"{k} {v}" for k, v in (("category", category), ("kind", kind)) if v is not None)
    summary = f"{len(rows)} sounds in the bank {version}" + (f" ({filters})" if filters else "")
    return {"summary": summary, "version": version, "sounds": rows}


def write(path: str, y: np.ndarray, sr: int = SR) -> str:
    """Write a 24-bit PCM WAV, creating the parent folder; returns the path with ``/``."""
    from hark.core import audio

    return str(audio.write_wav(str(path), np.asarray(y), sr, "PCM_24")).replace(os.sep, "/")


def _render_summary(meta: dict, file: str | None) -> str:
    head = (f"{meta['id']} seed {meta['seed']}: {meta['dur_s']:.3f} s, onset {meta['onset_s']:.3f} s, "
            f"peak at {meta['peak_s']:.3f} s, end {meta['end_s']:.3f} s, {meta['peak_db']:.1f} dBFS")
    if meta.get("lufs_m_max") is not None:
        head += f", {meta['lufs_m_max']:.1f} LUFS momentary max"
    parts = [head] + _edit_parts(meta.get("edits") or {})
    checks = meta.get("checks")
    if checks is not None:
        parts.append("checks: clean" if checks["ok"] else "checks: " + "; ".join(w["msg"] for w in checks["warnings"]))
    line = "; ".join(parts)
    return f"wrote {file}: {line}" if file else line


def _edit_parts(edits: dict) -> list[str]:
    parts = []
    if "apex" in edits:
        e = edits["apex"]
        parts.append(f"apex {e.get('target')} s (measured {e.get('measured')}, {e.get('err_ms', 0):+} ms)")
    if "dur" in edits:
        e = edits["dur"]
        parts.append(f"dur {e.get('target')} s ({e.get('method')}, {e.get('fit') or 'exact'})")
    if "pitch" in edits:
        e = edits["pitch"]
        measured = f" (measured {e['measured_hz']} Hz)" if e.get("measured_hz") is not None else ""
        parts.append(f"pitch {e.get('note')}{measured}")
    return parts


def sfx_render(sound: str, out: str | None = None, seed: int | None = None, dur: float | None = None,
               apex: float | None = None, pitch: Any = None, peak_db: float = -1.0, weight: float | None = None,
               sr: int = SR) -> dict[str, Any]:
    """Render one take with every knob and the audit: ``{summary, file, meta}``."""
    y, meta = bank.render(sound, seed=seed, dur=dur, apex=apex, pitch=pitch, peak_db=peak_db, weight=weight,
                          sr=sr, checks=True)
    file = write(out, y, meta["sr"]) if out else None
    return {"summary": _render_summary(meta, file), "file": file, "meta": meta}


def sfx_check(path_or_paths: Any, category: str | None = None, role: str | None = None) -> dict[str, Any]:
    """Audit one file, or a list of files with a total."""
    if isinstance(path_or_paths, (str, os.PathLike)):
        path = str(path_or_paths)
        return {**check(path, category=category, role=role, sound=os.path.basename(path)), "file": path}
    files = [sfx_check(p, category, role) for p in path_or_paths]
    clean = sum(1 for f in files if f["ok"])
    return {"summary": f"{clean} / {len(files)} clean", "ok": clean == len(files), "files": files}


# ---------------------------------------------------------------- CLI


def _list_text(result: dict) -> str:
    lines = [result["summary"], f"{'id':<30}{'dur s':>6}{'peak s':>7}  kinds / knobs"]
    for r in result["sounds"]:
        kinds = ",".join(r["kinds"]) or "-"
        lines.append(f"{r['id']:<30}{r['dur_s'] or 0:6.3f} {r['peak_s'] or 0:6.3f}  {kinds} | {knob_words(r['knobs'])}")
    return "\n".join(lines)


def knob_words(knobs: dict) -> str:
    """``dur <method>``, then ``apex`` and ``pitch <note>`` when they apply."""
    words = [f"dur {knobs.get('dur')}"]
    if knobs.get("apex"):
        words.append("apex")
    if knobs.get("pitch"):
        words.append(f"pitch {knobs['pitch'].get('note') or 'measured'}")
    return ", ".join(words)


def _check_text(result: dict) -> str:
    if "files" in result:
        return "\n".join([result["summary"]] + [f["summary"] for f in result["files"]])
    return result["summary"]


def _emit(result: dict, args: argparse.Namespace, text: str) -> None:
    from hark.core import report

    report.emit(result, args, text=text)


def _cmd_list(args: argparse.Namespace) -> int:
    result = sfx_list(args.category, args.kind)
    _emit(result, args, _list_text(result))
    return 0


def _cmd_render(args: argparse.Namespace) -> int:
    result = sfx_render(args.sound, out=args.output, seed=args.seed, dur=args.dur, apex=args.apex,
                        pitch=args.pitch, peak_db=args.peak, weight=args.weight, sr=args.sr)
    _emit(result, args, result["summary"])
    return 0


def _cmd_check(args: argparse.Namespace) -> int:
    target = args.files[0] if len(args.files) == 1 else args.files
    result = sfx_check(target, category=args.category, role=args.role)
    _emit(result, args, _check_text(result))
    return 0 if result["ok"] else 1


def _help_of(parser: argparse.ArgumentParser) -> Callable[[argparse.Namespace], int]:
    def show(args: argparse.Namespace) -> int:
        parser.print_help()
        return 2

    return show


def add_cli(sub: argparse._SubParsersAction) -> None:
    """Register ``hark sfx`` with its ``list``, ``render`` and ``check`` sub-commands."""
    from hark.core import report

    p = sub.add_parser("sfx", help="one bank sound by id, aimed at the picture; audit any effect")
    p.set_defaults(func=_help_of(p))
    s = p.add_subparsers(dest="sfx_command")
    for leaf in (_add_list(s), _add_render(s), _add_check(s)):
        report.add_common(leaf)


def _add_list(s: argparse._SubParsersAction) -> argparse.ArgumentParser:
    lp = s.add_parser("list", help="list the bank sounds")
    lp.add_argument("--category")
    lp.add_argument("--kind")
    lp.set_defaults(func=_cmd_list)
    return lp


def _add_render(s: argparse._SubParsersAction) -> argparse.ArgumentParser:
    rp = s.add_parser("render", help="render one take of a bank sound")
    rp.add_argument("sound")
    rp.add_argument("-o", "--output", required=True)
    rp.add_argument("--seed", type=int)
    rp.add_argument("--dur", type=float)
    rp.add_argument("--apex", type=float)
    rp.add_argument("--pitch")
    rp.add_argument("--peak", type=float, default=-1.0)
    rp.add_argument("--weight", type=float)
    rp.add_argument("--sr", type=int, default=SR, choices=SR_CHOICES)
    rp.set_defaults(func=_cmd_render)
    return rp


def _add_check(s: argparse._SubParsersAction) -> argparse.ArgumentParser:
    cp = s.add_parser("check", help="audit sound effect files with measurements only")
    cp.add_argument("files", nargs="+")
    cp.add_argument("--category")
    cp.add_argument("--role")
    cp.set_defaults(func=_cmd_check)
    return cp
