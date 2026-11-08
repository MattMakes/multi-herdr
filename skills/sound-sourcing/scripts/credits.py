#!/usr/bin/env python3
"""Write CREDITS.md and the video-description text for every audible recording.

The recordings come from the mix report's files_used. Each one is matched to
its manifest entry. CC BY lines get the page link and the licence link.
Run with the hark venv Python and -I. --self-test runs the built-in checks.
"""
from __future__ import annotations

import argparse
import json
import os
import sys
from collections import Counter
from itertools import groupby

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import rights  # noqa: E402

KEY_ORDER = ["by4", "by3", "by25", "by2", "cc0", "pd", "mixkit-sfx", "mixkit-music", "pixabay"]
DEFAULT_VERBS = ["trimmed", "layered", "level-adjusted", "equalised", "panned"]


def say(*parts, file=None):
    text = " ".join(str(p) for p in parts)
    print(text.encode("ascii", "replace").decode("ascii"), file=file or sys.stdout)


def join_words(words: list[str]) -> str:
    if len(words) <= 1:
        return "".join(words)
    return ", ".join(words[:-1]) + " and " + words[-1]


def _attr_line(attr: str, page: str, cls: dict) -> str:
    line = attr.rstrip(".")
    if page and "http" not in line and "www." not in line:
        line += f" ({page})"
    if cls["url"] and "creativecommons.org" not in line:
        line += f" ({cls['url']})"
    return line


def _plain_line(e: dict, cls: dict, page: str) -> str:
    title, author = e.get("title") or "untitled", e.get("author") or "unknown author"
    line = f"\"{title}\" by {author}" + (f" ({page})" if page else "")
    line += f", licensed under {cls['name'] or e.get('rights') or 'unknown terms'}"
    return line + (f" ({cls['url']})" if cls["url"] else "")


def credit_line(e: dict, cls: dict) -> str:
    page = e.get("page_url") or ""
    attr = (e.get("attribution") or "").strip()
    if cls["attribution"] and attr:
        return _attr_line(attr, page, cls)
    return _plain_line(e, cls, page)


def _objects(design: dict) -> list:
    return list(design.get("samples", {}).values()) + list(design.get("slices", {}).values())


def _equalised(design: dict) -> bool:
    buses = design.get("buses") or {}
    return any(o.get("eq") for o in _objects(design)) or \
        any(isinstance(b, dict) and b.get("eq") for b in buses.values())


def _panned(design: dict) -> bool:
    return any(r.get("pan") not in (None, 0, 0.0) for r in design.get("rules", [])) or \
        any(p.get("pan") for p in design.get("place", []))


def _pitched(design: dict) -> bool:
    return any(o.get("pitch") or o.get("glide") for o in _objects(design)) or \
        any(r.get("pitch") or r.get("pitch_ramp") for r in design.get("rules", []))


def _layered(design: dict) -> bool:
    rules, place = design.get("rules", []), design.get("place", [])
    sources = len(design.get("samples", {})) + len(design.get("slices", {}))
    return len(rules) + len(place) > sources / 2 or len(rules) > 1


def processing_sentence(design: dict | None) -> str:
    if design is None:
        return f"Each recording was {join_words(DEFAULT_VERBS)} for the film."
    verbs = ["trimmed"] + (["cut into single hits"] if design.get("slices", {}) else [])
    verbs += ["layered" if _layered(design) else "placed", "level-adjusted"]
    optional = [(_equalised(design), "equalised"), (_panned(design), "panned"),
                (_pitched(design), "pitch-shifted by resampling")]
    verbs += [v for cond, v in optional if cond]
    return f"Each recording was {join_words(verbs)} for the film."


def music_sentence(design: dict | None) -> str:
    if design is None or not design.get("music"):
        return "Edited for the film: trimmed to the picture and levels adjusted."
    m = design["music"]
    segs = m.get("segments") or []
    parts = ["cut and rearranged at bar lines" if len(segs) > 1 else "trimmed to the picture"]
    if stretched(design):
        parts.append(f"time-stretched from {float(m['src_bpm']):g} to {float(m['target_bpm']):g} BPM")
    parts.append("levels adjusted")
    if any(s.get("tail_fade") for s in segs):
        parts.append("a fade at the end")
    return "Edited for the film: " + join_words(parts) + "."


