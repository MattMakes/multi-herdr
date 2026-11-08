"""Text and listening output of a forge run: the Markdown reports and the audition reel."""

from __future__ import annotations

import json
import math
import os
from collections.abc import Sequence
from pathlib import Path
from typing import Any

import numpy as np

SR = 48000
EMPTY_REEL = 4800


# ---------------------------------------------------------------- audition

def _stereo(x: Any) -> np.ndarray:
    a = np.asarray(x, dtype=np.float64)
    return np.column_stack([a, a]) if a.ndim == 1 else a[:, :2]


def _peak(x: np.ndarray, peak_db: float) -> np.ndarray:
    pk = float(np.max(np.abs(x))) if x.size else 0.0
    return x * (10 ** (peak_db / 20) / pk) if pk > 0 else x


def _reel(made: Sequence[tuple], refs: Sequence[tuple], pairs: Sequence[Any], o: dict) -> tuple[list, list]:
    parts, cues, t = [], [], 0
    for i, (name, x) in enumerate(made):
        j = pairs[i] if pairs is not None and i < len(pairs) else None
        clips = [("made", name, x, o["keep"])] + ([("ref", refs[j][0], refs[j][1], False)] if j is not None else [])
        for c, (kind, nm, y, keep) in enumerate(clips):
            s = _stereo(y) if keep else _peak(_stereo(y), o["peak_db"])
            gap = int(round((o["pair_gap"] if c == len(clips) - 1 else o["gap"]) * SR))
            cues.append({"t": round(t / SR, 2), "kind": kind, "name": nm})
            parts += [s, np.zeros((gap, 2))]
            t += len(s) + gap
    return parts, cues


def _cue_text(cues: list[dict], keep: bool) -> str:
    rule = ("made sounds at their matched levels, refs peak-normalised" if keep
            else "every clip peak-matched")
    head = (f"each made sound is followed by its nearest ref by tonal balance and envelope; levels: {rule}\n")
    return head + "".join(f"{c['t']:8.2f} s  {c['kind']:<4}  {c['name']}\n" for c in cues)


def audition(path: Any, made: Sequence[tuple[str, Any]], refs: Sequence[tuple[str, Any]], pairs: Sequence[Any],
             gap_s: float = 0.6, pair_gap_s: float = 1.2, peak_db: float = -3.0,
             keep_made_levels: bool = False) -> dict:
    """Write a listening reel and its cue list; {'file', 'txt', 'cues'}."""
    from hark.core.audio import write_wav

    o = {"gap": gap_s, "pair_gap": pair_gap_s, "peak_db": peak_db, "keep": keep_made_levels}
    parts, cues = _reel(made, refs, pairs, o)
    reel = np.concatenate(parts, axis=0) if parts else np.zeros((EMPTY_REEL, 2))
    path = Path(path)
    f = write_wav(path, reel, SR, "PCM_24")
    txt = path.with_suffix(".txt")
    txt.write_text(_cue_text(cues, keep_made_levels))
    return {"file": str(f), "txt": str(txt), "cues": cues}


# ---------------------------------------------------------------- number format

def _f(v: Any, nd: int = 2) -> str:
    if v is None:
        return "-"
    if isinstance(v, bool):
        return "yes" if v else "no"
    if isinstance(v, float):
        return f"{v:.{nd}f}" if math.isfinite(v) else "-"
    if isinstance(v, (list, tuple)):
        return ", ".join(_f(u, nd) for u in v) or "-"
    return str(v)


def _base(p: Any) -> str:
    return os.path.basename(str(p).replace("\\", "/"))


def _round(v: Any) -> Any:
    if isinstance(v, float):
        return round(v, 5)
    if isinstance(v, dict):
        return {k: _round(u) for k, u in v.items()}
    if isinstance(v, (list, tuple)):
        return [_round(u) for u in v]
    return v


