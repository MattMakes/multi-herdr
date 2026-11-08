"""Features: envelope, loudness, spectrum and the measure result (core-io 2.6, 3.10, 4.13-4.18)."""

from __future__ import annotations

import os

import numpy as np
import pytest

from hark.core import features

SR = 48000


def _sine(dur: float, f: float = 1000.0, amp: float = 0.5) -> np.ndarray:
    return amp * np.sin(2 * np.pi * f * np.arange(int(dur * SR)) / SR)


def _stereo_at(dbfs: float, dur: float) -> np.ndarray:
    x = _sine(dur, 1000.0, 10 ** (dbfs / 20))
    return np.column_stack([x, x])


@pytest.mark.parametrize("freq", [40, 80, 200, 1000])
def test_at_1_decay_of_decaying_tone(freq, tone):
    x = tone(freq=freq, dur=2.0, amp=0.8, lead=0.1, decay=0.5)
    r = features.measure(x, light=True)
    assert abs(r["decay_ms"] - 1727) <= 0.02 * 1727
    assert r["attack_ms"] <= 6


def test_at_2_decay_of_noise(noise):
    r = features.measure(noise(dur=2.0, amp=0.5, seed=0, decay=0.1), light=True)
    assert abs(r["decay_ms"] - 345) <= 0.05 * 345


def test_at_3_steady_tone(tone):
    r = features.measure(tone(freq=440, dur=2.0, amp=0.5), light=True)
    assert r["attack_ms"] <= 2
    assert r["decay_ms"] is None


def test_at_4_range_errors(tone, wav):
    path = wav(tone(dur=1.0), "one.wav")
    with pytest.raises(ValueError, match="beyond the end"):
        features.measure(path, start=5.0)
    with pytest.raises(ValueError, match="greater than start"):
        features.measure(path, start=0.5, end=0.2)
    with pytest.raises(ValueError, match=">= 0"):
        features.measure(path, start=-1)


def test_e_f8_start_zero_not_checked(tone, wav):
    path = wav(tone(dur=1.0), "one.wav")
    assert features.measure(path, start=0, light=True)["duration"] == 1.0


def test_q5_array_range_is_applied(tone):
    r = features.measure(tone(dur=2.0), start=0.5, end=1.0, light=True)
    assert r["duration"] == 0.5


def test_at_28_bursts(wav, clicks):
    x = clicks(times=(0.2, 1.0, 1.8, 2.5), dur=3.0, freq=1000, length=0.2)
    r = features.measure(wav(x, "bursts.wav"))
    for key in ("lufs", "true_peak_dbtp", "bands_db", "bandwidth_hz", "brightness_hz", "onsets",
                "short_term_max_lufs"):
        assert key in r
    assert len(r["bands_db"]) == 7
    assert "lra_lu" not in r


def test_at_29_tech3341_case1_minus23():
    assert abs(features.integrated_lufs(_stereo_at(-23, 20)) - -23.0) <= 0.1


def test_at_29_tech3341_case2_minus33():
    assert abs(features.integrated_lufs(_stereo_at(-33, 20)) - -33.0) <= 0.1


def test_at_29_tech3341_case3_gated():
    x = np.vstack([_stereo_at(-36, 10), _stereo_at(-23, 60), _stereo_at(-36, 10)])
    assert abs(features.integrated_lufs(x) - -23.0) <= 0.1


def test_at_29_tech3341_case4_gated():
    parts = [(-72, 10), (-36, 10), (-23, 20), (-36, 10), (-72, 10)]
    x = np.vstack([_stereo_at(d, s) for d, s in parts])
    assert abs(features.integrated_lufs(x) - -23.0) <= 0.1


@pytest.mark.parametrize("second, expected", [(-30, 10.0), (-15, 5.0)])
def test_tech3342_lra(second, expected):
    x = np.vstack([_stereo_at(-20, 20), _stereo_at(second, 20)])
    assert abs(features.lra(x) - expected) <= 1.0


def test_at_30_digital_silence(silence, wav):
    path = wav(silence(dur=10.0), "quiet.wav")
    r = features.measure(path, light=True)
    assert r["peak_dbfs"] == -240.0
    assert r["lufs"] == -70.0
    line = features.summary_line(os.path.basename(path), r)
    assert line.startswith("quiet.wav: 10.000 s, digitally silent (peak -240 dBFS): nothing to measure")


def test_e_f1_too_short():
    r = features.measure(np.zeros(100), light=True)
    assert r == {"duration": 0.002, "error": "too short to measure"}
    assert features.summary_line("x.wav", r) == "x.wav: too short to measure"


