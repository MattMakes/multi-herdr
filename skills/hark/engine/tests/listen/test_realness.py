"""REA-1 .. REA-16: realness model file, signal measurements, clips, heads, verdicts, batches, training."""

from __future__ import annotations

import json
import os

import numpy as np
import pytest

from hark.listen import realness as rl
from hark.listen import realness_train as rt

SR = 48000


def _sine(dur=2.0, freq=440.0, dbfs=-12.0):
    t = np.arange(int(dur * SR)) / SR
    return 10 ** (dbfs / 20) * np.sin(2 * np.pi * freq * t)


def _noise_burst(dur=2.0, seed=0):
    rng = np.random.default_rng(seed)
    t = np.arange(int(dur * SR)) / SR
    env = np.minimum(t / 0.01, 1.0) * np.exp(-t / 0.4)
    return 0.5 * rng.standard_normal(len(t)) * env


def test_rea_1_model_format():
    model = json.loads(rl.MODEL_PATH.read_text())
    clap, sig = model["heads"]["clap"], model["heads"]["signal"]
    for head, d in ((clap, 512), (sig, 13)):
        assert len(head["mean"]) == len(head["scale"]) == len(head["coef"]) == d
        assert len(head["clip"][0]) == len(head["clip"][1]) == d
    assert sig["inputs"] == list(rl.SIGNAL_INPUTS) and clap["inputs"] == "clap_embedding_512"
    assert model["fusion"]["heads"] == ["clap", "signal"]
    assert model["thresholds"]["lo"] < model["thresholds"]["hi"]
    assert model["file_thresholds"]["lo"] >= model["thresholds"]["lo"]
    assert len(model["prompts"]["real"]) == len(model["prompts"]["synth"]) == 6
    assert len(model["trained"]) == 12 and int(model["trained"], 16) >= 0
    assert "zero-shot fallback" in model["about"]


def test_rea_1_missing_model(tmp_path):
    with pytest.raises(rl.ModelMissing, match="--train"):
        rl.load_model(tmp_path / "none.json")


def test_rea_2_tone_against_recording():
    tone = rl.signal_features(_sine())
    assert tone["tonal_frames"] >= 0.95 and tone["line_share"] >= 0.9
    assert tone["crest_db"] == pytest.approx(3.0, abs=0.2)
    noise = rl.signal_features(_noise_burst())
    assert noise["tonal_frames"] < 0.1 and noise["line_share"] < 0.1
    assert set(tone) == set(rl.SIGNAL_INPUTS)


def test_rea_3_level_does_not_matter():
    x = _noise_burst(1.5, seed=2) + 0.2 * _sine(1.5, 660)
    a, b = rl.signal_features(x), rl.signal_features(x * 10 ** (-30 / 20))
    for k in rl.SIGNAL_INPUTS:
        if k == "silence_frac" or not np.isfinite(a[k]):
            continue
        assert b[k] == pytest.approx(a[k], rel=1e-6, abs=1e-9), k


def test_rea_4_not_judged():
    assert rl.signal_features(_sine(0.01)) is None
    assert rl.signal_features(np.zeros(SR)) is None
    assert not rl.audible(_sine(1.0, dbfs=-65 + 3.01))


def test_rea_5_clips():
    def starts(sec):
        return [round(s / SR, 3) for s in rl.clip_starts(int(round(sec * SR)))]

    assert starts(3) == [0] and starts(6.0) == [0]
    assert starts(6.1) == [0, 2.1]
    assert starts(12) == [0, 4, 8]
    assert starts(12.1) == [0, 4, 8]


def test_rea_6_head_and_fusion():
    head = {"mean": [0, 0], "scale": [1, 2], "clip": [[-1, -1], [1, 1]], "coef": [1, 1], "intercept": 0}
    assert rl.head_logit(head, [5, float("nan")]) == pytest.approx(1.0)
    model = {"fusion": {"heads": ["clap", "signal"], "coef": [1, 0], "intercept": 0}}
    assert rl.fuse(model, {"clap": 1.0, "signal": 7.0}) == pytest.approx(0.731, abs=1e-3)
    assert rl.fuse(model, [1.0, 7.0]) == pytest.approx(0.731, abs=1e-3)


