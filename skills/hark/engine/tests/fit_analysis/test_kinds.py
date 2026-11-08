"""The uniform kind face (ARCHITECTURE 9.2), quality on synthetic sounds, and part edge cases."""

from __future__ import annotations

import json

import numpy as np
import pytest

from hark.fit import analysis
from hark.fit.analysis import gesture, modal, module_for, sinusoidal, space, spectral

SR = 48000
KIND_MODULES = {"sines": sinusoidal, "modal": modal, "spectral_noise": spectral, "gesture": gesture}
MODEL_OF = {"sines": "sines_s1", "modal": "modal_s1", "spectral_noise": "spectral_s3", "gesture": "gesture_s3"}


# ---------------------------------------------------------------- the face

def test_module_for_maps_every_kind():
    assert set(analysis.KINDS) == set(KIND_MODULES)
    for kind, mod in KIND_MODULES.items():
        assert module_for(kind) is mod
    with pytest.raises(ValueError):
        module_for("hybrid")


@pytest.mark.parametrize("kind", sorted(KIND_MODULES))
def test_face_names(kind):
    mod = KIND_MODULES[kind]
    assert mod.KIND == kind
    assert mod.VERSION == 1
    for name in ("fit", "render", "check", "summary", "save", "load"):
        assert callable(getattr(mod, name))
    for knob, (rng, ident, desc) in mod.KNOBS.items():
        assert rng[0] < rng[1] and (len(rng) == 2 or rng[2] == "log"), knob
        assert rng[0] <= ident <= rng[1], knob
        assert isinstance(desc, str) and desc


def test_kind_names():
    assert (sinusoidal.NAME, modal.NAME, spectral.NAME, gesture.NAME) == ("sines", "modal", "spectral", "gesture")


@pytest.mark.parametrize("kind", sorted(KIND_MODULES))
def test_check_rejects_wrong_kind_and_newer_version(models, kind):
    mod = KIND_MODULES[kind]
    m = models[MODEL_OF[kind]]
    mod.check(m)
    with pytest.raises(ValueError):
        mod.check(dict(m, version=2))
    with pytest.raises(ValueError):
        mod.check(dict(m, kind="other"))
    with pytest.raises(ValueError):
        mod.check([m])


@pytest.mark.parametrize("kind", sorted(KIND_MODULES))
def test_save_load_round_trip(models, kind, tmp_path):
    mod = KIND_MODULES[kind]
    m = models[MODEL_OF[kind]]
    path = tmp_path / "sub" / "m.json"
    mod.save(m, str(path))
    text = path.read_text()
    assert ": " not in text and ", " not in text
    back = mod.load(str(path))
    assert back == json.loads(json.dumps(m))
    assert np.array_equal(mod.render(back, 2), mod.render(m, 2))
    bad = tmp_path / "bad.json"
    mod.save(dict(m, kind="other"), str(bad))
    with pytest.raises(ValueError):
        mod.load(str(bad))


@pytest.mark.parametrize("kind", sorted(KIND_MODULES))
def test_identity_knobs_reproduce_the_default_render(models, kind):
    mod = KIND_MODULES[kind]
    m = models[MODEL_OF[kind]]
    ident = {k: v[1] for k, v in mod.KNOBS.items()}
    assert np.array_equal(mod.render(m, 0, **ident), mod.render(m, 0))


@pytest.mark.parametrize("kind", sorted(KIND_MODULES))
def test_summary_is_words(models, kind):
    s = KIND_MODULES[kind].summary(models[MODEL_OF[kind]])
    assert s if isinstance(s, dict) else len(s) > 20


def test_unknown_knob_raises(models):
    with pytest.raises(ValueError):
        modal.render(models["modal_s1"], 0, loudness=3)
    with pytest.raises(ValueError):
        sinusoidal.render(models["sines_s1"], 0, loudness=3)
    with pytest.raises(TypeError):
        spectral.render(models["spectral_s3"], 0, loudness=3)


def test_kind_modules_do_not_import_the_pipeline():
    import hark.fit.analysis as pkg
    import pathlib
    for f in pathlib.Path(pkg.__file__).parent.glob("*.py"):
        src = f.read_text()
        assert "hark.fit.pipeline" not in src and "from hark.fit import" not in src, f.name


# ---------------------------------------------------------------- quality on synthetic sounds

def test_modal_recovers_the_struck_modes(models, s1):
    m = models["modal_s1"]
    rows = m["strikes"][0]["modes"]
    for f, tau in ((440, 0.4), (1130, 0.2), (2390, 0.1)):
        best = min(rows, key=lambda r: abs(r[0] - f))
        assert abs(best[0] - f) < 1.0
        assert abs(best[1] / tau - 1) < 0.05
    y = modal.render(m, 0)
    assert modal.env_corr(s1, y) > 0.98
    assert modal.mel_distance(s1, y)[0] < 4.0
    assert m["fit"]["resid_db"] < -15


