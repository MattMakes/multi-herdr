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
    text = " ".join(str(pv) for pv in parts)
    print(text.encode("ascii", "replace").decode("ascii"), file=file or sys.stdout)


def join_words(words: list[str]) -> str:
    if len(words) <= 1:
        return "".join(words)
    return ", ".join(words[:-1]) + " and " + words[-1]


def _attr_line(attr: str, page: str, cls: dict) -> str:
    text_line = attr.rstrip(".")
    if page and "http" not in text_line and "www." not in text_line:
        text_line += f" ({page})"
    if cls["url"] and "creativecommons.org" not in text_line:
        text_line += f" ({cls['url']})"
    return text_line


def _plain_line(ent: dict, cls: dict, page: str) -> str:
    title, author = ent.get("title") or "untitled", ent.get("author") or "unknown author"
    text_line = f"\"{title}\" by {author}" + (f" ({page})" if page else "")
    text_line += f", licensed under {cls['name'] or ent.get('rights') or 'unknown terms'}"
    return text_line + (f" ({cls['url']})" if cls["url"] else "")


def credit_line(ent: dict, cls: dict) -> str:
    page = ent.get("page_url") or ""
    attr = (ent.get("attribution") or "").strip()
    if cls["attribution"] and attr:
        return _attr_line(attr, page, cls)
    return _plain_line(ent, cls, page)


def _objects(design: dict) -> list:
    return list(design.get("samples", {}).values()) + list(design.get("slices", {}).values())


def _equalised(design: dict) -> bool:
    buses = design.get("buses") or {}
    return any(o.get("eq") for o in _objects(design)) or \
        any(isinstance(bv, dict) and bv.get("eq") for bv in buses.values())


def _panned(design: dict) -> bool:
    return any(row_.get("pan") not in (None, 0, 0.0) for row_ in design.get("rules", [])) or \
        any(pv.get("pan") for pv in design.get("place", []))


def _pitched(design: dict) -> bool:
    return any(o.get("pitch") or o.get("glide") for o in _objects(design)) or \
        any(row_.get("pitch") or row_.get("pitch_ramp") for row_ in design.get("rules", []))


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
    verbs += [val for cond, val in optional if cond]
    return f"Each recording was {join_words(verbs)} for the film."


def music_sentence(design: dict | None) -> str:
    if design is None or not design.get("music"):
        return "Edited for the film: trimmed to the picture and levels adjusted."
    mus = design["music"]
    segs = mus.get("segments") or []
    pieces = ["cut and rearranged at bar lines" if len(segs) > 1 else "trimmed to the picture"]
    if stretched(design):
        pieces.append(f"time-stretched from {float(mus['src_bpm']):g} to {float(mus['target_bpm']):g} BPM")
    pieces.append("levels adjusted")
    if any(sv.get("tail_fade") for sv in segs):
        pieces.append("a fade at the end")
    return "Edited for the film: " + join_words(pieces) + "."


def stretched(design: dict | None) -> bool:
    mus = (design or {}).get("music") or {}
    src, tgt = mus.get("src_bpm"), mus.get("target_bpm")
    return bool(mus.get("stretch", "none") != "none" and src and tgt and abs(float(tgt) / float(src) - 1) > 1e-6)


def load_design(fpath: str | None, base_dir: str):
    if not fpath:
        return None
    import mix  # the mixer's own loader, so fragments merge the same way
    return mix.load_design(fpath, base_dir)


def _missing_input(rep_doc: str, mdir: str):
    if not os.path.isfile(rep_doc):
        return {"error": f"mix report not found: {rep_doc}"}
    if not os.path.isdir(mdir):
        return {"error": f"manifest folder not found: {mdir}"}
    return None


def _index(recs: list, base_dir: str) -> dict:
    index = {}
    for ent in recs:
        index.setdefault(rights.rel(ent["file"], base_dir).lower(), ent)
    return index


def _audit(cli, base_dir: str):
    """Confirmed audit records by lower-case path, {} for a missing file, None without --rights."""
    if not cli.rights:
        return None
    rp, aud_doc = os.path.join(base_dir, cli.rights), {}
    if not os.path.isfile(rp):
        return aud_doc
    with open(rp, encoding="utf-8") as fh:
        records = json.load(fh)
    for row_ in records:
        pv = ((row_.get("file_check") or {}).get("path") or "").lower()
        if pv and not row_.get("duplicate_of"):
            aud_doc.setdefault(pv, row_)
    return aud_doc


