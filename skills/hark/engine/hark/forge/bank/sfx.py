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
    flat = mono(y)
    win, hop = int(round(sr / 1000)), max(1, int(round(sr / 4000)))
    if len(flat) < win:
        flat = np.concatenate([flat, np.zeros(win - len(flat))])
    ms = rms_env(flat, win, hop) ** 2
    if ms.max() <= 0:
        return 0.0
    return round(float(np.count_nonzero(ms >= ms.max() * 10 ** (db / 10))) * hop * 1000 / sr, 2)


def _band_power(flat: np.ndarray, sr: int) -> tuple[np.ndarray, np.ndarray]:
    """Frame power spectra inside GLIDE_BAND, and their frequencies."""
    if len(flat) < GLIDE_WIN:
        flat = np.concatenate([flat, np.zeros(GLIDE_WIN - len(flat))])
    count = 1 + (len(flat) - GLIDE_WIN) // GLIDE_HOP
    idx = np.arange(GLIDE_WIN)[None, :] + GLIDE_HOP * np.arange(count)[:, None]
    power = np.abs(np.fft.rfft(flat[idx] * np.hanning(GLIDE_WIN), GLIDE_NFFT, axis=1)) ** 2
    freqs = np.fft.rfftfreq(GLIDE_NFFT, 1.0 / sr)
    keep = (freqs >= GLIDE_BAND[0]) & (freqs <= GLIDE_BAND[1])
    return power[:, keep], freqs[keep]


def _glide_frames(flat: np.ndarray, sr: int) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    """Per frame: band power T, strongest frequency fp, and the power near fp."""
    power, freqs = _band_power(flat, sr)
    total = power.sum(axis=1)
    fp = freqs[np.argmax(power, axis=1)]
    half = np.maximum(fp * (2 ** (3 / 12) - 1), 2 * sr / GLIDE_WIN)
    near = (np.abs(freqs[None, :] - fp[:, None]) <= half[:, None])
    return total, fp, (power * near).sum(axis=1)


def _tracks(frames: np.ndarray, semis: np.ndarray) -> list[list[int]]:
    acc: list[list[int]] = []
    for idx in frames:
        if acc and idx - acc[-1][-1] <= GLIDE_SPLIT_FRAMES and abs(semis[idx] - semis[acc[-1][-1]]) <= GLIDE_SPLIT_ST:
            acc[-1].append(int(idx))
        else:
            acc.append([int(idx)])
    return [times for times in acc if len(times) >= GLIDE_TRACK_MIN]


def _running_median(val: np.ndarray, width: int) -> np.ndarray:
    h = width // 2
    return np.array([np.median(val[max(0, idx - h): idx + h + 1]) for idx in range(len(val))])


def _track_climb(track: list[int], semis: np.ndarray, times: np.ndarray) -> tuple[float, float, float]:
    slope_ = _running_median(semis[track], GLIDE_MEDIAN)
    low = np.minimum.accumulate(slope_)
    climb = slope_ - low
    key_ = int(np.argmax(climb))
    start = int(np.argmin(slope_[: key_ + 1]))
    return float(climb[key_]), float(times[track[start]]), float(times[track[key_]])