def stretched(design: dict | None) -> bool:
    m = (design or {}).get("music") or {}
    src, tgt = m.get("src_bpm"), m.get("target_bpm")
    return bool(m.get("stretch", "none") != "none" and src and tgt and abs(float(tgt) / float(src) - 1) > 1e-6)


def load_design(path: str | None, root: str):
    if not path:
        return None
    import mix  # the mixer's own loader, so fragments merge the same way
    return mix.load_design(path, root)


def _missing_input(report: str, mdir: str):
    if not os.path.isfile(report):
        return {"error": f"mix report not found: {report}"}
    if not os.path.isdir(mdir):
        return {"error": f"manifest folder not found: {mdir}"}
    return None


def _index(entries: list, root: str) -> dict:
    index = {}
    for e in entries:
        index.setdefault(rights.rel(e["file"], root).lower(), e)
    return index


def _audit(a, root: str):
    """Confirmed audit records by lower-case path, {} for a missing file, None without --rights."""
    if not a.rights:
        return None
    rp, audit = os.path.join(root, a.rights), {}
    if not os.path.isfile(rp):
        return audit
    with open(rp, encoding="utf-8") as fh:
        records = json.load(fh)
    for r in records:
        p = ((r.get("file_check") or {}).get("path") or "").lower()
        if p and not r.get("duplicate_of"):
            audit.setdefault(p, r)
    return audit


def _audit_problems(f: str, audit) -> list[str]:
    if audit is None:
        return []
    r = audit.get(f.lower())
    if r is None:
        return [f"{f}: no audit record in the rights file"]
    return [] if r.get("rights_confirmed") else [f"{f}: the audit record is not confirmed"]


def _entry_problems(f: str, e: dict, cls: dict) -> list[str]:
    out = []
    if cls["status"] != "allowed":
        out.append(f"{f}: rights {cls['key']} ({cls['status']}): {cls['reason']}")
    miss = [k for k in ("author", "title", "page_url") if not e.get(k)] if cls["attribution"] else []
    if miss:
        out.append(f"{f}: CC BY without " + ", ".join(miss))
    return out


def _rows(used: list, index: dict, audit) -> tuple[list, list, list]:
    """(problems, music rows, effect rows) of the used files."""
    problems, music, effects = [], [], []
    for f in used:
        e = index.get(f.lower())
        if e is None:
            problems.append(f"{f}: no manifest entry")
            continue
        cls = rights.classify(e.get("rights"))
        problems += _entry_problems(f, e, cls) + _audit_problems(f, audit)
        row = {"entry": e, "cls": cls, "line": credit_line(e, cls), "source": rights.source_name(e)}
        is_music = e.get("category") == "music" or "/music/" in "/" + f.lower()
        (music if is_music else effects).append(row)
    rank = {k: i for i, k in enumerate(KEY_ORDER)}
    effects.sort(key=lambda r: (rank.get(r["cls"]["key"], len(KEY_ORDER)),
                                str(r["entry"].get("author") or "").lower(), str(r["entry"].get("title") or "")))
    return problems, music, effects


def _effects_md(effects: list, proc: str) -> list[str]:
    sources = Counter(r["source"] for r in effects)
    md = ["## Sound effects and ambience", "",
          f"Real recordings from {join_words([s for s, _ in sources.most_common()])}. {proc} "
          "Nothing was synthesized.", ""]
    for key, group in groupby(effects, key=lambda r: r["cls"]["key"]):
        group = list(group)
        md += [f"### {group[0]['cls']['name'] or key}", ""] + [f"- {r['line']}" for r in group] + [""]
    return md


def _markdown(a, design, rows: tuple) -> list[str]:
    problems, music, effects = rows
    title = f": {a.title}" if a.title else ""
    md = [f"# Sound credits{title}", "",
          "The credits live here and in the video description. CC BY needs them in the description.", ""]
    if music:
        md += ["## Music", ""] + [f"- {r['line']}" for r in music] + ["", music_sentence(design), ""]
    if effects:
        md += _effects_md(effects, processing_sentence(design))
    tools = "ffmpeg, NumPy/SciPy, soxr, librosa and a BS.1770 loudness meter"
    if stretched(design):
        tools = "ffmpeg, NumPy/SciPy, soxr, librosa, a BS.1770 loudness meter and Rubber Band"
    md += ["## Tools", "", f"Mixed and mastered with {tools}.", ""]
    if problems:
        md += ["## PROBLEMS (fix before publishing)", ""] + [f"- {p}" for p in problems] + [""]
    return md


