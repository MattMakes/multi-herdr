"""The listener's answer sheet and the scoring script written into a blind test folder."""

from __future__ import annotations

import math
import textwrap

WIDTH = 112


def p_at_least(k: int, n: int) -> float:
    """One-sided binomial P(at least k of n right by guessing)."""
    return sum(math.comb(n, i) for i in range(k, n + 1)) / 2 ** n


def k05(n: int) -> tuple[int, float]:
    """The smallest k with P(at least k of n) < 0.05, and that p."""
    k = next(k for k in range(n + 2) if k > n or p_at_least(k, n) < 0.05)
    return k, p_at_least(k, n) if k <= n else 0.0


def _clean_text(s: dict, n_warn: int) -> str:
    return (f"Cleanliness: {s['status']}. The loudest moments of the two clips differ by at most "
            f"{s.get('max_loud_diff_lu')} LU and no true peak passes {s.get('max_true_peak_dbtp')} dBTP. Each clip "
            "starts with a 4 ms fade in and ends with a fade over its last 6 dB of decay. Onset and end levels sit "
            f"above every recording floor (smallest margin {s.get('min_floor_margin_db')} dB). Gaps between clips are "
            f"digital silence. Warnings: {n_warn}.")


def answer_sheet(keys: list[dict], summary: dict, total: float, n: int) -> str:
    """The answer sheet text (no answers inside)."""
    k, p = k05(n)
    intro = ("Listen to each trial: clip A, then clip B. One clip comes from a real recording; the forge rendered "
             "the other from code alone. Write which one is real (A or B), or ? when you cannot tell. Do not open "
             "KEY_open_after.json first. Both clips get the same trim, both peak at the same loudness, and the real "
             "clips come from refs that no search used.")
    lines = textwrap.wrap(intro, WIDTH) + ["", f"Score: python score_answers.py <{n} letters, A B or ?>", ""]
    lines += textwrap.wrap(f"By guessing you expect about {n / 2:g} of {n} right. {k} or more right is unlikely by "
                           f"luck (p = {p:.3f}).", WIDTH) + [""]
    lines += textwrap.wrap(_clean_text(summary, len(summary.get("warnings") or [])), WIDTH) + [""]
    lines += [f"{'trial':>5}  {'sound':<20} {'A start':>9} {'B start':>9}  answer"]
    lines += [f"{e['trial']:>5}  {e['sound']:<20} {e['A_start']:>8.2f}s {e['B_start']:>8.2f}s  ____" for e in keys]
    lines += ["", f"Total {total:.1f} s, 48 kHz stereo PCM 24-bit, {n} trials, blind_test.wav plus one file per clip."]
    return "\n".join(lines) + "\n"


SCORE_SCRIPT = r'''"""Score a blind test: python score_answers.py A B B A ... (A or B = the side you think is real, ? = undecided)."""

import json
import math
import os
import re
import sys


def p_at_least(k, n):
    return sum(math.comb(n, i) for i in range(k, n + 1)) / 2 ** n if n else 1.0


def p_at_most(k, n):
    return sum(math.comb(n, i) for i in range(0, k + 1)) / 2 ** n if n else 1.0


def fmt_p(p):
    s = f"{p:.3f}"
    if p < 0.001:
        s += " (" + re.sub(r"e-0*(\d)", r"e-\1", f"{p:.1e}") + ")"
    return s


def tally(rows):
    done = [r for r in rows if r[2] != "?"]
    return sum(1 for r in done if r[2] == r[3]), len(done)


def main(argv):
    here = os.path.dirname(os.path.abspath(__file__))
    with open(os.path.join(here, "KEY_open_after.json")) as fh:
        trials = json.load(fh)["trials"]
    n = len(trials)
    letters = [t.strip().upper() for a in argv for t in a.split(",") if t.strip()]
    if len(letters) != n or any(x not in ("A", "B", "?") for x in letters):
        print(f"usage: python score_answers.py <{n} letters: A, B or ?>  (got {len(letters)})")
        return 2
    rows = []
    for t, a in zip(trials, letters):
        res = "skipped" if a == "?" else "right" if a == t["real"] else "wrong"
        rows.append((t["trial"], t["sound"], a, t["real"], t.get("tier", ""), res))
        print(f"{t['trial']:>3} {t['sound']:<20} answer {a}  real {t['real']}  {res:<8} "
              f"{os.path.basename(t['real_file'])} / {os.path.basename(t['made_file'])}")
    print()
    for sound in dict.fromkeys(r[1] for r in rows):
        k, m = tally([r for r in rows if r[1] == sound])
        print(f"{sound}: {k} of {m} right")
    for tier in ("strong", "middle", "weak"):
        k, m = tally([r for r in rows if r[4] == tier])
        if m:
            print(f"tier {tier}: {k} of {m} right (p = {fmt_p(p_at_least(k, m))})")
    k, m = tally(rows)
    print(f"overall: {k} of {m} right; guessing gives about {m / 2:g}; one-sided binomial p = {fmt_p(p_at_least(k, m))}")
    if p_at_least(k, m) < 0.05:
        print("p < 0.05: the listener can tell made from real for at least some sounds.")
    else:
        print("p >= 0.05: the test could not show that the listener tells made from real.")
    if p_at_most(k, m) < 0.05:
        print("So few right is itself unlikely: the listener heard the made sounds as real more often than not.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
'''
