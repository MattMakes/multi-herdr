"""Audio I/O: decode, cache, probe, ids, windows and paths (core-io 3.2-3.4, 4.1-4.3)."""

from __future__ import annotations

import os
import subprocess
from pathlib import Path

import numpy as np
import pytest
import soundfile as sf

from hark.core import audio


def _float_wav(path: Path, x: np.ndarray, sr: int = 48000) -> str:
    sf.write(str(path), x.astype(np.float32), sr, subtype="FLOAT")
    return str(path)


def _sine(dur: float = 1.0, f: float = 440.0, amp: float = 0.5) -> np.ndarray:
    t = np.arange(int(dur * 48000)) / 48000
    return amp * np.sin(2 * np.pi * f * t)


def test_at_5_file_id_sees_same_size_rewrite(tmp_path):
    path = _float_wav(tmp_path / "a.wav", _sine())
    before = os.stat(path)
    old = audio.file_id(path)
    _float_wav(tmp_path / "a.wav", _sine(f=880.0))
    os.utime(path, ns=(before.st_atime_ns, before.st_mtime_ns))
    after = os.stat(path)
    assert (after.st_size, after.st_mtime_ns) == (before.st_size, before.st_mtime_ns)
    assert audio.file_id(path) != old


def test_at_6_corrupt_file_cannot_decode(tmp_path):
    path = tmp_path / "corrupt.wav"
    path.write_text("this is not audio\n" * 20)
    with pytest.raises(audio.AudioError) as err:
        audio.load(str(path), cache=False)
    assert "cannot decode" in str(err.value)
    assert "\n" not in str(err.value)


def test_at_7_video_without_audio(tmp_path, ffmpeg):
    out = tmp_path / "black.mp4"
    made = subprocess.run([ffmpeg, "-v", "error", "-f", "lavfi", "-i", "color=c=black:s=64x64:d=1",
                           "-pix_fmt", "yuv420p", str(out)], capture_output=True)
    if made.returncode != 0 or not out.exists():
        pytest.skip("ffmpeg cannot make the black video")
    with pytest.raises(audio.AudioError) as err:
        audio.load(str(out))
    assert "has no audio track" in str(err.value)
    assert str(out) in str(err.value)


@pytest.mark.parametrize("bad", [np.nan, np.inf, -np.inf])
def test_at_8_nan_inf_sample(tmp_path, bad):
    x = _sine()
    x[24000] = bad
    path = _float_wav(tmp_path / "bad.wav", x)
    with pytest.raises(audio.AudioError, match=r"1 NaN / Inf sample\(s\), first at 0\.500 s"):
        audio.load(path, cache=False)


def test_e_a6_finite_check_runs_on_cached_decode(tmp_path, cfg):
    x = _sine()
    x[24000] = np.nan
    path = _float_wav(tmp_path / "bad.wav", x)
    for _ in range(2):
        with pytest.raises(audio.AudioError):
            audio.load(path)
    assert list(Path(cfg.DEC_CACHE).glob("*.npy"))


def test_at_9_check_paths(tmp_path):
    good = _float_wav(tmp_path / "good.wav", _sine())
    missing = str(tmp_path / "missing" / "nope.wav")
    with pytest.raises(audio.AudioError) as err:
        audio.check_paths([good, missing])
    assert "nope.wav" in str(err.value)
    assert "no such file or folder" in str(err.value)
    audio.check_paths([good])


def test_check_paths_lists_at_most_5(tmp_path):
    paths = [str(tmp_path / f"n{i}.wav") for i in range(7)]
    with pytest.raises(audio.AudioError) as err:
        audio.check_paths(paths, what="files")
    text = str(err.value)
    assert text.startswith("files: nothing playable in ")
    assert "n4.wav" in text and "n5.wav" not in text and text.endswith(" ...")


def test_check_paths_empty_folder(tmp_path):
    (tmp_path / "empty").mkdir()
    with pytest.raises(audio.AudioError, match="no audio or video files in it"):
        audio.check_paths([str(tmp_path / "empty")])