def _other_authors(rest: list) -> str:
    authors = {}
    for r in rest:
        name = str(r["entry"].get("author") or "unknown author")
        authors.setdefault(name, set()).add(rights.SHORT_NAMES.get(r["cls"]["key"], r["cls"]["name"] or "?"))
    names = sorted(authors, key=str.lower)
    return "Other sound effects by " + join_words([f"{n} ({', '.join(sorted(authors[n]))})" for n in names]) + "."


def _music_desc(design, music: list) -> list[str]:
    if not music:
        return []
    return ["Music:"] + [r["line"] for r in music] + [music_sentence(design), ""]


def _split_by(effects: list) -> tuple[list, list]:
    """(CC BY rows, other rows)."""
    by = [r for r in effects if r["cls"]["attribution"]]
    return by, [r for r in effects if not r["cls"]["attribution"]]


def _effects_desc(design, effects: list) -> list[str]:
    by, rest = _split_by(effects)
    desc = []
    if by:
        desc += ["Sound effects (CC BY):"] + [r["line"] for r in by]
        desc += ["Every sound effect and ambience was modified for the film (trimmed, mixed and processed).", ""]
    if rest:
        desc.append(_other_authors(rest))
    if not by and effects:
        desc.append(processing_sentence(design))
    return desc


def _description(design, music: list, effects: list) -> list[str]:
    return _music_desc(design, music) + _effects_desc(design, effects)


def _write(pairs) -> None:
    for path, text in pairs:
        os.makedirs(os.path.dirname(path) or ".", exist_ok=True)
        with open(path, "w", encoding="utf-8") as fh:
            fh.write(text if text.endswith("\n") else text + "\n")


def _summary(problems: list, music: list, effects: list) -> str:
    summary = (f"{len(music) + len(effects)} recording(s) credited ({len(music)} music, {len(effects)} effects), "
               f"{len([r for r in music + effects if r['cls']['attribution']])} CC BY; "
               + (f"{len(problems)} problem(s)" if problems else "no problems"))
    if any(r["cls"]["attribution"] for r in music):
        summary += ("; CC BY music: put the credit in the description, and dispute a Content ID claim "
                    "with the licence link")
    return summary


def build(a) -> tuple[int, dict]:
    root = os.path.abspath(a.root)
    report = os.path.join(root, a.report)
    missing = _missing_input(report, os.path.join(root, a.manifests))
    if missing:
        return 2, missing
    with open(report, encoding="utf-8") as fh:
        used = [rights.rel(f, root) for f in json.load(fh).get("files_used", [])]
    entries, _ = rights.load_manifests(os.path.join(root, a.manifests))
    design = load_design(a.design, root)
    rows = _rows(used, _index(entries, root), _audit(a, root))
    problems, music, effects = rows
    md = _markdown(a, design, rows)
    desc = _description(design, music, effects)
    out_md, out_desc = os.path.join(root, a.out), os.path.join(root, a.desc)
    _write(((out_md, "\n".join(md)), (out_desc, "\n".join(desc).strip() + "\n")))
    return (1 if problems else 0), {"summary": _summary(problems, music, effects),
                                    "written": [out_md, out_desc], "problems": problems}


def parse(argv):
    p = argparse.ArgumentParser(prog="credits.py", description="Credits for every audible recording.")
    p.add_argument("--root", default=".")
    p.add_argument("--report", default="out/qa/mix_report.json")
    p.add_argument("--manifests", default="audio/assets/manifests")
    p.add_argument("--design", help="the design, for the processing sentences")
    p.add_argument("--rights", help="rights.json: each used file must be confirmed there")
    p.add_argument("--out", default="CREDITS.md")
    p.add_argument("--desc", default="out/CREDITS_description.txt")
    p.add_argument("--title")
    p.add_argument("--self-test", action="store_true")
    return p.parse_args(argv)


def main(argv=None) -> int:
    a = parse(sys.argv[1:] if argv is None else argv)
    if a.self_test:
        return self_test()
    code, res = build(a)
    if code == 2:
        say(f"credits.py: {res['error']}", file=sys.stderr)
        return 2
    say(res["summary"])
    for w in res["written"]:
        say("wrote", w)
    for p in res["problems"]:
        say("PROBLEM:", p)
    return code