def test_modal_finds_the_gated_pulses(models, s4):
    m = models["modal_s4"]
    ts = [s["t"] for s in m["strikes"]]
    assert len(ts) == 4
    assert np.allclose(ts, [0.01, 0.09, 0.17, 0.25], atol=0.002)
    # Without the pipeline's gate (A-11, 20b) the rings run on into the gaps.
    assert modal.env_corr(s4, modal.render(m, 0)) > 0.7


def test_sines_tracks_the_partials(models):
    top = sorted(f["f"] for f in models["sines_s1"]["summary"]["top"][:3])
    assert np.allclose(top, [440, 1130, 2390], rtol=0.01)


def test_spectral_fit_is_near_its_floor(models, s3):
    m = models["spectral_s3"]
    fit = spectral.compare(s3, spectral.render(m, 11))
    floor = spectral.compare(spectral.render(m, 11), spectral.render(m, 12))
    assert fit["mel_db"] <= floor["mel_db"] + 1.0
    assert fit["env_corr"] >= 0.85


def test_spectral_render_is_float32_stereo_and_limited(models):
    y = spectral.render(models["spectral_s3"], 0, gain_db=30)
    assert y.dtype == np.float32 and y.shape[1] == 2
    assert np.max(np.abs(y)) <= 10 ** (-0.5 / 20) + 1e-6


def test_spectral_takes_keep_their_level(models):
    m = models["spectral_s3"]
    lev = []
    for s in range(4):
        y = spectral.render(m, s).astype(float)
        lev.append(10 * np.log10(np.mean(y ** 2)))
    assert max(lev) - min(lev) < 1.5


def test_spectral_band_axis():
    c = spectral.centers()
    assert len(c) == 48 and abs(c[0] - 20) < 1e-9 and abs(c[-1] - 20000) < 1e-6
    assert np.allclose(spectral.unwarp(spectral.warp(c)), c)
    f = np.linspace(c[0], 20000, 2000)
    assert np.allclose(spectral.weights(f, c).sum(axis=0), 1.0, atol=1e-9)
    assert spectral.n_frames(48000) == 201
    assert np.all(spectral.bandwidths(c) > 0)


def test_spectral_motion_knobs(models):
    m = models["spectral_s3"]
    y = spectral.render(m, 0, doppler=1.0, motion=0.4)
    assert y.shape[1] == 2 and len(y) > len(spectral.render(m, 0))
    tr = spectral.trajectory(m)
    assert 0 <= tr["peak_s"] <= m["dur"]


def test_spectral_finds_crackle_and_flutter():
    from scipy.signal import butter, sosfilt
    r = np.random.default_rng(5)
    n = SR
    t = np.arange(n) / SR
    fl = r.standard_normal(n) * (1 + 0.8 * np.sin(2 * np.pi * 20 * t))
    am = spectral.fit(0.3 * fl / np.max(np.abs(fl)))["texture"]["am"]
    rate = am["rates"][int(np.argmax(am["spec"]))]
    assert 11 <= rate <= 32
    cr = np.zeros(n)
    pos = r.integers(0, n, 300)
    cr[pos] = r.standard_normal(300) * np.exp(r.standard_normal(300))
    cr = sosfilt(butter(2, [800, 6000], "band", fs=SR, output="sos"), cr)
    tex = spectral.fit(0.3 * cr / np.max(np.abs(cr)))["texture"]
    assert tex is not None and tex["grains"] is not None


def test_gesture_layers(models):
    g = models["gesture_s1"]
    assert g["tones"] is not None and g["tones"]["layer"] == "deterministic"
    assert g["fit"]["tone_share"] > 0.5
    y = gesture.render(g, 0)
    assert y.ndim == 1
    t = gesture.render(g, 0, tone_db=-200)
    a = gesture.render(g, 0, air_db=-200)
    assert np.allclose(t + a, y, atol=1e-9)


def test_gesture_length_knob(models):
    g = models["gesture_s3"]
    assert abs(len(gesture.render(g, 0, length=1.2)) - round(1.2 * len(gesture.render(g, 0)))) <= 2


def test_gain_tones(models):
    det = gesture._deterministic(models["modal_s1"], False, 0)
    up = gesture.gain_tones(det, 6.0)
    assert np.allclose(modal.render(up, 0), modal.render(det, 0) * 10 ** (6 / 20), atol=1e-9)
    sdet = gesture._deterministic(models["sines_s1"], False, 0)
    sup = gesture.gain_tones(sdet, -6.0)
    assert sup["partials"][0]["amp"][0][1] == pytest.approx(sdet["partials"][0]["amp"][0][1] - 6.0)


