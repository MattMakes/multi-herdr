#!/usr/bin/env python3
"""Usage-rights and file audit of every manifest entry.

The rights auditor writes only rights_seen, rights_confirmed and agent_notes
into rights.json. This script computes every other field on each run, keeps
the auditor's fields, and blocks the film on any recording it cannot clear.
Run with the hark venv Python and -I. --self-test runs the built-in checks.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import time
import urllib.request
from collections import Counter

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import rights  # noqa: E402

TOOL = "sound-sourcing rights_audit v1"
AGENT_FIELDS = ("rights_seen", "rights_confirmed", "agent_notes")
BYTE_FIELDS = ("bytes_match_download_url", "fetch_note", "sha1_local", "sha1_remote")
TOOL_FILE_FIELDS = {"path", "exists", "codec", "sample_rate", "channels", "kbps", "duration_s", "video",
                    "decodes_clean", "max_volume_db"}
SKIP_FETCH_HOSTS = ("pixabay.com",)
USER_AGENT = "sound-sourcing-rights-check/1.0"
MAX_FETCH = 200 * 1024 * 1024


def say(*parts, file=None):
    text = " ".join(str(p) for p in parts)
    print(text.encode("ascii", "replace").decode("ascii"), file=file or sys.stdout)


# ------------------------------------------------------------------ file checks

def probe(path: str) -> dict:
    """ffprobe facts of a file: the first audio stream, and any real video stream."""
    cmd = ["ffprobe", "-v", "error", "-show_streams", "-show_format", "-of", "json", path]
    try:
        out = subprocess.run(cmd, capture_output=True, text=True, timeout=120)
    except (OSError, subprocess.TimeoutExpired) as e:
        return {"probe_error": str(e)}
    if out.returncode != 0:
        return {"probe_error": (out.stderr.strip().splitlines() or ["ffprobe failed"])[-1]}
    data = json.loads(out.stdout or "{}")
    streams, fmt = data.get("streams", []), data.get("format", {})
    audio = next((s for s in streams if s.get("codec_type") == "audio"), None)
    if audio is None:
        return {"probe_error": "no audio stream"}
    video = any(s.get("codec_type") == "video" and not (s.get("disposition") or {}).get("attached_pic")
                for s in streams)
    br = audio.get("bit_rate") or fmt.get("bit_rate")
    try:
        br = float(br)
        kbps = round(br / 1000.0, 1) if 1 <= br <= 10_000_000 else None
    except (TypeError, ValueError):
        kbps = None
    dur = audio.get("duration") or fmt.get("duration")
    return {"codec": audio.get("codec_name"), "sample_rate": int(audio.get("sample_rate") or 0),
            "channels": int(audio.get("channels") or 0), "kbps": kbps,
            "duration_s": round(float(dur), 3) if dur not in (None, "N/A") else None, "video": video}


def decode_check(path: str) -> dict:
    """One full decode of the first audio stream to null, with volume detection."""
    cmd = ["ffmpeg", "-hide_banner", "-nostdin", "-nostats", "-i", path,
           "-map", "0:a:0", "-af", "volumedetect", "-f", "null", "-"]
    try:
        out = subprocess.run(cmd, capture_output=True, text=True, timeout=600)
    except (OSError, subprocess.TimeoutExpired) as e:
        return {"decodes_clean": False, "decode_error": str(e)}
    err = out.stderr or ""
    bad = [ln for ln in err.splitlines() if re.search(r"\b(error|fatal)\b", ln, re.I)
           and "volumedetect" not in ln]
    vols = re.findall(r"max_volume:\s*(-?[0-9.]+|-inf) dB", err)
    res = {"decodes_clean": out.returncode == 0 and not bad}
    if vols:
        res["max_volume_db"] = -200.0 if vols[-1] == "-inf" else float(vols[-1])
    if bad:
        res["decode_error"] = bad[0][:200]
    return res


def sha1_file(path: str) -> str:
    h = hashlib.sha1()
    with open(path, "rb") as fh:
        for block in iter(lambda: fh.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def fetch_compare(url: str, path: str) -> dict:
    """Download url into memory (never to disk, never run) and compare its SHA-1 with the local file."""
    host = (urllib.request.urlparse(url).hostname or "").lower()
    if any(host == h or host.endswith("." + h) for h in SKIP_FETCH_HOSTS):
        return {"fetch_note": f"{host} blocks scripted downloads: compare by hand"}
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            body = r.read(MAX_FETCH + 1)
    except Exception as e:  # network errors are notes, not crashes
        return {"fetch_note": f"fetch failed: {e}"[:200]}
    if len(body) > MAX_FETCH:
        return {"fetch_note": "remote file is over 200 MB: not compared"}
    remote, local = hashlib.sha1(body).hexdigest(), sha1_file(path)
    return {"bytes_match_download_url": remote == local, "sha1_local": local, "sha1_remote": remote}


# ------------------------------------------------------------------ checks per entry

_FS_PAGE = re.compile(r"freesound\.org/people/([^/]+)/sounds/(\d+)", re.I)
_FS_PREVIEW = re.compile(r"previews/(\d+)/(\d+)_(\d+)-(hq|lq)\.(ogg|mp3)", re.I)
_MK = re.compile(r"assets\.mixkit\.co/active_storage/sfx/(\d+)/(\d+)\.(wav|mp3)", re.I)
_READ_NOTE = re.compile(r"[^.]*\b(?:rights?|licen[cs]e\w*)\b.{0,40}?\b(?:read|verified|checked|seen|confirmed)\b[^.]*",
                        re.I)


def url_warnings(e: dict) -> list[str]:
    out = []
    eid = str(e.get("id") or "")
    page = e.get("page_url") or ""
    dl = e.get("download_url") or ""
    m = _FS_PAGE.search(page)
    if m:
        user, sid = m.group(1), int(m.group(2))
        n = re.match(r"fs-(\d+)$", eid)
        if n and int(n.group(1)) != sid:
            out.append(f"id {eid} does not match the Freesound page sound {sid}")
        p = _FS_PREVIEW.search(dl)
        if p:
            folder, pid = int(p.group(1)), int(p.group(2))
            if pid != sid:
                out.append(f"the preview URL is sound {pid}, the page is sound {sid}")
            if folder != pid // 1000:
                out.append(f"the preview folder {folder} is not {pid} // 1000 = {pid // 1000}")
            if p.group(4).lower() == "lq":
                out.append("the download is the -lq preview: use -hq")
        if e.get("author") and str(e["author"]).lower() != user.lower():
            out.append(f"author {e['author']!r} is not the page user {user!r}")
    m = _MK.search(dl)
    if m:
        a, b = int(m.group(1)), int(m.group(2))
        if a != b:
            out.append(f"the Mixkit URL folder {a} and file {b} differ")
        n = re.match(r"mk-(\d+)$", eid)
        if n and int(n.group(1)) != a:
            out.append(f"id {eid} does not match the Mixkit item {a}")
    return out


def check_entry(e: dict, root: str, decode: bool, earlier: dict | None) -> dict:
    cls = rights.classify(e.get("rights"))
    agent = {k: (earlier or {}).get(k) for k in AGENT_FIELDS}
    seen = agent.get("rights_seen")
    confirmed = bool(agent.get("rights_confirmed")) and bool(seen)
    problems, warnings = [], []

    if not e.get("file"):
        problems.append("no file in the manifest entry")
    if not e.get("rights"):
        problems.append("no rights in the manifest entry")
    for f in ("id", "title", "author", "page_url"):
        if not e.get(f):
            warnings.append(f"no {f}")
    if cls["status"] != "allowed" and e.get("rights"):
        problems.append(f"rights {cls['key']} ({cls['status']}): {cls['reason']}")

    if not seen:
        m = _READ_NOTE.search(e.get("notes") or "")
        if m:
            problems.append(f"no page reading in rights_seen, but the notes say: \"{m.group(0).strip()[:90]}\"; "
                            "copy that reading into rights_seen")
        else:
            problems.append("no page reading: read the item page and fill in rights_seen")
    elif not confirmed:
        problems.append("rights_seen is recorded but rights_confirmed is not true")
    if seen:
        seen_text = seen if isinstance(seen, str) else json.dumps(seen)
        if isinstance(seen, dict):
            seen_text = " ".join(str(seen.get(k, "")) for k in ("rights", "licence", "license", "text")) or seen_text
        sc = rights.classify(seen_text)
        if sc["status"] != "unknown" and sc["key"] not in ("none", "unknown") and sc["key"] != cls["key"]:
            warnings.append(f"rights_seen reads as {sc['key']}, the manifest as {cls['key']}")

    attr_req = bool(cls["attribution"])
    attr_ok = None
    if attr_req:
        attr = e.get("attribution") or ""
        missing = [f for f in ("attribution", "author", "page_url") if not e.get(f)]
        if missing:
            problems.append("CC BY credit incomplete: no " + ", ".join(missing))
        if attr:
            author_part = attr.split(" (")[0]
            if e.get("author") and str(e["author"]).lower() not in author_part.lower():
                warnings.append("the attribution does not name the author")
            low = attr.lower()
            if not ("cc by" in low or "attribution" in low or "creativecommons.org" in low):
                warnings.append("the attribution names no licence")
        attr_ok = not missing

    warnings += url_warnings(e)

    path = rights.rel(e.get("file") or "", root)
    ap = os.path.join(root, path)
    fc = {"path": path, "exists": os.path.isfile(ap)}
    if not fc["exists"]:
        problems.append(f"file missing: {path}")
    else:
        pr = probe(ap)
        if "probe_error" in pr:
            problems.append(f"ffprobe failed: {pr['probe_error']}")
        else:
            fc.update(pr)
            if pr["video"]:
                warnings.append("the file carries a video stream")
            if pr["sample_rate"] and pr["sample_rate"] < 44100:
                warnings.append(f"sample rate {pr['sample_rate']} Hz is under 44100")
            if pr["codec"] == "mp3" and pr["kbps"] and pr["kbps"] < 192:
                warnings.append(f"mp3 at {pr['kbps']} kbps is under 192")
            want = (e.get("analysis") or {}).get("duration")
            if want is not None and pr["duration_s"] is not None and abs(float(want) - pr["duration_s"]) > 0.05:
                warnings.append(f"duration {pr['duration_s']} s differs from the manifest analysis {want} s "
                                "(file replaced?)")
        if decode and "probe_error" not in pr:
            dc = decode_check(ap)
            fc["decodes_clean"] = dc["decodes_clean"]
            if "max_volume_db" in dc:
                fc["max_volume_db"] = dc["max_volume_db"]
            if not dc["decodes_clean"]:
                problems.append("decode errors: " + dc.get("decode_error", "ffmpeg failed"))
            mv = dc.get("max_volume_db")
            if mv is not None and mv <= -90:
                problems.append("digital silence (max volume <= -90 dB)")
            elif mv is not None and mv < -60:
                warnings.append(f"very quiet file (max volume {mv} dB)")
    file_ok = fc["exists"] and fc.get("decodes_clean", True) is not False and "codec" in fc

    rec = {"id": e.get("id"), "manifest": e.get("_manifest"), "need": e.get("need"), "rank": e.get("rank"),
           "in_scope": True, "severity": "ok", "rights_in_manifest": e.get("rights"), "rights": cls["key"],
           "rights_status": cls["status"], "rights_seen": seen, "rights_confirmed": confirmed,
           "attribution_required": attr_req, "attribution_ok": attr_ok,
           "attribution_in_manifest": e.get("attribution"), "file_ok": bool(file_ok), "file_check": fc,
           "problems": problems, "warnings": warnings, "agent_notes": agent.get("agent_notes"),
           "audit_tool": TOOL}
    return rec


def carry(old: dict | None) -> tuple[dict, dict, dict]:
    """From an earlier record: (agent fields, earlier_file_check, extra keys kept)."""
    if not old:
        return {}, {}, {}
    agent = {k: old.get(k) for k in AGENT_FIELDS}
    efc = dict(old.get("earlier_file_check") or {})
    fc = old.get("file_check") or {}
    extra = {}
    if old.get("audit_tool"):
        for k in BYTE_FIELDS:
            if k in fc:
                efc[k] = fc[k]
    else:
        notes = agent.get("agent_notes")
        notes = list(notes) if isinstance(notes, list) else ([notes] if notes else [])
        for k in ("warnings", "problem", "fix"):
            v = old.get(k)
            if v:
                notes += v if isinstance(v, list) else [f"{k}: {v}"]
        agent["agent_notes"] = notes or None
        for k, v in fc.items():
            if k not in TOOL_FILE_FIELDS:
                efc[k] = v
        known = set(AGENT_FIELDS) | {"id", "manifest", "need", "rank", "in_scope", "severity", "rights_in_manifest",
                                      "rights", "rights_status", "attribution_required", "attribution_ok",
                                      "attribution_in_manifest", "file_ok", "file_check", "problems", "warnings",
                                      "problem", "fix", "earlier_file_check", "audit_tool", "duplicate_of", "file"}
        extra = {k: v for k, v in old.items() if k not in known}
    return agent, efc, extra


def severity(rec: dict) -> str:
    if rec["in_scope"]:
        return "blocker" if rec["problems"] else ("warning" if rec["warnings"] else "ok")
    return "info" if (rec["problems"] or rec["warnings"]) else "ok"


# ------------------------------------------------------------------ run

def load_used(report: str | None, root: str) -> set | None:
    if not report:
        return None
    with open(report, encoding="utf-8") as fh:
        rep = json.load(fh)
    return {rights.rel(f, root).lower() for f in rep.get("files_used", [])}


def load_earlier(path: str | None) -> tuple[list, bool]:
    if not path or not os.path.isfile(path):
        return [], False
    try:
        with open(path, encoding="utf-8") as fh:
            data = json.load(fh)
    except (OSError, ValueError):
        return [], True
    return (data if isinstance(data, list) else []), True


def audit(a) -> tuple[int, dict]:
    root = os.path.abspath(a.root)
    mdir = os.path.join(root, a.manifests)
    if not os.path.isdir(mdir):
        return 2, {"error": f"manifest folder not found: {mdir}"}
    entries, notes = rights.load_manifests(mdir)
    if not entries:
        return 2, {"error": f"no manifest entries in {mdir}"}
    out = os.path.join(root, a.out) if a.out else os.path.join(mdir, "rights.json")
    existing = os.path.join(root, a.existing) if a.existing else out
    report = os.path.join(root, a.report) if a.report else None
    if report and not os.path.isfile(report):
        return 2, {"error": f"mix report not found: {report}"}
    used = load_used(report, root)
    earlier, _ = load_earlier(existing)
    by_key = {}
    for r in earlier:
        if not isinstance(r, dict):
            continue
        path = rights.rel((r.get("file_check") or {}).get("path") or r.get("file") or "", root).lower()
        by_key.setdefault((r.get("id"), path), r)
        by_key.setdefault((r.get("id"), ""), r)

    records, seen_files = [], {}
    for e in entries:
        path = rights.rel(e["file"], root).lower()
        old = by_key.get((e.get("id"), path)) or by_key.get((e.get("id"), ""))
        agent, efc, extra = carry(old)
        if path in seen_files:
            first = seen_files[path]
            rec = {"id": e.get("id"), "manifest": e.get("_manifest"), "need": e.get("need"), "rank": e.get("rank"),
                   "in_scope": False, "rights_in_manifest": e.get("rights"),
                   "rights": rights.classify(e.get("rights"))["key"],
                   "rights_status": rights.classify(e.get("rights"))["status"],
                   "rights_seen": agent.get("rights_seen"), "rights_confirmed": bool(agent.get("rights_confirmed")),
                   "file_check": {"path": rights.rel(e["file"], root)}, "problems": [],
                   "warnings": [f"duplicate of {first['id']}: the same file is already audited"],
                   "agent_notes": agent.get("agent_notes"), "audit_tool": TOOL, "duplicate_of": first["id"]}
            if rec["rights_status"] != "allowed":
                first["problems"].append(f"the same file is listed again as {e.get('id')} under "
                                         f"{e.get('rights')!r}: one recording has one licence")
            elif rec["rights"] != first["rights"]:
                first["warnings"].append(f"the same file is listed again as {e.get('id')} under another "
                                         f"licence ({rec['rights']})")
            records.append(rec)
            continue
        in_scope = (path in used) if used is not None else (old.get("in_scope", True) if old else True)
        do_decode = a.decode == "all" or (a.decode == "scope" and in_scope)
        rec = check_entry(e, root, do_decode, agent)
        rec["in_scope"] = bool(in_scope)
        if efc:
            rec["earlier_file_check"] = efc
        rec.update(extra)
        seen_files[path] = rec
        records.append(rec)

    if a.fetch or a.fetch_all:
        first_req = True
        for rec, e in zip(records, entries):
            if rec.get("duplicate_of") or not (a.fetch_all or rec["in_scope"]):
                continue
            url, ap = e.get("download_url"), os.path.join(root, rec["file_check"]["path"])
            if not url or not os.path.isfile(ap):
                continue
            if not first_req:
                time.sleep(1.5)
            first_req = False
            res = fetch_compare(url, ap)
            rec["file_check"].update(res)
            if res.get("bytes_match_download_url") is False:
                rec["warnings"].append("the local bytes differ from download_url (re-encoded or edited file)")

    if used is not None:
        listed = set(seen_files)
        for f in sorted(used - listed):
            records.append({"id": None, "in_scope": True, "file_check": {"path": f, "exists": None},
                            "rights": None, "rights_status": "unknown", "rights_confirmed": False,
                            "problems": ["used in the film but in no manifest"], "warnings": [],
                            "audit_tool": TOOL})
    for rec in records:
        rec["severity"] = severity(rec)

    scope = [r for r in records if r["in_scope"]]
    sev = Counter(r["severity"] for r in scope)
    keys = Counter(r.get("rights") for r in scope)
    with_notes = sum(1 for r in records if r.get("agent_notes"))
    matched_now = sum(1 for r in records if (r.get("file_check") or {}).get("bytes_match_download_url") is True)
    matched_before = sum(1 for r in records if (r.get("earlier_file_check") or {}).get("bytes_match_download_url"))
    summary = (f"{len(entries)} entries, {len(scope)} in scope: {sev['ok']} ok, {sev['warning']} warning, "
               f"{sev['blocker']} blocker; rights in scope: "
               + (", ".join(f"{k} x{n}" for k, n in keys.most_common()) or "none")
               + f"; {with_notes} record(s) with auditor notes; byte match {matched_now} this run, "
               f"{matched_before} from earlier audits")
    if sev["blocker"]:
        summary += "; fix every blocker before the film ships"

    written, backups = [], []
    if os.path.isfile(out):
        stem = out[:-5] if out.endswith(".json") else out
        prev = stem + ".prev.json"
        shutil.copyfile(out, prev)
        backups.append(prev)
        old_list, parsed = load_earlier(out)
        hand = not any(isinstance(r, dict) and r.get("audit_tool") for r in old_list)
        try:
            with open(out, encoding="utf-8") as fh:
                json.load(fh)
        except ValueError:
            hand = True
        hw = stem + ".handwritten.json"
        if hand and not os.path.exists(hw):
            shutil.copyfile(out, hw)
            backups.append(hw)
    os.makedirs(os.path.dirname(out), exist_ok=True)
    with open(out, "w", encoding="utf-8") as fh:
        json.dump(records, fh, indent=1, ensure_ascii=False)
        fh.write("\n")
    written.append(out)
    table = os.path.join(root, a.table) if a.table else (out[:-5] if out.endswith(".json") else out) + ".md"
    with open(table, "w", encoding="utf-8") as fh:
        fh.write(markdown(records, summary))
    written.append(table)
    code = 1 if sev["blocker"] else 0
    return code, {"summary": summary, "notes": notes, "records": records, "written": written, "backups": backups}


def markdown(records: list, summary: str) -> str:
    order = {"blocker": 0, "warning": 1, "info": 2, "ok": 3}
    rows = sorted(records, key=lambda r: (not r["in_scope"], order.get(r["severity"], 9), str(r.get("id"))))
    lines = ["# Usage-rights audit", "", summary, "",
             "| id | in film | severity | rights | verified | credit | file | issues |",
             "|---|---|---|---|---|---|---|---|"]
    for r in rows:
        fc = r.get("file_check") or {}
        if fc.get("codec"):
            sr = (fc.get("sample_rate") or 0) / 1000.0
            f = f"{fc['codec']} {sr:g}k {fc.get('channels')}ch {fc.get('duration_s')}s"
        else:
            f = "MISSING" if not fc.get("exists") else "?"
        credit = "n/a" if not r.get("attribution_required") else ("ok" if r.get("attribution_ok") else "MISSING")
        issues = "; ".join(r.get("problems", []) + r.get("warnings", []))[:300].replace("|", "/")
        lines.append(f"| {r.get('id')} | {'yes' if r['in_scope'] else 'no'} | {r['severity']} | {r.get('rights')} | "
                     f"{'yes' if r.get('rights_confirmed') else 'NO'} | {credit} | {f} | {issues} |")
    return "\n".join(lines) + "\n"


def print_result(res: dict):
    say(res["summary"])
    for n in res["notes"]:
        say("  note:", n)
    groups = Counter()
    ids = {}
    for r in res["records"]:
        if not r["in_scope"] or r["severity"] == "ok":
            continue
        for level, msgs in (("BLOCKER", r["problems"]), ("warning", r["warnings"])):
            for m in msgs:
                k = (level, m)
                groups[k] += 1
                ids.setdefault(k, []).append(str(r.get("id")))
    for (level, m), n in sorted(groups.items(), key=lambda kv: (kv[0][0] != "BLOCKER", -kv[1], kv[0][1])):
        more = " ..." if len(ids[(level, m)]) > 8 else ""
        say(f"  {level} x{n}: {m}  [{' '.join(ids[(level, m)][:8])}{more}]")
    for w in res["written"]:
        say("wrote", w)
    for b in res["backups"]:
        say("kept", b)


def parse(argv):
    p = argparse.ArgumentParser(prog="rights_audit.py", description="Usage-rights and file audit of the manifests.")
    p.add_argument("--root", default=".")
    p.add_argument("--manifests", default="audio/assets/manifests")
    p.add_argument("--report", help="mix report: its files_used are in scope")
    p.add_argument("--out", help="audit file (default <manifests>/rights.json)")
    p.add_argument("--existing", help="read the auditor fields from this file instead of --out")
    p.add_argument("--table", help="Markdown table (default --out with .md)")
    p.add_argument("--fetch", action="store_true", help="compare local bytes with download_url, in scope only")
    p.add_argument("--fetch-all", action="store_true", help="the same for every entry")
    p.add_argument("--decode", choices=("scope", "all", "none"), default="scope")
    p.add_argument("--json", action="store_true")
    p.add_argument("--self-test", action="store_true")
    return p.parse_args(argv)


def main(argv=None) -> int:
    a = parse(sys.argv[1:] if argv is None else argv)
    if a.self_test:
        return self_test()
    code, res = audit(a)
    if code == 2:
        say(f"rights_audit.py: {res['error']}", file=sys.stderr)
        return 2
    if a.json:
        say(json.dumps({k: v for k, v in res.items()}, indent=1))
    else:
        print_result(res)
    return code


def self_test() -> int:
    import tempfile
    import fixtures as fx
    ck = fx.Checker()
    with tempfile.TemporaryDirectory() as d:
        mdir = os.path.join(d, "audio", "assets", "manifests")
        wav = fx.write(os.path.join(d, "audio", "assets", "ui", "fs-123456_tap.wav"), fx.burst(0.3))
        entry = {"id": "fs-123456", "category": "ui", "file": "audio/assets/ui/fs-123456_tap.wav",
                 "title": "Tap", "author": "alice", "source": "freesound",
                 "page_url": "https://freesound.org/people/alice/sounds/123456/",
                 "download_url": "https://cdn.freesound.org/previews/123/123456_9-hq.ogg",
                 "rights": "CC BY 4.0", "rights_url": "https://creativecommons.org/licenses/by/4.0/",
                 "attribution": "\"Tap\" by alice, licensed under CC BY 4.0", "need": "click", "rank": 1}
        fx.put_json(os.path.join(mdir, "ui.json"), [entry])
        q = lambda *x: main(["--root", d, *x])
        code = q()
        recs = json.load(open(os.path.join(mdir, "rights.json")))
        ck("S2 no page reading is a blocker, exit 1, files written",
           code == 1 and recs[0]["severity"] == "blocker"
           and any("no page reading" in p for p in recs[0]["problems"])
           and os.path.isfile(os.path.join(mdir, "rights.md")))
        recs[0]["rights_seen"] = {"site": "freesound", "rights": "Attribution 4.0", "read": "2026-11-07"}
        recs[0]["rights_confirmed"] = True
        recs[0]["agent_notes"] = "author asks for the credit form in the manifest"
        fx.put_json(os.path.join(mdir, "rights.json"), recs)
        code = q()
        recs = json.load(open(os.path.join(mdir, "rights.json")))
        ck("S3 a confirmed reading clears the entry", code == 0 and recs[0]["severity"] == "ok")
        code = q()
        recs = json.load(open(os.path.join(mdir, "rights.json")))
        ck("S3 a second run keeps the agent fields and writes rights.prev.json",
           code == 0 and recs[0]["rights_confirmed"] is True and recs[0]["agent_notes"]
           and os.path.isfile(os.path.join(mdir, "rights.prev.json")))
        rep = fx.put_json(os.path.join(d, "out", "qa", "mix_report.json"),
                          {"files_used": ["audio/assets/ui/fs-123456_tap.wav", "audio/assets/ui/stray.wav"]})
        code = q("--report", "out/qa/mix_report.json")
        recs = json.load(open(os.path.join(mdir, "rights.json")))
        ck("S4 a used file in no manifest is a blocker row with id null",
           code == 1 and any(r["id"] is None and r["severity"] == "blocker" for r in recs))
        other = dict(entry, id="fs-999", file="audio/assets/ui/other.wav", rights="CC BY-NC 4.0")
        fx.write(os.path.join(d, other["file"]), fx.burst(0.2))
        fx.put_json(os.path.join(mdir, "ui.json"), [entry, other])
        fx.put_json(rep, {"files_used": ["audio/assets/ui/fs-123456_tap.wav"]})
        code = q("--report", "out/qa/mix_report.json")
        recs = json.load(open(os.path.join(mdir, "rights.json")))
        ck("S4 an entry outside files_used is out of scope and does not block",
           code == 0 and [r["in_scope"] for r in recs] == [True, False])
        dup = dict(entry, id="fs-123456b", rights="CC BY-NC 4.0")
        fx.put_json(os.path.join(mdir, "zz.json"), [dup])
        q()
        recs = json.load(open(os.path.join(mdir, "rights.json")))
        first = next(r for r in recs if r["id"] == "fs-123456")
        d2 = next(r for r in recs if r["id"] == "fs-123456b")
        ck("S5 a duplicate under NC: a problem on the first, info row with duplicate_of",
           any("one recording has one licence" in p for p in first["problems"])
           and d2["severity"] == "info" and d2["duplicate_of"] == "fs-123456")
        os.remove(os.path.join(mdir, "zz.json"))
        bad = dict(entry, page_url="https://freesound.org/people/alice/sounds/123457/",
                   download_url="https://cdn.freesound.org/previews/123/123456_9-lq.ogg")
        w = url_warnings(bad)
        ck("S6 Freesound id, page/preview and lq warnings; folder 123 is right",
           any("does not match the Freesound page" in x for x in w)
           and any("preview URL is sound 123456" in x for x in w)
           and any("-lq" in x for x in w) and not any("folder" in x for x in w))
        ck("Mixkit id check", url_warnings({"id": "mk-5", "download_url":
                                            "https://assets.mixkit.co/active_storage/sfx/7/7.wav"}) != [])
        empty = os.path.join(d, "empty")
        os.makedirs(os.path.join(empty, "audio", "assets", "manifests"))
        ck("exit 2 without entries", main(["--root", empty]) == 2)
        noted = dict(entry, notes="Licence read on the item page on 2026-11-06. Close and dry.")
        rec = check_entry(noted, d, False, {})
        ck("a notes sentence about a reading is quoted", any("Licence read on the item page" in p
                                                             for p in rec["problems"]))
        sil = fx.write(os.path.join(d, "audio", "assets", "ui", "silent.wav"), fx.burst(0.3) * 0)
        rec = check_entry(dict(entry, file="audio/assets/ui/silent.wav"), d, True, {})
        ck("digital silence is a problem", any("digital silence" in p for p in rec["problems"]))
        del sil, wav
    return ck.code()


if __name__ == "__main__":
    sys.exit(main())
