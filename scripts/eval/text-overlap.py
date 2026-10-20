#!/usr/bin/env python3
"""Find word sequences that a document shares with another project's text.

Usage: python3 -I scripts/eval/text-overlap.py <document> <reference dir> [N]

Prints how many N-word sequences (default 8) of <document> also appear in
any text file under <reference dir>, and lists each one with the file it
came from. Case and punctuation are ignored. Used for the clean-room check
in docs/proposals/plan-format-v1.md, section 8. Reads files only.
"""
import sys, os, re
N = int(sys.argv[3]) if len(sys.argv) > 3 else 8
def words(t): return re.findall(r"[a-z0-9_]+", t.lower())
def grams(w): return {tuple(w[i:i+N]) for i in range(len(w)-N+1)}
doc = grams(words(open(sys.argv[1], encoding="utf-8").read()))
ref = {}
for root, _, files in os.walk(sys.argv[2]):
    if "/.git" in root: continue
    for f in files:
        if not f.endswith((".md", ".ts", ".json", ".yaml", ".yml", ".mjs", ".sh", ".py")): continue
        p = os.path.join(root, f)
        try: t = open(p, encoding="utf-8", errors="ignore").read()
        except Exception: continue
        for g in grams(words(t)): ref.setdefault(g, p)
hits = sorted(g for g in doc if g in ref)
print(f"{N}-grams in doc: {len(doc)}; shared: {len(hits)}")
for g in hits: print(" ".join(g), "<-", os.path.relpath(ref[g], sys.argv[2]))