def test_at_10_replace_quietly(tmp_path):
    tmp = tmp_path / "t.npy"
    tmp.write_text("x")
    assert audio.replace_quietly(str(tmp), str(tmp_path / "d.npy")) is True
    tmp.write_text("x")
    assert audio.replace_quietly(str(tmp), str(tmp_path / "no" / "d.npy"), tries=1) is False
    assert not tmp.exists()


def test_at_33_list_media_glob_and_folder(tmp_path):
    (tmp_path / "g1.wav").mkdir()
    (tmp_path / "g2.wav").mkdir()
    real = _float_wav(tmp_path / "g3.wav", _sine(0.1))
    assert audio.list_media(str(tmp_path / "g*.wav")) == [real]
    tree = tmp_path / "tree"
    (tree / "b").mkdir(parents=True)
    (tree / "a").mkdir()
    b = _float_wav(tree / "b" / "x.wav", _sine(0.1))
    a = _float_wav(tree / "a" / "y.wav", _sine(0.1))
    top = _float_wav(tree / "z.wav", _sine(0.1))
    (tree / "notes.txt").write_text("no")
    assert audio.list_media(str(tree)) == [top, a, b]
    assert audio.list_media(str(tree), recursive=False) == [top]


def test_e_a15_glob_of_only_folders(tmp_path):
    (tmp_path / "d1.wav").mkdir()
    pattern = str(tmp_path / "d*.wav")
    assert audio.list_media(pattern) == []
    with pytest.raises(audio.AudioError, match="d\\*\\.wav"):
        audio.check_paths([pattern])


def test_e_a16_duplicates_once(tmp_path, monkeypatch):
    path = _float_wav(tmp_path / "a.wav", _sine(0.1))
    monkeypatch.chdir(tmp_path)
    assert audio.list_media(["a.wav", path]) == ["a.wav"]


def test_existing_file_kept_whatever_extension(tmp_path):
    odd = tmp_path / "sound.bin"
    odd.write_bytes(b"x")
    assert audio.list_media([str(odd)]) == [str(odd)]


def test_at_34_probe_mp3_with_cover(tmp_path, ffmpeg):
    wav = _float_wav(tmp_path / "s.wav", np.column_stack([_sine(), _sine()]), sr=44100)
    cover = tmp_path / "c.png"
    mp3 = tmp_path / "s.mp3"
    steps = [
        [ffmpeg, "-v", "error", "-f", "lavfi", "-i", "color=c=red:s=32x32", "-frames:v", "1", str(cover)],
        [ffmpeg, "-v", "error", "-i", wav, "-i", str(cover), "-map", "0", "-map", "1", "-c:a", "libmp3lame",
         "-c:v", "png", "-disposition:v", "attached_pic", "-id3v2_version", "3", str(mp3)],
    ]
    for cmd in steps:
        if subprocess.run(cmd, capture_output=True).returncode != 0:
            pytest.skip("ffmpeg cannot make the MP3 with cover art")
    info = audio.probe(str(mp3))
    assert info["codec"] == "mp3"
    assert info["src_rate"] == 44100
    assert info["channels"] == 2
    assert info["lossy"] is True
    assert info["has_video"] is False
    assert isinstance(info["kbps"], int)


def test_probe_error_is_reported(tmp_path, monkeypatch):
    monkeypatch.setenv("PATH", str(tmp_path))
    info = audio.probe(str(tmp_path / "x.wav"))
    assert list(info) == ["probe_error"]
    assert len(info["probe_error"]) <= 200


def test_e_a14_kbps_falls_back_to_container():
    raw = {"streams": [{"codec_type": "audio", "codec_name": "vorbis", "sample_rate": "48000", "channels": 2,
                        "bit_rate": "4294967295"}],
           "format": {"duration": "1.0", "bit_rate": "128000"}}
    assert audio._probe_fields(raw)["kbps"] == 128


def test_e_a1_missing_file(tmp_path):
    with pytest.raises(audio.AudioError, match="^no such file: "):
        audio.load(str(tmp_path / "nope.wav"))


