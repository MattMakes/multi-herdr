"""kit tests: bank-kit-sfx AT-18 .. AT-31, AT-33, AT-34 and the part-3 behaviour."""

from __future__ import annotations

import copy
import json

import numpy as np
import pytest
import soundfile

from hark.forge.bank import bank, kit, sfx

SR = 48000
PRESET_TAKES = {"calm": 64, "crisp": 64, "minimal": 63, "launch": 72}


def _kit(**roles) -> dict:
    base = {"tick": {"sound": "click/soft", "takes": 3, "level_db": -6},
            "hit": {"sound": "notify/m0", "takes": 2, "level_db": -3},
            "whoosh.short": {"sound": "whoosh_fast/crisp", "takes": 2},
            "riser": None}
    base.update(roles)
    return {"kit": 1, "name": "t", "sr": 48000, "roles": base, "off": {"riser": "test"}}


# ---------------------------------------------------------------- init (AT-18, AT-19)


def test_at_18_init_writes_calm_then_needs_force(tmp_path, monkeypatch, cli, capsys):
    monkeypatch.chdir(tmp_path)
    assert cli(["kit", "init"]) == 0
    written = json.loads((tmp_path / "audio" / "kit.json").read_text(encoding="utf-8"))
    assert written == kit.preset("calm")
    assert capsys.readouterr().out.startswith("wrote audio/kit.json: preset calm, 17 roles sounded, 4 off (")
    with pytest.raises(kit.KitError, match="--force"):
        cli(["kit", "init"])
    (tmp_path / "audio" / "kit.json").write_text("{}", encoding="utf-8")
    assert cli(["kit", "init", "--force", "--preset", "crisp"]) == 0
    assert json.loads((tmp_path / "audio" / "kit.json").read_text())["name"] == "crisp"


def test_at_19_unknown_preset_lists_presets(tmp_path, cli):
    with pytest.raises(kit.KitError) as err:
        cli(["kit", "init", "--preset", "nosuch", "-o", str(tmp_path / "k.json")])
    assert "calm, crisp, launch, minimal" in str(err.value)
    assert kit.presets() == ["calm", "crisp", "launch", "minimal"]


def test_kit_without_subcommand_exits_2(cli):
    assert cli(["kit"]) == 2


# ---------------------------------------------------------------- render and cache (AT-20 .. AT-22)


def test_at_20_calm_renders_64_then_caches_shipped(shipped, tmp_path, hark_home):
    first = kit.render(kit.preset("calm"), tmp_path / "takes")
    assert first["stats"]["rendered"] == 64
    again = kit.render(kit.preset("calm"), tmp_path / "takes")
    assert (again["stats"]["rendered"], again["stats"]["cached"]) == (0, 64)


def test_at_20_render_then_cache_fixture(fixture_bank, tmp_path):
    out = tmp_path / "takes"
    first = kit.render(_kit(), out)
    assert first["stats"] | {"seconds": 0} == {"takes": 7, "rendered": 7, "cached": 0, "removed": 0, "seconds": 0}
    assert sorted(p.name for p in out.iterdir()) == sorted(
        ["kit.lock.json", "tick_1.wav", "tick_2.wav", "tick_3.wav", "hit_1.wav", "hit_2.wav", "whoosh-short_1.wav",
         "whoosh-short_2.wav"])
    again = kit.render(_kit(), out)
    assert (again["stats"]["rendered"], again["stats"]["cached"]) == (0, 7)
    assert again["summary"].startswith(f"kit t: 7 takes in {out} (0 rendered, 7 cached) in ")


def test_at_21_changed_role_renders_only_that_role(fixture_bank, tmp_path):
    out = tmp_path / "takes"
    kit.render(_kit(), out)
    changed = kit.render(_kit(tick={"sound": "pro_click/glass", "takes": 2}), out)
    assert changed["stats"]["rendered"] == 2 and changed["stats"]["cached"] == 4
    assert changed["stats"]["removed"] == 1 and not (out / "tick_3.wav").exists()
    assert ", 1 stale removed)" in changed["summary"]


