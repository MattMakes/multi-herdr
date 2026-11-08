"""Acceptance tests S1-S7 of docs/specs/hark/timeline-sheet-video.md (the sheet)."""

from __future__ import annotations

import os

import numpy as np
import pytest

SR = 48000


@pytest.fixture
def eight_s_clicks(wav, click_track):
    return wav(click_track(np.arange(0.5, 8.0, 0.5), 8.0, seed=0), "clicks8.wav")


def _png_size(path: str) -> tuple[int, int]:
    from PIL import Image

    with Image.open(path) as im:
        return im.size


def test_s1_sheet_of_click_track(eight_s_clicks, tmp_path, stub_models, hark_home):
    from hark.timeline.sheet import sheet

    r = sheet(eight_s_clicks, str(tmp_path / "out" / "s1.png"), cues="1.0,2.0", width=1400)
    assert os.path.isfile(r["png"])
    assert _png_size(r["png"]) == (r["width"], r["height"])
    assert r["width"] == 1400
    assert r["labelled_events"] > 3
    assert "Wrote" in r["summary"]
    assert r["text"].startswith(r["summary"]) and "Timeline: " in r["text"]


def test_s2_zoomed_sheet(eight_s_clicks, tmp_path, stub_models, hark_home):
    from hark.timeline.sheet import sheet

    r = sheet(eight_s_clicks, str(tmp_path / "s2.png"), start=2, end=4, width=1000)
    assert r["start"] == 2.0 and os.path.isfile(r["png"])


def test_s3_label_staggering():
    from hark.timeline.sheet import stagger

    items = [((1, -k), 1.0 + 0.01 * k, f"HIT label number {k}") for k in range(30)]
    placed = stagger(items, 0.0, 10.0, 1800, 10.0, rows=4)
    assert 1 <= len(placed) <= 30
    for row in range(4):
        spans = sorted((a, a + (len(text) * 10 + 16) / 180) for r, _, a, text in placed if r == row)
        assert all(spans[k][1] <= spans[k + 1][0] + 1e-9 for k in range(len(spans) - 1))


def test_s4_sheet_of_film(tmp_path, mp4, click_track, stub_models, hark_home):
    from hark.timeline.sheet import sheet

    path = mp4(tmp_path / "film.mp4", audio=click_track([0.6, 1.5, 2.4], 3.0, seed=0))
    r = sheet(path, str(tmp_path / "s4.png"), width=1200)
    assert r["thumbnails"] >= 3
    assert len(r["cuts"]) == 1


def test_s5_cli_parse(cli_parser):
    from hark.timeline.sheet import add_cli

    a = cli_parser(add_cli).parse_args(["sheet", "x.mp4", "-o", "o.png", "--width", "1600"])
    assert a.png == "o.png" and a.width == 1600


def test_s6_digital_silence(wav, tmp_path, stub_models, hark_home):
    from hark.timeline.sheet import header_lines, log_spectrogram, sheet, spec_scale, waveform_title

    path = wav(np.zeros((10 * SR, 2), dtype=np.float32), "zeros.wav")
    r = sheet(path, str(tmp_path / "s6.png"), width=1000)
    assert os.path.isfile(r["png"])
    from hark.timeline.timeline import timeline

    tl = timeline(path)
    assert "digital silence" in header_lines(tl, 12)[1][0]
    assert "digital silence" in waveform_title(np.zeros(10 * SR))
    S, _ = log_spectrogram(np.zeros(10 * SR), 900)
    lo, _ = spec_scale(S)
    assert np.all(S <= lo)


@pytest.mark.slow
def test_s7_long_film_text_fits(wav, click_track, stub_models, hark_home):
    from hark.timeline.sheet import build_figure
    from hark.timeline.timeline import timeline

    x = click_track(np.arange(0.5, 93.0, 0.37), 93.0, seed=0)
    path = wav(x, "long.wav")
    r = timeline(path, cues=list(np.arange(2.0, 90.0, 7.0)), detail="low")
    fig, counts = build_figure(r, x.astype(np.float64), 2400)
    fig.canvas.draw()
    renderer = fig.canvas.get_renderer()
    width, height = counts["width"], counts["height"]
    titles = [t for ax in fig.axes for t in ax.texts if t.get_text().split(" ")[0].split("\n")[0] in
              ("frames", "sections", "events", "spectrogram", "waveform", "loudness", "sync")]
    assert len(titles) == 6
    for t in titles + list(fig.texts):
        box = t.get_window_extent(renderer)
        assert box.x0 >= 0 and box.x1 <= width and box.y0 >= 0 and box.y1 <= height, t.get_text()


def test_width_is_clamped_and_default_name(eight_s_clicks, tmp_path, monkeypatch, stub_models, hark_home):
    from hark.timeline.sheet import sheet

    monkeypatch.chdir(tmp_path)
    r = sheet(eight_s_clicks, width=5000)
    assert r["width"] == 2400
    assert os.path.basename(r["png"]) == "clicks8_sheet.png"


def test_cli_handler_writes_png(eight_s_clicks, tmp_path, stub_models, hark_home, capsys, cli_parser):
    from hark.timeline.sheet import add_cli

    png = tmp_path / "cli.png"
    args = cli_parser(add_cli).parse_args(["sheet", eight_s_clicks, "-o", str(png), "--width", "1000",
                                           "--detail", "low"])
    assert args.func(args) == 0
    assert png.is_file() and "Wrote" in capsys.readouterr().out
