"""sfx tests: bank-kit-sfx AT-3 .. AT-8, AT-11, AT-12, AT-14 .. AT-17 and the part-2 behaviour."""

from __future__ import annotations

import json
import math

import numpy as np
import pytest
import soundfile

from hark.forge.bank import bank, sfx

SR = 48000


def _stand_in_knobs() -> bool:
    from hark.forge.engine import knobs

    return bool(getattr(knobs, "STAND_IN", False))


def _knob_sound(fixture_id: str, category: str, recipe_module: str, tmp_path, monkeypatch) -> str:
    """The fixture sound on the stand-in knob engine; with the real engine, the real recipe's default."""
    if _stand_in_knobs():
        return fixture_id
    mod = pytest.importorskip(recipe_module)
    sound_id = f"{category}/real_default"
    entry = {"type": "forge", "category": category, "member": "real_default", "recipe": recipe_module.rsplit(".", 1)[1],
             "fn": "render", "params": dict(mod.DEFAULTS), "seed": 1, "peak_db": -1.0, "gain_db": 0.0}
    raw = bank.renderer({**entry, "id": sound_id})()
    entry["meta"] = bank.meta_of(bank.normalize(raw, -1.0).astype(np.float32))
    path = tmp_path / "real.json"
    bank.write_bank(path, {"bank": 1, "name": "real", "entries": {sound_id: entry}})
    monkeypatch.setenv("HARK_BANK", str(path))
    return sound_id


def _decay_tone(f: float, dur: float = 0.4) -> np.ndarray:
    t = np.arange(int(dur * SR)) / SR
    x = 0.5 * np.sin(2 * np.pi * f * t) * (1 - np.exp(-t / 0.002)) * np.exp(-t / 0.06)
    x[-240:] *= np.linspace(1, 0, 240)
    return x


def _glide(f0: float, f1: float, rise_s: float, hold_s: float) -> np.ndarray:
    n_rise, n_hold = int(rise_s * SR), int(hold_s * SR)
    f = np.concatenate([f0 * (f1 / f0) ** (np.arange(n_rise) / n_rise), np.full(n_hold, f1)])
    x = 0.5 * np.sin(2 * np.pi * np.cumsum(f) / SR)
    x[:96] *= np.linspace(0, 1, 96)
    x[-1440:] *= np.linspace(1, 0, 1440) ** 2
    return x


def _burst(n: int, start_s: float, rng) -> np.ndarray:
    x = np.zeros(n)
    i = int(start_s * SR)
    t = np.arange(n - i) / SR
    x[i:] = rng.standard_normal(n - i) * np.exp(-t / 0.005) * 0.5
    return x


# ---------------------------------------------------------------- sfx render (AT-3 .. AT-11)


def test_at_3_default_render_file(fixture_bank, tmp_path):
    entry = bank.get("whoosh_fast/crisp")
    out = sfx.sfx_render("whoosh_fast/crisp", out=str(tmp_path / "w.wav"))
    info = soundfile.info(out["file"])
    y, sr = soundfile.read(out["file"])
    assert sr == SR and info.subtype == "PCM_24" and len(y) == round(entry["meta"]["dur_s"] * SR)
    assert abs(bank.times(y)["peak_s"] - entry["meta"]["peak_s"]) <= 0.005
    assert 20 * math.log10(np.max(np.abs(y))) == pytest.approx(-1.0, abs=0.05)
    assert out["summary"].startswith(f"wrote {out['file']}: whoosh_fast/crisp seed {entry['seed']}: ")
    assert "checks: " in out["summary"] and "/" in out["file"]


def test_at_3_cli_render(fixture_bank, tmp_path, cli, capsys):
    assert cli(["sfx", "render", "whoosh_fast/crisp", "-o", str(tmp_path / "w.wav")]) == 0
    assert capsys.readouterr().out.startswith("wrote ")


def test_at_4_dur_gives_exact_length(fixture_bank, tmp_path, monkeypatch):
    sound = _knob_sound("whoosh_fast/crisp", "whoosh_fast", "hark.forge.recipes.motion.whoosh", tmp_path, monkeypatch)
    y, meta = bank.render(sound, dur=0.5, checks=False)
    assert len(y) == 24000 and meta["edits"]["dur"]["target"] == 0.5


def test_at_5_apex_lands_within_5_ms(fixture_bank, tmp_path, monkeypatch):
    sound = _knob_sound("whoosh_fast/crisp", "whoosh_fast", "hark.forge.recipes.motion.whoosh", tmp_path, monkeypatch)
    y, meta = bank.render(sound, apex=0.3, dur=0.8, checks=False)
    assert len(y) == round(0.8 * SR)
    assert abs(bank.times(y)["peak_s"] - 0.3) <= 0.005 and meta["edits"]["apex"]["ok"] is True


