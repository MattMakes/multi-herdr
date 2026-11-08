"""``hark gallery``: 1 static HTML page to listen to forge results, with no server and no network."""

from __future__ import annotations

import argparse
import os
import time
from pathlib import Path
from typing import Any

from hark.surfaces import gallery_inputs as gi

CODECS = ("flac", "wav", "mp3", "opus", "vorbis")
LOSSY = {  # codec: (encoder, container, mime, extension, default kbps)
    "mp3": ("libmp3lame", "mp3", "audio/mpeg", ".mp3", 192),
    "opus": ("libopus", "ogg", "audio/ogg", ".opus", 128),
    "vorbis": ("libvorbis", "ogg", "audio/ogg", ".ogg", 160),
}
KBPS_RANGE = (16, 320)
NOTE_LINES = 12

FAMILIES: tuple[tuple[str, str, str, tuple[str, ...]], ...] = (
    ("whoosh", "Whooshes", "Moving air for transitions, pass-bys and slides.",
     ("whoosh_fast", "whoosh_big", "pro_whoosh_big", "swish", "swipe")),
    ("sweep", "Sweeps", "Builds into a cut, and falls away after it.",
     ("riser", "pro_riser", "downlifter", "pro_zip")),
    ("impact", "Impacts", "Hits on the cut, low weight and closing stings.",
     ("impact", "pro_impact", "sub_drop", "pro_sub_drop", "logo_sting", "pro_logo_sting")),
    ("ui", "Interface", "Clicks, taps, toggles and hover ticks.",
     ("click", "pro_click", "tap", "pro_tap", "pro_pop_in", "toggle", "hover_tick", "pro_hover_tick")),
    ("tonal", "Tonal accents", "Short pitched accents.",
     ("pop", "pro_pop", "notify", "pro_notify", "shimmer", "pro_shimmer")),
    ("digital", "Digital", "Digital stutters and key presses.",
     ("glitch", "pro_glitch", "typing_key", "pro_typing_key", "pro_typing_run")),
)
OTHER = ("other", "Other", "Categories outside the 6 families.", ())
KEYWORDS: tuple[tuple[str, tuple[str, ...]], ...] = (
    ("whoosh", ("whoosh", "swish", "swipe", "swoosh")),
    ("sweep", ("riser", "downlifter", "zip", "sweep")),
    ("impact", ("impact", "sub_drop", "boom", "sting", "hit")),
    ("ui", ("click", "tap", "toggle", "hover", "pop_in", "button")),
    ("tonal", ("pop", "notify", "ding", "chime", "shimmer", "bell")),
    ("digital", ("glitch", "typing", "key", "stutter")),
)
PRO_TEXT: dict[str, str] = {
    "pro_whoosh_big": "Swelling whooshes that build into a cut, after the effects of launch films.",
    "pro_riser": "Risers after the effects of launch films.",
    "pro_zip": "A zip: a quick upward pitch sweep of about 0.2 s.",
    "pro_impact": "Hits after the effects of launch films.",
    "pro_sub_drop": "A falling sub-bass tone that sits under a hit.",
    "pro_logo_sting": "The closing sting of a launch film that has effects and no music.",
    "pro_click": "Interface clicks after the effects of launch films.",
    "pro_tap": "A tap: a low thump near 150 Hz with a quiet click on top.",
    "pro_pop_in": "The effect those films use most: an element appears, with a soft low thump and a quiet tick.",
    "pro_hover_tick": "Hover ticks after the effects of launch films.",
    "pro_pop": "Pops after the effects of launch films.",
    "pro_notify": "Notification dings after the effects of launch films.",
    "pro_shimmer": "Shimmers after the effects of launch films.",
    "pro_glitch": "Glitch bursts after the effects of launch films.",
    "pro_typing_key": "Key presses after the effects of launch films.",
    "pro_typing_run": "Several key presses in a row, for typing on screen.",
}
TITLE_ONE = "hark sound gallery"
TITLE_MANY = "hark: version comparison"


# ---------------------------------------------------------------- families


def family_of(cat: str) -> str:
    """The family key of a category (member list, then without pro_, then keywords, else other)."""
    for name in (cat, cat[4:] if cat.startswith("pro_") else None):
        for key, _, _, members in FAMILIES:
            if name in members:
                return key
    return next((key for key, words in KEYWORDS if any(w in cat for w in words)), "other")


