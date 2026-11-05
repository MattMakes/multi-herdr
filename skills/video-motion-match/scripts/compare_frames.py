#!/usr/bin/env python3
"""Compare reference keyframes with rendered keyframes.

Usage:
  compare_frames.py --ref <dir-or-video> --rendered <dir-or-video>
                    [--spec motion-spec.json] [--out qa/motion-match-check.json]
                    [--diff-strip diff-strip.png] [--samples 8]
  compare_frames.py --self-test

A directory holds PNG or JPEG frames in name order. A video is sampled with
ffmpeg. Frames pair by normalized time tau in [0, 1]. The gate fails when the
mean SSIM < 0.85 or the mean foreground bounding-box IoU < 0.85.

Exit codes: 0 pass, 1 gate fail, 2 usage or input error.
Needs only the Python standard library plus ffmpeg and ffprobe on PATH.
"""

import argparse
import json
import re
import subprocess
import sys
import tempfile
from collections import Counter
from pathlib import Path

SSIM_MIN = 0.85
IOU_MIN = 0.85
ANALYSIS_WIDTH_MAX = 480
MISMATCH_DELTA = 25  # a pixel differs when |ref - rendered| > 25 of 255
LUMA_DELTA = 24  # a pixel is foreground when |luma - background| > 24 of 255
FRAME_EXTS = {".png", ".jpg", ".jpeg"}


class InputError(Exception):
    pass


def run(cmd, capture_bytes=False):
    proc = subprocess.run(cmd, capture_output=True)
    if proc.returncode != 0:
        tail = proc.stderr.decode("utf-8", "replace")[-600:]
        raise InputError(f"command failed: {' '.join(cmd[:4])} ...\n{tail}")
    return proc if capture_bytes else proc.stderr.decode("utf-8", "replace")


def ffmpeg(*args):
    return ["ffmpeg", "-hide_banner", "-loglevel", "info", "-y", *args]


def probe(path):
    out = subprocess.run(
        ["ffprobe", "-v", "error", "-select_streams", "v:0",
         "-show_entries", "stream=width,height,r_frame_rate:format=duration",
         "-of", "json", str(path)],
        capture_output=True, text=True)
    if out.returncode != 0:
        raise InputError(f"ffprobe cannot read {path}")
    data = json.loads(out.stdout)
    stream = data["streams"][0]
    num, _, den = stream.get("r_frame_rate", "0/1").partition("/")
    fps = float(num) / float(den or 1) if float(den or 1) else 0.0
    duration = float(data.get("format", {}).get("duration") or 0.0)
    return int(stream["width"]), int(stream["height"]), fps, duration


class Source:
    """A frame directory or a video file, sampled by tau in [0, 1]."""

    def __init__(self, path):
        self.path = Path(path)
        if self.path.is_dir():
            self.frames = sorted(p for p in self.path.iterdir()
                                 if p.suffix.lower() in FRAME_EXTS)
            if not self.frames:
                raise InputError(f"no PNG or JPEG frames in {self.path}")
            self.width, self.height, _, _ = probe(self.frames[0])
        elif self.path.is_file():
            self.frames = None
            self.width, self.height, self.fps, self.duration = probe(self.path)
        else:
            raise InputError(f"not found: {self.path}")

    def default_taus(self, samples):
        count = len(self.frames) if self.frames else samples
        if count <= 1:
            return [0.0]
        return [i / (count - 1) for i in range(count)]

    def input_args(self, tau):
        """Return (ffmpeg input args, label) for the frame at tau."""
        if self.frames:
            idx = round(tau * (len(self.frames) - 1))
            return ["-i", str(self.frames[idx])], self.frames[idx].name
        step = 1.0 / self.fps if self.fps > 0 else 0.0
        t = tau * max(self.duration - step, 0.0)
        return ["-ss", f"{t:.4f}", "-i", str(self.path)], f"{self.path.name}@{t:.3f}s"


def normalize(src, tau, dst, width, height):
    args, label = src.input_args(tau)
    run(ffmpeg(*args, "-frames:v", "1",
               "-vf", f"scale={width}:{height}:flags=area,format=rgba", str(dst)))
    return label


def read_ya8(png, width, height):
    proc = run(ffmpeg("-i", str(png), "-f", "rawvideo", "-pix_fmt", "ya8", "-"),
               capture_bytes=True)
    raw = proc.stdout
    if len(raw) != width * height * 2:
        raise InputError(f"unexpected raw size for {png}")
    return raw[0::2], raw[1::2]


