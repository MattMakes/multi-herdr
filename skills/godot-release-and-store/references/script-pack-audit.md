# Pack audit script

Usage: `python3 pack_audit.py --forbid forbid.txt [--require require.txt] FILE [FILE ...]`

The script searches exported packs (`.pck`, `.zip`, the UID cache, or any binary file) for forbidden and required strings. A `.zip` is opened and each member is searched too. It exits 0 when no forbidden string is found and every required string is found in every file. Copy the block into a file named `pack_audit.py` before you run it. It uses the Python standard library only.

```python
#!/usr/bin/env python3
"""Usage: python3 pack_audit.py --forbid forbid.txt [--require require.txt] FILE [FILE ...]

Check exported packs (.pck, .zip, the UID cache, any binary file) for strings.
forbid.txt and require.txt hold 1 string per line; blank lines and lines that start
with '#' are ignored. A forbidden string is a path, an id or a dev folder that must
not ship. A required string is a data path that proves the data is inside.
A .zip file is opened and each member is searched too, so a zipped build works.
Unencrypted Godot packs store paths as plain text, so a byte search finds them.
Exit 0 when no forbidden string is found and every required string is found in
every file. Exit 1 otherwise. Standard library only.
"""
import argparse
import sys
import zipfile


def read_list(path):
    if not path:
        return []
    with open(path, encoding="utf-8") as f:
        return [s.strip() for s in f if s.strip() and not s.lstrip().startswith("#")]


def blobs(path):
    """Yield (label, bytes) for the file and, for a zip, for each member."""
    with open(path, "rb") as f:
        data = f.read()
    yield path, data
    if zipfile.is_zipfile(path):
        with zipfile.ZipFile(path) as z:
            names = "\n".join(z.namelist()).encode("utf-8")
            yield path + "!names", names
            for info in z.infolist():
                if not info.is_dir():
                    yield path + "!" + info.filename, z.read(info)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--forbid", required=True, help="file of forbidden strings")
    ap.add_argument("--require", help="file of required strings")
    ap.add_argument("files", nargs="+")
    args = ap.parse_args()
    forbid = read_list(args.forbid)
    require = read_list(args.require)
    failed = False
    for path in args.files:
        found_required = set()
        hits = []
        for label, data in blobs(path):
            for s in forbid:
                if s.encode("utf-8") in data:
                    hits.append((label, s))
            for s in require:
                if s.encode("utf-8") in data:
                    found_required.add(s)
        missing = [s for s in require if s not in found_required]
        for label, s in hits:
            print(f"FORBIDDEN {s!r} in {label}")
        for s in missing:
            print(f"MISSING   {s!r} in {path}")
        status = "FAIL" if hits or missing else "OK"
        print(f"{status} {path}: {len(hits)} forbidden, {len(require) - len(missing)}/{len(require)} required")
        failed = failed or bool(hits or missing)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
```
