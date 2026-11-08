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


def credit_line(e: dict, cls: dict) -> str:
    title, author, page = e.get("title") or "untitled", e.get("author") or "unknown author", e.get("page_url") or ""
    attr = (e.get("attribution") or "").strip()
    if cls["attribution"] and attr:
        line = attr.rstrip(".")
        if "http" not in line and "www." not in line and page:
            line += f" ({page})"
        if "creativecommons.org" not in line and cls["url"]:
            line += f" ({cls['url']})"
        return line
    line = f"\"{title}\" by {author}"
    if page:
        line += f" ({page})"
    line += f", licensed under {cls['name'] or e.get('rights') or 'unknown terms'}"
    if cls["url"]:
        line += f" ({cls['url']})"
    return line


def processing_sentence(design: dict | None) -> str:
    if design is None:
        return f"Each recording was {join_words(DEFAULT_VERBS)} for the film."
    samples, slices = design.get("samples", {}), design.get("slices", {})
    rules, place = design.get("rules", []), design.get("place", [])
    verbs = ["trimmed"]
    if slices:
        verbs.append("cut into single hits")
    layered = len(rules) + len(place) > (len(samples) + len(slices)) / 2 or len(rules) > 1
    verbs.append("layered" if layered else "placed")
    verbs.append("level-adjusted")
    buses = design.get("buses") or {}
    if any(o.get("eq") for o in list(samples.values()) + list(slices.values())) or \
            any(isinstance(b, dict) and b.get("eq") for b in buses.values()):
        verbs.append("equalised")
    if any(r.get("pan") not in (None, 0, 0.0) for r in rules) or any(p.get("pan") for p in place):
        verbs.append("panned")
    if any(o.get("pitch") or o.get("glide") for o in list(samples.values()) + list(slices.values())) or \
            any(r.get("pitch") or r.get("pitch_ramp") for r in rules):
        verbs.append("pitch-shifted by resampling")
    return f"Each recording was {join_words(verbs)} for the film."


def music_sentence(design: dict | None) -> str:
    if design is None or not design.get("music"):
        return "Edited for the film: trimmed to the picture and levels adjusted."
    m = design["music"]
    segs = m.get("segments") or []
    parts = ["cut and rearranged at bar lines" if len(segs) > 1 else "trimmed to the picture"]
    src, tgt = m.get("src_bpm"), m.get("target_bpm")
    if m.get("stretch", "none") != "none" and src and tgt and abs(float(tgt) / float(src) - 1) > 1e-6:
        parts.append(f"time-stretched from {float(src):g} to {float(tgt):g} BPM")
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