def test_at_6_apex_refused_outside_whoosh_family(fixture_bank):
    from hark.forge.engine import knobs

    with pytest.raises(knobs.KnobError, match="whoosh"):
        bank.render("pro_click/glass", apex=0.3)


def test_at_7_pitch_e5_on_pluck(fixture_bank, tmp_path, monkeypatch):
    sound = _knob_sound("pluck/kalimba", "pluck", "hark.forge.recipes.impacts.pluck", tmp_path, monkeypatch)
    y, meta = bank.render(sound, pitch="E5")
    f = bank.estimate_pitch(y, near_hz=659.26)
    assert f is not None and abs(1200 * math.log2(f / 659.26)) <= 50
    assert meta["edits"]["pitch"]["ok"] is True


def test_at_8_pitch_refused_on_noise_sound(fixture_bank):
    from hark.forge.engine import knobs

    with pytest.raises(knobs.KnobError, match="noise|air"):
        bank.render("whoosh_big/deep", pitch="C5")


def test_at_11_other_rate_and_parser_refusal(fixture_bank, tmp_path, cli):
    dur = bank.get("notify/m0")["meta"]["dur_s"]
    out = sfx.sfx_render("notify/m0", out=str(tmp_path / "a.wav"), sr=44100)
    y, sr = soundfile.read(out["file"])
    assert sr == 44100 and abs(len(y) - round(dur * 44100)) <= 1
    with pytest.raises(SystemExit) as stop:
        cli(["sfx", "render", "notify/m0", "-o", str(tmp_path / "b.wav"), "--sr", "22050"])
    assert stop.value.code == 2


def test_render_summary_parts():
    meta = {"id": "a/b", "seed": 3, "dur_s": 0.8, "onset_s": 0.0, "peak_s": 0.3, "end_s": 0.7, "peak_db": -1.0,
            "lufs_m_max": -12.34, "checks": {"ok": True, "warnings": []},
            "edits": {"apex": {"target": 0.3, "measured": 0.301, "err_ms": 1.0},
                      "dur": {"target": 0.8, "method": "shape", "fit": None},
                      "pitch": {"note": "E5", "measured_hz": 659.3}}}
    line = sfx._render_summary(meta, None)
    assert line == ("a/b seed 3: 0.800 s, onset 0.000 s, peak at 0.300 s, end 0.700 s, -1.0 dBFS, -12.3 LUFS momentary "
                    "max; apex 0.3 s (measured 0.301, +1.0 ms); dur 0.8 s (shape, exact); pitch E5 (measured 659.3 "
                    "Hz); checks: clean")


def test_sfx_without_subcommand_exits_2(cli, capsys):
    assert cli(["sfx"]) == 2
    assert "list" in capsys.readouterr().out


# ---------------------------------------------------------------- sfx list (AT-12)


def test_at_12_kind_filter_shipped(shipped):
    rows = sfx.sfx_list(kind="whoosh.long")
    cats = {r["category"] for r in rows["sounds"]}
    assert len(rows["sounds"]) == 15 and cats <= {"whoosh_big", "pro_whoosh_big"}
    assert rows["summary"].startswith("15 sounds")


def test_at_12_kind_filter_fixture(fixture_bank):
    rows = sfx.sfx_list(kind="whoosh.long")
    assert [r["id"] for r in rows["sounds"]] == ["pro_whoosh_big/swell", "whoosh_big/deep"]
    assert rows["summary"] == f"2 sounds in the bank {bank.VERSION} (kind whoosh.long)"
    prefix = sfx.sfx_list(kind="whoosh")
    assert {r["id"] for r in prefix["sounds"]} >= {"whoosh_fast/crisp", "whoosh_big/deep"}


def test_list_rows_and_text(fixture_bank, cli, capsys):
    rows = sfx.sfx_list(category="notify")["sounds"]
    assert [r["id"] for r in rows] == ["notify/low", "notify/m0"]
    assert set(rows[0]) >= {"id", "category", "member", "type", "dur_s", "peak_s", "kinds", "knobs", "notes"}
    fit_row = sfx.sfx_list(category="fit")["sounds"][0]
    assert fit_row["type"] == "fit" and fit_row["source"]["license"] == "CC0"
    assert cli(["sfx", "list", "--category", "notify"]) == 0
    lines = capsys.readouterr().out.splitlines()
    assert lines[0].startswith("2 sounds in the bank") and lines[1].startswith("id" + " " * 28 + " dur s")
    assert lines[2].startswith("notify/low") and "| dur " in lines[2]


def test_list_json_out(fixture_bank, cli, tmp_path, capsys):
    target = tmp_path / "list.json"
    assert cli(["sfx", "list", "--json", "--out", str(target)]) == 0
    assert json.loads(target.read_text())["version"] == bank.VERSION


# ---------------------------------------------------------------- the audit (AT-14 .. AT-17)