def card_order(cats: list[str]) -> list[str]:
    """Categories in display order: by family index and list index, then the rest by name."""
    keys = [f[0] for f in FAMILIES]

    def key(cat: str) -> tuple:
        fam = family_of(cat)
        members = FAMILIES[keys.index(fam)][3] if fam in keys else ()
        if cat in members:
            return (0, keys.index(fam), members.index(cat), "")
        return (1, 0, 0, cat)
    return sorted(cats, key=key)


def display_name(cat: str) -> str:
    name = (cat[4:] if cat.startswith("pro_") else cat).replace("_", " ")
    return name[:1].upper() + name[1:]


# ---------------------------------------------------------------- options


def _codec_options(embed: bool, codec: str | None, compressed: bool, kbps: int | None) -> tuple[bool, str, int | None]:
    codec = codec or "flac"
    if codec not in CODECS:
        raise ValueError(f"unknown codec {codec!r}; codecs: {', '.join(CODECS)}")
    if compressed:
        embed = True
        codec = codec if codec in LOSSY else "mp3"
    if kbps is not None:
        if not (embed and codec in LOSSY):
            raise ValueError("--kbps needs an embedded lossy codec (--embed-compressed or --embed-codec mp3|opus|vorbis)")
        if not KBPS_RANGE[0] <= kbps <= KBPS_RANGE[1]:
            raise ValueError(f"--kbps must be {KBPS_RANGE[0]} to {KBPS_RANGE[1]} (got {kbps})")
    elif codec in LOSSY:
        kbps = LOSSY[codec][4]
    return embed, codec, kbps


def _check_refs(refs: str | None) -> None:
    if refs == "bench":
        raise ValueError("--refs bench is refused: a page never carries bench references")
    if refs not in (None, "forge") and not os.path.isdir(refs):
        raise ValueError(f"--refs must be 'forge' or a folder (got {refs!r})")


def _versions(folder: str | None, compare: list[str] | None, sub: str, alias: dict[str, str]) -> dict[str, dict[str, str]]:
    if compare:
        return gi.parse_compare(list(compare), alias)
    if folder is None:
        raise ValueError("give a forge output folder or --compare LABEL:GLOB ...")
    found = gi.discover(folder, sub)
    return {sub: {alias.get(c, c): f for c, f in found.items()}}


# ---------------------------------------------------------------- sounds


def _sound(path: str, rep: dict, warnings: list[str]) -> dict[str, Any] | None:
    from hark.surfaces import gallery_sound as gs
    try:
        m = gs.measure(path)
    except Exception as exc:
        warnings.append(f"{os.path.basename(path)}: unreadable ({type(exc).__name__}: {exc})")
        return None
    name = os.path.splitext(os.path.basename(path))[0]
    pick = rep["picks"].get(os.path.basename(path))
    s = dict(m, path=path, name=name, variant=None, critic=None, params={}, seed=None, bench=None, same_as=None)
    if isinstance(pick, dict):
        s.update(variant=gi.variant_of(pick), critic=gi.critic_of(pick),
                 params=pick["params"] if isinstance(pick.get("params"), dict) else {}, seed=pick.get("seed"))
    return s


def _version(label: str, folder: str | None, warnings: list[str]) -> dict[str, Any] | None:
    if folder is None:
        return None
    rep = gi.read_report(folder)
    sounds = [s for s in (_sound(p, rep, warnings) for p in gi.sound_files(folder)) if s]
    return {"label": label, "folder": folder, "report": rep, "sounds": sounds, "bench_summary": None,
            "bench_stale": False, "bench_from": None, "has_results": False}


# ---------------------------------------------------------------- benchmark


def _apply_own(v: dict, cat: str, res: dict, warnings: list[str]) -> None:
    c = res["cats"].get(cat)
    if c is None:
        return
    v["has_results"] = True
    notes: dict[str, list[str]] = {"renames": [], "stale": []}
    n_checks = (c.get("inside") or {}).get("n_checks")
    for s in v["sounds"]:
        e = gi.match_entry(c, s, res["created"], notes)
        s["bench"] = gi.bench_numbers(e, n_checks) if e else None
    _bench_warnings(cat, v["label"], notes, warnings)
    if v["sounds"] and all(s["bench"] for s in v["sounds"]):
        v["bench_summary"] = gi.bench_summary(c)
    else:
        v["bench_stale"] = True


def _bench_warnings(cat: str, label: str, notes: dict[str, list[str]], warnings: list[str]) -> None:
    if notes["renames"]:
        warnings.append(f"{cat} {label}: {len(notes['renames'])} file(s) renamed after the benchmark run, "
                        f"matched by length and peak: {'; '.join(notes['renames'])}")
    if notes["stale"]:
        more = " ..." if len(notes["stale"]) > 3 else ""
        warnings.append(f"{cat} {label}: benchmark numbers not shown for {len(notes['stale'])} sound(s) that "
                        f"changed after the run: {'; '.join(notes['stale'][:3])}{more}")


