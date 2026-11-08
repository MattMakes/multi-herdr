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
    text = " ".join(str(pv) for pv in parts)
    print(text.encode("ascii", "replace").decode("ascii"), file=file or sys.stdout)


# ------------------------------------------------------------------ file checks

def _ffprobe_json(fpath: str):
    """(parsed ffprobe JSON, None) or (None, error text)."""
    cmd = ["ffprobe", "-v", "error", "-show_streams", "-show_format", "-of", "json", fpath]
    try:
        acc = subprocess.run(cmd, capture_output=True, text=True, timeout=120)
    except (OSError, subprocess.TimeoutExpired) as ent:
        return None, str(ent)
    if acc.returncode != 0:
        return None, (acc.stderr.strip().splitlines() or ["ffprobe failed"])[-1]
    return json.loads(acc.stdout or "{}"), None


def _kbps(br):
    try:
        br = float(br)
    except (TypeError, ValueError):
        return None
    return round(br / 1000.0, 1) if 1 <= br <= 10_000_000 else None


def _has_video(streams: list) -> bool:
    return any(sv.get("codec_type") == "video" and not (sv.get("disposition") or {}).get("attached_pic")
               for sv in streams)


def _probe_facts(audio: dict, fmt: dict, streams: list) -> dict:
    dur = audio.get("duration") or fmt.get("duration")
    return {"codec": audio.get("codec_name"), "sample_rate": int(audio.get("sample_rate") or 0),
            "channels": int(audio.get("channels") or 0), "kbps": _kbps(audio.get("bit_rate") or fmt.get("bit_rate")),
            "duration_s": round(float(dur), 3) if dur not in (None, "N/A") else None, "video": _has_video(streams)}


def probe(fpath: str) -> dict:
    """ffprobe facts of a file: the first audio stream, and any real video stream."""
    data, err = _ffprobe_json(fpath)
    if err is not None:
        return {"probe_error": err}
    streams, fmt = data.get("streams", []), data.get("format", {})
    audio = next((sv for sv in streams if sv.get("codec_type") == "audio"), None)
    if audio is None:
        return {"probe_error": "no audio stream"}
    return _probe_facts(audio, fmt, streams)


def _decode_errors(err: str) -> list[str]:
    return [ln for ln in err.splitlines() if re.search(r"\b(error|fatal)\b", ln, re.I) and "volumedetect" not in ln]


def decode_check(fpath: str) -> dict:
    """One full decode of the first audio stream to null, with volume detection."""
    cmd = ["ffmpeg", "-hide_banner", "-nostdin", "-nostats", "-i", fpath,
           "-map", "0:a:0", "-af", "volumedetect", "-f", "null", "-"]
    try:
        acc = subprocess.run(cmd, capture_output=True, text=True, timeout=600)
    except (OSError, subprocess.TimeoutExpired) as ent:
        return {"decodes_clean": False, "decode_error": str(ent)}
    err = acc.stderr or ""
    bad = _decode_errors(err)
    vols = re.findall(r"max_volume:\s*(-?[0-9.]+|-inf) dB", err)
    result_ = {"decodes_clean": acc.returncode == 0 and not bad}
    if vols:
        result_["max_volume_db"] = -200.0 if vols[-1] == "-inf" else float(vols[-1])
    if bad:
        result_["decode_error"] = bad[0][:200]
    return result_


def sha1_file(fpath: str) -> str:
    hasher = hashlib.sha1()
    with open(fpath, "rb") as fh:
        for block in iter(lambda: fh.read(1 << 20), b""):
            hasher.update(block)
    return hasher.hexdigest()


def _blocked_host(url: str):
    host = (urllib.request.urlparse(url).hostname or "").lower()
    return host if any(host == hasher or host.endswith("." + hasher) for hasher in SKIP_FETCH_HOSTS) else None


def _fetch(url: str):
    """(body, None) or (None, a note)."""
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(req, timeout=60) as row_:
            body = row_.read(MAX_FETCH + 1)
    except Exception as ent:  # network errors are notes, not crashes
        return None, f"fetch failed: {ent}"[:200]
    if len(body) > MAX_FETCH:
        return None, "remote file is over 200 MB: not compared"
    return body, None


def fetch_compare(url: str, fpath: str) -> dict:
    """Download url into memory (never to disk, never run) and compare its SHA-1 with the local file."""
    host = _blocked_host(url)
    body, note = (None, f"{host} blocks scripted downloads: compare by hand") if host else _fetch(url)
    if note:
        return {"fetch_note": note}
    remote, local = hashlib.sha1(body).hexdigest(), sha1_file(fpath)
    return {"bytes_match_download_url": remote == local, "sha1_local": local, "sha1_remote": remote}