def test_at_22_deleted_take_renders_again(fixture_bank, tmp_path):
    out = tmp_path / "takes"
    kit.render(_kit(), out)
    (out / "hit_1.wav").unlink()
    again = kit.render(_kit(), out)
    assert (again["stats"]["rendered"], again["stats"]["cached"]) == (1, 6)
    (out / "hit_2.wav").write_bytes(b"changed")
    assert kit.render(_kit(), out)["stats"]["rendered"] == 1
    assert kit.render(_kit(), out, force=True)["stats"]["rendered"] == 7


def test_lock_contents(fixture_bank, tmp_path):
    out = tmp_path / "takes"
    doc = _kit(pop={"sound": "ui_pop/soft", "bed_db": 8.5, "offset_ms": 0, "width": 0.6})
    doc["mix"] = {"level": "bed"}
    result = kit.render(doc, out)
    lock = json.loads((out / "kit.lock.json").read_text(encoding="utf-8"))
    assert "stats" not in lock and result["file"].endswith("/kit.lock.json")
    assert lock["kit"] == 1 and lock["bank"] == bank.VERSION and lock["hark"] == "0.1.0" and lock["mix"] == doc["mix"]
    assert lock["roles"]["riser"] is None and lock["roles"]["whoosh.short"]["align"] == "peak"
    pop = lock["roles"]["pop"]
    assert pop["bed_db"] == 8.5 and pop["offset_ms"] == 0.0 and pop["width"] == 0.6 and pop["level_db"] == 0.0
    take = pop["takes"][0]
    assert set(take) >= {"file", "seed", "pitch", "onset_s", "peak_s", "end_s", "dur_s", "peak_db", "lufs_m_max",
                         "sha1", "key", "checks", "l50_lufs", "corr"}
    assert abs(take["corr"] - 0.6) <= 0.05
    y, _ = soundfile.read(str(out / "pop_1.wav"))
    assert y.ndim == 2


def test_cached_take_gets_l50_when_bed_db_is_added(fixture_bank, tmp_path):
    out = tmp_path / "takes"
    kit.render(_kit(), out)
    later = kit.render(_kit(hit={"sound": "notify/m0", "takes": 2, "level_db": -3, "bed_db": 7.0}), out)
    assert later["stats"]["rendered"] == 0
    assert all("l50_lufs" in t for t in later["roles"]["hit"]["takes"])


def test_take_seeds_and_names(fixture_bank):
    takes = kit.plan(_kit(bell={"sound": "notify/low", "pitches": ["C5", "D5"], "seed": 7}))
    names = [t["file"] for t in takes]
    assert names[:3] == ["tick_1.wav", "tick_2.wav", "tick_3.wav"] and names[-2:] == ["bell_C5.wav", "bell_D5.wav"]
    assert [t["seed"] for t in takes[-2:]] == [7, 7]
    entry_seed = bank.get("click/soft")["seed"]
    assert [t["seed"] for t in takes[:3]] == [entry_seed, entry_seed + 1, entry_seed + 2]
    two = kit.plan(_kit(text={"sound": "notify/low", "pitches": ["C#5"], "takes": 2}))
    assert [t["file"] for t in two[-2:]] == ["text_Cs5_1.wav", "text_Cs5_2.wav"]


def test_take_key_changes(fixture_bank):
    take = kit.plan(_kit())[0]
    key = kit.take_key(take)
    assert kit.take_key(take) == key and kit.take_key(take, 44100) != key
    assert kit.take_key({**take, "seed": take["seed"] + 1}) != key
    assert kit.take_key({**take, "width": 0.5}) != key


def test_reroll_picks_first_clean_seed(fixture_bank, monkeypatch):
    calls = []
    real = sfx.check

    def flaky(y, **kw):
        calls.append(1)
        result = real(y, **kw)
        if len(calls) < 3:
            bank.add_warning(result, {"code": "ring", "msg": "test ring"})
        return result

    monkeypatch.setattr(sfx, "check", flaky)
    take = kit.plan(_kit())[3]
    _, audit, _, used = kit.render_take(take)
    assert audit["ok"] and used == take["seed"] + 2000


# ---------------------------------------------------------------- validation (AT-23, AT-24)