def _borrow_one(v: dict, cat: str, res: dict) -> bool:
    c = res["cats"].get(cat)
    if c is None:
        return False
    n_checks = (c.get("inside") or {}).get("n_checks")
    hit = 0
    for s in v["sounds"]:
        e = c["made"].get(os.path.basename(s["path"]))
        if e and gi.same_file(str(e.get("file") or ""), s["path"], res["path"]) and \
                not gi.stale_reason(e, s, res["created"], gi.RENAME_TOL_S):
            s.update(bench=gi.bench_numbers(e, n_checks), same_as=res["label"])
            hit += 1
    if hit and hit == len(v["sounds"]):
        v.update(bench_summary=gi.bench_summary(c), bench_from=res["label"])
    return hit > 0


def _apply_results(v: dict, cat: str, results: list[dict], warnings: list[str]) -> None:
    own = [r for r in results if r["label"] == v["label"]]
    for r in own:
        _apply_own(v, cat, r, warnings)
    if v["has_results"]:
        return
    for r in results:
        if r["label"] != v["label"] and _borrow_one(v, cat, r):
            return


def _mark_identical(versions: list[dict | None]) -> None:
    seen: dict[str, str] = {}
    for v in versions:
        if not v:
            continue
        for s in v["sounds"]:
            if s["sha1"] in seen and seen[s["sha1"]] != v["label"]:
                s["same_as"] = seen[s["sha1"]]
        for s in v["sounds"]:
            seen.setdefault(s["sha1"], v["label"])


# ---------------------------------------------------------------- cards


def _refs_for(card: dict, refs: str | None, warnings: list[str]) -> None:
    newest = next((v for v in reversed(card["versions"]) if v), None)
    if newest is None or not refs:
        return
    paths = gi.guard_refs(card["cat"], gi.ref_paths(refs, card["cat"], newest["report"]["refs"]), warnings)
    pairs = gi.pair_refs(newest["order"], paths, card["max_refs"])
    empty = {"path": None}
    card["refs"] = [(_sound(p, {"picks": {}}, warnings) or empty) if p else empty for p in pairs]
    card["audition"] = gi.audition_reel(newest["folder"], card["cat"], warnings)


def _card(cat: str, vmap: dict[str, dict[str, str]], opts: dict, warnings: list[str]) -> dict[str, Any]:
    from hark.surfaces import gallery_sound as gs
    versions = [_version(label, cats.get(cat), warnings) for label, cats in vmap.items()]
    for v in versions:
        if v:
            _apply_results(v, cat, opts["results"], warnings)
    _mark_identical(versions)
    orders = gi.align([v["sounds"] if v else None for v in versions])
    for v, order in zip(versions, orders):
        if v:
            v["order"] = order or []
    card = {"cat": cat, "family": family_of(cat), "display": display_name(cat), "versions": versions,
            "desc": gi.category_description(cat, PRO_TEXT), "refs": [], "audition": None,
            "max_refs": opts["max_refs"]}
    _refs_for(card, opts["refs"], warnings)
    durs = [s["dur_s"] for v in versions if v for s in v["sounds"]] + [r.get("dur_s") for r in card["refs"]]
    card["span"] = gs.card_span(durs)
    return card


# ---------------------------------------------------------------- build


def _thumbs(cards: list[dict], assets: str, warnings: list[str]) -> dict[str, int]:
    from hark.surfaces import gallery_sound as gs
    os.makedirs(assets, exist_ok=True)
    stats = {"made": 0, "reused": 0, "used": set()}
    for card in cards:
        tiles = [(v["label"], s) for v in card["versions"] if v for s in v["sounds"]]
        tiles += [("ref", r) for r in card["refs"] if r.get("path")]
        for label, s in tiles:
            _thumb(card, label, s, assets, stats, warnings, gs)
    return stats


def _thumb(card: dict, label: str, s: dict, assets: str, stats: dict, warnings: list[str], gs: Any) -> None:
    try:
        name = gs.thumb_name(label, card["cat"], s["path"], card["span"])
        made = gs.write_thumb(s["path"], os.path.join(assets, name), card["span"])
    except Exception as exc:
        warnings.append(f"{os.path.basename(s['path'])}: no spectrogram ({type(exc).__name__}: {exc})")
        s["thumb"] = None
        return
    stats["made" if made else "reused"] += 1
    stats["used"].add(name)
    s["thumb"] = gi.slash(os.path.join(assets, name))