def ssim(a_png, b_png):
    log = run(ffmpeg("-i", str(a_png), "-i", str(b_png), "-lavfi",
                     "[0:v]format=gray[a];[1:v]format=gray[b];[a][b]ssim",
                     "-f", "null", "-"))
    match = re.search(r"All:\s*([0-9.]+)", log)
    if not match:
        raise InputError("ffmpeg ssim printed no score")
    return float(match.group(1))


def foreground_bbox(luma, alpha, width, height):
    """Normalized [x, y, w, h] of the foreground, or None when empty."""
    if min(alpha) < 255:
        mask = [a > 127 for a in alpha]
    else:
        border = list(luma[:width]) + list(luma[-width:])
        border += [luma[r * width] for r in range(height)]
        border += [luma[r * width + width - 1] for r in range(height)]
        bg = Counter(border).most_common(1)[0][0]
        mask = [abs(v - bg) > LUMA_DELTA for v in luma]
    xs0, ys0, xs1, ys1 = width, height, -1, -1
    for row in range(height):
        line = mask[row * width:(row + 1) * width]
        if not any(line):
            continue
        first = line.index(True)
        last = width - 1 - line[::-1].index(True)
        xs0, xs1 = min(xs0, first), max(xs1, last)
        ys0 = min(ys0, row)
        ys1 = row
    if xs1 < 0:
        return None
    return [xs0 / width, ys0 / height, (xs1 - xs0 + 1) / width, (ys1 - ys0 + 1) / height]


def iou(a, b):
    if a is None and b is None:
        return 1.0
    if a is None or b is None:
        return 0.0
    ix = max(0.0, min(a[0] + a[2], b[0] + b[2]) - max(a[0], b[0]))
    iy = max(0.0, min(a[1] + a[3], b[1] + b[3]) - max(a[1], b[1]))
    inter = ix * iy
    union = a[2] * a[3] + b[2] * b[3] - inter
    return inter / union if union > 0 else 0.0


def pixel_scores(a, b):
    total = len(a)
    abs_sum = 0
    differ = 0
    for x, y in zip(a, b):
        d = abs(x - y)
        abs_sum += d
        differ += d > MISMATCH_DELTA
    return 1.0 - abs_sum / (255.0 * total), differ / total


def load_spec(path):
    spec = json.loads(Path(path).read_text())
    keyframes = spec.get("keyframes")
    if not isinstance(keyframes, list) or not keyframes:
        raise InputError("motion-spec.json needs a non-empty keyframes list")
    for kf in keyframes:
        tau = kf.get("tau")
        if not isinstance(tau, (int, float)) or not 0.0 <= tau <= 1.0:
            raise InputError(f"keyframe tau must be in [0, 1]: {kf}")
        box = kf.get("foreground_bbox")
        if box is not None and (not isinstance(box, list) or len(box) != 4):
            raise InputError(f"foreground_bbox must be [x, y, w, h]: {kf}")
    return keyframes


def write_diff_strip(pairs, out_png):
    """One row per keyframe: reference | rendered | absolute difference."""
    inputs, chains, rows = [], [], []
    for i, (ref_png, ren_png) in enumerate(pairs):
        inputs += ["-i", str(ref_png), "-i", str(ren_png)]
        a, b = 2 * i, 2 * i + 1
        chains.append(
            f"[{a}:v]format=rgb24,split[r{i}][rd{i}];"
            f"[{b}:v]format=rgb24,split[c{i}][cd{i}];"
            f"[rd{i}][cd{i}]blend=all_mode=difference[d{i}];"
            f"[r{i}][c{i}][d{i}]hstack=inputs=3[row{i}]")
        rows.append(f"[row{i}]")
    if len(rows) == 1:
        graph = ";".join(chains) + f";{rows[0]}null[out]"
    else:
        graph = ";".join(chains) + f";{''.join(rows)}vstack=inputs={len(rows)}[out]"
    Path(out_png).parent.mkdir(parents=True, exist_ok=True)
    run(ffmpeg(*inputs, "-filter_complex", graph, "-map", "[out]",
               "-frames:v", "1", str(out_png)))


