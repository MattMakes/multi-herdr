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

def _ffprobe_json(path: str):
    """(parsed ffprobe JSON, None) or (None, error text)."""
    cmd = ["ffprobe", "-v", "error", "-show_streams", "-show_format", "-of", "json", path]
    try:
        out = subprocess.run(cmd, capture_output=True, text=True, timeout=120)
    except (OSError, subprocess.TimeoutExpired) as e:
        return None, str(e)
    if out.returncode != 0:
        return None, (out.stderr.strip().splitlines() or ["ffprobe failed"])[-1]
    return json.loads(out.stdout or "{}"), None


def _kbps(br):
    try:
        br = float(br)
    except (TypeError, ValueError):
        return None
    return round(br / 1000.0, 1) if 1 <= br <= 10_000_000 else None


def _has_video(streams: list) -> bool:
    return any(s.get("codec_type") == "video" and not (s.get("disposition") or {}).get("attached_pic")
               for s in streams)


def _probe_facts(audio: dict, fmt: dict, streams: list) -> dict:
    dur = audio.get("duration") or fmt.get("duration")
    return {"codec": audio.get("codec_name"), "sample_rate": int(audio.get("sample_rate") or 0),
            "channels": int(audio.get("channels") or 0), "kbps": _kbps(audio.get("bit_rate") or fmt.get("bit_rate")),
            "duration_s": round(float(dur), 3) if dur not in (None, "N/A") else None, "video": _has_video(streams)}


def probe(path: str) -> dict:
    """ffprobe facts of a file: the first audio stream, and any real video stream."""
    data, err = _ffprobe_json(path)
    if err is not None:
        return {"probe_error": err}
    streams, fmt = data.get("streams", []), data.get("format", {})
    audio = next((s for s in streams if s.get("codec_type") == "audio"), None)
    if audio is None:
        return {"probe_error": "no audio stream"}
    return _probe_facts(audio, fmt, streams)


def _decode_errors(err: str) -> list[str]:
    return [ln for ln in err.splitlines() if re.search(r"\b(error|fatal)\b", ln, re.I) and "volumedetect" not in ln]


def decode_check(path: str) -> dict:
    """One full decode of the first audio stream to null, with volume detection."""
    cmd = ["ffmpeg", "-hide_banner", "-nostdin", "-nostats", "-i", path,
           "-map", "0:a:0", "-af", "volumedetect", "-f", "null", "-"]
    try:
        out = subprocess.run(cmd, capture_output=True, text=True, timeout=600)
    except (OSError, subprocess.TimeoutExpired) as e:
        return {"decodes_clean": False, "decode_error": str(e)}
    err = out.stderr or ""
    bad = _decode_errors(err)
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


def _blocked_host(url: str):
    host = (urllib.request.urlparse(url).hostname or "").lower()
    return host if any(host == h or host.endswith("." + h) for h in SKIP_FETCH_HOSTS) else None


def _fetch(url: str):
    """(body, None) or (None, a note)."""
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            body = r.read(MAX_FETCH + 1)
    except Exception as e:  # network errors are notes, not crashes
        return None, f"fetch failed: {e}"[:200]
    if len(body) > MAX_FETCH:
        return None, "remote file is over 200 MB: not compared"
    return body, None


def fetch_compare(url: str, path: str) -> dict:
    """Download url into memory (never to disk, never run) and compare its SHA-1 with the local file."""
    host = _blocked_host(url)
    body, note = (None, f"{host} blocks scripted downloads: compare by hand") if host else _fetch(url)
    if note:
        return {"fetch_note": note}
    remote, local = hashlib.sha1(body).hexdigest(), sha1_file(path)
    return {"bytes_match_download_url": remote == local, "sha1_local": local, "sha1_remote": remote}


# ------------------------------------------------------------------ checks per entry

_FS_PAGE = re.compile(r"freesound\.org/people/([^/]+)/sounds/(\d+)", re.I)
_FS_PREVIEW = re.compile(r"previews/(\d+)/(\d+)_(\d+)-(hq|lq)\.(ogg|mp3)", re.I)
_MK = re.compile(r"assets\.mixkit\.co/active_storage/sfx/(\d+)/(\d+)\.(wav|mp3)", re.I)
_READ_NOTE = re.compile(r"[^.]*\b(?:rights?|licen[cs]e\w*)\b.{0,40}?\b(?:read|verified|checked|seen|confirmed)\b[^.]*",
                        re.I)


