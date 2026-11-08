"""CMP-1 .. CMP-8 and AB-1 .. AB-7: compare, ab, trim-proof measurements, typicality, words."""

from __future__ import annotations

import json

import numpy as np
import pytest
from scipy import signal as sps

from hark.listen import compare as cmp

SR = 48000


def _pad(x, before_s, after_s):
    return np.concatenate([np.zeros(int(before_s * SR)), x, np.zeros(int(after_s * SR))])


def _burst(dur=1.0, at=0.3, length=0.2, dbfs=-6.0, freq=1000.0):
    x = np.zeros(int(dur * SR))
    t = np.arange(int(length * SR)) / SR
    i = int(at * SR)
    x[i:i + len(t)] = 10 ** (dbfs / 20) * np.sin(2 * np.pi * freq * t)
    return x


def _unit(v):
    v = np.asarray(v, np.float64)
    return v / np.linalg.norm(v)


def test_cmp_1_trim_proof(write, sig):
    click = sig.click(dur=1.0, at=0.2)
    a = cmp.measure_path(write("click.wav", click))
    b = cmp.measure_path(write("click_pad.wav", _pad(click, 0.5, 1.0)))
    assert abs(a["loudness_lufs"] - b["loudness_lufs"]) <= 0.2
    assert abs(a["length_s"] - b["length_s"]) <= 0.01
    for key in ("attack_ms", "decay_ms"):
        if a.get(key) is not None:
            assert b[key] == pytest.approx(a[key], rel=0.1, abs=1)


def test_cmp_2_sounding_length(write):
    f = cmp.measure_path(write("burst.wav", _burst()))
    assert 0.19 <= f["length_s"] <= 0.22


def test_cmp_3_noise_floor_rule(write, sig):
    short = cmp.measure_path(write("short.wav", sig.click(dur=0.5, at=0.1)))
    assert "noise_floor_dbfs" not in short
    long = cmp.measure_path(write("long.wav", sig.click(dur=2.0, at=0.5)))
    assert long["noise_floor_dbfs"] == -120


def test_cmp_4_robust_z_and_words():
    rz = cmp.robust_z("loudness_lufs", -10.0, [-20, -21, -19, -20, -22])
    assert rz["robust_z"] == pytest.approx(10 / 1.4826, abs=0.01)
    assert rz["refs_median"] == -20
    words = cmp._outlier_words({"loudness_lufs": rz})
    assert words == ["10 dB louder than their median"]


def test_cmp_5_log_keys():
    rz = cmp.robust_z("brightness_hz", 2000.0, [1000, 1100, 900, 1000])
    g = np.log([1000, 1100, 900, 1000])
    med = np.median(g)
    scale = max(1.4826 * np.median(np.abs(g - med)), 0.05)
    assert rz["robust_z"] == pytest.approx((np.log(2000) - med) / scale, abs=0.01)
    assert rz["refs_median"] == pytest.approx(1000, abs=0.001)


def test_cmp_6_typicality_flags_outlier():
    rng = np.random.default_rng(3)
    centre = _unit(rng.standard_normal(512))
    cluster = []
    for _ in range(10):
        noise = _unit(rng.standard_normal(512) - (rng.standard_normal(512) @ centre) * centre)
        cluster.append(_unit(np.sqrt(0.9) * centre + np.sqrt(0.1) * noise))
    far = _unit(rng.standard_normal(512) - 3 * centre)
    refs = np.array(cluster + [far])
    member = cmp.typicality(cluster[0], np.array(cluster[1:] + [far]), k=5)
    outlier = cmp.typicality(far, np.array(cluster), k=5)
    assert member["z"] >= -1 and member["verdict"] == "fits the set"
    assert outlier["z"] < -2 and outlier["verdict"].startswith("an outlier")
    assert cmp.typicality(cluster[0], refs[:2], k=5)["k"] == 5


def test_cmp_7_too_few_refs(tmp_path, write, sig, fake_clap, no_require, run_cli):
    path = write("x.wav", sig.click())
    write("refs/good.wav", sig.click(at=0.4))
    (tmp_path / "refs" / "bad.wav").write_bytes(b"not audio at all")
    r = cmp.compare(path, [str(tmp_path / "refs")])
    assert r["n_refs"] == 1 and r["nearest"] == [] and len(r["skipped"]) == 1
    assert "at least 2" in r["summary"]
    code, out, _ = run_cli(cmp, ["compare", path, "--refs", str(tmp_path / "refs"), "--json"])
    assert code == 0 and json.loads(out)["n_refs"] == 1


def _self_excluded(tmp_path, write, sig):
    path = write("refs/x.wav", sig.click())
    for i in range(3):
        write(f"refs/o{i}.wav", sig.click(at=0.3 + 0.1 * i, freq=1500 + 500 * i))
    r = cmp.compare(path, [str(tmp_path / "refs")])
    assert r["n_refs"] == 3
    assert all(n["name"] != "x.wav" for n in r["nearest"])
    assert set(r["typicality"]) == {"mean_cos_knn", "z", "percentile", "k", "verdict"}
    assert r["feature_distance"] is None or r["feature_distance"] >= 0


def test_cmp_8_self_excluded_stub(tmp_path, write, sig, fake_clap):
    _self_excluded(tmp_path, write, sig)


@pytest.mark.ml
def test_cmp_8_self_excluded(tmp_path, write, sig, has_ml):
    if not has_ml:
        pytest.skip("needs the ML extras")
    _self_excluded(tmp_path, write, sig)


def test_ab_1_louder(write, sig, fake_clap):
    click = sig.click(amp=0.25)
    r = cmp.ab(write("a.wav", click), write("b.wav", click * 10 ** (6 / 20)))
    assert r["differences"][0] == "loudness_lufs"
    assert "6 dB louder" in r["summary"]