def _prune(assets: str, used: set[str], enc_used: set[str], lossy: bool) -> int:
    pruned = 0
    for name in os.listdir(assets):
        if name.endswith(".png") and name not in used and _rm(os.path.join(assets, name)):
            pruned += 1
    enc = os.path.join(assets, "enc")
    if lossy and os.path.isdir(enc):
        exts = tuple(v[3] for v in LOSSY.values()) + (".part",)
        for name in os.listdir(enc):
            if name.endswith(exts) and name not in enc_used:
                _rm(os.path.join(enc, name))
    return pruned


def _rm(path: str) -> bool:
    try:
        os.remove(path)
        return True
    except OSError:
        return False


def _all_audio(cards: list[dict]) -> list[str]:
    out = []
    for card in cards:
        out += [s["path"] for v in card["versions"] if v for s in v["sounds"]]
        out += [r["path"] for r in card["refs"] if r.get("path")]
        out += [card["audition"]] if card["audition"] else []
    return out


def _results_list(results: Any, labels: list[str]) -> list[dict]:
    return gi.assign_results(results, labels) if results else []


def _version_counts(cards: list[dict], labels: list[str]) -> dict[str, int]:
    return {lab: sum(1 for c in cards for v in c["versions"] if v and v["label"] == lab) for lab in labels}


def _critic_pass(cards: list[dict], labels: list[str]) -> dict[str, list[int]]:
    out = {}
    for lab in labels:
        sounds = [s for c in cards for v in c["versions"] if v and v["label"] == lab for s in v["sounds"]]
        known = [s for s in sounds if s["critic"] and s["critic"].get("passes") is not None]
        out[lab] = [sum(1 for s in known if s["critic"]["passes"]), len(known)]
    return out


def _codec_text(embed: bool, codec: str, kbps: int | None) -> str | None:
    if not embed:
        return None
    return f"{codec} {kbps} kbps" if codec in LOSSY else codec


def gallery(folder: str | None = None, out: str = "gallery.html", results: Any = None, refs: str | None = None,
            compare: list[str] | None = None, embed: bool = False, codec: str = "flac", sub: str = "final",
            title: str | None = None, max_refs: int = 6, compressed: bool = False, kbps: int | None = None,
            max_mb: float = 25.0, alias: Any = None) -> dict[str, Any]:
    """Build 1 static HTML listening page from forge output folders."""
    from hark.surfaces import gallery_page, gallery_sound
    t0 = time.monotonic()
    _check_refs(refs)
    embed, codec, kbps = _codec_options(embed, codec, compressed, kbps)
    vmap = _versions(folder, compare, sub, gi.parse_alias(alias))
    labels = list(vmap)
    opts = {"results": _results_list(results, labels), "refs": refs, "max_refs": max_refs}
    warnings: list[str] = []
    gallery_sound.clear_cache()
    cats = card_order(sorted({c for cs in vmap.values() for c in cs}))
    cards = [_card(c, vmap, opts, warnings) for c in cats]
    html_path = os.path.abspath(out)
    os.makedirs(os.path.dirname(html_path), exist_ok=True)
    assets = os.path.splitext(html_path)[0] + "_files"
    thumbs = _thumbs(cards, assets, warnings)
    media = gallery_page.Media(html_path, assets, embed, codec, kbps)
    media.prepare(_all_audio(cards))
    page = gallery_page.Page(cards, labels, opts["results"], media, title or (TITLE_ONE if len(labels) == 1 else TITLE_MANY),
                             warnings, FAMILIES + (OTHER,))
    html = page.render()
    Path(html_path).write_text(html, encoding="utf-8")
    pruned = _prune(assets, thumbs["used"], media.enc_used, codec in LOSSY and embed)
    size_mb = round(len(html.encode("utf-8")) / 1e6, 2)
    if embed and size_mb > max_mb:
        warnings.append(f"the page is {size_mb} MB, over the {max_mb} MB limit; try a lower --kbps, "
                        "--embed-codec opus, or fewer categories")
    result = {"html": gi.slash(html_path), "assets": gi.slash(assets), "url": Path(html_path).as_uri(),
              "versions": _version_counts(cards, labels), "categories": len(cards),
              "sounds": sum(len(v["sounds"]) for c in cards for v in c["versions"] if v),
              "refs": sum(1 for c in cards for r in c["refs"] if r.get("path")),
              "images": {"made": thumbs["made"], "reused": thumbs["reused"], "pruned": pruned},
              "embedded": embed, "codec": _codec_text(embed, codec, kbps), "size_mb": size_mb,
              "critic_pass": _critic_pass(cards, labels), "seconds": round(time.monotonic() - t0, 1),
              "warnings": warnings, "same_audio": media.dedup}
    if embed:
        result["embedded_mb"] = {"audio": round(media.audio_bytes * 4 / 3 / 1e6, 2),
                                 "images": round(media.image_bytes * 4 / 3 / 1e6, 2)}
    result["encoded"] = media.encoded
    result["text"] = summary_text(result)
    result["summary"] = result["text"].splitlines()[0]
    return result