def test_e_a4_ffmpeg_missing(tmp_path, monkeypatch):
    path = _float_wav(tmp_path / "a.wav", _sine(0.1))
    monkeypatch.setenv("PATH", str(tmp_path))
    with pytest.raises(audio.AudioError, match="ffmpeg not found on PATH"):
        audio.load(path, cache=False)


def test_load_mono_source_to_stereo_at_unity(tmp_path):
    path = _float_wav(tmp_path / "m.wav", _sine())
    x = audio.load(path, cache=False)
    assert x.shape == (48000, 2) and x.dtype == np.float32
    assert np.allclose(x[:, 0], x[:, 1])
    assert abs(np.max(np.abs(x)) - 0.5) < 1e-3


def test_load_range_and_cache(tmp_path, cfg):
    path = _float_wav(tmp_path / "s.wav", np.column_stack([_sine(), -_sine()]))
    whole = audio.load(path)
    part = audio.load(path, start=0.25, end=0.5)
    assert part.shape == (12000, 2)
    assert np.array_equal(part, whole[12000:24000])
    assert audio.load(path, start=5.0).shape[0] == 0
    assert len(list(Path(cfg.DEC_CACHE).glob("*.npy"))) == 1


def test_e_a7_corrupt_cache_is_redecoded(tmp_path, cfg):
    path = _float_wav(tmp_path / "s.wav", _sine())
    first = audio.load(path, mono=True)
    (cache,) = Path(cfg.DEC_CACHE).glob("*.npy")
    cache.write_bytes(b"garbage")
    assert np.array_equal(audio.load(path, mono=True), first)
    assert np.array_equal(np.load(cache), first)


def test_e_a8_unwritable_cache_still_returns(tmp_path, cfg, monkeypatch):
    path = _float_wav(tmp_path / "s.wav", _sine(0.1))
    monkeypatch.setattr(cfg, "DEC_CACHE", str(tmp_path / "file_not_dir"))
    (tmp_path / "file_not_dir").write_text("x")
    assert audio.load(path).shape == (4800, 2)


def test_e_a11_small_file_offsets():
    assert audio._sample_offsets(100) == [0]
    offsets = audio._sample_offsets(10**6)
    assert offsets[0] == 0 and offsets[-1] == 10**6 - 4096 and len(offsets) == 16


def test_e_a12_unreadable_hash(tmp_path):
    assert audio._sampled_hash(str(tmp_path / "missing"), 10) == "unreadable"
    assert len(audio.file_id(str(tmp_path / "missing"))) == 20


def test_array_id_depends_on_shape_and_content():
    a = np.zeros(4, np.float32)
    assert audio.array_id(a) == audio.array_id(a.copy())
    assert audio.array_id(a) != audio.array_id(a.reshape(2, 2))
    assert len(audio.array_id(a)) == 20


def test_windows():
    x = np.arange(100.0)
    assert len(audio.windows(x, 1.0, 0.5, sr=100)) == 1
    x = np.arange(260.0)
    starts = [t for t, _ in audio.windows(x, 1.0, 0.5, sr=100)]
    assert starts == [0.0, 0.5, 1.0, 1.5]
    padded = audio.windows(x, 1.0, 0.5, sr=100, pad_last=True)
    assert padded[-1][0] == 1.6 and len(padded[-1][1]) == 100


def test_db_and_mono_and_resample():
    assert audio.db(0.0) == pytest.approx(-240.0)
    assert audio.db(1.0) == 0.0
    st = np.column_stack([np.ones(4), np.zeros(4)])
    assert np.allclose(audio.to_mono(st), 0.5)
    y = audio.resample(np.ones(48000, np.float32), 48000, 16000)
    assert y.dtype == np.float32 and abs(len(y) - 16000) <= 1


def test_ffmpeg_reason():
    text = "[mp3 @ 0x7f8a] header missing\nError opening output file\n[aist#0 @ 0x12] Invalid data found\n\n"
    assert audio.ffmpeg_reason(text) == "Invalid data found"
    assert audio.ffmpeg_reason("") == "ffmpeg failed"


def test_write_wav_makes_parents(tmp_path):
    out = audio.write_wav(str(tmp_path / "a" / "b" / "x.wav"), _sine(0.1))
    assert sf.info(out).samplerate == 48000