def _audit_problems(fname: str, aud_doc) -> list[str]:
    if aud_doc is None:
        return []
    row_ = aud_doc.get(fname.lower())
    if row_ is None:
        return [f"{fname}: no audit record in the rights file"]
    return [] if row_.get("rights_confirmed") else [f"{fname}: the audit record is not confirmed"]


def _entry_problems(fname: str, ent: dict, cls: dict) -> list[str]:
    acc = []
    if cls["status"] != "allowed":
        acc.append(f"{fname}: rights {cls['key']} ({cls['status']}): {cls['reason']}")
    miss = [key_ for key_ in ("author", "title", "page_url") if not ent.get(key_)] if cls["attribution"] else []
    if miss:
        acc.append(f"{fname}: CC BY without " + ", ".join(miss))
    return acc


def _rows(in_use: list, index: dict, aud_doc) -> tuple[list, list, list]:
    """(problems, music rows, effect rows) of the used files."""
    issues, mus_info, effects = [], [], []
    for fname in in_use:
        ent = index.get(fname.lower())
        if ent is None:
            issues.append(f"{fname}: no manifest entry")
            continue
        cls = rights.classify(ent.get("rights"))
        issues += _entry_problems(fname, ent, cls) + _audit_problems(fname, aud_doc)
        line_row = {"entry": ent, "cls": cls, "line": credit_line(ent, cls), "source": rights.source_name(ent)}
        is_music = ent.get("category") == "music" or "/music/" in "/" + fname.lower()
        (mus_info if is_music else effects).append(line_row)
    rank = {key_: ix for ix, key_ in enumerate(KEY_ORDER)}
    effects.sort(key=lambda row_: (rank.get(row_["cls"]["key"], len(KEY_ORDER)),
                                str(row_["entry"].get("author") or "").lower(), str(row_["entry"].get("title") or "")))
    return issues, mus_info, effects


def _effects_md(effects: list, proc: str) -> list[str]:
    sources = Counter(row_["source"] for row_ in effects)
    md_text = ["## Sound effects and ambience", "",
          f"Real recordings from {join_words([sv for sv, _ in sources.most_common()])}. {proc} "
          "Nothing was synthesized.", ""]
    for key, group in groupby(effects, key=lambda row_: row_["cls"]["key"]):
        group = list(group)
        md_text += [f"### {group[0]['cls']['name'] or key}", ""] + [f"- {row_['line']}" for row_ in group] + [""]
    return md_text


def _markdown(cli, design, table: tuple) -> list[str]:
    issues, mus_info, effects = table
    title = f": {cli.title}" if cli.title else ""
    md_text = [f"# Sound credits{title}", "",
          "The credits live here and in the video description. CC BY needs them in the description.", ""]
    if mus_info:
        md_text += ["## Music", ""] + [f"- {row_['line']}" for row_ in mus_info] + ["", music_sentence(design), ""]
    if effects:
        md_text += _effects_md(effects, processing_sentence(design))
    tools = "ffmpeg, NumPy/SciPy, soxr, librosa and a BS.1770 loudness meter"
    if stretched(design):
        tools = "ffmpeg, NumPy/SciPy, soxr, librosa, a BS.1770 loudness meter and Rubber Band"
    md_text += ["## Tools", "", f"Mixed and mastered with {tools}.", ""]
    if issues:
        md_text += ["## PROBLEMS (fix before publishing)", ""] + [f"- {pv}" for pv in issues] + [""]
    return md_text


def _other_authors(rest: list) -> str:
    authors = {}
    for row_ in rest:
        label = str(row_["entry"].get("author") or "unknown author")
        authors.setdefault(label, set()).add(rights.SHORT_NAMES.get(row_["cls"]["key"], row_["cls"]["name"] or "?"))
    names = sorted(authors, key=str.lower)
    return "Other sound effects by " + join_words([f"{num} ({', '.join(sorted(authors[num]))})" for num in names]) + "."


def _music_desc(design, mus_info: list) -> list[str]:
    if not mus_info:
        return []
    return ["Music:"] + [row_["line"] for row_ in mus_info] + [music_sentence(design), ""]


def _split_by(effects: list) -> tuple[list, list]:
    """(CC BY rows, other rows)."""
    by = [row_ for row_ in effects if row_["cls"]["attribution"]]
    return by, [row_ for row_ in effects if not row_["cls"]["attribution"]]