def build(a) -> tuple[int, dict]:
    root = os.path.abspath(a.root)
    report = os.path.join(root, a.report)
    mdir = os.path.join(root, a.manifests)
    if not os.path.isfile(report):
        return 2, {"error": f"mix report not found: {report}"}
    if not os.path.isdir(mdir):
        return 2, {"error": f"manifest folder not found: {mdir}"}
    with open(report, encoding="utf-8") as fh:
        used = [rights.rel(f, root) for f in json.load(fh).get("files_used", [])]
    entries, _ = rights.load_manifests(mdir)
    index = {}
    for e in entries:
        index.setdefault(rights.rel(e["file"], root).lower(), e)
    audit = None
    if a.rights:
        rp = os.path.join(root, a.rights)
        audit = {}
        if os.path.isfile(rp):
            with open(rp, encoding="utf-8") as fh:
                for r in json.load(fh):
                    p = ((r.get("file_check") or {}).get("path") or "").lower()
                    if p and not r.get("duplicate_of"):
                        audit.setdefault(p, r)
    design = load_design(a.design, root)

    problems, music, effects = [], [], []
    for f in used:
        e = index.get(f.lower())
        if e is None:
            problems.append(f"{f}: no manifest entry")
            continue
        cls = rights.classify(e.get("rights"))
        if cls["status"] != "allowed":
            problems.append(f"{f}: rights {cls['key']} ({cls['status']}): {cls['reason']}")
        if cls["attribution"]:
            miss = [k for k in ("author", "title", "page_url") if not e.get(k)]
            if miss:
                problems.append(f"{f}: CC BY without " + ", ".join(miss))
        if audit is not None:
            r = audit.get(f.lower())
            if r is None:
                problems.append(f"{f}: no audit record in the rights file")
            elif not r.get("rights_confirmed"):
                problems.append(f"{f}: the audit record is not confirmed")
        row = {"entry": e, "cls": cls, "line": credit_line(e, cls), "source": rights.source_name(e)}
        is_music = e.get("category") == "music" or "/music/" in "/" + f.lower()
        (music if is_music else effects).append(row)
    rank = {k: i for i, k in enumerate(KEY_ORDER)}
    effects.sort(key=lambda r: (rank.get(r["cls"]["key"], len(KEY_ORDER)),
                                str(r["entry"].get("author") or "").lower(), str(r["entry"].get("title") or "")))

    title = f": {a.title}" if a.title else ""
    proc, msent = processing_sentence(design), music_sentence(design)
    md = [f"# Sound credits{title}", "",
          "The credits live here and in the video description. CC BY needs them in the description.", ""]
    if music:
        md += ["## Music", ""] + [f"- {r['line']}" for r in music] + ["", msent, ""]
    if effects:
        sources = Counter(r["source"] for r in effects)
        md += ["## Sound effects and ambience", "",
               f"Real recordings from {join_words([s for s, _ in sources.most_common()])}. {proc} "
               "Nothing was synthesized.", ""]
        last = None
        for r in effects:
            if r["cls"]["key"] != last:
                last = r["cls"]["key"]
                md += [f"### {r['cls']['name'] or last}", ""]
            md.append(f"- {r['line']}")
            if r is effects[-1] or effects[effects.index(r) + 1]["cls"]["key"] != last:
                md.append("")
    tools = "ffmpeg, NumPy/SciPy, soxr, librosa and a BS.1770 loudness meter"
    if stretched(design):
        tools = "ffmpeg, NumPy/SciPy, soxr, librosa, a BS.1770 loudness meter and Rubber Band"
    md += ["## Tools", "", f"Mixed and mastered with {tools}.", ""]
    if problems:
        md += ["## PROBLEMS (fix before publishing)", ""] + [f"- {p}" for p in problems] + [""]

    desc = []
    if music:
        desc += ["Music:"] + [r["line"] for r in music] + [msent, ""]
    by = [r for r in effects if r["cls"]["attribution"]]
    rest = [r for r in effects if not r["cls"]["attribution"]]
    if by:
        desc += ["Sound effects (CC BY):"] + [r["line"] for r in by]
        desc += ["Every sound effect and ambience was modified for the film (trimmed, mixed and processed).", ""]
    if rest:
        authors = {}
        for r in rest:
            name = str(r["entry"].get("author") or "unknown author")
            authors.setdefault(name, set()).add(rights.SHORT_NAMES.get(r["cls"]["key"], r["cls"]["name"] or "?"))
        names = sorted(authors, key=str.lower)
        desc.append("Other sound effects by " + join_words(
            [f"{n} ({', '.join(sorted(authors[n]))})" for n in names]) + ".")
    if not by and effects:
        desc.append(proc)
    out_md, out_desc = os.path.join(root, a.out), os.path.join(root, a.desc)
    for path, text in ((out_md, "\n".join(md)), (out_desc, "\n".join(desc).strip() + "\n")):
        os.makedirs(os.path.dirname(path) or ".", exist_ok=True)
        with open(path, "w", encoding="utf-8") as fh:
            fh.write(text if text.endswith("\n") else text + "\n")
    summary = (f"{len(music) + len(effects)} recording(s) credited ({len(music)} music, {len(effects)} effects), "
               f"{len([r for r in music + effects if r['cls']['attribution']])} CC BY; "
               + (f"{len(problems)} problem(s)" if problems else "no problems"))
    if any(r["cls"]["attribution"] for r in music):
        summary += ("; CC BY music: put the credit in the description, and dispute a Content ID claim "
                    "with the licence link")
    return (1 if problems else 0), {"summary": summary, "written": [out_md, out_desc], "problems": problems}


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


def self_test() -> int:
    import tempfile
    import fixtures as fx
    ck = fx.Checker()
    with tempfile.TemporaryDirectory() as d:
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
        fx.put_json(os.path.join(m, "rights.json"), [
            {"id": "fs-1", "file_check": {"path": "audio/assets/ui/fs-1_tap.wav"}, "rights_confirmed": True},
            {"id": "fs-2", "file_check": {"path": "audio/assets/ui/fs-2_pop.wav"}, "rights_confirmed": False},
            {"id": "cm-9", "file_check": {"path": "audio/assets/music/cm-9_song.wav"}, "rights_confirmed": True}])
        code = main(["--root", d, "--rights", "audio/assets/manifests/rights.json"])
        md = open(os.path.join(d, "CREDITS.md")).read()
        ck("S8 an unconfirmed file gives PROBLEMS and exit 1", code == 1 and "## PROBLEMS" in md)
        ck("exit 2 without a report", main(["--root", d, "--report", "nope.json"]) == 2)
    return ck.code()


if __name__ == "__main__":
    sys.exit(main())