def compare(ref_path, rendered_path, spec_path=None, out=None, diff_strip=None,
            samples=8):
    ref = Source(ref_path)
    ren = Source(rendered_path)
    keyframes = load_spec(spec_path) if spec_path else [
        {"tau": t} for t in ref.default_taus(samples)]
    width = min(ref.width, ANALYSIS_WIDTH_MAX)
    height = max(2, round(ref.height * width / ref.width / 2) * 2)

    frames, pairs = [], []
    with tempfile.TemporaryDirectory() as tmp:
        work = Path(tmp)
        for i, kf in enumerate(keyframes):
            tau = float(kf["tau"])
            ref_png, ren_png = work / f"ref_{i:04d}.png", work / f"ren_{i:04d}.png"
            ref_label = normalize(ref, tau, ref_png, width, height)
            ren_label = normalize(ren, tau, ren_png, width, height)
            ref_luma, ref_alpha = read_ya8(ref_png, width, height)
            ren_luma, ren_alpha = read_ya8(ren_png, width, height)
            accuracy, mismatch = pixel_scores(ref_luma, ren_luma)
            ref_box = kf.get("foreground_bbox") or foreground_bbox(
                ref_luma, ref_alpha, width, height)
            ren_box = foreground_bbox(ren_luma, ren_alpha, width, height)
            frames.append({
                "tau": round(tau, 6),
                "ref": ref_label,
                "rendered": ren_label,
                "ssim": round(ssim(ref_png, ren_png), 6),
                "pixel_accuracy": round(accuracy, 6),
                "mismatch_ratio": round(mismatch, 6),
                "ref_bbox": [round(v, 6) for v in ref_box] if ref_box else None,
                "ref_bbox_source": "spec" if kf.get("foreground_bbox") else "pixels",
                "rendered_bbox": [round(v, 6) for v in ren_box] if ren_box else None,
                "iou": round(iou(ref_box, ren_box), 6),
            })
            pairs.append((ref_png, ren_png))
        if diff_strip:
            write_diff_strip(pairs, diff_strip)

    count = len(frames)
    mean_ssim = sum(f["ssim"] for f in frames) / count
    mean_iou = sum(f["iou"] for f in frames) / count
    failures = []
    if mean_ssim < SSIM_MIN:
        failures.append(f"mean SSIM {mean_ssim:.4f} < {SSIM_MIN}")
    if mean_iou < IOU_MIN:
        failures.append(f"mean IoU {mean_iou:.4f} < {IOU_MIN}")
    report = {
        "schema_version": 1,
        "pass": not failures,
        "thresholds": {"mean_ssim_min": SSIM_MIN, "mean_iou_min": IOU_MIN},
        "analysis_size": [width, height],
        "keyframes": count,
        "mean_ssim": round(mean_ssim, 6),
        "mean_iou": round(mean_iou, 6),
        "mean_pixel_accuracy": round(sum(f["pixel_accuracy"] for f in frames) / count, 6),
        "mean_mismatch_ratio": round(sum(f["mismatch_ratio"] for f in frames) / count, 6),
        "max_mismatch_ratio": max(f["mismatch_ratio"] for f in frames),
        "worst_tau": min(frames, key=lambda f: f["ssim"])["tau"],
        "diff_strip": str(diff_strip) if diff_strip else None,
        "failures": failures,
        "frames": frames,
    }
    if out:
        Path(out).parent.mkdir(parents=True, exist_ok=True)
        Path(out).write_text(json.dumps(report, indent=2) + "\n")
    return report


# ---------------------------------------------------------------- self-test

SKILL_BUDGET_BYTES = 12288
SKILLS = ("video-motion", "video-character", "video-motion-match")
TEXT_ONLY = ("video-character",)


def make_frames(dirpath, xs, y=60, size=(320, 180), noise=False):
    dirpath.mkdir(parents=True)
    w, h = size
    for i, x in enumerate(xs):
        vf = f"drawbox=x={x}:y={y}:w=80:h=60:color=0xF2C14E:t=fill"
        if noise:
            vf += ",noise=alls=3:allf=t"
        subprocess.run(
            ffmpeg("-f", "lavfi", "-i", f"color=c=0x14213D:s={w}x{h}:d=1",
                   "-vf", vf, "-frames:v", "1", str(dirpath / f"f{i:03d}.png")),
            check=True, capture_output=True)