def _effects_desc(design, effects: list) -> list[str]:
    by, rest = _split_by(effects)
    blurb = []
    if by:
        blurb += ["Sound effects (CC BY):"] + [row_["line"] for row_ in by]
        blurb += ["Every sound effect and ambience was modified for the film (trimmed, mixed and processed).", ""]
    if rest:
        blurb.append(_other_authors(rest))
    if not by and effects:
        blurb.append(processing_sentence(design))
    return blurb


def _description(design, mus_info: list, effects: list) -> list[str]:
    return _music_desc(design, mus_info) + _effects_desc(design, effects)


def _write(pairs) -> None:
    for fpath, text in pairs:
        os.makedirs(os.path.dirname(fpath) or ".", exist_ok=True)
        with open(fpath, "w", encoding="utf-8") as fh:
            fh.write(text if text.endswith("\n") else text + "\n")


def _summary(issues: list, mus_info: list, effects: list) -> str:
    summary = (f"{len(mus_info) + len(effects)} recording(s) credited ({len(mus_info)} music, {len(effects)} effects), "
               f"{len([row_ for row_ in mus_info + effects if row_['cls']['attribution']])} CC BY; "
               + (f"{len(issues)} problem(s)" if issues else "no problems"))
    if any(row_["cls"]["attribution"] for row_ in mus_info):
        summary += ("; CC BY music: put the credit in the description, and dispute a Content ID claim "
                    "with the licence link")
    return summary


def build(cli) -> tuple[int, dict]:
    base_dir = os.path.abspath(cli.root)
    rep_doc = os.path.join(base_dir, cli.report)
    missing = _missing_input(rep_doc, os.path.join(base_dir, cli.manifests))
    if missing:
        return 2, missing
    with open(rep_doc, encoding="utf-8") as fh:
        in_use = [rights.rel(fname, base_dir) for fname in json.load(fh).get("files_used", [])]
    recs, _ = rights.load_manifests(os.path.join(base_dir, cli.manifests))
    design = load_design(cli.design, base_dir)
    table = _rows(in_use, _index(recs, base_dir), _audit(cli, base_dir))
    issues, mus_info, effects = table
    md_text = _markdown(cli, design, table)
    blurb = _description(design, mus_info, effects)
    out_md, out_desc = os.path.join(base_dir, cli.out), os.path.join(base_dir, cli.desc)
    _write(((out_md, "\n".join(md_text)), (out_desc, "\n".join(blurb).strip() + "\n")))
    return (1 if issues else 0), {"summary": _summary(issues, mus_info, effects),
                                    "written": [out_md, out_desc], "problems": issues}


def parse(argv):
    ap = argparse.ArgumentParser(prog="credits.py", description="Credits for every audible recording.")
    ap.add_argument("--root", default=".")
    ap.add_argument("--report", default="out/qa/mix_report.json")
    ap.add_argument("--manifests", default="audio/assets/manifests")
    ap.add_argument("--design", help="the design, for the processing sentences")
    ap.add_argument("--rights", help="rights.json: each used file must be confirmed there")
    ap.add_argument("--out", default="CREDITS.md")
    ap.add_argument("--desc", default="out/CREDITS_description.txt")
    ap.add_argument("--title")
    ap.add_argument("--self-test", action="store_true")
    return ap.parse_args(argv)


def main(argv=None) -> int:
    cli = parse(sys.argv[1:] if argv is None else argv)
    if cli.self_test:
        return self_test()
    code, result_ = build(cli)
    if code == 2:
        say(f"credits.py: {result_['error']}", file=sys.stderr)
        return 2
    say(result_["summary"])
    for wv in result_["written"]:
        say("wrote", wv)
    for pv in result_["problems"]:
        say("PROBLEM:", pv)
    return code


