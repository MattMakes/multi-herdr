#!/usr/bin/env python3
"""Measure downloaded recordings: format, level, floor, bandwidth, onsets, spikes.

With --music it also reads tempo, key, bars and sections. With --png it draws
one spectrogram per file. Every number comes from the decoded audio.
Run with the hark venv Python and -I. --self-test runs the built-in checks.
"""
from __future__ import annotations

import argparse
import json
import math
import os
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import numpy as np  # noqa: E402

import master as mst  # noqa: E402
import mix  # noqa: E402

SR = 48000
FEAT_SR = 22050
N_FFT, HOP = 2048, 512
KEYS = ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"]
# Krumhansl-Kessler key profiles, index 0 = the tonic.
KK_MAJOR = np.array([6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88])
KK_MINOR = np.array([6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17])


def say(*parts, file=None):
    text = " ".join(str(p) for p in parts)
    print(text.encode("ascii", "replace").decode("ascii"), file=file or sys.stdout)


def dbv(x: float) -> float:
    return round(mst.db(float(x)), 2)


def _kbps(*rates):
    """The first plausible bit rate in kbps, or None."""
    def one(br):
        try:
            v = float(br)
        except (TypeError, ValueError):
            return None
        return round(v / 1000.0, 1) if 1 <= v <= 10_000_000 else None
    return next((k for k in map(one, rates) if k is not None), None)


def probe(path: str) -> dict:
    r = subprocess.run(["ffprobe", "-v", "error", "-select_streams", "a:0", "-show_entries",
                        "stream=codec_name,sample_rate,channels,bit_rate:format=bit_rate", "-of", "json", path],
                       capture_output=True, text=True)
    if r.returncode != 0:
        return {"probe_error": (r.stderr.strip().splitlines() or ["ffprobe failed"])[-1]}
    data = json.loads(r.stdout or "{}")
    st = (data.get("streams") or [{}])[0]
    if not st:
        return {"probe_error": "no audio stream"}
    fmt = data.get("format") or {}
    return {"codec": st.get("codec_name"), "src_rate": int(st.get("sample_rate") or 0),
            "channels": int(st.get("channels") or 0), "kbps": _kbps(st.get("bit_rate"), fmt.get("bit_rate"))}


def frame_rms(x: np.ndarray, n: int) -> np.ndarray:
    m = len(x) // n
    if m == 0:
        return np.array([math.sqrt(float(np.mean(x ** 2))) if len(x) else 0.0])
    return np.sqrt(np.mean(x[:m * n].reshape(m, n) ** 2, axis=1))


def band_rel(mono: np.ndarray) -> dict:
    seg = mono[:60 * SR]
    spec = np.abs(np.fft.rfft(seg * np.hanning(len(seg)))) ** 2
    freqs = np.fft.rfftfreq(len(seg), 1 / SR)

    def power(lo, hi):
        sel = (freqs >= lo) & (freqs < hi)
        return float(np.mean(spec[sel])) if sel.any() else 0.0
    ref = power(1000, 4000) + 1e-30
    out = {}
    for name, lo, hi in (("40-120", 40, 120), ("8-12k", 8000, 12000), ("12-16k", 12000, 16000),
                         ("16-19k", 16000, 19000), ("19-22k", 19000, 22000)):
        out[name] = round(10 * math.log10(power(lo, hi) / ref + 1e-30), 1)
    return out


def key_estimate(y: np.ndarray, sr: int) -> list[dict]:
    import librosa
    chroma = librosa.feature.chroma_cqt(y=y, sr=sr, hop_length=HOP).mean(axis=1)
    scores = []
    for tonic in range(12):
        for mode, prof in (("major", KK_MAJOR), ("minor", KK_MINOR)):
            r = float(np.corrcoef(chroma, np.roll(prof, tonic))[0, 1])
            scores.append({"key": f"{KEYS[tonic]} {mode}", "r": round(r if math.isfinite(r) else 0.0, 3)})
    return sorted(scores, key=lambda s: -s["r"])[:3]