def _table(head: Sequence[str], rows: list[list[str]]) -> list[str]:
    return ["| " + " | ".join(head) + " |", "|" + "---|" * len(head)] + ["| " + " | ".join(r) + " |" for r in rows]


# ---------------------------------------------------------------- report_md

def _asked(r: dict) -> list[str]:
    refs = r.get("refs") or []
    c = r.get("critic") or {}
    return [f"**Asked:** recipe {r.get('recipe')}, target '{r.get('target')}', alternatives "
            f"{', '.join(r.get('alts') or []) or '-'}, refset {_f(r.get('refset'))}, {len(refs)} refs "
            f"({', '.join(_base(p) for p in refs) or '-'}), critic v{_f(c.get('version'))} mode {_f(c.get('mode'))}.", ""]


def _timing_line(tm: dict) -> str:
    mins = (tm.get("total_s") or 0) / 60
    return (f" {mins:.1f} min, {_f(tm.get('rendered'))} renders, {_f(tm.get('scored'))} scored "
            f"({_f(tm.get('rendered_per_min'), 1)} renders/min, {_f(tm.get('scored_per_min'), 1)} scored/min); "
            f"render {_f(tm.get('render_s'))} s, measurement {_f(tm.get('pre_s'))} s, critic {_f(tm.get('score_s'))} s.")


def _budget(r: dict) -> list[str]:
    pre = f"pre-screen {r['prescreen']} per generation" if r.get("prescreen") else "no pre-screen"
    line = (f"**Budget:** strategy {r.get('strategy')}, budget {r.get('budget')}, pop {r.get('pop')}, "
            f"{_f(r.get('gens'))} generations, {pre}, reps {r.get('reps')}, tone weight {_f(r.get('tone_weight'))}, "
            f"seed {r.get('seed')}, {r.get('n_bank', 0)} bank starts, {r.get('restarts', 0)} restarts.")
    if r.get("timing"):
        line += _timing_line(r["timing"])
    return [line, ""]


def _search(r: dict) -> list[str]:
    rows = []
    for g in r.get("generations") or []:
        note = f"restart from {g['restart']}" if g.get("restart") else ""
        rows.append([_f(g.get("gen")), _f(g.get("best")), _f(g.get("gen_best")), _f(g.get("gen_median")),
                     f"{_f(g.get('passes'))}/{_f(g.get('scored'))}", _f(g.get("best_tone_db"), 1),
                     _f(g.get("sigma"), 3), note])
    head = ["gen", "best total", "this gen best", "this gen median", "passes", "best tone dB", "sigma", "note"]
    return ["## Search", ""] + _table(head, rows) + [""]


def _robust_line(rb: dict) -> str:
    return (f"Robust: n {rb.get('n')}, mean {_f(rb.get('mean'))}, min {_f(rb.get('min'))}, sd {_f(rb.get('sd'))}, "
            f"pass rate {_f(rb.get('pass_rate'))}, tone mean {_f(rb.get('tone_db_mean'))} dB.")


def _measured_line(m: dict, score: dict) -> str:
    return (f"Measured: tone {_f(m.get('tone_db'))} dB from the median (typical ref spread {_f(m.get('ref_spread'))}),"
            f" t40 {_f(m.get('t40_ms'), 0)} ms (refs {_f(m.get('t40_ref'), 0)}), length {_f(m.get('length_s'), 3)} s "
            f"(refs {_f(m.get('length_ref'), 3)}), nearest ref {_f(m.get('nearest_ref'))} by measurement and "
            f"{_f(score.get('sounds_like'))} by CLAP, artifacts {_f(m.get('art'))}.")


def _holdout_line(h: dict) -> str:
    tags = ", ".join(f"{t[0]} {float(t[1]):.2f}" for t in h.get("audioset_top") or [])
    return f"Held-out AudioSet tags (decide nothing): {tags or '-'}."