def _fixture(fx, tmp: str) -> str:
    """Manifests, a mix report and a design under d. Returns the manifest folder."""
    mus = os.path.join(tmp, "audio", "assets", "manifests")
    fx.put_json(os.path.join(mus, "ui.json"), [
        {"id": "fs-1", "category": "ui", "file": "audio/assets/ui/fs-1_tap.wav", "title": "Tap",
         "author": "Bea", "page_url": "https://freesound.org/people/Bea/sounds/1/", "rights": "CC BY 4.0",
         "attribution": "Tap by Bea, CC BY 4.0."},
        {"id": "fs-2", "category": "ui", "file": "audio/assets/ui/fs-2_pop.wav", "title": "Pop",
         "author": "al", "page_url": "https://freesound.org/people/al/sounds/2/", "rights": "CC0"}])
    fx.put_json(os.path.join(mus, "music.json"), [
        {"id": "cm-9", "category": "music", "file": "audio/assets/music/cm-9_song.wav", "title": "Song",
         "author": "Composer", "page_url": "https://composer.example/song", "rights": "CC BY 4.0",
         "attribution": "\"Song\" by Composer (https://composer.example/song), CC BY 4.0 "
                        "(https://creativecommons.org/licenses/by/4.0/)"}])
    fx.put_json(os.path.join(tmp, "out", "qa", "mix_report.json"), {"files_used": [
        "audio/assets/ui/fs-2_pop.wav", "audio/assets/ui/fs-1_tap.wav", "audio/assets/music/cm-9_song.wav"]})
    fx.put_json(os.path.join(tmp, "audio", "sounddesign.json"), {
        "samples": {"a.pop": {"file": "audio/assets/ui/fs-2_pop.wav", "eq": {"hp": 100}}},
        "slices": {"a.taps": {"file": "audio/assets/ui/fs-1_tap.wav"}},
        "rules": [{"name": "r1", "pool": "a.pop"}, {"name": "r2", "pool": "a.taps"}],
        "music": {"file": "audio/assets/music/cm-9_song.wav", "src_bpm": 120, "target_bpm": 120,
                  "stretch": "none", "segments": [{"src_from": 0, "src_to": 2, "at": 0},
                                                  {"src_from": 4, "src_to": 6, "at": 2, "tail_fade": 1}]}})
    return mus


def _test_credits(ck, tmp: str) -> None:
    code = main(["--root", tmp, "--design", "audio/sounddesign.json", "--title", "Demo"])
    md_text = open(os.path.join(tmp, "CREDITS.md")).read()
    blurb = open(os.path.join(tmp, "out", "CREDITS_description.txt")).read()
    tap = next(ln for ln in md_text.splitlines() if ln.startswith("- Tap by Bea"))
    ck("S7 sections present, by4 before cc0",
       code == 0 and "## Music" in md_text and "## Sound effects and ambience" in md_text and "## Tools" in md_text
       and md_text.index("### CC BY 4.0") < md_text.index("### CC0"))
    ck("S7 the CC BY line gains the page link and the licence link",
       tap == "- Tap by Bea, CC BY 4.0 (https://freesound.org/people/Bea/sounds/1/) "
              "(https://creativecommons.org/licenses/by/4.0/)")
    ck("S7 the description lists the CC0 author", "Other sound effects by al (CC0)." in blurb)
    ck("S7 processing names slices and EQ", "cut into single hits" in md_text and "equalised" in md_text)
    ck("music sentence", "cut and rearranged at bar lines" in md_text and "a fade at the end" in md_text)


def _test_audit(ck, fx, tmp: str, mus: str) -> None:
    fx.put_json(os.path.join(mus, "rights.json"), [
        {"id": "fs-1", "file_check": {"path": "audio/assets/ui/fs-1_tap.wav"}, "rights_confirmed": True},
        {"id": "fs-2", "file_check": {"path": "audio/assets/ui/fs-2_pop.wav"}, "rights_confirmed": False},
        {"id": "cm-9", "file_check": {"path": "audio/assets/music/cm-9_song.wav"}, "rights_confirmed": True}])
    code = main(["--root", tmp, "--rights", "audio/assets/manifests/rights.json"])
    md_text = open(os.path.join(tmp, "CREDITS.md")).read()
    ck("S8 an unconfirmed file gives PROBLEMS and exit 1", code == 1 and "## PROBLEMS" in md_text)
    ck("exit 2 without a report", main(["--root", tmp, "--report", "nope.json"]) == 2)


def self_test() -> int:
    import tempfile
    import fixtures as fx
    ck = fx.Checker()
    with tempfile.TemporaryDirectory() as tmp:
        mus = _fixture(fx, tmp)
        _test_credits(ck, tmp)
        _test_audit(ck, fx, tmp, mus)
    return ck.code()


if __name__ == "__main__":
    sys.exit(main())