def test_at_23_one_pitch_twice(fixture_bank):
    found = kit.problems(_kit(bell={"sound": "notify/low", "pitches": ["C#5", "Db5"]}))
    assert any("are one pitch" in p and "list each pitch once" in p for p in found)


def test_at_24_case_collision(fixture_bank):
    doc = _kit()
    doc["roles"]["Hit"] = {"sound": "notify/low"}
    doc["roles"]["hit"] = {"sound": "notify/m0"}
    assert any("when case is ignored" in p for p in kit.problems(doc))


@pytest.mark.parametrize("change, words", [
    ({"tick": {"sound": "click/sotf"}}, "did you mean click/soft"),
    ({"tick": {"sound": "click/soft", "takes": 0}}, "takes must be an integer"),
    ({"tick": {"sound": "click/soft", "takes": True}}, "takes must be an integer"),
    ({"tick": {"sound": "click/soft", "seed": 1.5}}, "seed must be an integer"),
    ({"tick": {"sound": "click/soft", "colour": 1}}, "unknown keys colour"),
    ({"tick": {"sound": "click/soft", "edits": {"speed": 2}}}, "unknown edits speed"),
    ({"tick": {"sound": "click/soft", "pitches": []}}, "pitches must be a non-empty list"),
    ({"tick": {"sound": "click/soft", "align": "middle"}}, "align must be one of"),
    ({"tick": {"sound": "click/soft", "max_s": 0.001}}, "max_s must be"),
    ({"tick": {"sound": "click/soft", "bed_db": 40}}, "bed_db must be a number from 0.5 to 30"),
    ({"tick": {"sound": "click/soft", "edits": {"peak_db": 1}}}, "clips"),
    ({"tick": {"sound": "click/soft", "edits": {"apex": 0.3}}}, "whoosh"),
    ({"tick": "loud"}, "must be an object or null"),
])
def test_role_problems(fixture_bank, change, words):
    assert any(words in p for p in kit.problems(_kit(**change))), kit.problems(_kit(**change))


def test_kit_level_problems(fixture_bank, tmp_path):
    assert kit.problems([]) == ["not a kit file"] and kit.problems({"kit": 2}) == ["not a kit file"]
    assert kit.problems({"kit": 1, "roles": {}}) == ["roles must be a non-empty object"]
    assert any("sr" in p for p in kit.problems({**_kit(), "sr": 22050}))
    bad_mix = {**_kit(), "mix": {"duck_db": "x", "stop_down": {"before": ["nope"], "depth_db": [12, 6]}}}
    assert len(kit.problems(bad_mix)) == 3
    assert kit.problems(_kit()) == []
    path = tmp_path / "bad.json"
    path.write_text("{oops", encoding="utf-8")
    with pytest.raises(kit.KitError, match="not JSON"):
        kit.load(str(path))
    with pytest.raises(kit.KitError, match="invalid kit: "):
        kit.load(_kit(tick={"sound": "nope/x"}))


# ---------------------------------------------------------------- check (AT-25)


def test_at_25_riser_role_note(fixture_bank):
    result = kit.check(_kit(riser={"sound": "whoosh_big/deep", "align": "end"}))
    notes = " | ".join(result["notes"])
    assert "calm, crisp and minimal" in notes and "rings on" in notes
    assert result["summary"].startswith("kit t: ") and "note" in result["summary"]


def test_check_audits_rendered_files_and_memory(fixture_bank, tmp_path, cli, capsys):
    out = tmp_path / "takes"
    kit.render(_kit(), out)
    from_files = kit.check(_kit(), out)
    sources = {r["source"] for rows in from_files["roles"].values() for r in rows}
    assert sources == {"rendered file"}
    in_memory = kit.check(_kit())
    assert {r["source"] for rows in in_memory["roles"].values() for r in rows} == {"rendered in memory"}
    assert "; off: riser" in in_memory["summary"]
    path = tmp_path / "kit.json"
    path.write_text(json.dumps(_kit()), encoding="utf-8")
    code = cli(["kit", "check", str(path), "--out", str(out)])
    text = capsys.readouterr().out
    assert code == (0 if from_files["ok"] else 1) and "kit t: " in text