def test_magnitude_residual_removes_what_y_holds():
    t = np.arange(SR // 2) / SR
    tone = 0.5 * np.sin(2 * np.pi * 1000 * t)
    noise = 0.05 * np.random.default_rng(1).standard_normal(len(t))
    r = gesture.residual(tone + noise, tone)
    assert np.sum(r ** 2) < 2 * np.sum(noise ** 2)
    assert np.allclose(gesture.residual(noise, np.zeros_like(noise)), noise, atol=1e-6)


# ---------------------------------------------------------------- modal pieces

def test_modal_onsets_and_quick_checks(s1, s4):
    assert modal.onsets(np.zeros(1000)) == [0]
    on = modal.onsets(s4)
    assert len(on) == 4
    assert abs(modal.rise_ms(s1)) < 5
    assert modal.env_corr(s1, s1) == pytest.approx(1.0)
    assert modal.mel_distance(s1, s1)[0] == 0.0
    flux, e, centre = modal.onset_strength(s1)
    assert len(flux) == len(e) == len(centre) and centre[1] == 32


def test_modal_fade_out():
    t = np.arange(SR) / SR
    x = np.random.default_rng(2).standard_normal(SR) * 0.1
    assert modal.fade_out(x) == 0.0
    y = x * np.clip((1.0 - t) / 0.05, 0, 1)
    assert modal.fade_out(y) == pytest.approx(0.05)


def test_modal_esprit_and_ls():
    n = 600
    k = np.arange(n)
    z = np.exp(-1 / (0.01 * SR) + 2j * np.pi * 1000 / SR)
    s = np.real(0.7 * np.exp(0.3j) * z ** k)
    poles, order = modal.esprit(s, 16)
    assert order >= 2
    f = np.angle(poles) * SR / (2 * np.pi)
    assert np.min(np.abs(f - 1000)) < 1
    c = modal.ls_modes(s, [0], [1000.0], [0.01])
    assert abs(c[0]) == pytest.approx(0.7, rel=1e-3)
    assert np.angle(c[0]) == pytest.approx(0.3, abs=1e-3)


def test_modal_bands():
    m = modal.band_masks(4096)
    assert m.shape == (28, 2049)
    assert np.allclose(np.sum(m ** 2, axis=0), 1.0)
    c = modal.band_centres()
    assert len(c) == 28 and c[0] == pytest.approx(50 / np.sqrt(2))


def test_modal_silent_input_is_noise_only():
    m = modal.fit(np.zeros(SR // 4))
    assert len(m["strikes"]) == 1 and m["strikes"][0]["t"] == 0.0
    assert m["strikes"][0]["modes"] == [] and m["strikes"][0]["ramp_ms"] == 0.0
    assert np.all(np.isfinite(modal.render(m, 0)))


def test_modal_stereo_model(make):
    x = make.s1()
    st = np.column_stack([x, 0.6 * x + 0.02 * np.random.default_rng(4).standard_normal(len(x))])
    m = modal.fit(st)
    assert m["channels"] == 2 and len(m["side"]) == 28 and m["side_t"]["bands"]
    assert modal.render(m, 0).shape[1] == 2
    mono = modal.fit(x)
    y = modal.render(mono, 0, stereo=True)
    assert np.array_equal(y[:, 0], y[:, 1])


def test_modal_knobs_move_the_sound(models):
    m = models["modal_s1"]
    y = modal.render(m, 0)
    assert len(modal.render(m, 0, size=1.25)) > len(y)
    assert not np.array_equal(modal.render(m, 0, brightness=2), y)
    assert modal.render(m, 0, room=0.2).shape == y.shape


# ---------------------------------------------------------------- sines and space edge cases

def test_sines_noise_only_model():
    x = 0.1 * np.random.default_rng(3).standard_normal(SR // 4)
    m = sinusoidal.fit(x, max_partials=0)
    assert m["partials"] == [] and sinusoidal.summary(m) == {"partials": 0}
    assert np.all(np.isfinite(sinusoidal.render(m, 0)))
    assert "noise only" in sinusoidal.describe(m)


def test_space_measures(s1, s3):
    assert not space.is_stereo(s1)
    assert space.is_stereo(s3)
    assert space.corr(np.zeros(100), np.ones(100)) is None
    assert space.tail_corr(np.column_stack([s1, s1])) == pytest.approx(1.0)
    dry = np.random.default_rng(0).standard_normal(1000)
    out = space.render_space(None, dry)
    assert out.shape == (1000, 2) and np.array_equal(out[:, 0], dry)
    assert len(space.shaped_side(np.zeros(0), [1000.0], [0.0], [[0.0]], [[0.0]])) == 0
    assert space.describe(None) == "no space layer"


def test_space_fits_a_decorrelated_tail():
    r = np.random.default_rng(8)
    n = int(0.8 * SR)
    t = np.arange(n) / SR
    dry = np.exp(-t / 0.008) * np.sin(2 * np.pi * 880 * t)
    tail = np.exp(-t / 0.25)[:, None] * r.standard_normal((n, 2)) * 0.1
    x = dry[:, None] + tail
    assert space.needs_space(x)
    m = space.fit_space(x, dry=np.column_stack([dry + tail.mean(axis=1), dry + tail.mean(axis=1)]))
    assert m is not None and m["kind"] == "space"
    y = space.render_space(m, dry + tail.mean(axis=1), 0)
    assert y.shape == (n, 2)
    assert space.tail_corr(y) < 0.9
