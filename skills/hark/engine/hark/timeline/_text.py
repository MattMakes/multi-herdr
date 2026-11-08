"""Summary paragraph and plain-ASCII transcript of a timeline result (analysis 2.11)."""

from __future__ import annotations

import os
from collections import Counter

from hark.timeline._dsp import ascii_safe
from hark.timeline._sync import top_accents
from hark.timeline._tags import short_label

BUILD_WORDS = ("whoosh", "swoosh", "swish", "riser", "rise", "swell", "sweep", "wave", "uplift")


def fmt_t(t) -> str:
    from hark.core.report import fmt_t as core_fmt

    return core_fmt(t)


def _pct(v) -> str:
    return f"{100 * v:.0f}%"


# ---------------------------------------------------------------- summary

def _lufs_sentence(L: dict) -> str:
    s = f"Integrated {L['integrated_lufs']:.1f} LUFS, true peak {L['true_peak_dbtp']:.1f} dBTP"
    if L.get("lra_lu") is not None:
        s += f", LRA {L['lra_lu']:.1f} LU"
    st = L.get("max_short_term")
    if st:
        s += f"; loudest 3 s at {fmt_t(st['start'])}-{fmt_t(st['end'])} ({st['lufs']:.1f} LUFS)"
    return s + "."


def _ungated_sentence(r: dict) -> str:
    tail = "below the -70 LUFS gate: near silence" if r["duration"] >= 0.4 else "too short for a loudness measurement"
    tp = r["loudness"]["true_peak_dbtp"]
    return f"True peak {tp:.1f} dBTP; {tail}." if tp is not None else f"{tail.capitalize()}."


def _level_sentence(r: dict) -> str:
    L = r["loudness"]
    if not r["has_audio"]:
        return (f"NO AUDIO STREAM: picture only, {len(r['cuts'])} picture cut(s), so every cut or cue is "
                f"without sound.")
    if L["digital_silence"]:
        return "DIGITAL SILENCE: every sample is zero."
    return _lufs_sentence(L) if L["integrated_lufs"] is not None else _ungated_sentence(r)


OVERSHOOT = {
    "risk": "True peak {tp:.1f} dBTP risks overs after encoding; limit the master to -1 dBTP.",
    "clips": ("The decoded lossy audio peaks at {tp:.1f} dBTP and clips on playback; leave the master at -1 dBTP "
              "or lower before encoding."),
    "codec": "True peak {tp:.1f} dBTP includes codec overshoot, which is fine for a delivered file.",
}


def _overshoot_sentence(tp: float, lossy) -> str | None:
    if tp <= -1:
        return None
    key = "clips" if lossy and tp > 0 else "codec" if lossy else "risk"
    return OVERSHOOT[key].format(tp=tp)


def _peak_sentence(r: dict) -> str | None:
    clip, tp = r["loudness"]["clipping"], r["loudness"]["true_peak_dbtp"]
    if clip["runs"]:
        return f"CLIPPING: {clip['runs']} run(s) of clipped samples, the first at {fmt_t(clip['times'][0])}."
    return _overshoot_sentence(tp, r.get("lossy")) if tp is not None else None


def _gap_sentences(r: dict) -> list[str]:
    gaps = r["loudness"]["silences"]
    out = []
    stops = [g for g in gaps if not g.get("where")]
    if stops:
        out.append(f"{len(stops)} stop-down(s): " + ", ".join(
            f"{fmt_t(g['start'])} {g['dur']:.2f} s {g['kind']}" for g in stops[:5]) + ".")
    edges = [g for g in gaps if g.get("where") in ("lead-in", "tail")]
    if edges:
        out.append(" and ".join(f"Quiet {g['where']} {g['dur']:.2f} s ({g['kind']})" for g in edges) + ".")
    return out


def _beat_sentence(r: dict) -> str | None:
    mu = r["music"]
    if mu.get("tempo") is not None and mu["confidence"] != "none":
        alt = f" or {mu['tempo_alt']}" if mu.get("tempo_alt") else ""
        return (f"Beat grid ~{mu['tempo']}{alt} BPM ({mu['confidence']}, clarity {mu['pulse_clarity']:.2f}, "
                f"{len(mu['beats'])} beats).")
    if r["loudness"]["digital_silence"]:
        return None
    c = mu.get("pulse_clarity")
    return (f"No steady beat grid (clarity {max(0.0, c):.2f}, needs 0.15)." if c is not None
            else "No steady beat grid.")


def _section_sentence(r: dict) -> str | None:
    if r["loudness"]["digital_silence"] or not r["sections"]:
        return None
    parts = [f"{fmt_t(s['start'])}-{fmt_t(s['end'])} {s['label']}" for s in r["sections"][:6]]
    return f"{len(r['sections'])} section(s): " + "; ".join(parts) + "."