def test_check_reports_render_error_and_problems(fixture_bank, tmp_path, monkeypatch):
    def boom(*a, **kw):
        raise bank.BankError("the render failed or is silent")

    monkeypatch.setattr(kit, "render_take", boom)
    result = kit.check(_kit())
    row = result["roles"]["tick"][0]
    assert not result["ok"] and row["warnings"] == [{"code": "render", "msg": "BankError: the render failed or is silent"}]
    bad = kit.check(_kit(tick={"sound": "nope/x"}))
    assert bad["summary"].startswith("1 problem: ") and bad["roles"] == {}
    path = tmp_path / "broken.json"
    path.write_text("{", encoding="utf-8")
    assert kit.check(str(path))["summary"].startswith("kit unreadable: ")


# ---------------------------------------------------------------- mix helpers (AT-26 .. AT-30)


def test_at_26_gain_db():
    role = {"bed_db": 8.5, "level_db": -4}
    assert kit.gain_db({"l50_lufs": -15, "peak_db": -1}, role, -24) == -1.16
    assert kit.gain_db({"l50_lufs": -30, "peak_db": -1}, role, -24) == 0.0
    assert kit.gain_db({"l50_lufs": -30}, role, None) == -4.0
    assert kit.gain_db({"l50_lufs": -30}, role, -70) == -4.0
    assert kit.over_bed(3.0) == pytest.approx(0.0, abs=0.05) and kit.over_bed(8.5) == pytest.approx(7.84, abs=0.01)


def test_at_27_place():
    assert kit.place({"onset_s": 0.012}, {"align": "onset", "offset_ms": -40}, 2.0) == 1.948
    assert kit.place({"onset_s": 0.0, "peak_s": 0.3}, {"align": "peak"}, 1.0) == 0.7


def test_at_28_space_run():
    assert kit.space_run([1.0, 1.05, 1.3], 120) == [1.0, 1.12, 1.3]
    assert kit.space_run([2.0, 1.0], None) == [1.0, 2.0]


def test_at_29_stereo_width(rng):
    n = SR
    stereo = rng.standard_normal((n, 2)) * 0.3
    mono = rng.standard_normal(n) * 0.3
    for x in (stereo, mono):
        y, info = kit.stereo_width(x, 0.6)
        assert y.shape == (n, 2) and abs(kit.corr(y) - 0.6) <= 0.05 and info["ok"] is True
        assert np.max(np.abs(y)) == pytest.approx(np.max(np.abs(x)), rel=1e-6)
    same, info = kit.stereo_width(np.column_stack([mono, mono]), 1.0)
    assert info["before"] == 1.0 and np.allclose(same[:, 0], mono)


def test_at_30_bed_level_of_silence():
    assert kit.bed_level(np.zeros(2 * SR), 1.5) is None
    with pytest.raises(kit.KitError, match="no music before"):
        kit.bed_level(np.zeros(2 * SR), 0.01)


def test_loudness_helpers(rng):
    noise = rng.standard_normal(SR) * 0.1
    assert kit.loud_lufs(noise) == pytest.approx(kit.loud_lufs(np.column_stack([noise, noise])), abs=0.01)
    level = kit.bed_level(np.column_stack([noise, noise]), 1.0)
    assert level is not None and level < kit.loud_lufs(np.column_stack([noise, noise]))
    assert kit.corr(noise) == 1.0


# ---------------------------------------------------------------- audition (AT-31)


def test_at_31_audition_writes_wav_and_index(fixture_bank, tmp_path, cli, capsys):
    path = tmp_path / "kit.json"
    path.write_text(json.dumps(_kit()), encoding="utf-8")
    assert cli(["kit", "audition", str(path), "-o", str(tmp_path / "a.wav")]) == 0
    lines = (tmp_path / "a.txt").read_text(encoding="utf-8").splitlines()
    assert len(lines) == 7 and [ln.split()[1] for ln in lines] == ["tick"] * 3 + ["hit"] * 2 + ["whoosh.short"] * 2
    assert lines[0].startswith("   0.000  tick")
    y, sr = soundfile.read(str(tmp_path / "a.wav"))
    assert sr == SR and y.ndim == 2 and np.max(np.abs(y)) <= 10 ** (-1 / 20) + 1e-3
    assert "takes of 3 roles" in capsys.readouterr().out