def _id_number(eid: str, prefix: str):
    n = re.match(prefix + r"-(\d+)$", eid)
    return int(n.group(1)) if n else None


def _preview_warnings(dl: str, sid: int) -> list[str]:
    p = _FS_PREVIEW.search(dl)
    if not p:
        return []
    folder, pid = int(p.group(1)), int(p.group(2))
    checks = [(pid != sid, f"the preview URL is sound {pid}, the page is sound {sid}"),
              (folder != pid // 1000, f"the preview folder {folder} is not {pid} // 1000 = {pid // 1000}"),
              (p.group(4).lower() == "lq", "the download is the -lq preview: use -hq")]
    return [msg for bad, msg in checks if bad]


def _freesound_warnings(e: dict, eid: str, dl: str) -> list[str]:
    m = _FS_PAGE.search(e.get("page_url") or "")
    if not m:
        return []
    user, sid = m.group(1), int(m.group(2))
    num = _id_number(eid, "fs")
    out = [f"id {eid} does not match the Freesound page sound {sid}"] if num is not None and num != sid else []
    out += _preview_warnings(dl, sid)
    if e.get("author") and str(e["author"]).lower() != user.lower():
        out.append(f"author {e['author']!r} is not the page user {user!r}")
    return out


def _mixkit_warnings(eid: str, dl: str) -> list[str]:
    m = _MK.search(dl)
    if not m:
        return []
    a, b = int(m.group(1)), int(m.group(2))
    num = _id_number(eid, "mk")
    checks = [(a != b, f"the Mixkit URL folder {a} and file {b} differ"),
              (num is not None and num != a, f"id {eid} does not match the Mixkit item {a}")]
    return [msg for bad, msg in checks if bad]


def url_warnings(e: dict) -> list[str]:
    eid = str(e.get("id") or "")
    dl = e.get("download_url") or ""
    return _freesound_warnings(e, eid, dl) + _mixkit_warnings(eid, dl)


def _field_findings(e: dict, cls: dict) -> tuple[list, list]:
    problems = [msg for f, msg in (("file", "no file in the manifest entry"), ("rights", "no rights in the manifest entry"))
                if not e.get(f)]
    warnings = [f"no {f}" for f in ("id", "title", "author", "page_url") if not e.get(f)]
    if cls["status"] != "allowed" and e.get("rights"):
        problems.append(f"rights {cls['key']} ({cls['status']}): {cls['reason']}")
    return problems, warnings


def _reading_problems(e: dict, seen, confirmed: bool) -> list[str]:
    if seen:
        return [] if confirmed else ["rights_seen is recorded but rights_confirmed is not true"]
    m = _READ_NOTE.search(e.get("notes") or "")
    note = (f"no page reading in rights_seen, but the notes say: \"{m.group(0).strip()[:90]}\"; "
            "copy that reading into rights_seen") if m else None
    return [note or "no page reading: read the item page and fill in rights_seen"]


def _seen_text(seen) -> str:
    if isinstance(seen, dict):
        joined = " ".join(str(seen.get(k, "")) for k in ("rights", "licence", "license", "text"))
        return joined or json.dumps(seen)
    return seen if isinstance(seen, str) else json.dumps(seen)


def _seen_warnings(seen, cls: dict) -> list[str]:
    if not seen:
        return []
    sc = rights.classify(_seen_text(seen))
    if sc["status"] != "unknown" and sc["key"] not in ("none", "unknown") and sc["key"] != cls["key"]:
        return [f"rights_seen reads as {sc['key']}, the manifest as {cls['key']}"]
    return []


def _attribution_warnings(e: dict, attr: str) -> list[str]:
    if not attr:
        return []
    low = attr.lower()
    checks = [(e.get("author") and str(e["author"]).lower() not in attr.split(" (")[0].lower(),
               "the attribution does not name the author"),
              (not ("cc by" in low or "attribution" in low or "creativecommons.org" in low),
               "the attribution names no licence")]
    return [msg for bad, msg in checks if bad]


def _attribution(e: dict, cls: dict) -> tuple:
    """(attribution ok or None when not required, problems, warnings)."""
    if not cls["attribution"]:
        return None, [], []
    missing = [f for f in ("attribution", "author", "page_url") if not e.get(f)]
    problems = ["CC BY credit incomplete: no " + ", ".join(missing)] if missing else []
    return not missing, problems, _attribution_warnings(e, e.get("attribution") or "")


def _probe_warnings(pr: dict, e: dict) -> list[str]:
    want = (e.get("analysis") or {}).get("duration")
    replaced = want is not None and pr["duration_s"] is not None and abs(float(want) - pr["duration_s"]) > 0.05
    checks = [(pr["video"], "the file carries a video stream"),
              (0 < (pr["sample_rate"] or 0) < 44100, f"sample rate {pr['sample_rate']} Hz is under 44100"),
              (pr["codec"] == "mp3" and 0 < (pr["kbps"] or 0) < 192, f"mp3 at {pr['kbps']} kbps is under 192"),
              (replaced, f"duration {pr['duration_s']} s differs from the manifest analysis {want} s "
                         "(file replaced?)")]
    return [msg for bad, msg in checks if bad]


def _decode_findings(ap: str, fc: dict) -> tuple[list, list]:
    dc = decode_check(ap)
    fc["decodes_clean"] = dc["decodes_clean"]
    if "max_volume_db" in dc:
        fc["max_volume_db"] = dc["max_volume_db"]
    problems = [] if dc["decodes_clean"] else ["decode errors: " + dc.get("decode_error", "ffmpeg failed")]
    mv = dc.get("max_volume_db")
    if mv is not None and mv <= -90:
        problems.append("digital silence (max volume <= -90 dB)")
    warnings = [f"very quiet file (max volume {mv} dB)"] if mv is not None and -90 < mv < -60 else []
    return problems, warnings


def _file_findings(e: dict, root: str, decode: bool) -> tuple[dict, list, list]:
    """(file_check, problems, warnings) of the entry's file."""
    path = rights.rel(e.get("file") or "", root)
    ap = os.path.join(root, path)
    fc = {"path": path, "exists": os.path.isfile(ap)}
    if not fc["exists"]:
        return fc, [f"file missing: {path}"], []
    pr = probe(ap)
    if "probe_error" in pr:
        return fc, [f"ffprobe failed: {pr['probe_error']}"], []
    fc.update(pr)
    problems, warnings = _decode_findings(ap, fc) if decode else ([], [])
    return fc, problems, _probe_warnings(pr, e) + warnings


def check_entry(e: dict, root: str, decode: bool, earlier: dict | None) -> dict:
    cls = rights.classify(e.get("rights"))
    agent = {k: (earlier or {}).get(k) for k in AGENT_FIELDS}
    seen = agent.get("rights_seen")
    confirmed = bool(agent.get("rights_confirmed")) and bool(seen)
    problems, warnings = _field_findings(e, cls)
    problems += _reading_problems(e, seen, confirmed)
    warnings += _seen_warnings(seen, cls)
    attr_ok, ap, aw = _attribution(e, cls)
    problems += ap
    warnings += aw + url_warnings(e)
    fc, fp, fw = _file_findings(e, root, decode)
    problems += fp
    warnings += fw
    file_ok = fc["exists"] and fc.get("decodes_clean", True) is not False and "codec" in fc
    return {"id": e.get("id"), "manifest": e.get("_manifest"), "need": e.get("need"), "rank": e.get("rank"),
            "in_scope": True, "severity": "ok", "rights_in_manifest": e.get("rights"), "rights": cls["key"],
            "rights_status": cls["status"], "rights_seen": seen, "rights_confirmed": confirmed,
            "attribution_required": bool(cls["attribution"]), "attribution_ok": attr_ok,
            "attribution_in_manifest": e.get("attribution"), "file_ok": bool(file_ok), "file_check": fc,
            "problems": problems, "warnings": warnings, "agent_notes": agent.get("agent_notes"),
            "audit_tool": TOOL}


_KNOWN = set(AGENT_FIELDS) | {"id", "manifest", "need", "rank", "in_scope", "severity", "rights_in_manifest",
                              "rights", "rights_status", "attribution_required", "attribution_ok",
                              "attribution_in_manifest", "file_ok", "file_check", "problems", "warnings",
                              "problem", "fix", "earlier_file_check", "audit_tool", "duplicate_of", "file"}


def _as_list(v) -> list:
    if isinstance(v, list):
        return list(v)
    return [v] if v else []


def _hand_notes(old: dict, notes) -> list | None:
    """A hand-written record's notes, warnings, problem and fix as 1 notes list."""
    out = _as_list(notes)
    for k in ("warnings", "problem", "fix"):
        v = old.get(k)
        out += v if isinstance(v, list) else ([f"{k}: {v}"] if v else [])
    return out or None


def _byte_fields(fc: dict) -> dict:
    return {k: fc[k] for k in BYTE_FIELDS if k in fc}


def _hand_file_fields(fc: dict) -> dict:
    return {k: v for k, v in fc.items() if k not in TOOL_FILE_FIELDS}


def _unknown_keys(old: dict) -> dict:
    return {k: v for k, v in old.items() if k not in _KNOWN}


def carry(old: dict | None) -> tuple[dict, dict, dict]:
    """From an earlier record: (agent fields, earlier_file_check, extra keys kept)."""
    if not old:
        return {}, {}, {}
    agent = {k: old.get(k) for k in AGENT_FIELDS}
    efc = dict(old.get("earlier_file_check") or {})
    fc = old.get("file_check") or {}
    if old.get("audit_tool"):
        efc.update(_byte_fields(fc))
        return agent, efc, {}
    agent["agent_notes"] = _hand_notes(old, agent.get("agent_notes"))
    efc.update(_hand_file_fields(fc))
    return agent, efc, _unknown_keys(old)


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


class AuditError(Exception):
    """An input error: exit 2."""


def _inputs(a, root: str) -> dict:
    """Paths of the run: mdir, out, existing, report. Raises AuditError on a missing input."""
    mdir = os.path.join(root, a.manifests)
    if not os.path.isdir(mdir):
        raise AuditError(f"manifest folder not found: {mdir}")
    out = os.path.join(root, a.out) if a.out else os.path.join(mdir, "rights.json")
    report = os.path.join(root, a.report) if a.report else None
    if report and not os.path.isfile(report):
        raise AuditError(f"mix report not found: {report}")
    return {"mdir": mdir, "out": out, "existing": os.path.join(root, a.existing) if a.existing else out,
            "report": report}


def _earlier_index(earlier: list, root: str) -> dict:
    by_key = {}
    for r in (r for r in earlier if isinstance(r, dict)):
        path = rights.rel((r.get("file_check") or {}).get("path") or r.get("file") or "", root).lower()
        by_key.setdefault((r.get("id"), path), r)
        by_key.setdefault((r.get("id"), ""), r)
    return by_key


def _duplicate(e: dict, root: str, agent: dict, first: dict) -> dict:
    """The info row of a file listed again, and its note on the first record."""
    cls = rights.classify(e.get("rights"))
    rec = {"id": e.get("id"), "manifest": e.get("_manifest"), "need": e.get("need"), "rank": e.get("rank"),
           "in_scope": False, "rights_in_manifest": e.get("rights"), "rights": cls["key"],
           "rights_status": cls["status"],
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
    return rec


def _in_scope(path: str, used, old) -> bool:
    if used is not None:
        return path in used
    return old.get("in_scope", True) if old else True


def _record(a, root: str, e: dict, old, used) -> dict:
    agent, efc, extra = carry(old)
    in_scope = _in_scope(rights.rel(e["file"], root).lower(), used, old)
    do_decode = a.decode == "all" or (a.decode == "scope" and in_scope)
    rec = check_entry(e, root, do_decode, agent)
    rec["in_scope"] = bool(in_scope)
    if efc:
        rec["earlier_file_check"] = efc
    rec.update(extra)
    return rec


def _records(a, root: str, entries: list, by_key: dict, used) -> tuple[list, dict]:
    """(records, first record by lower-case path)."""
    records, seen_files = [], {}
    for e in entries:
        path = rights.rel(e["file"], root).lower()
        old = by_key.get((e.get("id"), path)) or by_key.get((e.get("id"), ""))
        if path in seen_files:
            records.append(_duplicate(e, root, carry(old)[0], seen_files[path]))
            continue
        rec = _record(a, root, e, old, used)
        seen_files[path] = rec
        records.append(rec)
    return records, seen_files


def _fetch_targets(a, root: str, records: list, entries: list) -> list:
    """(record, url, local path) of every record to compare with its download_url."""
    out = []
    for rec, e in zip(records, entries):
        wanted = not rec.get("duplicate_of") and (a.fetch_all or rec["in_scope"])
        url, ap = e.get("download_url"), os.path.join(root, rec["file_check"]["path"])
        if wanted and url and os.path.isfile(ap):
            out.append((rec, url, ap))
    return out


def _fetch_all(targets: list) -> None:
    for i, (rec, url, ap) in enumerate(targets):
        if i:
            time.sleep(1.5)
        res = fetch_compare(url, ap)
        rec["file_check"].update(res)
        if res.get("bytes_match_download_url") is False:
            rec["warnings"].append("the local bytes differ from download_url (re-encoded or edited file)")


def _unlisted(used: set, listed: set) -> list:
    return [{"id": None, "in_scope": True, "file_check": {"path": f, "exists": None},
             "rights": None, "rights_status": "unknown", "rights_confirmed": False,
             "problems": ["used in the film but in no manifest"], "warnings": [], "audit_tool": TOOL}
            for f in sorted(used - listed)]


def _byte_counts(records: list) -> tuple[int, int, int]:
    """(records with auditor notes, byte matches this run, byte matches from earlier audits)."""
    with_notes = sum(1 for r in records if r.get("agent_notes"))
    now = sum(1 for r in records if (r.get("file_check") or {}).get("bytes_match_download_url") is True)
    before = sum(1 for r in records if (r.get("earlier_file_check") or {}).get("bytes_match_download_url"))
    return with_notes, now, before


def _rights_counts(scope: list) -> str:
    keys = Counter(r.get("rights") for r in scope)
    return ", ".join(f"{k} x{n}" for k, n in keys.most_common()) or "none"


def _summary(entries: list, records: list) -> tuple[str, Counter]:
    scope = [r for r in records if r["in_scope"]]
    sev = Counter(r["severity"] for r in scope)
    with_notes, matched_now, matched_before = _byte_counts(records)
    summary = (f"{len(entries)} entries, {len(scope)} in scope: {sev['ok']} ok, {sev['warning']} warning, "
               f"{sev['blocker']} blocker; rights in scope: " + _rights_counts(scope)
               + f"; {with_notes} record(s) with auditor notes; byte match {matched_now} this run, "
               f"{matched_before} from earlier audits")
    if sev["blocker"]:
        summary += "; fix every blocker before the film ships"
    return summary, sev


def _stem(out: str) -> str:
    return out[:-5] if out.endswith(".json") else out


def _hand_written(out: str) -> bool:
    """True when the audit file was not written by this tool, or is not valid JSON."""
    old_list, _ = load_earlier(out)
    try:
        with open(out, encoding="utf-8") as fh:
            json.load(fh)
    except ValueError:
        return True
    return not any(isinstance(r, dict) and r.get("audit_tool") for r in old_list)


def _backups(out: str) -> list[str]:
    """Keep rights.prev.json, and rights.handwritten.json once for a hand-written file."""
    if not os.path.isfile(out):
        return []
    prev, hw = _stem(out) + ".prev.json", _stem(out) + ".handwritten.json"
    shutil.copyfile(out, prev)
    if _hand_written(out) and not os.path.exists(hw):
        shutil.copyfile(out, hw)
        return [prev, hw]
    return [prev]


def _write_audit(out: str, table: str, records: list, summary: str) -> list[str]:
    os.makedirs(os.path.dirname(out), exist_ok=True)
    with open(out, "w", encoding="utf-8") as fh:
        json.dump(records, fh, indent=1, ensure_ascii=False)
        fh.write("\n")
    with open(table, "w", encoding="utf-8") as fh:
        fh.write(markdown(records, summary))
    return [out, table]


def _audit(a, root: str) -> tuple[int, dict]:
    io_ = _inputs(a, root)
    entries, notes = rights.load_manifests(io_["mdir"])
    if not entries:
        raise AuditError(f"no manifest entries in {io_['mdir']}")
    used = load_used(io_["report"], root)
    earlier, _ = load_earlier(io_["existing"])
    records, seen_files = _records(a, root, entries, _earlier_index(earlier, root), used)
    if a.fetch or a.fetch_all:
        _fetch_all(_fetch_targets(a, root, records, entries))
    records += _unlisted(used, set(seen_files)) if used is not None else []
    for rec in records:
        rec["severity"] = severity(rec)
    summary, sev = _summary(entries, records)
    out = io_["out"]
    backups = _backups(out)
    table = os.path.join(root, a.table) if a.table else _stem(out) + ".md"
    written = _write_audit(out, table, records, summary)
    return (1 if sev["blocker"] else 0), {"summary": summary, "notes": notes, "records": records,
                                          "written": written, "backups": backups}


def audit(a) -> tuple[int, dict]:
    try:
        return _audit(a, os.path.abspath(a.root))
    except AuditError as e:
        return 2, {"error": str(e)}


def _file_cell(fc: dict) -> str:
    if fc.get("codec"):
        sr = (fc.get("sample_rate") or 0) / 1000.0
        return f"{fc['codec']} {sr:g}k {fc.get('channels')}ch {fc.get('duration_s')}s"
    return "MISSING" if not fc.get("exists") else "?"


def _credit_cell(r: dict) -> str:
    if not r.get("attribution_required"):
        return "n/a"
    return "ok" if r.get("attribution_ok") else "MISSING"


def _row(r: dict) -> str:
    issues = "; ".join(r.get("problems", []) + r.get("warnings", []))[:300].replace("|", "/")
    return (f"| {r.get('id')} | {'yes' if r['in_scope'] else 'no'} | {r['severity']} | {r.get('rights')} | "
            f"{'yes' if r.get('rights_confirmed') else 'NO'} | {_credit_cell(r)} | "
            f"{_file_cell(r.get('file_check') or {})} | {issues} |")


def markdown(records: list, summary: str) -> str:
    order = {"blocker": 0, "warning": 1, "info": 2, "ok": 3}
    rows = sorted(records, key=lambda r: (not r["in_scope"], order.get(r["severity"], 9), str(r.get("id"))))
    lines = ["# Usage-rights audit", "", summary, "",
             "| id | in film | severity | rights | verified | credit | file | issues |",
             "|---|---|---|---|---|---|---|---|"]
    return "\n".join(lines + [_row(r) for r in rows]) + "\n"


def _issue_groups(records: list) -> tuple[Counter, dict]:
    """Count of each (level, message) over in-scope records that are not ok, and the ids per group."""
    groups, ids = Counter(), {}
    for r in (r for r in records if r["in_scope"] and r["severity"] != "ok"):
        for level, msgs in (("BLOCKER", r["problems"]), ("warning", r["warnings"])):
            for m in msgs:
                groups[(level, m)] += 1
                ids.setdefault((level, m), []).append(str(r.get("id")))
    return groups, ids


def print_result(res: dict):
    say(res["summary"])
    for n in res["notes"]:
        say("  note:", n)
    groups, ids = _issue_groups(res["records"])
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


ENTRY = {"id": "fs-123456", "category": "ui", "file": "audio/assets/ui/fs-123456_tap.wav",
         "title": "Tap", "author": "alice", "source": "freesound",
         "page_url": "https://freesound.org/people/alice/sounds/123456/",
         "download_url": "https://cdn.freesound.org/previews/123/123456_9-hq.ogg",
         "rights": "CC BY 4.0", "rights_url": "https://creativecommons.org/licenses/by/4.0/",
         "attribution": "\"Tap\" by alice, licensed under CC BY 4.0", "need": "click", "rank": 1}


def _records_of(mdir: str) -> list:
    with open(os.path.join(mdir, "rights.json"), encoding="utf-8") as fh:
        return json.load(fh)


def _test_reading(ck, fx, d: str, mdir: str) -> None:
    code = main(["--root", d])
    recs = _records_of(mdir)
    ck("S2 no page reading is a blocker, exit 1, files written",
       code == 1 and recs[0]["severity"] == "blocker"
       and any("no page reading" in p for p in recs[0]["problems"])
       and os.path.isfile(os.path.join(mdir, "rights.md")))
    recs[0]["rights_seen"] = {"site": "freesound", "rights": "Attribution 4.0", "read": "2026-11-07"}
    recs[0]["rights_confirmed"] = True
    recs[0]["agent_notes"] = "author asks for the credit form in the manifest"
    fx.put_json(os.path.join(mdir, "rights.json"), recs)
    code = main(["--root", d])
    ck("S3 a confirmed reading clears the entry", code == 0 and _records_of(mdir)[0]["severity"] == "ok")
    code = main(["--root", d])
    recs = _records_of(mdir)
    ck("S3 a second run keeps the agent fields and writes rights.prev.json",
       code == 0 and recs[0]["rights_confirmed"] is True and bool(recs[0]["agent_notes"])
       and os.path.isfile(os.path.join(mdir, "rights.prev.json")))


def _test_scope(ck, fx, d: str, mdir: str) -> None:
    rep = fx.put_json(os.path.join(d, "out", "qa", "mix_report.json"),
                      {"files_used": ["audio/assets/ui/fs-123456_tap.wav", "audio/assets/ui/stray.wav"]})
    code = main(["--root", d, "--report", "out/qa/mix_report.json"])
    ck("S4 a used file in no manifest is a blocker row with id null",
       code == 1 and any(r["id"] is None and r["severity"] == "blocker" for r in _records_of(mdir)))
    other = dict(ENTRY, id="fs-999", file="audio/assets/ui/other.wav", rights="CC BY-NC 4.0")
    fx.write(os.path.join(d, other["file"]), fx.burst(0.2))
    fx.put_json(os.path.join(mdir, "ui.json"), [ENTRY, other])
    fx.put_json(rep, {"files_used": ["audio/assets/ui/fs-123456_tap.wav"]})
    code = main(["--root", d, "--report", "out/qa/mix_report.json"])
    ck("S4 an entry outside files_used is out of scope and does not block",
       code == 0 and [r["in_scope"] for r in _records_of(mdir)] == [True, False])


def _test_duplicate(ck, fx, d: str, mdir: str) -> None:
    fx.put_json(os.path.join(mdir, "zz.json"), [dict(ENTRY, id="fs-123456b", rights="CC BY-NC 4.0")])
    main(["--root", d])
    recs = _records_of(mdir)
    first = next(r for r in recs if r["id"] == "fs-123456")
    d2 = next(r for r in recs if r["id"] == "fs-123456b")
    ck("S5 a duplicate under NC: a problem on the first, info row with duplicate_of",
       any("one recording has one licence" in p for p in first["problems"])
       and d2["severity"] == "info" and d2["duplicate_of"] == "fs-123456")
    os.remove(os.path.join(mdir, "zz.json"))


def _test_urls(ck) -> None:
    bad = dict(ENTRY, page_url="https://freesound.org/people/alice/sounds/123457/",
               download_url="https://cdn.freesound.org/previews/123/123456_9-lq.ogg")
    w = " | ".join(url_warnings(bad))
    ck("S6 Freesound id, page/preview and lq warnings; folder 123 is right",
       "does not match the Freesound page" in w and "preview URL is sound 123456" in w
       and "-lq" in w and "folder" not in w)
    ck("Mixkit id check", url_warnings({"id": "mk-5", "download_url":
                                        "https://assets.mixkit.co/active_storage/sfx/7/7.wav"}) != [])


def _test_entries(ck, fx, d: str) -> None:
    empty = os.path.join(d, "empty")
    os.makedirs(os.path.join(empty, "audio", "assets", "manifests"))
    ck("exit 2 without entries", main(["--root", empty]) == 2)
    rec = check_entry(dict(ENTRY, notes="Licence read on the item page on 2026-11-06. Close and dry."), d, False, {})
    ck("a notes sentence about a reading is quoted", any("Licence read on the item page" in p
                                                         for p in rec["problems"]))
    fx.write(os.path.join(d, "audio", "assets", "ui", "silent.wav"), fx.burst(0.3) * 0)
    rec = check_entry(dict(ENTRY, file="audio/assets/ui/silent.wav"), d, True, {})
    ck("digital silence is a problem", any("digital silence" in p for p in rec["problems"]))


def self_test() -> int:
    import tempfile
    import fixtures as fx
    ck = fx.Checker()
    with tempfile.TemporaryDirectory() as d:
        mdir = os.path.join(d, "audio", "assets", "manifests")
        fx.write(os.path.join(d, "audio", "assets", "ui", "fs-123456_tap.wav"), fx.burst(0.3))
        fx.put_json(os.path.join(mdir, "ui.json"), [ENTRY])
        _test_reading(ck, fx, d, mdir)
        _test_scope(ck, fx, d, mdir)
        _test_duplicate(ck, fx, d, mdir)
        _test_urls(ck)
        _test_entries(ck, fx, d)
    return ck.code()


if __name__ == "__main__":
    sys.exit(main())
