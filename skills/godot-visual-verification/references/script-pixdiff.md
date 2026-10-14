# Script: pixdiff.py

Usage: `python3 -I pixdiff.py A.png B.png [--tol N] [--box X,Y,W,H] [--mask OUT.png] [--floor BASE2.png]`

The script counts changed pixels between 2 PNG captures, with a channel tolerance, an optional
box and an optional diff mask image. With `--floor` it takes a second base capture, prints the
noise floor, and counts only the pixels where the change differs from both base captures. It
reads 8-bit non-interlaced PNG files and uses the Python standard library only. Copy the block
into a file named `pixdiff.py` to use it. The procedure that uses it is in `pixel-diff.md`.

```python
#!/usr/bin/env python3
"""Count changed pixels between 2 PNG captures. Standard library only.

Usage:
  python3 -I pixdiff.py A.png B.png [--tol N] [--box X,Y,W,H] [--mask OUT.png]
  python3 -I pixdiff.py A.png B.png --floor BASE2.png [--tol N] [--box X,Y,W,H]

A pixel counts as changed when any RGB channel differs by more than --tol (default 0).
--box limits the count to 1 rectangle. --mask writes a black and white diff image.
--floor takes a second base capture: A and --floor are 2 base captures of the same build,
B is the change. The script prints the noise floor (A against the floor capture) and the
pixels where B differs from both base captures, which is the change outside the noise.
Exit code: 0 after a compare (read the printed counts), 1 on a size mismatch.
Supports 8-bit non-interlaced PNG: grey, grey+alpha, RGB, RGBA, palette.
"""
import argparse
import struct
import sys
import zlib

_CHANNELS = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}


def read_png(path):
    """Return (width, height, rows) where rows[y][x] is an (r, g, b) tuple."""
    with open(path, "rb") as f:
        data = f.read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError(f"{path}: not a PNG file")
    pos, idat, palette = 8, [], None
    width = height = depth = ctype = interlace = None
    while pos < len(data):
        (length,) = struct.unpack(">I", data[pos:pos + 4])
        kind = data[pos + 4:pos + 8]
        body = data[pos + 8:pos + 8 + length]
        pos += 12 + length
        if kind == b"IHDR":
            width, height, depth, ctype, _, _, interlace = struct.unpack(">IIBBBBB", body)
        elif kind == b"PLTE":
            palette = [tuple(body[i:i + 3]) for i in range(0, len(body), 3)]
        elif kind == b"IDAT":
            idat.append(body)
        elif kind == b"IEND":
            break
    if depth != 8 or interlace != 0 or ctype not in _CHANNELS:
        raise ValueError(f"{path}: need 8-bit non-interlaced PNG (depth={depth}, type={ctype})")
    bpp = _CHANNELS[ctype]
    raw = zlib.decompress(b"".join(idat))
    stride = width * bpp
    rows, prev, i = [], bytearray(stride), 0
    for _ in range(height):
        ftype = raw[i]
        line = bytearray(raw[i + 1:i + 1 + stride])
        i += 1 + stride
        _unfilter(ftype, line, prev, bpp)
        rows.append(_to_rgb(line, ctype, bpp, palette))
        prev = line
    return width, height, rows


def _unfilter(ftype, line, prev, bpp):
    for x in range(len(line)):
        a = line[x - bpp] if x >= bpp else 0
        b = prev[x]
        c = prev[x - bpp] if x >= bpp else 0
        if ftype == 1:
            line[x] = (line[x] + a) & 0xFF
        elif ftype == 2:
            line[x] = (line[x] + b) & 0xFF
        elif ftype == 3:
            line[x] = (line[x] + ((a + b) >> 1)) & 0xFF
        elif ftype == 4:
            p = a + b - c
            pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
            pred = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
            line[x] = (line[x] + pred) & 0xFF


def _to_rgb(line, ctype, bpp, palette):
    px = [line[x:x + bpp] for x in range(0, len(line), bpp)]
    if ctype == 3:
        return [palette[p[0]] for p in px]
    if ctype in (0, 4):
        return [(p[0], p[0], p[0]) for p in px]
    return [(p[0], p[1], p[2]) for p in px]


def write_mask(path, width, height, changed):
    """Write a greyscale PNG: white where changed[y][x] is True."""
    raw = b"".join(b"\x00" + bytes(255 if v else 0 for v in row) for row in changed)

    def chunk(kind, body):
        return (struct.pack(">I", len(body)) + kind + body
                + struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF))

    ihdr = struct.pack(">IIBBBBB", width, height, 8, 0, 0, 0, 0)
    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr)
                + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b""))


def diff_map(a, b, tol, box):
    x0, y0, x1, y1 = box
    return [[x0 <= x < x1 and y0 <= y < y1
             and max(abs(pa[k] - pb[k]) for k in range(3)) > tol
             for x, (pa, pb) in enumerate(zip(ra, rb))]
            for y, (ra, rb) in enumerate(zip(a, b))]


def count(m):
    return sum(sum(row) for row in m)


def main(argv):
    ap = argparse.ArgumentParser(description="Count changed pixels between 2 PNG captures.")
    ap.add_argument("a")
    ap.add_argument("b")
    ap.add_argument("--tol", type=int, default=0, help="max channel difference that still matches")
    ap.add_argument("--box", help="X,Y,W,H region to compare")
    ap.add_argument("--mask", help="write a diff mask PNG here")
    ap.add_argument("--floor", help="second base capture; A and FLOOR are base, B is the change")
    args = ap.parse_args(argv)

    wa, ha, a = read_png(args.a)
    wb, hb, b = read_png(args.b)
    if (wa, ha) != (wb, hb):
        print(f"SIZE MISMATCH {args.a}={wa}x{ha} {args.b}={wb}x{hb}")
        return 1
    box = (0, 0, wa, ha)
    if args.box:
        x, y, w, h = (int(v) for v in args.box.split(","))
        box = (x, y, x + w, y + h)
    total = (min(box[2], wa) - box[0]) * (min(box[3], ha) - box[1])

    ab = diff_map(a, b, args.tol, box)
    print(f"size {wa}x{ha} box {box[0]},{box[1]},{box[2] - box[0]},{box[3] - box[1]} "
          f"pixels {total} tol {args.tol}")
    print(f"changed A-B {count(ab)}")
    result = ab
    if args.floor:
        wf, hf, f = read_png(args.floor)
        if (wf, hf) != (wa, ha):
            print(f"SIZE MISMATCH {args.floor}={wf}x{hf}")
            return 1
        af = diff_map(a, f, args.tol, box)
        fb = diff_map(f, b, args.tol, box)
        result = [[v1 and v2 for v1, v2 in zip(r1, r2)] for r1, r2 in zip(ab, fb)]
        print(f"noise floor A-FLOOR {count(af)}")
        print(f"changed FLOOR-B {count(fb)}")
        print(f"changed vs both bases {count(result)}")
    if args.mask:
        write_mask(args.mask, wa, ha, result)
        print(f"mask {args.mask}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
```