def test_at_14_rising_glide_warning_and_drip_allowance():
    whip = _glide(200, 800, 0.3, 0.05)
    for category in (None, "drip"):
        result = sfx.check(whip, category=category)
        assert "rising_glide" in [w["code"] for w in result["warnings"]]
    bubble = _glide(400, 800, 0.1, 0.1)
    drip = sfx.check(bubble, category="drip")
    assert drip["ok"] and {"code": "rising_glide", "msg": "a bubble rising (drip allowance)"} in drip["notes"]
    plain = sfx.check(bubble)
    assert "rising_glide" in [w["code"] for w in plain["warnings"]]


def test_glide_detector_values():
    g = sfx.rising_glide(_glide(200, 800, 0.3, 0.05))
    assert g["rise_st"] == pytest.approx(24, abs=1.5) and g["to_s"] - g["from_s"] >= 0.25
    flat = sfx.rising_glide(_decay_tone(1000))
    assert flat["rise_st"] < 1 and flat["tonal_share"] > 0.5


def test_at_15_stacked_hits_only_with_hit_role(rng):
    x = _burst(int(0.4 * SR), 0.05, rng) + _burst(int(0.4 * SR), 0.11, rng)
    x[-240:] *= np.linspace(1, 0, 240)
    lay = sfx.layers(x)
    assert lay["count"] == 2
    assert "stacked" in [w["code"] for w in sfx.check(x, role="hit")["warnings"]]
    plain = sfx.check(x)
    assert "stacked" not in [w["code"] for w in plain["warnings"] + plain["notes"]]
    as_category = sfx.check(x, category="impact")
    assert "stacked" in [w["code"] for w in as_category["notes"]]


def test_at_16_silent_dc_edge():
    silent = sfx.check(np.zeros(SR // 2))
    assert silent["warnings"] == [{"code": "silent", "msg": "digital silence"}] and not silent["ok"]
    t = np.arange(SR // 2) / SR
    dc = sfx.check(0.5 * np.sin(2 * np.pi * 1000 * t) + 0.1)
    assert "dc" in [w["code"] for w in dc["warnings"]]
    n = int(0.3 * SR)
    t = np.arange(n) / SR
    phase = np.pi / 2 - 2 * np.pi * 1000 * t[-1]
    edge = 0.5 * np.sin(2 * np.pi * 1000 * t + phase)
    edge[:96] *= np.linspace(0, 1, 96)
    assert "edge" in [w["code"] for w in sfx.check(edge)["warnings"]]


def test_at_17_cli_check_two_files(tmp_path, cli, capsys, hark_home):
    clean, silent = tmp_path / "clean.wav", tmp_path / "silent.wav"
    soundfile.write(str(clean), _decay_tone(1000), SR, subtype="PCM_24")
    soundfile.write(str(silent), np.zeros(SR // 2), SR, subtype="PCM_24")
    assert cli(["sfx", "check", str(clean), str(silent)]) == 1
    lines = capsys.readouterr().out.splitlines()
    assert lines == ["1 / 2 clean", "clean.wav: clean", "silent.wav: 1 warning: digital silence"]


def test_check_result_shape_and_context():
    result = sfx.check(_decay_tone(800), category="notify", role="bell", sound="ding")
    assert set(result) >= {"ok", "warnings", "notes", "summary", "measures", "glide", "layers", "context"}
    assert result["context"] == {"category": "notify", "role": "bell", "sound": "ding"}
    assert result["summary"].startswith("ding: ") and result["measures"]["live_ms"] > 5


def test_check_resamples_other_rates():
    t = np.arange(44100 // 2) / 44100
    x = 0.5 * np.sin(2 * np.pi * 440 * t) * np.exp(-t / 0.06)
    x[-200:] *= np.linspace(1, 0, 200)
    assert sfx.check(x, sr=44100)["measures"]["peak_dbfs"] == pytest.approx(-6.0, abs=0.3)


def test_live_ms_and_layers_short():
    assert sfx.live_ms(np.zeros(100)) == 0.0
    y = np.zeros(SR)
    y[1000:1480] = 0.5
    assert sfx.live_ms(y) == pytest.approx(10.75, abs=1.0)
    assert sfx.layers(np.ones(100)) == {"attacks": [0.0], "count": 1, "pre_swell_ms": 0.0}


def test_sfx_check_list_result(tmp_path, hark_home):
    a = tmp_path / "a.wav"
    soundfile.write(str(a), _decay_tone(900), SR, subtype="PCM_24")
    one = sfx.sfx_check(str(a))
    assert one["file"] == str(a) and one["context"]["sound"] == "a.wav"
    many = sfx.sfx_check([str(a), str(a)])
    assert many["summary"] == "2 / 2 clean" and many["ok"] and len(many["files"]) == 2