# ------------------------------------------------------------------ checks per entry

_FS_PAGE = re.compile(r"freesound\.org/people/([^/]+)/sounds/(\d+)", re.I)
_FS_PREVIEW = re.compile(r"previews/(\d+)/(\d+)_(\d+)-(hq|lq)\.(ogg|mp3)", re.I)
_MK = re.compile(r"assets\.mixkit\.co/active_storage/sfx/(\d+)/(\d+)\.(wav|mp3)", re.I)
_READ_NOTE = re.compile(r"[^.]*\b(?:rights?|licen[cs]e\w*)\b.{0,40}?\b(?:read|verified|checked|seen|confirmed)\b[^.]*",
                        re.I)


def _id_number(eid: str, prefix: str):
    num = re.match(prefix + r"-(\d+)$", eid)
    return int(num.group(1)) if num else None


def _preview_warnings(dl: str, sid: int) -> list[str]:
    pv = _FS_PREVIEW.search(dl)
    if not pv:
        return []
    folder, pid = int(pv.group(1)), int(pv.group(2))
    checks = [(pid != sid, f"the preview URL is sound {pid}, the page is sound {sid}"),
              (folder != pid // 1000, f"the preview folder {folder} is not {pid} // 1000 = {pid // 1000}"),
              (pv.group(4).lower() == "lq", "the download is the -lq preview: use -hq")]
    return [msg for bad, msg in checks if bad]


def _freesound_warnings(ent: dict, eid: str, dl: str) -> list[str]:
    hit = _FS_PAGE.search(ent.get("page_url") or "")
    if not hit:
        return []
    user, sid = hit.group(1), int(hit.group(2))
    num = _id_number(eid, "fs")
    acc = [f"id {eid} does not match the Freesound page sound {sid}"] if num is not None and num != sid else []
    acc += _preview_warnings(dl, sid)
    if ent.get("author") and str(ent["author"]).lower() != user.lower():
        acc.append(f"author {ent['author']!r} is not the page user {user!r}")
    return acc


def _mixkit_warnings(eid: str, dl: str) -> list[str]:
    hit = _MK.search(dl)
    if not hit:
        return []
    cli, bv = int(hit.group(1)), int(hit.group(2))
    num = _id_number(eid, "mk")
    checks = [(cli != bv, f"the Mixkit URL folder {cli} and file {bv} differ"),
              (num is not None and num != cli, f"id {eid} does not match the Mixkit item {cli}")]
    return [msg for bad, msg in checks if bad]


def url_warnings(ent: dict) -> list[str]:
    eid = str(ent.get("id") or "")
    dl = ent.get("download_url") or ""
    return _freesound_warnings(ent, eid, dl) + _mixkit_warnings(eid, dl)


def _field_findings(ent: dict, cls: dict) -> tuple[list, list]:
    issues = [msg for fld, msg in (("file", "no file in the manifest entry"), ("rights", "no rights in the manifest entry"))
                if not ent.get(fld)]
    warnings = [f"no {fld}" for fld in ("id", "title", "author", "page_url") if not ent.get(fld)]
    if cls["status"] != "allowed" and ent.get("rights"):
        issues.append(f"rights {cls['key']} ({cls['status']}): {cls['reason']}")
    return issues, warnings


def _reading_problems(ent: dict, seen, confirmed: bool) -> list[str]:
    if seen:
        return [] if confirmed else ["rights_seen is recorded but rights_confirmed is not true"]
    hit = _READ_NOTE.search(ent.get("notes") or "")
    note = (f"no page reading in rights_seen, but the notes say: \"{hit.group(0).strip()[:90]}\"; "
            "copy that reading into rights_seen") if hit else None
    return [note or "no page reading: read the item page and fill in rights_seen"]


def _seen_text(seen) -> str:
    if isinstance(seen, dict):
        joined = " ".join(str(seen.get(key_, "")) for key_ in ("rights", "licence", "license", "text"))
        return joined or json.dumps(seen)
    return seen if isinstance(seen, str) else json.dumps(seen)


def _seen_warnings(seen, cls: dict) -> list[str]:
    if not seen:
        return []
    sc = rights.classify(_seen_text(seen))
    if sc["status"] != "unknown" and sc["key"] not in ("none", "unknown") and sc["key"] != cls["key"]:
        return [f"rights_seen reads as {sc['key']}, the manifest as {cls['key']}"]
    return []


def _attribution_warnings(ent: dict, attr: str) -> list[str]:
    if not attr:
        return []
    low = attr.lower()
    checks = [(ent.get("author") and str(ent["author"]).lower() not in attr.split(" (")[0].lower(),
               "the attribution does not name the author"),
              (not ("cc by" in low or "attribution" in low or "creativecommons.org" in low),
               "the attribution names no licence")]
    return [msg for bad, msg in checks if bad]


def _attribution(ent: dict, cls: dict) -> tuple:
    """(attribution ok or None when not required, problems, warnings)."""
    if not cls["attribution"]:
        return None, [], []
    missing = [fld for fld in ("attribution", "author", "page_url") if not ent.get(fld)]
    issues = ["CC BY credit incomplete: no " + ", ".join(missing)] if missing else []
    return not missing, issues, _attribution_warnings(ent, ent.get("attribution") or "")


def _probe_warnings(pr: dict, ent: dict) -> list[str]:
    want = (ent.get("analysis") or {}).get("duration")
    replaced = want is not None and pr["duration_s"] is not None and abs(float(want) - pr["duration_s"]) > 0.05
    checks = [(pr["video"], "the file carries a video stream"),
              (0 < (pr["sample_rate"] or 0) < 44100, f"sample rate {pr['sample_rate']} Hz is under 44100"),
              (pr["codec"] == "mp3" and 0 < (pr["kbps"] or 0) < 192, f"mp3 at {pr['kbps']} kbps is under 192"),
              (replaced, f"duration {pr['duration_s']} s differs from the manifest analysis {want} s "
                         "(file replaced?)")]
    return [msg for bad, msg in checks if bad]


def _decode_findings(apath: str, fchk: dict) -> tuple[list, list]:
    dc = decode_check(apath)
    fchk["decodes_clean"] = dc["decodes_clean"]
    if "max_volume_db" in dc:
        fchk["max_volume_db"] = dc["max_volume_db"]
    issues = [] if dc["decodes_clean"] else ["decode errors: " + dc.get("decode_error", "ffmpeg failed")]
    mv = dc.get("max_volume_db")
    if mv is not None and mv <= -90:
        issues.append("digital silence (max volume <= -90 dB)")
    warnings = [f"very quiet file (max volume {mv} dB)"] if mv is not None and -90 < mv < -60 else []
    return issues, warnings


def _file_findings(ent: dict, base_dir: str, decode: bool) -> tuple[dict, list, list]:
    """(file_check, problems, warnings) of the entry's file."""
    fpath = rights.rel(ent.get("file") or "", base_dir)
    apath = os.path.join(base_dir, fpath)
    fchk = {"path": fpath, "exists": os.path.isfile(apath)}
    if not fchk["exists"]:
        return fchk, [f"file missing: {fpath}"], []
    pr = probe(apath)
    if "probe_error" in pr:
        return fchk, [f"ffprobe failed: {pr['probe_error']}"], []
    fchk.update(pr)
    issues, warnings = _decode_findings(apath, fchk) if decode else ([], [])
    return fchk, issues, _probe_warnings(pr, ent) + warnings


def check_entry(ent: dict, base_dir: str, decode: bool, earlier: dict | None) -> dict:
    cls = rights.classify(ent.get("rights"))
    agent = {key_: (earlier or {}).get(key_) for key_ in AGENT_FIELDS}
    seen = agent.get("rights_seen")
    confirmed = bool(agent.get("rights_confirmed")) and bool(seen)
    issues, warnings = _field_findings(ent, cls)
    issues += _reading_problems(ent, seen, confirmed)
    warnings += _seen_warnings(seen, cls)
    attr_ok, apath, aw = _attribution(ent, cls)
    issues += apath
    warnings += aw + url_warnings(ent)
    fchk, fp, fw = _file_findings(ent, base_dir, decode)
    issues += fp
    warnings += fw
    file_ok = fchk["exists"] and fchk.get("decodes_clean", True) is not False and "codec" in fchk
    return {"id": ent.get("id"), "manifest": ent.get("_manifest"), "need": ent.get("need"), "rank": ent.get("rank"),
            "in_scope": True, "severity": "ok", "rights_in_manifest": ent.get("rights"), "rights": cls["key"],
            "rights_status": cls["status"], "rights_seen": seen, "rights_confirmed": confirmed,
            "attribution_required": bool(cls["attribution"]), "attribution_ok": attr_ok,
            "attribution_in_manifest": ent.get("attribution"), "file_ok": bool(file_ok), "file_check": fchk,
            "problems": issues, "warnings": warnings, "agent_notes": agent.get("agent_notes"),
            "audit_tool": TOOL}


_KNOWN = set(AGENT_FIELDS) | {"id", "manifest", "need", "rank", "in_scope", "severity", "rights_in_manifest",
                              "rights", "rights_status", "attribution_required", "attribution_ok",
                              "attribution_in_manifest", "file_ok", "file_check", "problems", "warnings",
                              "problem", "fix", "earlier_file_check", "audit_tool", "duplicate_of", "file"}


def _as_list(val) -> list:
    if isinstance(val, list):
        return list(val)
    return [val] if val else []


def _hand_notes(prior: dict, notes) -> list | None:
    """A hand-written record's notes, warnings, problem and fix as 1 notes list."""
    acc = _as_list(notes)
    for key_ in ("warnings", "problem", "fix"):
        val = prior.get(key_)
        acc += val if isinstance(val, list) else ([f"{key_}: {val}"] if val else [])
    return acc or None


def _byte_fields(fchk: dict) -> dict:
    return {key_: fchk[key_] for key_ in BYTE_FIELDS if key_ in fchk}


def _hand_file_fields(fchk: dict) -> dict:
    return {key_: val for key_, val in fchk.items() if key_ not in TOOL_FILE_FIELDS}


def _unknown_keys(prior: dict) -> dict:
    return {key_: val for key_, val in prior.items() if key_ not in _KNOWN}


def carry(prior: dict | None) -> tuple[dict, dict, dict]:
    """From an earlier record: (agent fields, earlier_file_check, extra keys kept)."""
    if not prior:
        return {}, {}, {}
    agent = {key_: prior.get(key_) for key_ in AGENT_FIELDS}
    efc = dict(prior.get("earlier_file_check") or {})
    fchk = prior.get("file_check") or {}
    if prior.get("audit_tool"):
        efc.update(_byte_fields(fchk))
        return agent, efc, {}
    agent["agent_notes"] = _hand_notes(prior, agent.get("agent_notes"))
    efc.update(_hand_file_fields(fchk))
    return agent, efc, _unknown_keys(prior)


def severity(record: dict) -> str:
    if record["in_scope"]:
        return "blocker" if record["problems"] else ("warning" if record["warnings"] else "ok")
    return "info" if (record["problems"] or record["warnings"]) else "ok"


# ------------------------------------------------------------------ run

def load_used(report: str | None, base_dir: str) -> set | None:
    if not report:
        return None
    with open(report, encoding="utf-8") as fh:
        rep = json.load(fh)
    return {rights.rel(fld, base_dir).lower() for fld in rep.get("files_used", [])}


def load_earlier(fpath: str | None) -> tuple[list, bool]:
    if not fpath or not os.path.isfile(fpath):
        return [], False
    try:
        with open(fpath, encoding="utf-8") as fh:
            data = json.load(fh)
    except (OSError, ValueError):
        return [], True
    return (data if isinstance(data, list) else []), True


class AuditError(Exception):
    """An input error: exit 2."""


def _inputs(cli, base_dir: str) -> dict:
    """Paths of the run: mdir, out, existing, report. Raises AuditError on a missing input."""
    man_dir = os.path.join(base_dir, cli.manifests)
    if not os.path.isdir(man_dir):
        raise AuditError(f"manifest folder not found: {man_dir}")
    acc = os.path.join(base_dir, cli.out) if cli.out else os.path.join(man_dir, "rights.json")
    report = os.path.join(base_dir, cli.report) if cli.report else None
    if report and not os.path.isfile(report):
        raise AuditError(f"mix report not found: {report}")
    return {"mdir": man_dir, "out": acc, "existing": os.path.join(base_dir, cli.existing) if cli.existing else acc,
            "report": report}


def _earlier_index(earlier: list, base_dir: str) -> dict:
    by_key = {}
    for row_ in (row_ for row_ in earlier if isinstance(row_, dict)):
        fpath = rights.rel((row_.get("file_check") or {}).get("path") or row_.get("file") or "", base_dir).lower()
        by_key.setdefault((row_.get("id"), fpath), row_)
        by_key.setdefault((row_.get("id"), ""), row_)
    return by_key


def _duplicate(ent: dict, base_dir: str, agent: dict, first: dict) -> dict:
    """The info row of a file listed again, and its note on the first record."""
    cls = rights.classify(ent.get("rights"))
    record = {"id": ent.get("id"), "manifest": ent.get("_manifest"), "need": ent.get("need"), "rank": ent.get("rank"),
           "in_scope": False, "rights_in_manifest": ent.get("rights"), "rights": cls["key"],
           "rights_status": cls["status"],
           "rights_seen": agent.get("rights_seen"), "rights_confirmed": bool(agent.get("rights_confirmed")),
           "file_check": {"path": rights.rel(ent["file"], base_dir)}, "problems": [],
           "warnings": [f"duplicate of {first['id']}: the same file is already audited"],
           "agent_notes": agent.get("agent_notes"), "audit_tool": TOOL, "duplicate_of": first["id"]}
    if record["rights_status"] != "allowed":
        first["problems"].append(f"the same file is listed again as {ent.get('id')} under "
                                 f"{ent.get('rights')!r}: one recording has one licence")
    elif record["rights"] != first["rights"]:
        first["warnings"].append(f"the same file is listed again as {ent.get('id')} under another "
                                 f"licence ({record['rights']})")
    return record


def _in_scope(fpath: str, in_use, prior) -> bool:
    if in_use is not None:
        return fpath in in_use
    return prior.get("in_scope", True) if prior else True


def _record(cli, base_dir: str, ent: dict, prior, in_use) -> dict:
    agent, efc, extra = carry(prior)
    in_scope = _in_scope(rights.rel(ent["file"], base_dir).lower(), in_use, prior)
    do_decode = cli.decode == "all" or (cli.decode == "scope" and in_scope)
    record = check_entry(ent, base_dir, do_decode, agent)
    record["in_scope"] = bool(in_scope)
    if efc:
        record["earlier_file_check"] = efc
    record.update(extra)
    return record


def _records(cli, base_dir: str, entries: list, by_key: dict, in_use) -> tuple[list, dict]:
    """(records, first record by lower-case path)."""
    records, seen_files = [], {}
    for ent in entries:
        fpath = rights.rel(ent["file"], base_dir).lower()
        prior = by_key.get((ent.get("id"), fpath)) or by_key.get((ent.get("id"), ""))
        if fpath in seen_files:
            records.append(_duplicate(ent, base_dir, carry(prior)[0], seen_files[fpath]))
            continue
        record = _record(cli, base_dir, ent, prior, in_use)
        seen_files[fpath] = record
        records.append(record)
    return records, seen_files


def _fetch_targets(cli, base_dir: str, records: list, entries: list) -> list:
    """(record, url, local path) of every record to compare with its download_url."""
    acc = []
    for record, ent in zip(records, entries):
        wanted = not record.get("duplicate_of") and (cli.fetch_all or record["in_scope"])
        url, apath = ent.get("download_url"), os.path.join(base_dir, record["file_check"]["path"])
        if wanted and url and os.path.isfile(apath):
            acc.append((record, url, apath))
    return acc


def _fetch_all(targets: list) -> None:
    for ix, (record, url, apath) in enumerate(targets):
        if ix:
            time.sleep(1.5)
        result_ = fetch_compare(url, apath)
        record["file_check"].update(result_)
        if result_.get("bytes_match_download_url") is False:
            record["warnings"].append("the local bytes differ from download_url (re-encoded or edited file)")


def _unlisted(in_use: set, listed: set) -> list:
    return [{"id": None, "in_scope": True, "file_check": {"path": fld, "exists": None},
             "rights": None, "rights_status": "unknown", "rights_confirmed": False,
             "problems": ["used in the film but in no manifest"], "warnings": [], "audit_tool": TOOL}
            for fld in sorted(in_use - listed)]


def _byte_counts(records: list) -> tuple[int, int, int]:
    """(records with auditor notes, byte matches this run, byte matches from earlier audits)."""
    with_notes = sum(1 for row_ in records if row_.get("agent_notes"))
    now = sum(1 for row_ in records if (row_.get("file_check") or {}).get("bytes_match_download_url") is True)
    before = sum(1 for row_ in records if (row_.get("earlier_file_check") or {}).get("bytes_match_download_url"))
    return with_notes, now, before


def _rights_counts(scope: list) -> str:
    keys = Counter(row_.get("rights") for row_ in scope)
    return ", ".join(f"{key_} x{num}" for key_, num in keys.most_common()) or "none"


def _summary(entries: list, records: list) -> tuple[str, Counter]:
    scope = [row_ for row_ in records if row_["in_scope"]]
    sev = Counter(row_["severity"] for row_ in scope)
    with_notes, matched_now, matched_before = _byte_counts(records)
    summary = (f"{len(entries)} entries, {len(scope)} in scope: {sev['ok']} ok, {sev['warning']} warning, "
               f"{sev['blocker']} blocker; rights in scope: " + _rights_counts(scope)
               + f"; {with_notes} record(s) with auditor notes; byte match {matched_now} this run, "
               f"{matched_before} from earlier audits")
    if sev["blocker"]:
        summary += "; fix every blocker before the film ships"
    return summary, sev


def _stem(acc: str) -> str:
    return acc[:-5] if acc.endswith(".json") else acc


def _hand_written(acc: str) -> bool:
    """True when the audit file was not written by this tool, or is not valid JSON."""
    old_list, _ = load_earlier(acc)
    try:
        with open(acc, encoding="utf-8") as fh:
            json.load(fh)
    except ValueError:
        return True
    return not any(isinstance(row_, dict) and row_.get("audit_tool") for row_ in old_list)


def _backups(acc: str) -> list[str]:
    """Keep rights.prev.json, and rights.handwritten.json once for a hand-written file."""
    if not os.path.isfile(acc):
        return []
    prev, hw = _stem(acc) + ".prev.json", _stem(acc) + ".handwritten.json"
    shutil.copyfile(acc, prev)
    if _hand_written(acc) and not os.path.exists(hw):
        shutil.copyfile(acc, hw)
        return [prev, hw]
    return [prev]


def _write_audit(acc: str, table: str, records: list, summary: str) -> list[str]:
    os.makedirs(os.path.dirname(acc), exist_ok=True)
    with open(acc, "w", encoding="utf-8") as fh:
        json.dump(records, fh, indent=1, ensure_ascii=False)
        fh.write("\n")
    with open(table, "w", encoding="utf-8") as fh:
        fh.write(markdown(records, summary))
    return [acc, table]


def _audit(cli, base_dir: str) -> tuple[int, dict]:
    io_ = _inputs(cli, base_dir)
    entries, notes = rights.load_manifests(io_["mdir"])
    if not entries:
        raise AuditError(f"no manifest entries in {io_['mdir']}")
    in_use = load_used(io_["report"], base_dir)
    earlier, _ = load_earlier(io_["existing"])
    records, seen_files = _records(cli, base_dir, entries, _earlier_index(earlier, base_dir), in_use)
    if cli.fetch or cli.fetch_all:
        _fetch_all(_fetch_targets(cli, base_dir, records, entries))
    records += _unlisted(in_use, set(seen_files)) if in_use is not None else []
    for record in records:
        record["severity"] = severity(record)
    summary, sev = _summary(entries, records)
    acc = io_["out"]
    backups = _backups(acc)
    table = os.path.join(base_dir, cli.table) if cli.table else _stem(acc) + ".md"
    written = _write_audit(acc, table, records, summary)
    return (1 if sev["blocker"] else 0), {"summary": summary, "notes": notes, "records": records,
                                          "written": written, "backups": backups}


def audit(cli) -> tuple[int, dict]:
    try:
        return _audit(cli, os.path.abspath(cli.root))
    except AuditError as ent:
        return 2, {"error": str(ent)}


def _file_cell(fchk: dict) -> str:
    if fchk.get("codec"):
        sr = (fchk.get("sample_rate") or 0) / 1000.0
        return f"{fchk['codec']} {sr:g}k {fchk.get('channels')}ch {fchk.get('duration_s')}s"
    return "MISSING" if not fchk.get("exists") else "?"


def _credit_cell(row_: dict) -> str:
    if not row_.get("attribution_required"):
        return "n/a"
    return "ok" if row_.get("attribution_ok") else "MISSING"


def _row(row_: dict) -> str:
    issues = "; ".join(row_.get("problems", []) + row_.get("warnings", []))[:300].replace("|", "/")
    return (f"| {row_.get('id')} | {'yes' if row_['in_scope'] else 'no'} | {row_['severity']} | {row_.get('rights')} | "
            f"{'yes' if row_.get('rights_confirmed') else 'NO'} | {_credit_cell(row_)} | "
            f"{_file_cell(row_.get('file_check') or {})} | {issues} |")


def markdown(records: list, summary: str) -> str:
    order = {"blocker": 0, "warning": 1, "info": 2, "ok": 3}
    table = sorted(records, key=lambda row_: (not row_["in_scope"], order.get(row_["severity"], 9), str(row_.get("id"))))
    lines = ["# Usage-rights audit", "", summary, "",
             "| id | in film | severity | rights | verified | credit | file | issues |",
             "|---|---|---|---|---|---|---|---|"]
    return "\n".join(lines + [_row(row_) for row_ in table]) + "\n"


def _issue_groups(records: list) -> tuple[Counter, dict]:
    """Count of each (level, message) over in-scope records that are not ok, and the ids per group."""
    groups, ids = Counter(), {}
    for row_ in (row_ for row_ in records if row_["in_scope"] and row_["severity"] != "ok"):
        for level, msgs in (("BLOCKER", row_["problems"]), ("warning", row_["warnings"])):
            for hit in msgs:
                groups[(level, hit)] += 1
                ids.setdefault((level, hit), []).append(str(row_.get("id")))
    return groups, ids


def print_result(result_: dict):
    say(result_["summary"])
    for num in result_["notes"]:
        say("  note:", num)
    groups, ids = _issue_groups(result_["records"])
    for (level, hit), num in sorted(groups.items(), key=lambda kv: (kv[0][0] != "BLOCKER", -kv[1], kv[0][1])):
        more = " ..." if len(ids[(level, hit)]) > 8 else ""
        say(f"  {level} x{num}: {hit}  [{' '.join(ids[(level, hit)][:8])}{more}]")
    for wv in result_["written"]:
        say("wrote", wv)
    for bv in result_["backups"]:
        say("kept", bv)


def parse(argv):
    ap = argparse.ArgumentParser(prog="rights_audit.py", description="Usage-rights and file audit of the manifests.")
    ap.add_argument("--root", default=".")
    ap.add_argument("--manifests", default="audio/assets/manifests")
    ap.add_argument("--report", help="mix report: its files_used are in scope")
    ap.add_argument("--out", help="audit file (default <manifests>/rights.json)")
    ap.add_argument("--existing", help="read the auditor fields from this file instead of --out")
    ap.add_argument("--table", help="Markdown table (default --out with .md)")
    ap.add_argument("--fetch", action="store_true", help="compare local bytes with download_url, in scope only")
    ap.add_argument("--fetch-all", action="store_true", help="the same for every entry")
    ap.add_argument("--decode", choices=("scope", "all", "none"), default="scope")
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--self-test", action="store_true")
    return ap.parse_args(argv)


def main(argv=None) -> int:
    cli = parse(sys.argv[1:] if argv is None else argv)
    if cli.self_test:
        return self_test()
    code, result_ = audit(cli)
    if code == 2:
        say(f"rights_audit.py: {result_['error']}", file=sys.stderr)
        return 2
    if cli.json:
        say(json.dumps({key_: val for key_, val in result_.items()}, indent=1))
    else:
        print_result(result_)
    return code


ENTRY = {"id": "fs-123456", "category": "ui", "file": "audio/assets/ui/fs-123456_tap.wav",
         "title": "Tap", "author": "alice", "source": "freesound",
         "page_url": "https://freesound.org/people/alice/sounds/123456/",
         "download_url": "https://cdn.freesound.org/previews/123/123456_9-hq.ogg",
         "rights": "CC BY 4.0", "rights_url": "https://creativecommons.org/licenses/by/4.0/",
         "attribution": "\"Tap\" by alice, licensed under CC BY 4.0", "need": "click", "rank": 1}


def _records_of(man_dir: str) -> list:
    with open(os.path.join(man_dir, "rights.json"), encoding="utf-8") as fh:
        return json.load(fh)


def _test_reading(ck, fx, tmp: str, man_dir: str) -> None:
    code = main(["--root", tmp])
    recs = _records_of(man_dir)
    ck("S2 no page reading is a blocker, exit 1, files written",
       code == 1 and recs[0]["severity"] == "blocker"
       and any("no page reading" in pv for pv in recs[0]["problems"])
       and os.path.isfile(os.path.join(man_dir, "rights.md")))
    recs[0]["rights_seen"] = {"site": "freesound", "rights": "Attribution 4.0", "read": "2026-11-07"}
    recs[0]["rights_confirmed"] = True
    recs[0]["agent_notes"] = "author asks for the credit form in the manifest"
    fx.put_json(os.path.join(man_dir, "rights.json"), recs)
    code = main(["--root", tmp])
    ck("S3 a confirmed reading clears the entry", code == 0 and _records_of(man_dir)[0]["severity"] == "ok")
    code = main(["--root", tmp])
    recs = _records_of(man_dir)
    ck("S3 a second run keeps the agent fields and writes rights.prev.json",
       code == 0 and recs[0]["rights_confirmed"] is True and bool(recs[0]["agent_notes"])
       and os.path.isfile(os.path.join(man_dir, "rights.prev.json")))


def _test_scope(ck, fx, tmp: str, man_dir: str) -> None:
    rep = fx.put_json(os.path.join(tmp, "out", "qa", "mix_report.json"),
                      {"files_used": ["audio/assets/ui/fs-123456_tap.wav", "audio/assets/ui/stray.wav"]})
    code = main(["--root", tmp, "--report", "out/qa/mix_report.json"])
    ck("S4 a used file in no manifest is a blocker row with id null",
       code == 1 and any(row_["id"] is None and row_["severity"] == "blocker" for row_ in _records_of(man_dir)))
    other = dict(ENTRY, id="fs-999", file="audio/assets/ui/other.wav", rights="CC BY-NC 4.0")
    fx.write(os.path.join(tmp, other["file"]), fx.burst(0.2))
    fx.put_json(os.path.join(man_dir, "ui.json"), [ENTRY, other])
    fx.put_json(rep, {"files_used": ["audio/assets/ui/fs-123456_tap.wav"]})
    code = main(["--root", tmp, "--report", "out/qa/mix_report.json"])
    ck("S4 an entry outside files_used is out of scope and does not block",
       code == 0 and [row_["in_scope"] for row_ in _records_of(man_dir)] == [True, False])


def _test_duplicate(ck, fx, tmp: str, man_dir: str) -> None:
    fx.put_json(os.path.join(man_dir, "zz.json"), [dict(ENTRY, id="fs-123456b", rights="CC BY-NC 4.0")])
    main(["--root", tmp])
    recs = _records_of(man_dir)
    first = next(row_ for row_ in recs if row_["id"] == "fs-123456")
    d2 = next(row_ for row_ in recs if row_["id"] == "fs-123456b")
    ck("S5 a duplicate under NC: a problem on the first, info row with duplicate_of",
       any("one recording has one licence" in pv for pv in first["problems"])
       and d2["severity"] == "info" and d2["duplicate_of"] == "fs-123456")
    os.remove(os.path.join(man_dir, "zz.json"))


def _test_urls(ck) -> None:
    bad = dict(ENTRY, page_url="https://freesound.org/people/alice/sounds/123457/",
               download_url="https://cdn.freesound.org/previews/123/123456_9-lq.ogg")
    wv = " | ".join(url_warnings(bad))
    ck("S6 Freesound id, page/preview and lq warnings; folder 123 is right",
       "does not match the Freesound page" in wv and "preview URL is sound 123456" in wv
       and "-lq" in wv and "folder" not in wv)
    ck("Mixkit id check", url_warnings({"id": "mk-5", "download_url":
                                        "https://assets.mixkit.co/active_storage/sfx/7/7.wav"}) != [])


def _test_entries(ck, fx, tmp: str) -> None:
    empty = os.path.join(tmp, "empty")
    os.makedirs(os.path.join(empty, "audio", "assets", "manifests"))
    ck("exit 2 without entries", main(["--root", empty]) == 2)
    record = check_entry(dict(ENTRY, notes="Licence read on the item page on 2026-11-06. Close and dry."), tmp, False, {})
    ck("a notes sentence about a reading is quoted", any("Licence read on the item page" in pv
                                                         for pv in record["problems"]))
    fx.write(os.path.join(tmp, "audio", "assets", "ui", "silent.wav"), fx.burst(0.3) * 0)
    record = check_entry(dict(ENTRY, file="audio/assets/ui/silent.wav"), tmp, True, {})
    ck("digital silence is a problem", any("digital silence" in pv for pv in record["problems"]))


def self_test() -> int:
    import tempfile
    import fixtures as fx
    ck = fx.Checker()
    with tempfile.TemporaryDirectory() as tmp:
        man_dir = os.path.join(tmp, "audio", "assets", "manifests")
        fx.write(os.path.join(tmp, "audio", "assets", "ui", "fs-123456_tap.wav"), fx.burst(0.3))
        fx.put_json(os.path.join(man_dir, "ui.json"), [ENTRY])
        _test_reading(ck, fx, tmp, man_dir)
        _test_scope(ck, fx, tmp, man_dir)
        _test_duplicate(ck, fx, tmp, man_dir)
        _test_urls(ck)
        _test_entries(ck, fx, tmp)
    return ck.code()


if __name__ == "__main__":
    sys.exit(main())