def _fixture(fx, d: str) -> str:
    """Manifests, a mix report and a design under d. Returns the manifest folder."""
    m = os.path.join(d, "audio", "assets", "manifests")
    fx.put_json(os.path.join(m, "ui.json"), [
        {"id": "fs-1", "category": "ui", "file": "audio/assets/ui/fs-1_tap.wav", "title": "Tap",
         "author": "Bea", "page_url": "https://freesound.org/people/Bea/sounds/1/", "rights": "CC BY 4.0",
         "attribution": "Tap by Bea, CC BY 4.0."},
        {"id": "fs-2", "category": "ui", "file": "audio/assets/ui/fs-2_pop.wav", "title": "Pop",
         "author": "al", "page_url": "https://freesound.org/people/al/sounds/2/", "rights": "CC0"}])
    fx.put_json(os.path.join(m, "music.json"), [
        {"id": "cm-9", "category": "music", "file": "audio/assets/music/cm-9_song.wav", "title": "Song",
         "author": "Composer", "page_url": "https://composer.example/song", "rights": "CC BY 4.0",
         "attribution": "\"Song\" by Composer (https://composer.example/song), CC BY 4.0 "
                        "(https://creativecommons.org/licenses/by/4.0/)"}])
    fx.put_json(os.path.join(d, "out", "qa", "mix_report.json"), {"files_used": [
        "audio/assets/ui/fs-2_pop.wav", "audio/assets/ui/fs-1_tap.wav", "audio/assets/music/cm-9_song.wav"]})
    fx.put_json(os.path.join(d, "audio", "sounddesign.json"), {
        "samples": {"a.pop": {"file": "audio/assets/ui/fs-2_pop.wav", "eq": {"hp": 100}}},
        "slices": {"a.taps": {"file": "audio/assets/ui/fs-1_tap.wav"}},
        "rules": [{"name": "r1", "pool": "a.pop"}, {"name": "r2", "pool": "a.taps"}],
        "music": {"file": "audio/assets/music/cm-9_song.wav", "src_bpm": 120, "target_bpm": 120,
                  "stretch": "none", "segments": [{"src_from": 0, "src_to": 2, "at": 0},
                                                  {"src_from": 4, "src_to": 6, "at": 2, "tail_fade": 1}]}})
    return m


def _test_credits(ck, d: str) -> None:
    code = main(["--root", d, "--design", "audio/sounddesign.json", "--title", "Demo"])
    md = open(os.path.join(d, "CREDITS.md")).read()
    desc = open(os.path.join(d, "out", "CREDITS_description.txt")).read()
    tap = next(ln for ln in md.splitlines() if ln.startswith("- Tap by Bea"))
    ck("S7 sections present, by4 before cc0",
       code == 0 and "## Music" in md and "## Sound effects and ambience" in md and "## Tools" in md
       and md.index("### CC BY 4.0") < md.index("### CC0"))
    ck("S7 the CC BY line gains the page link and the licence link",
       tap == "- Tap by Bea, CC BY 4.0 (https://freesound.org/people/Bea/sounds/1/) "
              "(https://creativecommons.org/licenses/by/4.0/)")
    ck("S7 the description lists the CC0 author", "Other sound effects by al (CC0)." in desc)
    ck("S7 processing names slices and EQ", "cut into single hits" in md and "equalised" in md)
    ck("music sentence", "cut and rearranged at bar lines" in md and "a fade at the end" in md)


def _test_audit(ck, fx, d: str, m: str) -> None:
    fx.put_json(os.path.join(m, "rights.json"), [
        {"id": "fs-1", "file_check": {"path": "audio/assets/ui/fs-1_tap.wav"}, "rights_confirmed": True},
        {"id": "fs-2", "file_check": {"path": "audio/assets/ui/fs-2_pop.wav"}, "rights_confirmed": False},
        {"id": "cm-9", "file_check": {"path": "audio/assets/music/cm-9_song.wav"}, "rights_confirmed": True}])
    code = main(["--root", d, "--rights", "audio/assets/manifests/rights.json"])
    md = open(os.path.join(d, "CREDITS.md")).read()
    ck("S8 an unconfirmed file gives PROBLEMS and exit 1", code == 1 and "## PROBLEMS" in md)
    ck("exit 2 without a report", main(["--root", d, "--report", "nope.json"]) == 2)


def self_test() -> int:
    import tempfile
    import fixtures as fx
    ck = fx.Checker()
    with tempfile.TemporaryDirectory() as d:
        m = _fixture(fx, d)
        _test_credits(ck, d)
        _test_audit(ck, fx, d, m)
    return ck.code()


if __name__ == "__main__":
    sys.exit(main())
