# Script: contrast_table.py

Usage: `python3 -I contrast_table.py --fill 6abe30 --outline 222034 --bg lit=99e550 --bg panel=eec39a`

The script prints a WCAG contrast table of sprite fill colours and the outline colour against each background colour. A row passes when the better of the fill contrast and the outline contrast is at least `--min` (default 3:1). The exit code is 1 when any row fails. The script uses the standard library only. Copy the block into a file, for example `contrast_table.py` in a review folder, and run it.

```python
#!/usr/bin/env python3
"""Print a WCAG contrast table of sprite colours against background colours. Standard library only.

Usage:
  python3 -I contrast_table.py --fill HEX [--fill HEX ...] [--outline HEX]
      --bg NAME=HEX [--bg NAME=HEX ...] [--min 3.0]

For each fill and each background, the script prints the fill contrast, the outline contrast
and the better of the two. A row passes when max(fill, outline) >= --min. This is the
"fill or outline carries the contrast" rule: a dark 1-pixel outline on all 4 sides can carry
a sprite whose fill is close to the background. Without --outline, the fill alone decides.
The exit code is 1 when any row fails.

Example:
  python3 -I contrast_table.py --fill 6abe30 --fill d95763 --outline 222034 \\
      --bg lit=99e550 --bg shadow=37946e --bg panel=eec39a
"""
import argparse
import sys


def channel(c):
    c /= 255
    return c / 12.92 if c <= 0.03928 else ((c + 0.055) / 1.055) ** 2.4


def luminance(hex_text):
    h = hex_text.lstrip("#")
    r, g, b = (int(h[i:i + 2], 16) for i in (0, 2, 4))
    return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)


def ratio(a, b):
    la, lb = sorted((luminance(a), luminance(b)), reverse=True)
    return (la + 0.05) / (lb + 0.05)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--fill", action="append", required=True)
    ap.add_argument("--outline")
    ap.add_argument("--bg", action="append", required=True, help="NAME=HEX")
    ap.add_argument("--min", type=float, default=3.0)
    args = ap.parse_args()
    fails = rows = 0
    print(f"{'fill':8} {'background':14} {'fill:1':>7} {'ink:1':>7} {'best':>6}  result")
    for fill in args.fill:
        for bg in args.bg:
            name, colour = bg.split("=", 1)
            f = ratio(fill, colour)
            o = ratio(args.outline, colour) if args.outline else 0.0
            best = max(f, o)
            ok = best >= args.min
            rows += 1
            fails += not ok
            ink = f"{o:7.2f}" if args.outline else f"{'-':>7}"
            print(f"{fill.lstrip('#'):8} {name:14} {f:7.2f} {ink} {best:6.2f}  "
                  f"{'PASS' if ok else 'FAIL'}")
    print(f"{fails} of {rows} rows fail (min {args.min}:1)")
    sys.exit(1 if fails else 0)


if __name__ == "__main__":
    main()
```