def self_test():
    script = Path(__file__).resolve()
    skills_root = script.parents[2]
    checks = []

    def check(name, ok, detail=""):
        checks.append(ok)
        print(f"{'PASS' if ok else 'FAIL'} {name}{': ' + detail if detail else ''}")

    for skill in SKILLS:
        md = skills_root / skill / "SKILL.md"
        size = md.stat().st_size if md.is_file() else -1
        check(f"{skill}/SKILL.md < {SKILL_BUDGET_BYTES} bytes",
              0 < size < SKILL_BUDGET_BYTES, f"{size} bytes")
    for skill in TEXT_ONLY:
        bad = [str(p.relative_to(skills_root)) for p in (skills_root / skill).rglob("*")
               if p.is_file() and p.suffix not in (".md", ".txt")]
        check(f"{skill} holds only .md and .txt files", not bad, ", ".join(bad))

    def cli(*args):
        return subprocess.run([sys.executable, str(script), *map(str, args)],
                              capture_output=True, text=True)

    with tempfile.TemporaryDirectory() as tmp:
        t = Path(tmp)
        xs = [40, 70, 100, 130, 160]
        make_frames(t / "ref", xs)
        make_frames(t / "near", [x + 1 for x in xs], noise=True)
        make_frames(t / "shifted", [x + 120 for x in xs], y=100)

        out, strip = t / "qa" / "motion-match-check.json", t / "qa" / "diff-strip.png"
        proc = cli("--ref", t / "ref", "--rendered", t / "near",
                   "--out", out, "--diff-strip", strip)
        report = json.loads(out.read_text()) if out.is_file() else {}
        check("near-identical frames exit 0", proc.returncode == 0, proc.stderr[-300:])
        check("near-identical mean SSIM >= 0.85",
              report.get("mean_ssim", 0) >= SSIM_MIN, str(report.get("mean_ssim")))
        check("near-identical mean IoU >= 0.85",
              report.get("mean_iou", 0) >= IOU_MIN, str(report.get("mean_iou")))
        check("diff-strip.png written", strip.is_file() and strip.stat().st_size > 0)

        out2 = t / "qa" / "shifted.json"
        proc = cli("--ref", t / "ref", "--rendered", t / "shifted", "--out", out2)
        report = json.loads(out2.read_text()) if out2.is_file() else {}
        check("shifted frames exit 1", proc.returncode == 1, f"exit {proc.returncode}")
        check("shifted report fails", report.get("pass") is False,
              "; ".join(report.get("failures", [])))

        video = t / "ref.mkv"
        subprocess.run(ffmpeg("-framerate", "5", "-i", str(t / "ref" / "f%03d.png"),
                              "-c:v", "ffv1", str(video)),
                       check=True, capture_output=True)
        proc = cli("--ref", video, "--rendered", t / "near", "--samples", "5")
        check("video reference against frame dir exits 0",
              proc.returncode == 0, proc.stderr[-300:])

        spec = t / "motion-spec.json"
        spec.write_text(json.dumps({"schema_version": 1, "keyframes": [
            {"tau": 0.0, "foreground_bbox": [40 / 320, 60 / 180, 80 / 320, 60 / 180]},
            {"tau": 1.0, "foreground_bbox": [160 / 320, 60 / 180, 80 / 320, 60 / 180]},
        ]}))
        proc = cli("--ref", t / "ref", "--rendered", t / "near", "--spec", spec)
        check("spec boxes with matching frames exit 0", proc.returncode == 0,
              proc.stderr[-300:])
        proc = cli("--ref", t / "ref", "--rendered", t / "shifted", "--spec", spec)
        check("spec boxes with shifted frames exit 1", proc.returncode == 1,
              f"exit {proc.returncode}")

        proc = cli("--ref", t / "missing", "--rendered", t / "near")
        check("missing input exits 2", proc.returncode == 2, f"exit {proc.returncode}")

    passed = sum(checks)
    print(f"self-test: {passed} of {len(checks)} checks pass")
    return 0 if passed == len(checks) else 1


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--ref", help="reference frame directory or video")
    parser.add_argument("--rendered", help="rendered frame directory or video")
    parser.add_argument("--spec", help="motion-spec.json with keyframe taus and boxes")
    parser.add_argument("--out", help="write the JSON report here")
    parser.add_argument("--diff-strip", help="write a reference|rendered|diff PNG here")
    parser.add_argument("--samples", type=int, default=8,
                        help="keyframes to sample from a video reference (default 8)")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args(argv)
    if args.self_test:
        return self_test()
    if not args.ref or not args.rendered:
        parser.print_usage(sys.stderr)
        print("error: --ref and --rendered are required", file=sys.stderr)
        return 2
    if args.samples < 1:
        print("error: --samples must be >= 1", file=sys.stderr)
        return 2
    try:
        report = compare(args.ref, args.rendered, args.spec, args.out,
                         args.diff_strip, args.samples)
    except (InputError, ValueError, KeyError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    summary = {k: report[k] for k in ("pass", "mean_ssim", "mean_iou",
                                      "mean_pixel_accuracy", "max_mismatch_ratio",
                                      "worst_tau", "failures")}
    print(json.dumps(summary, indent=2))
    return 0 if report["pass"] else 1


if __name__ == "__main__":
    sys.exit(main())