def test_rea_7_digital_silence_never_a_reason(write, fake_clap):
    x = np.concatenate([_noise_burst(1.0), np.zeros(SR)])
    feats = rl.signal_features(x)
    assert feats["silence_frac"] >= 0.45
    model = rl.load_model()
    assert all("silence" not in f["text"] for f in rl.fired_facts(model, feats))
    r = rl.realness(write("half.wav", x), cache=False)
    assert "digital silence" not in r["why"]
    assert any("digital silence" in e for e in r["evidence"])


def test_rea_8_silent_file_needs_no_model(write, no_models):
    r = rl.realness(write("zeros.wav", np.zeros(SR)), cache=False)
    assert r["verdict"] == "silent" and r["p_real"] is None and "exact digital silence" in r["why"]


def test_rea_9_pure_tone_rule(write, fake_clap):
    r = rl.realness(write("tone.wav", _sine(30.0, dbfs=-10.5)), cache=False)
    assert all(g["verdict"] == "likely code-synthesized" for g in r["segments"])
    assert r["p_real"] <= 0.03 and r["verdict"] == "likely code-synthesized"
    assert "8 decided by the pure-tone rule" in r["why"]


def test_rea_10_long_file_times(write, fake_clap):
    steady = 0.1 * np.random.default_rng(1).standard_normal(20 * SR)
    r = rl.realness(write("long.wav", steady), start=4, end=16, cache=False)
    assert r["range"] == [4.0, 16.0]
    assert [g["start"] for g in r["segments"]] == [4.0, 8.0, 12.0]


def test_rea_11_bad_ranges(write, no_models):
    p = write("one.wav", _sine(1.0))
    with pytest.raises(ValueError, match="start must be >= 0 s"):
        rl.realness(p, start=-1)
    with pytest.raises(ValueError, match="end must be greater than start"):
        rl.realness(p, start=0, end=0)
    with pytest.raises(ValueError, match="one.wav"):
        rl.realness(p, start=5)


def test_rea_12_many_files(tmp_path, write, fake_clap, no_require, run_cli):
    write("pack/sound.wav", _noise_burst(1.0))
    write("pack/silent.wav", np.zeros(SR))
    (tmp_path / "pack" / "corrupt.wav").write_bytes(b"junk" * 100)
    missing = str(tmp_path / "missing")
    r = rl.realness_many([str(tmp_path / "pack"), missing])
    assert r["counts"]["silent"] == 1 and r["counts"]["error"] == 2
    assert set(r["counts"]) == set(rl.VERDICTS)
    assert r["files"][-1]["why"] == "no such file or folder"
    code, _, err = run_cli(rl, ["realness", str(tmp_path / "pack"), missing])
    assert code == 1 and "hark realness: error:" in err and "missing" in err
    code, out, _ = run_cli(rl, ["realness", str(tmp_path / "pack")])
    assert code == 1 and "corrupt.wav" in out


def test_rea_13_quiet_lift():
    def clip(rms, peak):
        x = np.full(SR, 10 ** (rms / 20))
        x[0] = 10 ** (peak / 20)
        return x

    assert rl.lift_gain(clip(-40, -28)) == pytest.approx(10, abs=0.1)
    assert rl.lift_gain(clip(-45, -5)) == pytest.approx(4, abs=0.1)
    assert rl.lift_gain(clip(-90, -85)) == 0


def test_rea_13_quiet_clip_scores_close(write, fake_clap):
    x = _noise_burst(1.0, seed=3)
    a = rl.realness(write("a.wav", x), cache=False)["p_real"]
    b = rl.realness(write("b.wav", x * 0.1), cache=False)["p_real"]
    assert abs(a - b) < 0.1


