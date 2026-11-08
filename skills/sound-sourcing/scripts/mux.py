#!/usr/bin/env python3
"""Put the soundtrack on the picture.

The video stream is copied. The audio is encoded to AAC 48 kHz stereo and cut
at the picture's end. The result is probed before it replaces anything.
Run with the hark venv Python and -I. --self-test runs the built-in checks.
"""
from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
from fractions import Fraction


class MuxError(Exception):
    pass


def say(*parts, file=None):
    text = " ".join(str(p) for p in parts)
    print(text.encode("ascii", "replace").decode("ascii"), file=file or sys.stdout)


def probe(path: str) -> dict:
    r = subprocess.run(["ffprobe", "-v", "error", "-show_streams", "-show_format", "-of", "json", path],
                       capture_output=True, text=True)
    if r.returncode != 0:
        raise MuxError(f"ffprobe failed on {path}: {r.stderr.strip()[:200]}")
    return json.loads(r.stdout or "{}")


def stream(info: dict, kind: str) -> dict | None:
    return next((s for s in info.get("streams", []) if s.get("codec_type") == kind
                 and not (s.get("disposition") or {}).get("attached_pic")), None)


def duration(info: dict, st: dict | None) -> float | None:
    for v in ((st or {}).get("duration"), (info.get("format") or {}).get("duration")):
        try:
            return float(v)
        except (TypeError, ValueError):
            continue
    return None


def fps_of(st: dict) -> float | None:
    for k in ("avg_frame_rate", "r_frame_rate"):
        try:
            f = Fraction(st.get(k) or "0/0")
        except (ValueError, ZeroDivisionError):
            continue
        if f > 0:
            return float(f)
    return None


def mux(a) -> list[str]:
    root = os.path.abspath(a.root)
    rp = lambda p: p if os.path.isabs(p) else os.path.join(root, p)
    video, audio = rp(a.video), rp(a.audio)
    out = rp(a.out) if a.out else video
    lines = []
    if not os.path.isfile(audio):
        raise MuxError(f"audio not found: {audio}")
    if not os.path.isfile(video):
        raise MuxError(f"video not found: {video}")
    vi = probe(video)
    vs = stream(vi, "video")
    if vs is None:
        raise MuxError(f"no video stream in {video}")
    fps = fps_of(vs) or 60.0
    vdur = duration(vi, vs)
    if vdur is None:
        raise MuxError(f"cannot read the duration of {video}")
    ai = probe(audio)
    adur = duration(ai, stream(ai, "audio"))
    frame = 1.0 / fps
    if adur is not None and abs(adur - vdur) > frame:
        d = adur - vdur
        if d > 0:
            lines.append(f"note: the audio is longer by {d:.3f} s ({d / frame:.1f} frames); it is cut at the "
                         "picture's end")
        else:
            lines.append(f"note: the audio is SHORTER by {-d:.3f} s ({-d / frame:.1f} frames); the end will be "
                         "silent")
    src = video
    keep = None
    if os.path.abspath(out) == os.path.abspath(video):
        stem, ext = os.path.splitext(os.path.basename(video))
        keep = rp(a.keep_old) if a.keep_old else os.path.join(root, "out", "superseded", f"{stem}_prev{ext}")
        if os.path.exists(keep):
            raise MuxError(f"the keep-old file exists: {keep}; move it or give another --keep-old")
        os.makedirs(os.path.dirname(keep), exist_ok=True)
        shutil.move(video, keep)
        src = keep
        lines.append(f"the previous video is at {keep}")
    stem, ext = os.path.splitext(out)
    tmp = f"{stem}.muxing{ext}"
    cmd = ["ffmpeg", "-v", "error", "-nostdin", "-y", "-i", src, "-i", audio, "-map", "0:v:0", "-map", "1:a:0",
           "-c:v", "copy", "-c:a", "aac", "-b:a", a.abr, "-ar", "48000", "-ac", "2", "-t", f"{vdur:.6f}"]
    if ext.lower() in (".mp4", ".m4v", ".mov"):
        cmd += ["-movflags", "+faststart"]
    cmd.append(tmp)
    os.makedirs(os.path.dirname(os.path.abspath(out)), exist_ok=True)
    r = subprocess.run(cmd, capture_output=True, text=True)
    where = f"; the original video is kept at {keep}" if keep else ""
    if r.returncode != 0:
        if os.path.exists(tmp):
            os.remove(tmp)
        raise MuxError(f"ffmpeg failed: {r.stderr.strip()[:300]}{where}")
    oi = probe(tmp)
    ov, oa = stream(oi, "video"), stream(oi, "audio")
    if ov is None or oa is None:
        os.remove(tmp)
        raise MuxError(f"the result lacks a video or an audio stream{where}")
    os.replace(tmp, out)
    mb = os.path.getsize(out) / 1e6
    abr = oa.get("bit_rate")
    abr = f"{int(abr) // 1000} kbps" if abr and str(abr).isdigit() else "? kbps"
    odur = duration(oi, oa)
    lines.append(f"wrote {out} ({mb:.2f} MB): video {ov.get('codec_name')} copied, {fps:.3f} fps, "
                 f"{duration(oi, ov) or vdur:.3f} s; audio {oa.get('codec_name')} {abr}, "
                 f"{odur if odur is not None else 0:.3f} s")
    return lines