def music_features(y: np.ndarray, sr: int, dur: float) -> dict:
    import librosa
    out = {}
    tempo, beats = librosa.beat.beat_track(y=y, sr=sr, hop_length=HOP, start_bpm=120, tightness=200)
    bt = librosa.frames_to_time(beats, sr=sr, hop_length=HOP)
    out["tempo"] = round(float(np.atleast_1d(tempo)[0]), 2)
    out["beats"] = [round(float(t), 3) for t in bt]
    if len(bt) > 4:
        ibi = np.diff(bt)
        out["tempo_from_ibi"] = round(60.0 / float(np.median(ibi)), 2)
        out["ibi_std_ms"] = round(float(np.std(ibi)) * 1000, 1)
    out["key"] = key_estimate(y, sr)
    blocks = frame_rms(y, 2 * sr)
    out["rms_db_per_2s"] = [dbv(v) for v in blocks]
    if len(bt) > 8:
        starts = bt[::4]
        out["bar_starts_if_beat0_is_downbeat"] = [round(float(t), 3) for t in starts]
        ends = list(starts[1:]) + [dur]
        out["rms_db_per_bar"] = [dbv(math.sqrt(float(np.mean(y[int(a * sr):max(int(a * sr) + 1, int(b * sr))] ** 2))))
                                 for a, b in zip(starts, ends)]
    try:
        chroma = librosa.feature.chroma_cqt(y=y, sr=sr, hop_length=HOP)
        mfcc = librosa.feature.mfcc(y=y, sr=sr, n_mfcc=13, hop_length=HOP)
        n = min(chroma.shape[1], mfcc.shape[1])
        feat = np.vstack([chroma[:, :n], mfcc[:, :n]])
        feat = (feat - feat.mean(axis=1, keepdims=True)) / (feat.std(axis=1, keepdims=True) + 1e-9)
        k = int(min(12, max(4, dur // 15)))
        bounds = librosa.segment.agglomerative(feat, k)
        out["sections_at"] = [round(float(t), 2) for t in librosa.frames_to_time(bounds, sr=sr, hop_length=HOP)]
    except Exception as e:  # segmentation is advice, never fatal
        out["sections_error"] = str(e)[:200]
    return out


def spectrogram(path: str, folder: str) -> dict:
    os.makedirs(folder, exist_ok=True)
    png = os.path.join(folder, os.path.splitext(os.path.basename(path))[0] + "_spec.png")
    r = subprocess.run(["ffmpeg", "-v", "error", "-nostdin", "-y", "-i", path, "-lavfi",
                        "showspectrumpic=s=1400x420:mode=combined:color=intensity:scale=log:legend=1", png],
                       capture_output=True, text=True)
    if r.returncode != 0:
        return {"png_error": (r.stderr.strip().splitlines() or ["ffmpeg failed"])[-1][:200]}
    return {"png": png}


def _load(path: str, res: dict):
    """Probe and decode path into res. Returns the samples, or None with res["error"] set."""
    if not os.path.isfile(path):
        res["error"] = "file not found"
        return None
    res.update(probe(path))
    try:
        x = mix.decode(path).astype(np.float64)
    except mix.DesignError as e:
        res["error"] = f"decode failed: {e}"
        return None
    res["duration"] = round(len(x) / SR, 3)
    return x


def _levels(x: np.ndarray, mono: np.ndarray, peak: float, dur: float) -> dict:
    lufs = mst.integrated(x) if dur > 0.45 else None
    r20 = frame_rms(mono, int(0.02 * SR))
    floor, loud = dbv(np.percentile(r20, 10)), dbv(np.max(r20))
    return {"peak_dbfs": dbv(peak), "rms_dbfs": dbv(math.sqrt(float(np.mean(x ** 2)))),
            "clipped_samples": int(np.count_nonzero(np.abs(x) >= 0.999)), "dc": round(float(np.mean(mono)), 6),
            "lufs": round(lufs, 2) if lufs is not None and math.isfinite(lufs) else None,
            "noise_floor_dbfs": floor, "loudest_20ms_dbfs": loud, "dynamic_range_db": round(loud - floor, 2)}


def _stereo(x: np.ndarray) -> dict:
    side = (x[:, 0] - x[:, 1]) / 2
    if not np.any(side != 0):
        return {"side_to_mid_db": None}
    mid = (x[:, 0] + x[:, 1]) / 2
    return {"side_to_mid_db": round(dbv(math.sqrt(float(np.mean(side ** 2))))
                                    - dbv(math.sqrt(float(np.mean(mid ** 2)))), 2)}


def _envelope(mono: np.ndarray) -> dict:
    r1 = frame_rms(mono, int(0.001 * SR))
    attack = round(float(np.argmax(r1 >= 0.1 * r1.max())) * 0.001, 4) if r1.max() > 0 else None
    return {"attack_at": attack, "peak_at": round(float(np.argmax(r1)) * 0.001, 4)}


def _spectral(y: np.ndarray, audible: bool) -> tuple[dict, np.ndarray]:
    """Centroid and rolloff of the feature signal, and its onset times."""
    import librosa
    if len(y) < N_FFT or not audible:
        return {"centroid_hz": None, "rolloff95_hz": None}, np.array([])
    S = np.abs(librosa.stft(y, n_fft=N_FFT, hop_length=HOP))
    cen = librosa.feature.spectral_centroid(S=S, sr=FEAT_SR)[0]
    roll = librosa.feature.spectral_rolloff(S=S, sr=FEAT_SR, roll_percent=0.95)[0]
    live = S.sum(axis=0) > 1e-6
    if live.any():
        cen, roll = cen[live], roll[live]
    on = librosa.onset.onset_detect(y=y, sr=FEAT_SR, hop_length=HOP, backtrack=False, units="time")
    return {"centroid_hz": round(float(np.median(cen)), 1),
            "rolloff95_hz": round(float(np.percentile(roll, 90)), 1)}, on


def _spikes(mono: np.ndarray) -> dict:
    w = frame_rms(mono, int(0.1 * SR))
    med = float(np.median(w)) if len(w) else 0.0
    spikes = [round(i * 0.1, 1) for i, v in enumerate(w) if med > 0 and v > 4 * med]
    std = round(float(np.std(20 * np.log10(w + 1e-10))), 2) if len(w) > 1 else 0.0
    return {"spikes_over_12db": spikes[:30], "level_std_db_100ms": std}


def analyse(path: str, music: bool = False, png_dir: str | None = None) -> dict:
    res = {"file": path}
    x = _load(path, res)
    if x is None:
        return res
    dur = len(x) / SR
    if dur < 0.02:
        return {"file": path, "error": "too short"}
    mono = x.mean(axis=1)
    peak = float(np.max(np.abs(x)))
    res.update(_levels(x, mono, peak, dur))
    res.update(_stereo(x))
    res.update(_envelope(mono))
    import soxr
    y = soxr.resample(mono.astype(np.float32), SR, FEAT_SR).astype(np.float32)
    spec, on = _spectral(y, peak > 0)
    res.update(spec)
    res["band_db_rel_1_4k"] = band_rel(mono)
    res["onset_count"] = int(len(on))
    res["onsets"] = [round(float(t), 3) for t in on[:40]]
    res.update(_spikes(mono))
    if music:
        res.update(music_features(y, FEAT_SR, dur))
    if png_dir:
        res.update(spectrogram(path, png_dir))
    res["summary"] = line(res, music)
    return res


def _format_parts(r: dict) -> list[str]:
    fmt = f"{r.get('codec')} {r.get('kbps') or '?'} kbps {(r.get('src_rate') or 0) / 1000:g} kHz {r.get('channels')}ch"
    lufs = f"{r['lufs']:.1f} LUFS" if r.get("lufs") is not None else "LUFS n.a."
    stereo = "mono" if r.get("side_to_mid_db") is None else f"side/mid {r['side_to_mid_db']:.1f} dB"
    return [os.path.basename(r["file"]), fmt, f"{r['duration']:.2f} s", f"peak {r['peak_dbfs']:.1f}", lufs,
            f"floor {r['noise_floor_dbfs']:.1f}", f"attack {r['attack_at']} s",
            f"centroid {r.get('centroid_hz')} Hz", f"rolloff95 {r.get('rolloff95_hz')} Hz", stereo]


def _music_parts(r: dict) -> list[str]:
    parts = [f"{r.get('tempo_from_ibi') or r.get('tempo')} BPM"]
    if "ibi_std_ms" in r:
        parts.append(f"beat spread {r['ibi_std_ms']} ms")
    if r.get("key"):
        parts.append(r["key"][0]["key"])
    return parts


def _is_dull(r: dict, silent: bool) -> bool:
    roll = r.get("rolloff95_hz")
    return not silent and roll is not None and roll < 8000


def _checks(r: dict) -> list[str]:
    silent = r["peak_dbfs"] <= -90
    tests = [(silent, "digital silence"),
             (r["clipped_samples"] > 0, f"clipped samples ({r['clipped_samples']})"),
             (r.get("codec") == "mp3" and (r.get("kbps") or 999) < 192, "low-bitrate mp3"),
             ((r.get("src_rate") or 44100) < 44100, "low sample rate"),
             (_is_dull(r, silent), "dull"),
             (bool(r["spikes_over_12db"]), f"spikes ({len(r['spikes_over_12db'])})")]
    return [label for cond, label in tests if cond]


def line(r: dict, music: bool) -> str:
    parts = _format_parts(r) + (_music_parts(r) if music else [])
    checks = _checks(r)
    text = ", ".join(parts)
    return text + ("; CHECK: " + ", ".join(checks) if checks else "")


def parse(argv):
    p = argparse.ArgumentParser(prog="analyze.py", description="Measure downloaded recordings.")
    p.add_argument("files", nargs="*", metavar="FILE")
    p.add_argument("--music", action="store_true", help="also tempo, key, bars and sections")
    p.add_argument("--png", metavar="DIR", help="write one spectrogram per file")
    p.add_argument("--out", metavar="OUT.json")
    p.add_argument("--json", action="store_true")
    p.add_argument("--self-test", action="store_true")
    return p.parse_args(argv)


def _show(results: list, as_json: bool) -> None:
    if as_json:
        say(json.dumps(results, indent=1))
        return
    for r in results:
        say(r["summary"] if "summary" in r else f"{os.path.basename(r['file'])}: {r['error']}")


def _status(results: list, failed: list) -> str:
    msg = f"{len(results)} file(s) analysed, {len(failed)} failed"
    if failed:
        msg += " (" + ", ".join(os.path.basename(r["file"]) for r in failed[:6]) + ")"
    return msg


def _write_out(out: str, results: list) -> str:
    os.makedirs(os.path.dirname(os.path.abspath(out)), exist_ok=True)
    with open(out, "w", encoding="utf-8") as fh:
        json.dump(results, fh, indent=1)
    return f"; wrote {out}"


def _close(a, results: list, failed: list) -> None:
    """Write --out and print the status line (not with --json)."""
    msg = _status(results, failed) + (_write_out(a.out, results) if a.out else "")
    if not a.json:
        say(msg)


def main(argv=None) -> int:
    a = parse(sys.argv[1:] if argv is None else argv)
    if a.self_test:
        return self_test()
    if not a.files:
        say("analyze.py: give at least 1 file", file=sys.stderr)
        return 2
    results = [analyse(f, a.music, a.png) for f in a.files]
    failed = [r for r in results if r.get("error")]
    _show(results, a.json)
    _close(a, results, failed)
    return 1 if failed else 0


def _test_mono(ck, fx, d: str) -> str:
    x = np.zeros(SR)
    b = fx.burst(0.1, 900, 40, 1, 0.8)[:, 0]
    x[int(0.25 * SR):int(0.25 * SR) + len(b)] = b
    mono = fx.write(os.path.join(d, "mono.wav"), x, subtype="FLOAT")
    r = analyse(mono, png_dir=os.path.join(d, "png"))
    ck("S9 mono: 1 channel, side_to_mid null", r["channels"] == 1 and r["side_to_mid_db"] is None)
    ck("S9 attack at 0.25 s +-2 ms", abs(r["attack_at"] - 0.25) <= 0.002)
    ck("S9 peak equals the burst peak (no 3 dB loss)", abs(r["peak_dbfs"] - dbv(np.max(np.abs(b)))) < 0.05)
    ck("spectrogram written", os.path.isfile(r.get("png", "")))
    short = fx.write(os.path.join(d, "short.wav"), np.zeros(int(0.01 * SR)) + 0.1)
    ck("S10 a 10 ms file is too short, exit 1",
       analyse(short).get("error") == "too short" and main([short]) == 1)
    return mono


def _test_bed(ck, fx, d: str) -> None:
    bed = fx.noise_bed(3.0, 0.05)
    bed[int(1.5 * SR):int(1.6 * SR)] *= 20
    r = analyse(fx.write(os.path.join(d, "amb.wav"), bed))
    spikes = r["spikes_over_12db"]
    ck("a loud event in a bed is a spike", bool(spikes) and abs(spikes[0] - 1.5) < 0.11
       and r["side_to_mid_db"] is not None and "CHECK: " in r["summary"])


def _test_music(ck, fx, d: str) -> None:
    r = analyse(fx.write(os.path.join(d, "song.wav"), fx.click_track(120, 12)), music=True)
    ck("music: tempo near 120 and a key", abs((r.get("tempo_from_ibi") or 0) - 120) < 4 and len(r["key"]) == 3)
    ck("music: bars and sections", len(r.get("bar_starts_if_beat0_is_downbeat", [])) > 2 and
       ("sections_at" in r or "sections_error" in r))


def self_test() -> int:
    import tempfile
    import fixtures as fx
    ck = fx.Checker()
    with tempfile.TemporaryDirectory() as d:
        mono = _test_mono(ck, fx, d)
        _test_bed(ck, fx, d)
        _test_music(ck, fx, d)
        out = os.path.join(d, "o", "a.json")
        ck("--out writes the list", main([mono, "--out", out]) == 0 and len(json.load(open(out))) == 1)
    return ck.code()


if __name__ == "__main__":
    sys.exit(main())