def _pick_md(p: dict) -> list[str]:
    s = p.get("score") or {}
    out = [f"### {p.get('name')} ({_base(p['file']) if p.get('file') else '-'})", "", str(s.get("summary", ""))]
    if p.get("robust"):
        out.append(_robust_line(p["robust"]))
    if p.get("measured"):
        out.append(_measured_line(p["measured"], s))
    if p.get("holdout"):
        out.append(_holdout_line(p["holdout"]))
    out += [f"Params (seed {p.get('seed')}): `{json.dumps(_round(p.get('params')), separators=(',', ':'))}`", ""]
    return out


def _gap_rows(r: dict) -> list[list[str]]:
    rows = []
    for p in r.get("picks") or []:
        s, m = p.get("score") or {}, p.get("measured") or {}
        rows.append(_gap_row(p.get("name"), s, m.get("tone_db"), m.get("t40_ms"), m.get("length_s")))
    for f in r.get("ref_scores") or []:
        rows.append(_gap_row(f"real: {f.get('file')}", f, f.get("tone_loo_db"), f.get("t40_ms"), f.get("length_s")))
    return rows


def _gap_row(name: Any, s: dict, tone_db: Any, t40: Any, length: Any) -> list[str]:
    return [str(name), _f(s.get("total")), _f(s.get("passes")), _f(s.get("match_z")), _f(s.get("p_real")),
            _f(tone_db), _f(s.get("env_pen")), _f(s.get("art_pen")), _f(s.get("polish_pen")), _f(s.get("hard")),
            _f(t40, 0), _f(length, 3)]


def _gap_md(r: dict) -> list[str]:
    if not r.get("ref_scores"):
        return []
    head = ["sound", "total", "passes", "match z", "p_real", "tone dB", "env pen", "art pen", "polish pen", "hard",
            "t40 ms", "length s"]
    note = ("The references are scored leave-one-out with the same critic. Model checks saturate, so a small "
            "total difference near the top means little.")
    return ["## Gap to the real references", "", note, ""] + _table(head, _gap_rows(r)) + ["", r.get("gap") or "", ""]


def _files_md(r: dict) -> list[str]:
    wavs = [_base(p["file"]) for p in r.get("picks") or [] if p.get("file")]
    out = ["## Files", "", f"WAVs: {', '.join(wavs) or '-'}"]
    return out + ["compare.png: each pick beside its nearest reference", "audition.wav: the listening reel",
                  "report.json: the full report, every pick re-renders from its render block", ""]


def report_md(r: dict) -> str:
    """Markdown text of a forge run report."""
    lines = [f"# forge {r.get('recipe')}: '{r.get('target')}'", ""] + _asked(r) + _budget(r) + _search(r)
    lines += ["## Picks", "", str(r.get("pick_rule", "")), ""]
    for p in r.get("picks") or []:
        lines += _pick_md(p)
    return "\n".join(lines + _gap_md(r) + _files_md(r))


# ---------------------------------------------------------------- palette_md

def _palette_row(p: dict) -> list[str]:
    s, m = p.get("score") or {}, p.get("measured") or {}
    return [str(p.get("name")), _f(p.get("family")), _f(s.get("total")), _f(s.get("passes")), _f(m.get("tone_db")),
            _f(s.get("env_pen")), _f(s.get("art_pen")), _f(s.get("polish_pen")), _f(s.get("hard")),
            _f(p.get("gain_db")), _f(m.get("nearest_ref"))]


def palette_md(r: dict) -> str:
    """Markdown text of a palette report."""
    head = ["member", "family", "total", "passes", "tone dB", "env pen", "art pen", "polish pen", "hard", "gain dB",
            "nearest ref"]
    lines = [f"# palette {r.get('category')} ({r.get('recipe')})", "", str(r.get("summary", "")), "",
             str(r.get("pick_rule", "")), ""]
    lines += _table(head, [_palette_row(p) for p in r.get("picks") or []]) + ["", "## Critic words", ""]
    lines += [f"- {p.get('name')}: {(p.get('score') or {}).get('summary', '')}" for p in r.get("picks") or []]
    return "\n".join(lines) + "\n"