def _plural(n: int, one: str, many: str) -> str:
    return f"{n} {one if n == 1 else many}"


def summary_text(r: dict[str, Any]) -> str:
    """The CLI text of a gallery result."""
    refs = f", {r['refs']} refs" if r["refs"] else ""
    emb = f", embedded {r['codec']}" if r["embedded"] else ""
    lines = [f"gallery: {_plural(r['categories'], 'category', 'categories')}, {_plural(r['sounds'], 'sound', 'sounds')}"
             f"{refs} -> {r['html']} ({r['size_mb']} MB{emb}, {r['seconds']} s)",
             "  versions: " + ", ".join(f"{k} ({v})" for k, v in r["versions"].items()),
             f"  thumbnails: {r['images']['made']} new, {r['images']['reused']} cached, "
             f"{r['images']['pruned']} pruned in {r['assets']}"]
    if r["embedded"]:
        lines.append(_embedded_line(r))
    lines.append(f"  open: {r['url']}")
    lines += [f"  note: {w}" for w in r["warnings"][:NOTE_LINES]]
    if len(r["warnings"]) > NOTE_LINES:
        lines.append(f"  ... {len(r['warnings']) - NOTE_LINES} more notes")
    return "\n".join(lines)


def _embedded_line(r: dict[str, Any]) -> str:
    line = f"  embedded (base64): audio {r['embedded_mb']['audio']} MB, images {r['embedded_mb']['images']} MB"
    if r["same_audio"]:
        line += f"; {r['same_audio']} identical file(s) embedded once"
    if r["codec"] and " kbps" in r["codec"]:
        line += f"; previews {r['encoded']} encoded, the rest from the cache"
    return line


# ---------------------------------------------------------------- CLI


def _add_flags(p: argparse.ArgumentParser) -> None:
    p.add_argument("folder", nargs="?", default=None, help="forge output tree, category folder or final folder")
    p.add_argument("-o", dest="html", default="gallery.html", help="output HTML (default gallery.html)")
    p.add_argument("--results", nargs="+", default=None, help="benchmark results JSON, [LABEL:]PATH")
    p.add_argument("--refs", default=None, help="'forge' or a folder: add a forge-reference A/B row")
    p.add_argument("--compare", nargs="+", default=None, help="LABEL:GLOB versions shown as rows (quote the globs)")
    p.add_argument("--sub", default="final", help="subfolder per category that holds the sounds")
    p.add_argument("--alias", nargs="+", default=None, help="OLD=NEW: show category OLD on the card of NEW")
    p.add_argument("--embed", action="store_true", help="put audio and images inside the page")
    p.add_argument("--embed-compressed", action="store_true", help="1 small page with lossy previews")
    p.add_argument("--embed-codec", choices=CODECS, default=None, help="codec of embedded audio")
    p.add_argument("--kbps", type=int, default=None, help="bit rate of lossy previews, 16 to 320")
    p.add_argument("--max-mb", type=float, default=25.0, help="warn when an embedded page is larger")
    p.add_argument("--title", default=None, help="page title")
    p.add_argument("--max-refs", type=int, default=6, help="at most this many reference tiles per card")


def add_cli(sub: Any) -> None:
    """Register ``hark gallery``."""
    from hark.core import report
    p = sub.add_parser("gallery", help="One static HTML page of forge results.")
    _add_flags(p)
    report.add_common(p)
    p.set_defaults(func=handle)


def handle(args: argparse.Namespace) -> int:
    from hark.core import report
    result = gallery(args.folder, out=args.html, results=args.results, refs=args.refs, compare=args.compare,
                     embed=args.embed, codec=args.embed_codec or "flac", sub=args.sub, title=args.title,
                     max_refs=args.max_refs, compressed=args.embed_compressed, kbps=args.kbps,
                     max_mb=args.max_mb, alias=args.alias)
    report.emit(result, args, text=result["text"])
    return 0
