"""Acceptance tests V1-V6 of docs/specs/hark/timeline-sheet-video.md (video helpers)."""

from __future__ import annotations

import os

import numpy as np
import pytest


def _cache_files(home: str, prefix: str) -> list[str]:
    folder = os.path.join(home, "video")
    return sorted(f for f in os.listdir(folder) if f.startswith(prefix)) if os.path.isdir(folder) else []


def test_v1_visual_events_from_difference_signal():
    from hark.timeline.video import diff_events

    d = np.full(300, 0.003)
    k = np.arange(40)
    d[100:140] = 0.02 + 0.005 * np.sin(k)
    d[60] = 0.4
    d[200] = 0.05
    assert diff_events(d, 30.0) == [60, 200]


def test_v2_cut_and_its_cache(tmp_path, mp4, hark_home):
    from hark.timeline import video

    path = mp4(tmp_path / "bw.mp4")
    assert video.cuts(path) == [1.5]
    [name] = _cache_files(hark_home, "diff_")
    with open(os.path.join(hark_home, "video", name), "wb") as fh:
        np.savez(fh, fps=30.0, d=np.zeros(90))
    assert video.cuts(path) == []
    assert video.cuts(path, cache=False) == [1.5]


def test_v3_broken_file_scene_scan(tmp_path, hark_home):
    from hark.core.audio import AudioError
    from hark.timeline import video

    path = tmp_path / "broken.mp4"
    path.write_bytes(np.random.default_rng(0).bytes(4096))
    with pytest.raises(AudioError, match="broken.mp4"):
        video.cuts(str(path), method="scene")
    assert _cache_files(hark_home, "scene_") == []


def test_v4_still_picture_scene_scan(tmp_path, mp4, hark_home):
    from hark.timeline import video

    path = mp4(tmp_path / "black.mp4", dur=1.0, colours=[("black", 1.0)])
    assert video.cuts(path, method="scene") == []
    assert len(_cache_files(hark_home, "scene_")) == 1


def test_v5_old_version_entry_is_ignored(tmp_path, hark_home):
    from hark.core.audio import AudioError, file_id
    from hark.timeline import video

    path = tmp_path / "broken.mp4"
    path.write_bytes(np.random.default_rng(1).bytes(4096))
    old = os.path.join(hark_home, "video", f"scene_{video._key(file_id(str(path)), 0.3, 0.2, video.SCENE_VERSION - 1)}.json")
    os.makedirs(os.path.dirname(old))
    with open(old, "w", encoding="utf-8") as fh:
        fh.write("[]")
    with pytest.raises(AudioError):
        video.cuts(str(path), method="scene")
    assert _cache_files(hark_home, "scene_") == [os.path.basename(old)]
    with open(old, encoding="utf-8") as fh:
        assert fh.read() == "[]"


def test_v6_colour_clips(tmp_path, mp4, hark_home):
    from hark.timeline import video

    path = mp4(tmp_path / "rgb.mp4", dur=6.0, fps=25, colours=[("red", 2.0), ("blue", 2.0), ("white", 2.0)])
    assert video.cuts(path) == [2.0, 4.0]


def test_info_and_frames(tmp_path, mp4, hark_home):
    from hark.timeline import video

    path = mp4(tmp_path / "bw.mp4")
    info = video.info(path)
    assert info["fps"] == 30 and info["width"] == 96 and info["has_video"] and not info["has_audio"]
    assert video.has_video(path)
    img = video.frame(path, 2.0, width=64)
    assert img is not None and img.size[0] == 64 and np.asarray(img).mean() > 200
    assert video.frames(path, [0.5, 99.0], width=64)[1] is None