def _tonal_frames(total: np.ndarray, near: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Active frames (within GLIDE_ACTIVE_DB of the loudest) and the tonal ones among them."""
    active = total >= total.max() * 10 ** (GLIDE_ACTIVE_DB / 10) if total.max() > 0 else total > 0
    return active, active & (near >= 0.5 * total)


def _best_climb(tonal: np.ndarray, semis: np.ndarray, times: np.ndarray) -> tuple[float, Any, Any]:
    best: tuple[float, Any, Any] = (0.0, None, None)
    for track in _tracks(np.nonzero(tonal)[0], semis):
        climb = _track_climb(track, semis, times)
        if climb[0] > best[0]:
            best = climb
    return best


def _round_or_none(val: float | None, digits: int) -> float | None:
    return None if val is None else round(val, digits)


def rising_glide(y: np.ndarray, sr: int = SR) -> dict[str, Any]:
    """The largest pitch climb of a tonal track (part 2, 3.6)."""
    total, fp, near = _glide_frames(mono(y), sr)
    active, tonal = _tonal_frames(total, near)
    semis = 12 * np.log2(np.maximum(fp, 1e-9) / 440.0)
    times = np.arange(len(total)) * GLIDE_HOP / sr + (GLIDE_WIN / 2) / sr
    rise, fr, to = _best_climb(tonal, semis, times)
    share = round(float(tonal.sum()) / max(1, int(active.sum())), 2)
    return {"tonal_share": share, "rise_st": round(rise, 1), "from_s": _round_or_none(fr, 3),
            "to_s": _round_or_none(to, 3)}


def _layer_env(flat: np.ndarray) -> np.ndarray:
    count = 1 + (len(flat) - LAYER_WIN) // LAYER_HOP
    csum = np.concatenate([[0.0], np.cumsum(flat * flat)])
    starts = np.arange(count) * LAYER_HOP
    env_ = 10 * np.log10((csum[starts + LAYER_WIN] - csum[starts]) / LAYER_WIN + 1e-24)
    return env_ - env_.max()


def _is_peak(env_: np.ndarray, idx: int) -> bool:
    """A local maximum of ``e`` within LAYER_PEAK_DB of the loudest."""
    rises = idx == 0 or env_[idx] >= env_[idx - 1]
    falls = idx == len(env_) - 1 or env_[idx] >= env_[idx + 1]
    return bool(env_[idx] >= LAYER_PEAK_DB and rises and falls)


def _no_dip(env_: np.ndarray, last: int, cand: int) -> bool:
    """True when the level between two peaks never dips LAYER_DIP_DB below the lower one."""
    return bool(env_[last: cand + 1].min() > min(env_[last], env_[cand]) - LAYER_DIP_DB)


def _attack_frames(env_: np.ndarray, sr: int) -> list[int]:
    kept: list[int] = []
    span = LAYER_SPAN_S * sr / LAYER_HOP
    for cand in (idx for idx in range(len(env_)) if _is_peak(env_, idx)):
        if kept and cand - kept[0] > span:
            break
        if kept and _no_dip(env_, kept[-1], cand):
            kept[-1] = cand if env_[cand] > env_[kept[-1]] else kept[-1]
        else:
            kept.append(cand)
    return kept


def layers(y: np.ndarray, sr: int = SR) -> dict[str, Any]:
    """Separate attacks and the pre-swell of a hit (part 2, 3.7)."""
    flat = mono(y)
    if len(flat) < LAYER_WIN + 2 * LAYER_HOP:
        return {"attacks": [0.0], "count": 1, "pre_swell_ms": 0.0}
    env_ = _layer_env(flat)
    i30 = int(np.argmax(env_ >= -30.0))
    i6 = int(np.argmax(env_ >= LAYER_PEAK_DB))
    kept = _attack_frames(env_, sr)
    attacks = [round((idx * LAYER_HOP + LAYER_WIN / 2) / sr, 3) for idx in kept]
    return {"attacks": attacks, "count": len(attacks), "pre_swell_ms": round((i6 - i30) * LAYER_HOP * 1000 / sr, 1)}


# ---------------------------------------------------------------- the audit


def _polish(src_obj: np.ndarray) -> dict[str, Any]:
    from hark.forge.engine import polish

    flat = mono(src_obj)
    got: dict[str, Any] = {}
    for fn in (polish.clicks, polish.dc, polish.ring, polish.truncated, polish.silence, polish.tail,
               polish.attack, polish.body, polish.resonances):
        got.update(fn(flat))
    got.update(polish.clipping(np.asarray(src_obj, dtype=np.float64)))
    return got


def _load(src_obj: Any, sr: int | None) -> tuple[np.ndarray, str | None]:
    if isinstance(src_obj, (str, os.PathLike)):
        from hark.core import audio

        return np.asarray(audio.load(str(src_obj), sr=SR), dtype=np.float64), os.path.basename(str(src_obj))
    wave = np.asarray(src_obj, dtype=np.float64)
    if sr is not None and int(sr) != SR:
        from hark.core import audio

        wave = np.asarray(audio.resample(wave, int(sr), SR), dtype=np.float64)
    return wave, None


class _Audit:
    """Collects warnings and notes of one audit in rule order."""

    def __init__(self) -> None:
        self.warnings: list[dict] = []
        self.notes: list[dict] = []

    def add(self, code: str, msg: str, warn: bool = True) -> None:
        (self.warnings if warn else self.notes).append({"code": code, "msg": msg})


def _silent(aud: _Audit, peak: float, live: float) -> bool:
    peak_db = 20 * math.log10(peak) if peak > 0 else -math.inf
    if peak_db >= SILENT_PEAK_DB and live >= SILENT_LIVE_MS:
        return False
    if peak == 0:
        aud.add("silent", "digital silence")
    else:
        aud.add("silent", f"silent: peak {peak_db:.1f} dBFS, live for {live:.1f} ms")
    return True


def _above(meas: dict, field: str, limit: float) -> bool:
    """True when measure ``key`` exists and is over ``limit``."""
    return meas.get(field) is not None and meas[field] > limit


def _count(meas: dict, field: str) -> float:
    return meas.get(field) or 0


def _rule_clicks(aud: _Audit, meas: dict, designed: bool, gated: bool) -> None:
    if _count(meas, "clicks") > 0:
        times = ", ".join(f"{sec:.3f}" for sec in (meas.get("click_times") or [])[:4])
        aud.add("clicks", f"{meas['clicks']} click(s) inside the sound at {times} s", not designed)


def _rule_edge(aud: _Audit, meas: dict, designed: bool, gated: bool) -> None:
    if _above(meas, "edge_db", EDGE_DB):
        aud.add("edge", f"the sound starts or ends at {meas['edge_db']:.1f} dB re its peak: it clicks when played")


def _rule_clipping(aud: _Audit, meas: dict, designed: bool, gated: bool) -> None:
    runs, overs = _count(meas, "clip_runs"), _count(meas, "overs")
    if runs > 0 or overs > 0:
        aud.add("clipping", f"clipped: {runs} flat top(s), {overs} sample(s) over full scale")


def _rule_dc(aud: _Audit, meas: dict, designed: bool, gated: bool) -> None:
    if _above(meas, "dc_db", DC_DB):
        aud.add("dc", f"DC offset at {meas['dc_db']:.1f} dB re the peak")


def _rule_ring(aud: _Audit, meas: dict, designed: bool, gated: bool) -> None:
    if _above(meas, "ring_z", RING_Z):
        aud.add("ring", f"metallic comb ring in the tail ({_count(meas, 'ring_ms'):.1f} ms delay, z {meas['ring_z']:.1f})")


def _rule_truncated(aud: _Audit, meas: dict, designed: bool, gated: bool) -> None:
    if not _above(meas, "trunc_db", TRUNC_DB):
        return
    if gated:
        aud.add("truncated", f"the tail stops at {meas['trunc_db']:.1f} dB (the stop is the design)", False)
    else:
        aud.add("truncated", f"the tail is cut at {meas['trunc_db']:.1f} dB re the peak: fade it out or render it longer")


def _rule_silence(aud: _Audit, meas: dict, designed: bool, gated: bool) -> None:
    if _count(meas, "silence_ms") > SILENCE_MS:
        aud.add("silence", f"{meas['silence_ms']:.1f} ms of digital silence inside the sound", not designed)


def _rule_tail(aud: _Audit, meas: dict, designed: bool, gated: bool) -> None:
    if _count(meas, "bump_db") > TAIL_DB:
        aud.add("tail", f"the level climbs {meas['bump_db']:.1f} dB again after the peak (a second event or a roll)", False)


ARTIFACT_RULES = (_rule_clicks, _rule_edge, _rule_clipping, _rule_dc, _rule_ring, _rule_truncated, _rule_silence,
                  _rule_tail)


def _artifact_rules(aud: _Audit, meas: dict, designed: bool, gated: bool) -> None:
    for rule in ARTIFACT_RULES:
        rule(aud, meas, designed, gated)


def drip_allowed(glide: dict) -> bool:
    """True when a drip's climb is a bubble's (part 2, 3.9)."""
    if glide["from_s"] is None:
        return True
    return (glide["rise_st"] <= DRIP_MAX_ST and glide["from_s"] <= DRIP_START_S
            and glide["to_s"] - glide["from_s"] <= DRIP_SPAN_S)


def _glide_rule(aud: _Audit, gl_info: dict, category: str | None) -> None:
    if gl_info["from_s"] is None or gl_info["rise_st"] <= GLIDE_ST or gl_info["to_s"] - gl_info["from_s"] < GLIDE_MIN_S:
        return
    if category == "drip" and drip_allowed(gl_info):
        aud.add("rising_glide", "a bubble rising (drip allowance)", False)
        return
    span = gl_info["to_s"] - gl_info["from_s"]
    aud.add("rising_glide", f"a rising synth whip reads as cheap: +{gl_info['rise_st']:.1f} st over {span * 1000:.0f} ms "
                          f"({gl_info['from_s']:.3f}-{gl_info['to_s']:.3f} s)")


def _layer_rules(aud: _Audit, lay_info: dict, role: str | None, category: str | None) -> None:
    hit_role = role in HIT_ROLES
    if not hit_role and category not in HIT_CATEGORIES:
        return
    if lay_info["count"] >= 2:
        at = ", ".join(f"{times:.2f}" for times in lay_info["attacks"])
        aud.add("stacked", f"{lay_info['count']} separate attacks at {at} s: one hit should have one attack", hit_role)
    if lay_info["pre_swell_ms"] > PRE_SWELL_MS:
        aud.add("pre_swell", f"a {lay_info['pre_swell_ms']:.0f} ms build before the hit", hit_role)


def _summary(label: str, aud: _Audit) -> str:
    if aud.warnings:
        nm = len(aud.warnings)
        return f"{label}: {nm} warning{'' if nm == 1 else 's'}: " + "; ".join(words["msg"] for words in aud.warnings)
    if aud.notes:
        return f"{label}: clean (" + "; ".join(words["msg"] for words in aud.notes) + ")"
    return f"{label}: clean"


def _measures(meas: dict, peak: float, live: float) -> dict[str, Any]:
    acc = {key_: meas.get(key_) for key_ in MEASURE_KEYS}
    acc["peak_dbfs"] = None if peak == 0 else round(20 * math.log10(peak), 2)
    acc["live_ms"] = live
    return acc


def check(x: Any, sr: int | None = None, category: str | None = None, role: str | None = None,
          sound: str | None = None) -> dict[str, Any]:
    """Audit one sound: ``{ok, warnings, notes, summary, measures, glide, layers, context}``."""
    wave, base = _load(x, sr)
    context = {key_: val for key_, val in (("category", category), ("role", role), ("sound", sound)) if val}
    aud = _Audit()
    peak = float(np.max(np.abs(wave))) if wave.size else 0.0
    live = live_ms(wave)
    payload: dict[str, Any] = {"context": context, "glide": None, "layers": None}
    if _silent(aud, peak, live):
        payload["measures"] = _measures({}, peak, live)
    else:
        payload.update(_audible_rules(aud, wave, category, role))
        payload["measures"] = _measures(payload.pop("polish"), peak, live)
    payload.update(ok=not aud.warnings, warnings=aud.warnings, notes=aud.notes,
                  summary=_summary(sound or base or "sound", aud))
    return payload


def _ends_on_peak(wave: np.ndarray) -> bool:
    flat = mono(wave)
    return int(np.argmax(np.abs(flat))) >= ENDS_ON_PEAK * len(flat)


def _audible_rules(aud: _Audit, wave: np.ndarray, category: str | None, role: str | None) -> dict[str, Any]:
    """Run the rules after ``silent``; returns ``{polish, glide, layers}``."""
    meas = _polish(wave)
    gated = category in GATED or _ends_on_peak(wave)
    _artifact_rules(aud, meas, category in DESIGNED, gated)
    gl_info, lay_info = rising_glide(wave), layers(wave)
    _glide_rule(aud, gl_info, category)
    _layer_rules(aud, lay_info, role, category)
    return {"polish": meas, "glide": gl_info, "layers": lay_info}


# ---------------------------------------------------------------- list, render, check


def sfx_list(category: str | None = None, kind: str | None = None) -> dict[str, Any]:
    """``{summary, version, sounds}``: the bank rows with the filters."""
    rows = bank.list_sounds(category=category, kind=kind)
    version = bank.VERSION
    filters = " ".join(f"{key_} {val}" for key_, val in (("category", category), ("kind", kind)) if val is not None)
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
        parts.append("checks: clean" if checks["ok"] else "checks: " + "; ".join(words["msg"] for words in checks["warnings"]))
    line = "; ".join(parts)
    return f"wrote {file}: {line}" if file else line


def _edit_parts(edits: dict) -> list[str]:
    parts = []
    if "apex" in edits:
        env_ = edits["apex"]
        parts.append(f"apex {env_.get('target')} s (measured {env_.get('measured')}, {env_.get('err_ms', 0):+} ms)")
    if "dur" in edits:
        env_ = edits["dur"]
        parts.append(f"dur {env_.get('target')} s ({env_.get('method')}, {env_.get('fit') or 'exact'})")
    if "pitch" in edits:
        env_ = edits["pitch"]
        measured = f" (measured {env_['measured_hz']} Hz)" if env_.get("measured_hz") is not None else ""
        parts.append(f"pitch {env_.get('note')}{measured}")
    return parts


def sfx_render(sound: str, out: str | None = None, seed: int | None = None, dur: float | None = None,
               apex: float | None = None, pitch: Any = None, peak_db: float = -1.0, weight: float | None = None,
               sr: int = SR) -> dict[str, Any]:
    """Render one take with every knob and the audit: ``{summary, file, meta}``."""
    wave, meta = bank.render(sound, seed=seed, dur=dur, apex=apex, pitch=pitch, peak_db=peak_db, weight=weight,
                          sr=sr, checks=True)
    file = write(out, wave, meta["sr"]) if out else None
    return {"summary": _render_summary(meta, file), "file": file, "meta": meta}


def sfx_check(path_or_paths: Any, category: str | None = None, role: str | None = None) -> dict[str, Any]:
    """Audit one file, or a list of files with a total."""
    if isinstance(path_or_paths, (str, os.PathLike)):
        path = str(path_or_paths)
        return {**check(path, category=category, role=role, sound=os.path.basename(path)), "file": path}
    files = [sfx_check(meas, category, role) for meas in path_or_paths]
    clean = sum(1 for fpath in files if fpath["ok"])
    return {"summary": f"{clean} / {len(files)} clean", "ok": clean == len(files), "files": files}


# ---------------------------------------------------------------- CLI


def _list_text(payload: dict) -> str:
    lines = [payload["summary"], f"{'id':<30}{'dur s':>6}{'peak s':>7}  kinds / knobs"]
    for row in payload["sounds"]:
        kinds = ",".join(row["kinds"]) or "-"
        lines.append(f"{row['id']:<30}{row['dur_s'] or 0:6.3f} {row['peak_s'] or 0:6.3f}  {kinds} | {knob_words(row['knobs'])}")
    return "\n".join(lines)


def knob_words(knobs: dict) -> str:
    """``dur <method>``, then ``apex`` and ``pitch <note>`` when they apply."""
    words = [f"dur {knobs.get('dur')}"]
    if knobs.get("apex"):
        words.append("apex")
    if knobs.get("pitch"):
        words.append(f"pitch {knobs['pitch'].get('note') or 'measured'}")
    return ", ".join(words)


def _check_text(payload: dict) -> str:
    if "files" in payload:
        return "\n".join([payload["summary"]] + [fpath["summary"] for fpath in payload["files"]])
    return payload["summary"]


def _emit(payload: dict, args: argparse.Namespace, text: str) -> None:
    from hark.core import report

    report.emit(payload, args, text=text)


def _cmd_list(args: argparse.Namespace) -> int:
    payload = sfx_list(args.category, args.kind)
    _emit(payload, args, _list_text(payload))
    return 0


def _cmd_render(args: argparse.Namespace) -> int:
    payload = sfx_render(args.sound, out=args.output, seed=args.seed, dur=args.dur, apex=args.apex,
                        pitch=args.pitch, peak_db=args.peak, weight=args.weight, sr=args.sr)
    _emit(payload, args, payload["summary"])
    return 0


def _cmd_check(args: argparse.Namespace) -> int:
    target = args.files[0] if len(args.files) == 1 else args.files
    payload = sfx_check(target, category=args.category, role=args.role)
    _emit(payload, args, _check_text(payload))
    return 0 if payload["ok"] else 1


def _help_of(parser: argparse.ArgumentParser) -> Callable[[argparse.Namespace], int]:
    def show(args: argparse.Namespace) -> int:
        parser.print_help()
        return 2

    return show


def add_cli(sub: argparse._SubParsersAction) -> None:
    """Register ``hark sfx`` with its ``list``, ``render`` and ``check`` sub-commands."""
    from hark.core import report

    meas = sub.add_parser("sfx", help="one bank sound by id, aimed at the picture; audit any effect")
    meas.set_defaults(func=_help_of(meas))
    subs = meas.add_subparsers(dest="sfx_command")
    for leaf in (_add_list(subs), _add_render(subs), _add_check(subs)):
        report.add_common(leaf)


def _add_list(subs: argparse._SubParsersAction) -> argparse.ArgumentParser:
    lp = subs.add_parser("list", help="list the bank sounds")
    lp.add_argument("--category")
    lp.add_argument("--kind")
    lp.set_defaults(func=_cmd_list)
    return lp


def _add_render(subs: argparse._SubParsersAction) -> argparse.ArgumentParser:
    rp = subs.add_parser("render", help="render one take of a bank sound")
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


def _add_check(subs: argparse._SubParsersAction) -> argparse.ArgumentParser:
    cp = subs.add_parser("check", help="audit sound effect files with measurements only")
    cp.add_argument("files", nargs="+")
    cp.add_argument("--category")
    cp.add_argument("--role")
    cp.set_defaults(func=_cmd_check)
    return cp