def _hints(tagged: list[dict]) -> str:
    if not tagged:
        return ""
    kinds = Counter(e["kind"].lower() for e in tagged).most_common()
    return " (hints: " + ", ".join(f"{n} {k}" for k, n in kinds) + ")"


def _accent_text(e: dict) -> str:
    return f"{fmt_t(e['t'])} +{e['prominence_db']:.0f} dB" + (f" {e['label']}" if e.get("label") else "")


def _event_sentence(r: dict) -> str:
    ev = r["events"]
    rate = len(ev) / r["duration"] if r["duration"] > 0 else 0.0
    tagged = [e for e in ev if e.get("kind")]
    acc = top_accents(ev)
    s = f"{len(ev)} event(s) ({rate:.1f}/s), {len(tagged)} tagged" + _hints(tagged)
    if acc:
        s += "; strongest accents " + ", ".join(_accent_text(e) for e in acc)
    return s + "."


def _accent_clause(st: dict, fr: str, zoomed: bool) -> str:
    if "accent_" + fr not in st:
        return ""
    ctx = ", ranked over the +-30 s context" if zoomed else ""
    return (f"; accents (+{st['accent_min_db']:.1f} dB over the bed{ctx}) {_pct(st['accent_' + fr])} "
            f"(chance {_pct(st['accent_chance_' + fr])})")


def _tail_clauses(st: dict) -> list[str]:
    """The optional on-beat, swell, level-peak and no-hit clauses of a sync sentence."""
    out = [f"; on beat {_pct(st['on_beat'])} (chance {_pct(st['on_beat_chance'])})" if "on_beat" in st else "",
           f"; {st['answered_by_swell']} met by a whoosh or riser peak" if st["answered_by_swell"] else "",
           f"; {st['answered_by_level_peak']} met only by a level peak" if st["answered_by_level_peak"] else ""]
    if st["no_sound"]:
        out.append(f"; {len(st['no_sound'])} without a hit: " + ", ".join(fmt_t(t) for t in st["no_sound"][:6]))
    return out


def _sync_sentence(kind: str, block: dict, zoomed: bool) -> str | None:
    st = block["stats"]
    if not block["items"]:
        return None
    fr, word = ("within_1_frame", "within 1 frame") if "frame_ms" in st else ("within_40ms", "within 40 ms")
    s = (f"{kind.capitalize()}: {st['n']}; {_pct(st['hit_' + fr])} {word} (chance {_pct(st.get('chance_' + fr, 0))}), "
         f"{_pct(st['hit_within_100ms'])} within 100 ms")
    med = st.get("median_abs_offset_ms")
    s += f", median offset {med:.1f} ms" if med is not None else ""
    return s + _accent_clause(st, fr, zoomed) + "".join(_tail_clauses(st)) + "."


def summary(r: dict) -> str:
    name = os.path.basename(r["file"])
    parts = [f"{name} {fmt_t(r['start'])}-{fmt_t(r['end'])} ({r['duration']:.1f} s).", _level_sentence(r),
             _peak_sentence(r), *_gap_sentences(r), _beat_sentence(r), _section_sentence(r), _event_sentence(r)]
    zoomed = list(r["context"]) != [r["start"], r["end"]]
    for kind in ("cuts", "cues"):
        if kind in r["sync"]:
            parts.append(_sync_sentence(kind, r["sync"][kind], zoomed))
    return ascii_safe(" ".join(p for p in parts if p))


# ---------------------------------------------------------------- transcript

def _centroid(c) -> str:
    if c is None:
        return "-"
    return f"{c} Hz" if c < 1000 else f"{c / 1000:.1f} kHz"


def _runner_up(e: dict) -> str:
    for tg in e.get("tags", [])[1:3]:
        if tg["kind"] != e.get("kind"):
            return " / " + short_label(tg["tag"])
    return ""


def _ast_text(e: dict) -> str:
    labs = e.get("ast") or []
    pick = next((a for a in labs if a[0] != "Music"), labs[0] if labs else None)
    return f" [AST {pick[0]} {pick[1]:.2f}]" if pick else ""


def _event_row(e: dict, marks: list[str]) -> str:
    if e.get("label"):
        kind = e["kind"] + ("" if e.get("exposed") else "?")
        what = (e["label"] + _runner_up(e))[:46]
    else:
        kind, what = "-", f"{e['band']} {'swell' if e['shape'] == 'swell' else 'onset'}"
    over = f"{e['prominence_db']:+.0f} dB" if e.get("bed_db", -80) > -60 else "silent"
    row = (f"{fmt_t(e['t']):>8}  {kind:<7} {what:<46} {e.get('peak_dbfs', 0):6.1f} dBFS  {over:>7}  "
           f"{e['length_ms']:5d} ms  {_centroid(e.get('centroid_hz')):>8}{_ast_text(e)}")
    return row + (" | " + "; ".join(marks) if marks else "")


