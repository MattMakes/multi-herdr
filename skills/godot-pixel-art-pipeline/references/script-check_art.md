# Script: check_art.py

Usage: `python3 -I check_art.py --palette palette.hex atlas.png --cell 16x16 --frames 12 --hard-alpha --outline 222034 --base /tmp/atlas_base.png --keep-cells 8`

The script checks pixel-art PNG files against a palette and a cell contract. It prints 1 PASS or FAIL line for each check: size, palette, hard alpha, frame grid and count, closed outline, and old cells byte-identical to a base copy. Make the base copy with `git show <base>:<atlas path> > /tmp/atlas_base.png`. The exit code is 1 when any check fails. The script uses the standard library only. Copy the block into a file, for example `check_art.py` in a review folder, and run it.

```python
#!/usr/bin/env python3
"""Check pixel-art PNG files against a palette and a cell contract. Standard library only.

Usage:
  python3 -I check_art.py --palette palette.hex IMAGE.png [IMAGE.png ...]
      [--size WxH] [--cell WxH] [--frames N] [--hard-alpha] [--outline HEX]
      [--base OLD.png --keep-cells N]

Checks, each one printed as PASS or FAIL:
  size        the image is exactly WxH pixels (--size).
  palette     every pixel with alpha above 0 has an RGB value from the palette file.
  hard-alpha  every alpha value is 0 or 255 (--hard-alpha).
  grid        the width and height divide into cells of WxH (--cell), and the cell count
              is N (--frames). Cells run left to right, then top to bottom.
  outline     each opaque pixel that touches a transparent pixel or the cell edge
              (4-neighbour) has the outline colour (--outline). This is a closed-outline test.
  keep-cells  the first N cells are byte-identical to the same cells in --base. Use it when
              you append cells to an atlas. Get the base copy with `git show <base>:<path>`.

The palette file holds 1 hex colour per line (#RRGGBB or RRGGBB). Lines that start with
'#' and a space, or ';', are comments. The exit code is 1 when any check fails.

Supported PNG files: 8-bit greyscale, RGB, palette, grey+alpha and RGBA, not interlaced.
"""
import argparse
import struct
import sys
import zlib


def read_png(path):
    """Return (width, height, rows) where rows is a list of lists of (r, g, b, a)."""
    with open(path, "rb") as f:
        data = f.read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError(f"{path}: not a PNG file")
    pos, idat, plte, trns = 8, b"", None, None
    width = height = depth = ctype = interlace = None
    while pos < len(data):
        (length,) = struct.unpack(">I", data[pos:pos + 4])
        kind = data[pos + 4:pos + 8]
        body = data[pos + 8:pos + 8 + length]
        pos += 12 + length
        if kind == b"IHDR":
            width, height, depth, ctype, _, _, interlace = struct.unpack(">IIBBBBB", body)
        elif kind == b"PLTE":
            plte = [tuple(body[i:i + 3]) for i in range(0, len(body), 3)]
        elif kind == b"tRNS":
            trns = body
        elif kind == b"IDAT":
            idat += body
        elif kind == b"IEND":
            break
    if depth != 8 or interlace != 0:
        raise ValueError(f"{path}: only 8-bit, non-interlaced PNG files are supported")
    channels = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}[ctype]
    raw = zlib.decompress(idat)
    stride = width * channels
    rows, prev, i = [], bytearray(stride), 0
    for _ in range(height):
        ftype = raw[i]
        line = bytearray(raw[i + 1:i + 1 + stride])
        i += 1 + stride
        for x in range(stride):
            a = line[x - channels] if x >= channels else 0
            b = prev[x]
            c = prev[x - channels] if x >= channels else 0
            if ftype == 1:
                line[x] = (line[x] + a) & 255
            elif ftype == 2:
                line[x] = (line[x] + b) & 255
            elif ftype == 3:
                line[x] = (line[x] + (a + b) // 2) & 255
            elif ftype == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pred = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
                line[x] = (line[x] + pred) & 255
        prev = line
        row = []
        for x in range(width):
            px = line[x * channels:(x + 1) * channels]
            if ctype == 0:
                row.append((px[0], px[0], px[0], 255))
            elif ctype == 2:
                row.append((px[0], px[1], px[2], 255))
            elif ctype == 3:
                r, g, b = plte[px[0]]
                alpha = trns[px[0]] if trns and px[0] < len(trns) else 255
                row.append((r, g, b, alpha))
            elif ctype == 4:
                row.append((px[0], px[0], px[0], px[1]))
            else:
                row.append(tuple(px))
        rows.append(row)
    return width, height, rows


def read_palette(path):
    colours = set()
    with open(path) as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith(";") or line.startswith("# "):
                continue
            colours.add(parse_hex(line.split()[0]))
    return colours


def parse_hex(text):
    text = text.lstrip("#")
    return tuple(int(text[i:i + 2], 16) for i in (0, 2, 4))


def parse_wh(text):
    w, h = text.lower().split("x")
    return int(w), int(h)


def cells(width, height, cw, ch):
    for cy in range(0, height, ch):
        for cx in range(0, width, cw):
            yield cx, cy


def check(path, args, palette):
    fails = []
    width, height, rows = read_png(path)

    def report(name, ok, detail=""):
        print(f"{'PASS' if ok else 'FAIL'} {name:10} {path} {detail}".rstrip())
        if not ok:
            fails.append(name)

    if args.size:
        w, h = parse_wh(args.size)
        report("size", (width, height) == (w, h), f"got {width}x{height}, want {w}x{h}")

    off = {}
    for y, row in enumerate(rows):
        for x, (r, g, b, a) in enumerate(row):
            if a > 0 and (r, g, b) not in palette:
                off.setdefault((r, g, b), (x, y))
    sample = ", ".join(f"#{r:02x}{g:02x}{b:02x}@{xy[0]},{xy[1]}"
                       for (r, g, b), xy in list(off.items())[:5])
    report("palette", not off, f"{len(off)} off-palette colours {sample}".strip())

    if args.hard_alpha:
        soft = sum(1 for row in rows for p in row if p[3] not in (0, 255))
        report("hard-alpha", soft == 0, f"{soft} soft alpha pixels")

    cw = ch = None
    if args.cell:
        cw, ch = parse_wh(args.cell)
        ok = width % cw == 0 and height % ch == 0
        count = (width // cw) * (height // ch)
        detail = f"{count} cells of {cw}x{ch}"
        if args.frames is not None:
            ok = ok and count == args.frames
            detail += f", want {args.frames}"
        report("grid", ok, detail)
    else:
        cw, ch = width, height

    if args.outline:
        ink = parse_hex(args.outline)
        bad = 0
        for cx, cy in cells(width, height, cw, ch):
            for y in range(cy, min(cy + ch, height)):
                for x in range(cx, min(cx + cw, width)):
                    p = rows[y][x]
                    if p[3] == 0:
                        continue
                    edge = False
                    for nx, ny in ((x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)):
                        inside = cx <= nx < cx + cw and cy <= ny < cy + ch
                        if not inside or nx >= width or ny >= height or rows[ny][nx][3] == 0:
                            edge = True
                            break
                    if edge and p[:3] != ink:
                        bad += 1
        report("outline", bad == 0, f"{bad} edge pixels not {args.outline}")

    if args.base:
        bw, bh, brows = read_png(args.base)
        changed = []
        for n, (cx, cy) in enumerate(cells(bw, bh, cw, ch)):
            if n >= args.keep_cells:
                break
            for y in range(cy, cy + ch):
                if y >= height or rows[y][cx:cx + cw] != brows[y][cx:cx + cw]:
                    changed.append(n)
                    break
        report("keep-cells", not changed, f"changed cells {changed}" if changed else
               f"first {args.keep_cells} cells identical")
    return fails


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("images", nargs="+")
    ap.add_argument("--palette", required=True)
    ap.add_argument("--size")
    ap.add_argument("--cell")
    ap.add_argument("--frames", type=int)
    ap.add_argument("--hard-alpha", action="store_true")
    ap.add_argument("--outline")
    ap.add_argument("--base")
    ap.add_argument("--keep-cells", type=int, default=0)
    args = ap.parse_args()
    palette = read_palette(args.palette)
    total = 0
    for path in args.images:
        total += len(check(path, args, palette))
    print(f"{total} failed checks")
    sys.exit(1 if total else 0)


if __name__ == "__main__":
    main()
```