def test_audition_needs_a_sounded_role(fixture_bank, tmp_path):
    with pytest.raises(kit.KitError, match="the kit sounds no role"):
        kit.audition({"kit": 1, "name": "x", "roles": {"riser": None}}, str(tmp_path / "a.wav"))


def test_audition_default_folder_under_hark_home(fixture_bank, hark_home, tmp_path):
    result = kit.audition(_kit(), str(tmp_path / "a.wav"))
    assert result["takes_dir"].startswith(str(hark_home).replace("\\", "/")) and "/kits/t_" in result["takes_dir"]


# ---------------------------------------------------------------- presets (AT-33, AT-34)


def test_at_33_presets_validate_shipped(shipped):
    for name, count in PRESET_TAKES.items():
        doc = kit.preset(name)
        assert kit.problems(doc) == []
        assert len(kit.plan(doc)) == count


def test_launch_swap_logged_and_clean(shipped, open_ids):
    # T-41: the text role swaps the refused mallet/pluck for the closest passing bank sound, and logs why
    doc = kit.preset("launch")
    swap = doc["swaps"]["text"]
    assert swap["from"] == "mallet/pluck" and swap["from"] in open_ids["forge04"]
    assert swap["from"] not in shipped.ids() and swap["reason"]
    assert doc["roles"]["text"]["sound"] == swap["to"] == "mallet/celesta"
    role = {k: v for k, v in doc["roles"]["text"].items() if k in kit.ROLE_KEYS}
    assert kit.check({"kit": 1, "sr": 48000, "roles": {"text": role}})["ok"]


@pytest.mark.parametrize("name", sorted(PRESET_TAKES))
def test_at_33_preset_structure_and_take_counts(name):
    doc = kit.preset(name)
    assert doc["kit"] == 1 and doc["name"] == name and doc["sr"] == 48000 and doc["note"]
    count = 0
    for role, spec in doc["roles"].items():
        if spec is None:
            assert doc["off"][role]
            continue
        assert set(spec) <= set(kit.ROLE_KEYS) and spec["note"] and not spec["sound"].startswith("fit/")
        count += spec.get("takes", 1) * len(spec.get("pitches") or [None])
    assert count == PRESET_TAKES[name]
    assert [r for r, s in doc["roles"].items() if s is None] == {
        "calm": ["suck", "riser", "drop", "swell"], "crisp": ["suck", "riser", "swell"],
        "minimal": ["suck", "riser", "swell"], "launch": ["suck"]}[name]


def test_launch_mix_block_validates():
    doc = kit.preset("launch")
    assert kit._mix_problems(doc["mix"], doc["roles"]) == []
    assert all(s["offset_ms"] is not None for s in doc["roles"].values() if s)


def test_at_34_default_takes_audit_shipped(shipped, open_ids):
    known = {"pro_zip/up", "pro_zip/up_hi", "pro_zip/arc", "logo_sting/zip_ping", "pro_logo_sting/sweep_hit", "pop/2",
             "pro_pop/bloop", "typing_key/light", "typing_key/laptop", "pro_hover_tick/air", "pro_hover_tick/glass",
             "hover_tick/detent", "pro_typing_key/soft", "glitch/dropout", "logo_sting/reverse_bloom",
             "pro_shimmer/m3"} | open_ids["derived"]
    bad = [i for i in shipped.ids() if i not in known and not shipped.render(i)[1]["checks"]["ok"]]
    assert bad == []


def test_at_34_default_takes_audit_fixture(fixture_bank):
    bad = {}
    for sound_id in bank.ids():
        checks = bank.render(sound_id)[1]["checks"]
        if not checks["ok"]:
            bad[sound_id] = checks["summary"]
    assert bad == {}


def test_presets_copy_is_independent():
    a = kit.preset("calm")
    b = copy.deepcopy(a)
    a["roles"]["tick"]["takes"] = 99
    assert kit.preset("calm") == b