def _cue_head(item: dict, who: str) -> str:
    head = f"{fmt_t(item['t']):>8}  {'CUT' if who == 'cut' else 'CUE'}"
    if who == "cut":
        return head
    kind = item.get("kind")
    head += f" {kind}" if kind and kind != "cue" else ""
    return head + (f" {item['id']}" if item.get("id") else "")


def _why(item: dict) -> str:
    if item["status"] == "peak":
        return (f"no onset; level peak {item['offset_ms']:+.0f} ms, +{item['event_prominence_db']:.1f} dB: a soft "
                f"whoosh or swell, or just the bed? <- CHECK")
    if item["status"] == "off":
        return f"no onset within 100 ms; strong sound at {item['offset_ms']:+.0f} ms <- NO HIT"
    return "no onset or level peak within 250 ms <- NO HIT"


def _unanswered_row(item: dict, who: str) -> str:
    why = _why(item)
    text = f"{item.get('kind') or ''} {item.get('id') or ''}".lower()
    if any(w in text for w in BUILD_WORDS):
        why += " (a soft build-up can still be missed: look at the sheet)"
    return f"{_cue_head(item, who)}  {why}"


def _gap_row(g: dict) -> str:
    lvl = "< -70" if g["level_lufs"] <= -70 else f"{g['level_lufs']:.0f}"
    where = f" = {g['where']}" if g.get("where") else ""
    return f"{fmt_t(g['start']):>8}  GAP     {g['kind']} {g['dur']:.2f} s at {lvl} LUFS{where}"


def _mark_item(it: dict, who: str | None, marks: dict, unanswered: list) -> None:
    if it["status"] in ("no sound", "off", "peak"):
        unanswered.append((it, "cut" if who else "cue"))
    if it.get("event_t") is not None and it["status"] not in ("off", "peak"):
        name = who or it.get("kind") or "cue"
        marks.setdefault(it["event_t"], []).append(f"{name} {it['offset_ms']:+.0f} ms {it['status']}")


def _marks(r: dict) -> tuple[dict, list]:
    marks: dict[float, list[str]] = {}
    unanswered: list = []
    for kind, who in (("cuts", "cut"), ("cues", None)):
        for it in r["sync"].get(kind, {}).get("items", []):
            _mark_item(it, who, marks, unanswered)
    for e in top_accents(r["events"]):
        marks.setdefault(e["t"], []).append("strong accent (summary)")
    return marks, unanswered


def _section_row(s: dict, events: list[dict], marks: dict) -> str:
    quiet = [e for e in events if s["start"] <= e["t"] < s["end"] and not e.get("label") and e["t"] not in marks]
    row = f"{fmt_t(s['start']):>8}  ==      {fmt_t(s['start'])}-{fmt_t(s['end'])} {s['label']}"
    if quiet:
        rate = len(quiet) / max(s["end"] - s["start"], 1e-9)
        row += f" (+{len(quiet)} untagged onsets, {rate:.1f}/s)"
    return row


def _rows(r: dict) -> list[str]:
    marks, unanswered = _marks(r)
    rows = [(s["start"], 0, _section_row(s, r["events"], marks)) for s in r["sections"]]
    rows += [(e["t"], 1, _event_row(e, marks.get(e["t"], []))) for e in r["events"]
             if e.get("label") or e["t"] in marks]
    rows += [(g["start"], 1, _gap_row(g)) for g in r["loudness"]["silences"]]
    rows += [(it["t"], 2, _unanswered_row(it, who)) for it, who in unanswered]
    return [row for _, _, row in sorted(rows, key=lambda x: (x[0], x[1]))]


NOTES = ("NOTES: times, levels, loudness and sync offsets are measurements. Kinds and tags are CLAP guesses. A ? "
         "marks an event less than 12 dB over its bed, so its tag is a weak hint (right kind about 1 in 4 on the "
         "tuning mixes, about 1 in 2 for exposed events, often wrong on music and voice). AST speech sets the kind "
         "to VOICE. Section genres with ? are weak. \"over bed\" is the rise over the level just before the event; "
         "\"silent\" means that bed was below -60. The full data is in --json.")


def transcript(r: dict) -> str:
    lines = [r["summary"], ""]
    if r["sections"]:
        lines.append("SECTIONS")
        for s in r["sections"]:
            row = f"  {fmt_t(s['start'])}-{fmt_t(s['end'])}  {s['label']}"
            if s.get("lufs") is not None:
                row += f"  {s['lufs']:.0f} LUFS"
            if s.get("ast"):
                row += f"  [AST music {s['ast']['Music']:.2f}, speech {s['ast']['Speech']:.2f}]"
            lines.append(row)
        lines.append("")
    lines.append("TRANSCRIPT  time | kind | what (CLAP tag / runner-up) | peak | over bed | length | centroid | sync")
    lines += _rows(r)
    lines += ["", NOTES]
    return ascii_safe("\n".join(lines))