def test_rea_14_code_beep_rule_parts(write, fake_clap):
    t = np.arange(SR) / SR
    beep = 0.5 * np.sign(np.sin(2 * np.pi * 1000 * t)) * np.minimum(1, np.minimum(t, 1 - t) / 0.05)
    r = rl.realness(write("beep.wav", beep), cache=False)
    assert r["verdict"] in rl.VERDICTS and 0 <= r["p_real"] <= 1


@pytest.mark.ml
def test_rea_14_code_beep(write):
    t = np.arange(SR) / SR
    beep = 0.5 * np.sign(np.sin(2 * np.pi * 1000 * t)) * np.minimum(1, np.minimum(t, 1 - t) / 0.05)
    r = rl.realness(write("beep.wav", beep), cache=False)
    assert r["verdict"] == "likely code-synthesized" or r["p_real"] <= 0.3


@pytest.mark.ml
def test_rea_15_held_out_target():
    if not os.environ.get("HARK_EVAL_ASSETS"):
        pytest.skip("needs-data: a labelled real / synth corpus (HARK_EVAL_ASSETS)")
    pytest.skip("needs-data: grouped leave-one-group-out run of a rebuilt model")


def test_rea_16_real_never_fires_rule_on_noise():
    for seed in range(5):
        assert not rl.pure_tone(_noise_burst(2.0, seed))


@pytest.mark.ml
def test_rea_16_real_never_fires_rule():
    root = os.environ.get("HARK_EVAL_REFS")
    if not root:
        pytest.skip("needs-data: real recordings (HARK_EVAL_REFS)")
    from hark.core import audio

    for f in audio.list_media([root]):
        x = np.asarray(audio.load(f, mono=True), np.float64)
        for _, _, c in rl.clips_of(x):
            assert not (rl.audible(c) and rl.pure_tone(c)), f


def test_cache_returns_this_path(tmp_path, write, fake_clap, listen_cache):
    p = write("x.wav", _noise_burst(1.0))
    first = rl.realness(p)
    other = os.path.join(str(tmp_path), ".", "x.wav")
    again = rl.realness(other)
    assert again["path"] == other and again["p_real"] == first["p_real"]


def test_p_text_and_verdict():
    th = {"lo": 0.17, "hi": 0.77}
    assert rl.p_text(0.168, th) == "0.168" and rl.p_text(0.5, th) == "0.50" and rl.p_text(0.17, th) == "0.17"
    assert rl.verdict_of(0.77, th) == "real or pro-produced"
    assert rl.verdict_of(0.17, th) == "likely code-synthesized"
    assert rl.verdict_of(0.5, th) == "uncertain"


def test_training_on_synthetic_corpus(tmp_path, write, fake_clap):
    for g in ("noiseA", "noiseB"):
        for i in range(4):
            write(f"corpus/real/{g}/n{i}.wav", _noise_burst(1.0, seed=10 * i + len(g)))
    for g in ("toneA", "toneB"):
        for i in range(4):
            write(f"corpus/synth/{g}/t{i}.wav", _sine(1.0, 300 + 90 * i + 7 * len(g)))
    model = rt.train_model(str(tmp_path / "corpus"))
    out = tmp_path / "model.json"
    rt.write_model(model, out)
    loaded = rl.load_model(out)
    assert loaded["data"]["clips"] == 16 and len(loaded["heads"]["clap"]["coef"]) == 512
    assert loaded["thresholds"]["lo"] < loaded["thresholds"]["hi"]
    for k, c in zip(rl.SIGNAL_INPUTS, loaded["heads"]["signal"]["coef"]):
        assert (c >= 0) if k in rl.POSITIVE else (c <= 0)
    w = rt.sample_weights(np.array([1, 1, 1, 0]), ["a", "a", "b", "c"])
    assert w.sum() == pytest.approx(4) and w[2] == pytest.approx(2 * w[0])