def parse(argv):
    p = argparse.ArgumentParser(prog="mux.py", description="Put the soundtrack on the picture.")
    p.add_argument("--video", default="out/film.mp4")
    p.add_argument("--audio", default="out/audio/soundtrack.wav")
    p.add_argument("--out", help="output file (default: the video itself, after keeping the old one)")
    p.add_argument("--abr", default="320k")
    p.add_argument("--keep-old", help="where the old video goes when --out is the video")
    p.add_argument("--root", default=".")
    p.add_argument("--self-test", action="store_true")
    return p.parse_args(argv)


def main(argv=None) -> int:
    a = parse(sys.argv[1:] if argv is None else argv)
    if a.self_test:
        return self_test()
    try:
        for ln in mux(a):
            say(ln)
    except MuxError as e:
        say(f"mux: {e}", file=sys.stderr)
        return 1
    return 0


def self_test() -> int:
    import contextlib
    import io
    import tempfile
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    import fixtures as fx
    import numpy as np
    ck = fx.Checker()
    with tempfile.TemporaryDirectory() as d:
        film = os.path.join(d, "film.mp4")
        subprocess.run(["ffmpeg", "-v", "error", "-nostdin", "-y", "-f", "lavfi", "-i",
                        "testsrc=size=160x90:rate=30:duration=2", "-c:v", "libx264", "-pix_fmt", "yuv420p", film],
                       check=True)
        t = np.arange(int(2.5 * 48000)) / 48000
        tone = 0.3 * np.sin(2 * np.pi * 440 * t)
        wav = fx.write(os.path.join(d, "s.wav"), np.stack([tone, tone], 1))
        out = os.path.join(d, "film_sound.mp4")
        o = io.StringIO()
        with contextlib.redirect_stdout(o):
            code = main(["--video", film, "--audio", wav, "--out", out])
        info = probe(out)
        vs = [s for s in info["streams"] if s["codec_type"] == "video"]
        aus = [s for s in info["streams"] if s["codec_type"] == "audio"]
        ck("S16 note that the audio is longer and cut", code == 0 and "audio is longer" in o.getvalue())
        ck("S16 1 copied video stream, 1 AAC 48 kHz stereo stream",
           len(vs) == 1 and vs[0]["codec_name"] == "h264" and len(aus) == 1 and aus[0]["codec_name"] == "aac"
           and aus[0]["sample_rate"] == "48000" and aus[0]["channels"] == 2)
        ck("S16 audio duration within 1 AAC frame of 2.0 s", abs(float(aus[0]["duration"]) - 2.0) <= 1024 / 48000 + 1e-3)
        keep = os.path.join(d, "keep.mp4")
        open(keep, "wb").write(b"x")
        before = open(film, "rb").read()
        with contextlib.redirect_stderr(io.StringIO()) as e:
            code = main(["--video", film, "--audio", wav, "--out", film, "--keep-old", keep])
        ck("S17 an existing keep-old file refuses; the video is untouched",
           code == 1 and open(film, "rb").read() == before and "keep-old" in e.getvalue())
        os.remove(keep)
        with contextlib.redirect_stdout(io.StringIO()):
            code = main(["--video", film, "--audio", wav, "--keep-old", keep])
        ck("in-place mux keeps the old video", code == 0 and open(keep, "rb").read() == before
           and stream(probe(film), "audio") is not None)
        with contextlib.redirect_stderr(io.StringIO()):
            ck("missing audio fails", main(["--video", film, "--audio", os.path.join(d, "no.wav")]) == 1)
    return ck.code()


if __name__ == "__main__":
    sys.exit(main())