def test_e_f5_dual_mono(tone):
    r = features.measure(tone(dur=1.0, stereo=True), light=True)
    assert r["side_to_mid_db"] is None
    assert r["correlation"] == 1.0


def test_e_f6_silent_channel(tone):
    x = tone(dur=1.0)
    r = features.measure(np.column_stack([x, np.zeros_like(x)]), light=True)
    assert "correlation" not in r
    assert r["side_to_mid_db"] == pytest.approx(0.0, abs=0.1)


def test_e_f4_ends_at_full_level(tone):
    r = features.measure(tone(dur=1.0, amp=0.9), light=True)
    line = features.summary_line("s.wav", r)
    assert "no -30 dB decay (sustained or cut off)" in line


def test_e_f11_onsets_listed_200(clicks):
    times = [0.05 + 0.1 * i for i in range(230)]
    r = features.measure(clicks(times=times, dur=23.5, length=0.01))
    assert len(r["onsets"]) == 200
    assert r["onset_count"] > 200


def test_e_f12_music_under_5s(tone):
    r = features.measure(tone(dur=2.0), music=True)
    assert not {"tempo", "beats", "key"} & set(r)


def test_music_fields_and_key(clicks):
    times = [0.5 * i for i in range(16)]
    r = features.measure(clicks(times=times, dur=8.0, freq=440, length=0.1), music=True)
    assert r["tempo"] > 0
    assert len(r["key"]) == 3 and all({"key", "r"} == set(k) for k in r["key"])
    assert "beat_ibi_std_ms" in r


def test_key_estimate_of_a_major_chord():
    t = np.arange(4 * SR) / SR
    chord = sum(np.sin(2 * np.pi * f * t) for f in (261.63, 329.63, 392.0)) / 3
    keys = features.key_estimate(chord)
    assert keys[0]["key"] in ("C major", "A minor", "E minor")


def test_light_has_no_librosa_fields(tone):
    r = features.measure(tone(dur=1.0), light=True)
    assert not {"harmonic_ratio", "f0_hz", "onsets"} & set(r)


def test_f0_of_a_steady_tone(tone):
    r = features.measure(tone(freq=440, dur=2.0))
    assert r["harmonic_ratio"] >= 0.8
    assert abs(r["f0_hz"] - 440) < 5


def test_e_f13_nan_value_is_null_in_json():
    from hark.core import report

    assert report.clean({"flatness": float("nan")}) == {"flatness": None}


def test_spectrum_fields(noise):
    r = features.measure(noise(dur=1.0), light=True)
    assert set(r["bands_db"]) == set(features.BANDS)
    assert 0 < r["flatness"] <= 1
    assert r["bandwidth_hz"] >= 16000
    assert r["centroid_hz"] > 5000


def test_clipped_and_levels():
    x = np.ones(SR) * 0.9995
    x[::2] *= -1
    r = features.measure(x, light=True)
    assert r["clipped_samples"] == 2 * SR
    assert r["peak_dbfs"] == pytest.approx(0.0, abs=0.1)


def test_summary_line_parts(tone, wav):
    path = wav(tone(dur=1.0, decay=0.05, lead=0.1), "hit.wav")
    r = features.measure(path, light=True)
    line = features.summary_line("hit.wav", r)
    assert line.startswith("hit.wav: 1.100 s, peak ")
    assert "LUFS" in line and "attack " in line and "decay " in line and line.endswith(" dB.")
    assert "\n" not in line


def test_smooth_env_and_frame_rms():
    m = np.ones(4800)
    env = features.smooth_env(m, 5)
    assert len(env) == 100 and np.allclose(env[5:-5], 1.0)
    assert len(features.frame_rms(m, 960)) == 5
    assert len(features.frame_rms(np.ones(10), 960)) == 1


def test_k_weight_other_rate_matches_48k_table():
    for derive, table in ((features._shelf, features.K_SHELF_48K), (features._highpass, features.K_HIGHPASS_48K)):
        b, a = derive(48000)
        assert np.allclose(b, table[0], atol=1e-9)
        assert np.allclose(a, table[1], atol=1e-9)
    x = np.random.default_rng(0).standard_normal(44100)
    assert features.k_weight(x, 44100).shape == x.shape


def test_true_peak_above_sample_peak():
    x = 0.5 * np.sin(2 * np.pi * 12000 * np.arange(4800) / SR + 0.3)
    assert features.true_peak_dbtp(x) >= 20 * np.log10(np.max(np.abs(x))) - 1e-9


def test_brightness_short_is_none():
    assert features.brightness_hz(np.ones(100)) is None
