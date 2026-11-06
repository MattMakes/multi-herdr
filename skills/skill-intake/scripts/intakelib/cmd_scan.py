"""`intake scan --task T`: sweep the quarantined tree with the scanner rules (spec 4.2).

Writes C/sweep/scan.jsonl, one hit per line, and C/sweep/opaque/<id>.json for
each opaque artifact (spec 4.4). Findings are data, not failure: the command
exits 0 whatever it finds. It reads quarantine/src/ as bytes and text only; the
static readers (nm, otool, objdump) get an opaque file only as the file to read.
"""

from __future__ import annotations

import json
import sys

from . import opaque, sweep, task

HELP = "sweep the quarantined tree with the scanner rules; write sweep/scan.jsonl and sweep/opaque/"


def add_arguments(parser) -> None:
    parser.add_argument("--task", required=True, help="the task dir that acquire printed")


def run(args) -> int:
    t = task.load(args.task)
    src = t.c / "quarantine" / "src"
    if not src.is_dir():
        raise task.TaskError(f"no quarantine/src in {t.c}: run intake acquire first")
    hits = sweep.scan_tree(src)
    out = t.c / "sweep" / "scan.jsonl"
    sweep.write_jsonl(out, hits)
    records = opaque.write_records(src, hits, t.c / "sweep" / "opaque")
    counts = {s: sum(1 for h in hits if h["severity"] == s) for s in sweep.SEVERITIES}
    malicious = sum(1 for h in hits if h["malicious"])
    print(json.dumps({"scan": str(out), "hits": len(hits), "counts": counts, "malicious": malicious,
                      "opaque": records}))
    print(f"intake scan: {len(hits)} hit(s), {malicious} malicious: {out}", file=sys.stderr)
    return 0
