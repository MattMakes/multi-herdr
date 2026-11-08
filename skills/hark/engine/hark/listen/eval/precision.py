"""bf16 against fp32 CLAP embedding agreement and speed (listen-eval E14).

    python -m hark.listen.eval.precision [--out DIR]

Each precision runs in its own process with ``HARK_PRECISION`` set, because core-io reads the
precision once. bf16 is used only where the CPU supports it natively (core-io decides).
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
from collections.abc import Sequence
from pathlib import Path

import numpy as np

from hark.core import audio
from hark.listen.eval import dataset as ds
from hark.listen.eval.run_eval import write_result

FOLDERS = ("ambience", "foley", "keyboard", "motion", "music", "toy")
PER_FOLDER = 2
CLIP_S = 10.0
BATCH = 4
PRECISIONS = ("fp32", "bf16")


def pick_clips(root: Path) -> list[str]:
    """The first 2 files of each of the 6 folders."""
    out = []
    for f in FOLDERS:
        files = [p for p, _ in ds.category_files(root, (f,))][:PER_FOLDER]
        if len(files) < PER_FOLDER:
            raise ds.EvalError(f"folder {f!r} needs {PER_FOLDER} recordings, has {len(files)}")
        out += files
    return out


def agreement(a: np.ndarray, b: np.ndarray) -> dict:
    """Mean and minimum cosine between the rows of a and b."""
    a = a / np.linalg.norm(a, axis=1, keepdims=True)
    b = b / np.linalg.norm(b, axis=1, keepdims=True)
    c = (a * b).sum(axis=1)
    return {"cos_mean": round(float(c.mean()), 5), "cos_min": round(float(c.min()), 5)}


def embed_child(paths: Sequence[str], dest: Path) -> None:
    """In the child process: embed the clips after one warm-up batch; write embeddings and seconds per window."""
    from hark.models import clap

    model = clap.get()
    clips = [np.asarray(audio.load(p, mono=True, end=CLIP_S), np.float32) for p in paths]
    model.embed_audio(clips[:BATCH], pad="repeatpad", batch=BATCH)
    t0 = time.perf_counter()
    emb = np.asarray(model.embed_audio(clips, pad="repeatpad", batch=BATCH), np.float64)
    sec = (time.perf_counter() - t0) / len(clips)
    np.save(dest.with_suffix(".npy"), emb)
    dest.with_suffix(".json").write_text(json.dumps({"s_per_window": sec}), encoding="utf-8")


def run(out: Path) -> dict:
    paths = pick_clips(ds.location("HARK_EVAL_ASSETS"))
    work = out / "precision_work"
    work.mkdir(parents=True, exist_ok=True)
    embs, secs = {}, {}
    for prec in PRECISIONS:
        dest = work / prec
        env = {**os.environ, "HARK_PRECISION": prec}
        subprocess.run([sys.executable, "-m", "hark.listen.eval.precision", "--child", str(dest), *paths],
                       env=env, check=True)
        embs[prec] = np.load(dest.with_suffix(".npy"))
        secs[prec] = json.loads(dest.with_suffix(".json").read_text())["s_per_window"]
    return {"clips": [Path(p).name for p in paths], **agreement(embs["fp32"], embs["bf16"]),
            "s_per_window": {k: round(v, 3) for k, v in secs.items()},
            "speedup": round(secs["fp32"] / secs["bf16"], 2) if secs["bf16"] else None}


def main(argv: Sequence[str] | None = None) -> int:
    ap = argparse.ArgumentParser(prog="python -m hark.listen.eval.precision", description=__doc__.splitlines()[0])
    ap.add_argument("--out", default=str(ds.HERE))
    ap.add_argument("--child", help=argparse.SUPPRESS)
    ap.add_argument("paths", nargs="*", help=argparse.SUPPRESS)
    args = ap.parse_args(argv)
    if args.child:
        embed_child(args.paths, Path(args.child))
        return 0
    try:
        result = run(Path(args.out))
    except ds.EvalError as exc:
        print(f"precision: error: {exc}", file=sys.stderr)
        return 1
    write_result(Path(args.out), "precision.json", result,
                 [f"cos mean {result['cos_mean']} min {result['cos_min']}, speed-up {result['speedup']}"])
    return 0


if __name__ == "__main__":
    sys.exit(main())