def test_ab_2_brightness_not_attack(write, sig, fake_clap):
    rng = np.random.default_rng(0)
    t = np.arange(int(0.08 * SR)) / SR
    hit = rng.standard_normal(len(t)) * np.exp(-t / 0.015) * 0.5
    x = _pad(hit, 0.2, 0.5)
    sos = sps.butter(4, 2000, "low", fs=SR, output="sos")
    r = cmp.ab(write("a.wav", sps.sosfilt(sos, x)), write("b.wav", x))
    assert "brightness_hz" in r["differences"]
    assert "brighter" in r["summary"]
    fa, fb = r["features"]["a"], r["features"]["b"]
    if abs(fa.get("attack_ms", 0) - fb.get("attack_ms", 0)) < 3:
        assert "attack_ms" not in r["differences"]


def test_ab_2_small_attack_change_is_never_reported():
    keys = [k for k, _ in cmp.differences({"attack_ms": 2.0}, {"attack_ms": 4.0})]
    assert keys == []
    keys = [k for k, _ in cmp.differences({"attack_ms": 2.0}, {"attack_ms": 9.0})]
    assert keys == ["attack_ms"]


def test_ab_3_no_difference(write, sig, fake_clap):
    x = _pad(sig.click(dur=0.5, at=0.0), 1.0, 1.0)
    p = write("pack.wav", x)
    r = cmp.ab(p, p, a_start=0.9, a_end=1.6, b_start=0.7, b_end=2.2)
    assert r["differences"] == []
    assert "nothing audible differs" in r["summary"]


def test_ab_3_same_file_no_trims(write, sig, fake_clap):
    p = write("same.wav", sig.click())
    r = cmp.ab(p, p)
    assert r["cosine_ab"] == pytest.approx(1.0, abs=1e-3)
    assert "nearly the same sound" in r["summary"] and r["differences"] == []


def test_ab_4_closer_dead_band():
    assert cmp.closer(2.1, 2.3) == (None, "neither is closer")
    assert cmp.closer(1.0, 2.2) == ("b", "clearly closer")
    assert cmp.closer(2.0, 2.5) == ("b", "slightly closer")


def test_ab_5_similarity_words():
    words = [cmp.similarity_words(c) for c in (0.95, 0.8, 0.6, 0.2)]
    assert words == ["nearly the same sound", "similar", "related", "different sounds"]


def _two_trims(write, sig):
    x = np.zeros(int(9 * SR))
    x[int(1.3 * SR):int(1.3 * SR) + SR // 2] += sig.click(dur=0.5, at=0.0, amp=0.8)
    x[int(8.0 * SR):int(8.0 * SR) + SR // 2] += sig.click(dur=0.5, at=0.0, amp=0.1, freq=600)
    p = write("pack.wav", x)
    r = cmp.ab(p, p, a_start=1.2, a_end=1.6, b_start=7.9, b_end=8.3)
    assert r["a_range"] == [1.2, 1.6] and r["b_range"] == [7.9, 8.3]
    assert r["features"]["a"]["duration"] == pytest.approx(0.4, abs=0.002)
    assert r["features"]["a"]["peak_dbfs"] > r["features"]["b"]["peak_dbfs"] + 10
    assert r["summary"].startswith("A = pack.wav [0:01.20-0:01.60], B = pack.wav [0:07.90-0:08.30].")


def test_ab_6_same_file_two_trims_stub(write, sig, fake_clap):
    _two_trims(write, sig)


@pytest.mark.ml
def test_ab_6_same_file_two_trims(write, sig, has_ml):
    if not has_ml:
        pytest.skip("needs the ML extras")
    _two_trims(write, sig)


def test_ab_7_ordering_of_phrases():
    fa = {"brightness_hz": 1000.0, "decay_ms": 100.0}
    fb = {"brightness_hz": 2000.0, "decay_ms": 300.0}
    text = cmp.join_words([p for _, p in cmp.differences(fa, fb)])
    assert text.index("brighter (") < text.index("with a longer tail (")
    assert " and " in text
    assert cmp.join_words(["with x", "y", "z"]) == "y, z and with x"


def test_ab_query_and_refs(tmp_path, write, sig, fake_clap):
    a = write("a.wav", sig.click())
    b = write("b.wav", sig.sine(dur=1.0))
    for i in range(3):
        write(f"refs/r{i}.wav", sig.click(at=0.2 + 0.1 * i))
    r = cmp.ab(a, b, query="a mouse click", refs=[str(tmp_path / "refs")])
    assert set(r["query"]) == {"query", "a", "b", "closer", "dz"}
    assert r["refs"]["n_refs"] == 3 and r["refs"]["closer"] in ("a", "b", None)
    few = cmp.ab(a, b, refs=[a, b])
    assert "refs" not in few and "at least 2" in few["summary"]


def test_ordinals():
    assert [cmp.ordinal(n) for n in (1, 2, 3, 4, 11, 12, 13, 21, 22, 111)] == [
        "1st", "2nd", "3rd", "4th", "11th", "12th", "13th", "21st", "22nd", "111th"]


def test_compare_errors_cli(tmp_path, write, sig, no_require, run_cli):
    path = write("x.wav", sig.click())
    code, _, err = run_cli(cmp, ["compare", path, "--refs", str(tmp_path / "nothing_here")])
    assert code == 1 and err.startswith("hark compare: error:")
    code, out, err = run_cli(cmp, ["ab", path, path, "--a-start", "-1", "--json"])
    assert code == 1 and json.loads(out)["error"]
