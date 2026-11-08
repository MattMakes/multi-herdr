"""Bank tests: bank-kit-sfx AT-1, 2, 9, 10, 13, 32 and the part-1 behaviour."""

from __future__ import annotations

import json
import math

import numpy as np
import pytest

from hark.forge.bank import _measures as M
from hark.forge.bank import bank

SR = 48000


def _sine(f: float, dur: float, amp: float = 0.5) -> np.ndarray:
    t = np.arange(int(dur * SR)) / SR
    return amp * np.sin(2 * np.pi * f * t)


# ---------------------------------------------------------------- AT-1, loading and merge


def test_at_1_ids_counts_and_version_shipped(shipped, open_ids):
    b = shipped
    files = b._bank().files
    has_fit = any(f.endswith("/fit.json") for f in files.values())
    want = {"forge_v2.json": 183 - len(open_ids["dropped"]), "forge04.json": 51 - len(open_ids["forge04"]),
            "pluck.json": 3}
    tags = ["forge_v2h1", "forge04h1", "pluck1h"]
    if has_fit:  # fit.json ships only after the operator allows the 15 CC0 downloads (bank Q5)
        want["fit.json"], tags = 15, ["forge_v2h1", "forge04h1", "fit1h", "pluck1h"]
    assert b.ids() == sorted(b.ids()) and len(b.ids()) == sum(want.values())
    assert not (open_ids["forge04"] | open_ids["dropped"]) & set(b.ids())
    counts: dict[str, int] = {}
    for sound_id in b.ids():
        name = files[sound_id].rsplit("/", 1)[-1]
        counts[name] = counts.get(name, 0) + 1
    assert counts == want
    assert b.VERSION.split("+") == tags


def test_at_1_ids_counts_and_version_fixture(fixture_bank):
    ids = bank.ids()
    assert ids == sorted(ids) and len(ids) == 13
    files = bank._bank().files
    per_file = {name: sum(1 for i in ids if files[i].endswith(f"/{name}.json"))
                for name in ("forge_v2", "forge04", "fit", "pluck")}
    assert per_file == {"forge_v2": 9, "forge04": 2, "fit": 1, "pluck": 1}
    assert bank.VERSION == "fv2t+f04t+fit1t+pluck1t"


def test_hark_bank_adds_and_overrides(fixture_bank, tmp_path, monkeypatch):
    extra = tmp_path / "user.json"
    entry = bank.get("notify/m0")
    entry.pop("id")
    entry["notes"] = "user override"
    bank.write_bank(extra, {"bank": 1, "name": "user", "categories": {"notify": {"recipe": "x", "kinds": ["nope"]}},
                            "entries": {"notify/m0": entry}})
    later = tmp_path / "later.json"
    bank.write_bank(later, {"bank": 1, "name": "later", "categories": {"notify": {"recipe": "y"}}, "entries": {}})
    monkeypatch.setenv("HARK_BANK", f"{extra}:{later}:")
    assert bank.get("notify/m0")["notes"] == "user override"
    assert bank.VERSION == "fv2t+f04t+fit1t+pluck1t+user+later"
    assert bank.category_info("notify")["recipe"] == "x"


def test_category_info_fills_from_builtin_table(fixture_bank):
    info = bank.category_info("whoosh_big")
    assert "whoosh.long" in info["kinds"] and info["use"]
    assert bank.category_info("no_such") == {"recipe": None, "target": None, "kinds": [], "use": ""}


def test_at_32_missing_hark_bank_file_names_path(fixture_bank, monkeypatch):
    monkeypatch.setenv("HARK_BANK", "/no/such.json")
    with pytest.raises(bank.BankError, match="/no/such.json"):
        bank.ids()


def test_at_32_cli_list_raises_bank_error(fixture_bank, monkeypatch, cli):
    monkeypatch.setenv("HARK_BANK", "/no/such.json")
    with pytest.raises(bank.BankError, match="/no/such.json"):
        cli(["sfx", "list"])


def test_bad_bank_files(tmp_path, fixture_bank, monkeypatch):
    broken = tmp_path / "broken.json"
    broken.write_text("{not json", encoding="utf-8")
    monkeypatch.setenv("HARK_BANK", str(broken))
    with pytest.raises(bank.BankError, match="broken.json"):
        bank.ids()
    noentries = tmp_path / "noentries.json"
    noentries.write_text(json.dumps({"bank": 1}), encoding="utf-8")
    monkeypatch.setenv("HARK_BANK", str(noentries))
    with pytest.raises(bank.BankError, match="noentries.json"):
        bank.ids()


# ---------------------------------------------------------------- AT-13, lookups


def test_at_13_typo_suggests_close_id(fixture_bank):
    with pytest.raises(bank.BankError) as err:
        bank.get("click/sotf")
    msg = str(err.value)
    assert "click/soft" in msg and "Did you mean" in msg and "hark sfx list" in msg


def test_unknown_member_lists_category(fixture_bank):
    with pytest.raises(bank.BankError, match="notify has: notify/low, notify/m0"):
        bank.get("notify/zzzzzzzzzzzzzzzzzz")


def test_get_returns_deep_copy_with_id(fixture_bank):
    e = bank.get("notify/m0")
    e["params"]["f0"] = 1.0
    assert bank.get("notify/m0")["params"]["f0"] == 1046.5 and e["id"] == "notify/m0"


# ---------------------------------------------------------------- level, rate (AT-9, AT-10)


def test_at_9_peak_over_zero_refused(fixture_bank):
    with pytest.raises(bank.BankError, match="clips"):
        bank.render("notify/m0", peak_db=0.5)


def test_level_rules():
    assert bank.level() == -1.0
    assert bank.level(-1.0, 0.5) == pytest.approx(-7.0206, abs=1e-3)
    for bad in (True, "x", float("nan")):
        with pytest.raises(bank.BankError, match="peak_db must be a number"):
            bank.level(bad)
    for w in (0, -1, 4.5, float("inf")):
        with pytest.raises(bank.BankError):
            bank.level(-1.0, w)
    with pytest.raises(bank.BankError, match="lower peak_db"):
        bank.level(-1.0, 2.0)


def test_at_10_weight_half_gives_minus_7_02(fixture_bank):
    y, meta = bank.render("notify/m0", weight=0.5)
    assert 20 * math.log10(np.max(np.abs(y))) == pytest.approx(-7.02, abs=0.05)
    assert meta["edits"]["weight"] == {"weight": 0.5, "peak_db": -7.02}


def test_check_sr():
    assert bank.check_sr(48000.0) == 48000 and bank.check_sr(44100) == 44100
    for bad in (True, "48000", 22050, 48000.5):
        with pytest.raises(bank.BankError, match="44100, 48000, 88200, 96000"):
            bank.check_sr(bad)


# ---------------------------------------------------------------- rendering a take (part 1, 5)


def test_default_take_matches_meta(fixture_bank):
    entry = bank.get("whoosh_fast/crisp")
    y, meta = bank.render("whoosh_fast/crisp")
    assert y.dtype == np.float32 and y.shape[1] == 2
    assert meta["seed"] == entry["seed"] and meta["sr"] == SR and meta["edits"] == {}
    assert meta["dur_s"] == entry["meta"]["dur_s"] and meta["peak_s"] == entry["meta"]["peak_s"]
    assert meta["checks"]["ok"] in (True, False) and "summary" in meta["checks"]


def test_new_seed_is_a_new_take(fixture_bank):
    a, _ = bank.render("whoosh_fast/crisp", checks=False)
    b, _ = bank.render("whoosh_fast/crisp", seed=99, checks=False)
    c, _ = bank.render("whoosh_fast/crisp", seed=99, checks=False)
    assert a.shape == b.shape and not np.array_equal(a, b) and np.array_equal(b, c)


def test_render_accepts_entry_dict(fixture_bank):
    y, meta = bank.render(bank.get("notify/low"), checks=False)
    assert meta["id"] == "notify/low" and meta["checks"] is None


def test_unknown_type_and_silent_render(fixture_bank, test_recipes, monkeypatch):
    with pytest.raises(bank.BankError, match="unknown entry type 'tape'"):
        bank.renderer({"id": "x/y", "type": "tape"})
    entry = bank.get("notify/m0")
    monkeypatch.setitem(test_recipes["t_bell"].__dict__, "render", lambda p, r: np.zeros(100))
    with pytest.raises(bank.BankError, match="failed or is silent"):
        bank.render(entry)


def test_renderer_takes_edit_and_fit_knobs(fixture_bank):
    run = bank.renderer(bank.get("notify/m0"))
    assert np.array_equal(run(), run(None, None))
    assert not np.array_equal(run(), run({"f0": 700.0}, {"ignored": 1}))
    fit = bank.renderer(bank.get("fit/tick/domino"), seed=3)
    assert fit().ndim == 1


def test_resampled_take_length(fixture_bank):
    y, meta = bank.render("notify/m0", sr=44100, checks=False)
    dur = bank.get("notify/m0")["meta"]["dur_s"]
    assert abs(len(y) - round(dur * 44100)) <= 1 and meta["sr"] == 44100
    assert meta["dur_s"] == round(len(y) / 44100, 4)


def test_render_final_applies_palette_gain(fixture_bank):
    entry = bank.get("notify/m0")
    entry["gain_db"] = -6.0
    y = bank.render_final(entry)
    assert 20 * math.log10(np.max(np.abs(y))) == pytest.approx(-7.0, abs=0.01)
    with pytest.raises(bank.BankError, match="forge entry"):
        bank.render_final("fit/tick/domino")


def test_add_warning_rebuilds_summary():
    result = {"ok": True, "warnings": [], "summary": "hit_1.wav: clean"}
    bank.add_warning(result, None)
    assert result["ok"] is True
    bank.add_warning(result, {"code": "a", "msg": "first"})
    bank.add_warning(result, "b", "second")
    assert result["ok"] is False and result["summary"] == "hit_1.wav: 2 warnings: first; second"


# ---------------------------------------------------------------- measures (part 1, 6 and 8)


def test_times_of_a_click():
    y = np.zeros(SR)
    y[int(0.25 * SR): int(0.25 * SR) + 48] = 1.0
    t = bank.times(y)
    assert t["onset_s"] == pytest.approx(0.25, abs=0.001)
    assert t["peak_s"] == pytest.approx(0.25, abs=0.004)
    assert t["end_s"] == pytest.approx(0.256, abs=0.003) and t["dur_s"] == 1.0


def test_lufs_m_max_of_a_full_scale_sine():
    # a 997 Hz sine at 0 dBFS peak reads about -3.0 LUFS on one channel
    assert M.lufs_m_max(_sine(997, 1.0, 1.0)) == pytest.approx(-3.0, abs=0.2)
    assert M.lufs_m_max(_sine(997, 1.0), sr=44100) is None


def test_fingerprint_drops_the_quiet_tail():
    y = np.concatenate([_sine(1000, 0.2), np.zeros(SR)])
    fp = bank.fingerprint(y)
    assert fp["hop_s"] == 0.05 and len(fp["env_db"]) == 4 and max(fp["env_db"]) == 0.0


def test_compare_fp_tolerances():
    a = {"dur_s": 1.0, "peak_s": 0.1, "fp": {"env_db": [0.0, -3.0, -50.0]}}
    same = bank.compare_fp(a, {"dur_s": 1.0, "peak_s": 0.104, "fp": {"env_db": [0.0, -3.4, -10.0]}})
    assert same["ok"] and same["env_db"] == 0.4 and same["peak_diff_ms"] == 4.0
    assert not bank.compare_fp(a, {"dur_s": 1.0001, "peak_s": 0.1, "fp": a["fp"]})["ok"]
    assert not bank.compare_fp(a, {"dur_s": 1.0, "peak_s": 0.1, "fp": {"env_db": [0.0, -3.6]}})["ok"]


def test_estimate_pitch_on_tone_and_noise(rng):
    y = _sine(659.26, 0.8) * np.exp(-np.arange(int(0.8 * SR)) / SR / 0.3)
    assert bank.estimate_pitch(y, near_hz=650.0) == pytest.approx(659.26, rel=0.003)
    assert bank.estimate_pitch(y) == pytest.approx(659.26, rel=0.003)
    assert bank.estimate_pitch(rng.standard_normal(SR), near_hz=1000.0) is None
    assert bank.estimate_pitch(_sine(440, 0.005), near_hz=440.0) is None


def test_meta_of_keys():
    meta = bank.meta_of(np.column_stack([_sine(500, 0.3), _sine(500, 0.3)]))
    assert set(meta) >= {"dur_s", "onset_s", "peak_s", "end_s", "attack_ms", "centroid_hz", "lufs_m_max",
                         "channels", "fp"}
    assert meta["channels"] == 2 and meta["centroid_hz"] == pytest.approx(500, rel=0.05)


# ---------------------------------------------------------------- writer and AT-2 check


def test_writer_layout_and_atomicity(tmp_path):
    doc = {"bank": 1, "name": "x", "version": "x1", "categories": {"a": {"kinds": []}, "b": {"kinds": []}},
           "entries": {"a/1": {"seed": 1}, "b/2": {"seed": np.int64(2), "v": np.float32(0.5)}}}
    path = bank.write_bank(tmp_path / "sub" / "x.json", doc)
    lines = open(path, encoding="utf-8").read().split("\n")
    assert lines[0] == '{"bank": 1, "name": "x", "version": "x1",' and lines[1] == ' "categories": {'
    assert lines[2].startswith('  "a": ') and lines[2].endswith(",") and not lines[3].endswith(",")
    assert lines[4:6] == [" },", ' "entries": {'] and lines[-3:] == [" }", "}", ""]
    assert json.load(open(path))["entries"]["b/2"] == {"seed": 2, "v": 0.5}
    assert list(tmp_path.joinpath("sub").iterdir()) == [tmp_path / "sub" / "x.json"]


def test_at_2_bank_check_shipped(shipped):
    assert shipped.check()["ok"]


def test_at_2_bank_check_fixture(fixture_bank):
    report = bank.check()
    assert report["ok"], report["differ"]
    assert report["summary"] == "13 / 13 entries re-render as stored"


def test_check_finds_a_changed_entry(fixture_bank, tmp_path, monkeypatch):
    entry = bank.get("notify/m0")
    entry.pop("id")
    entry["params"]["length"] = 0.5
    extra = tmp_path / "changed.json"
    bank.write_bank(extra, {"bank": 1, "name": "changed", "entries": {"notify/m0": entry}})
    monkeypatch.setenv("HARK_BANK", str(extra))
    report = bank.check(["notify/m0", "notify/low"])
    assert not report["ok"] and report["differ"] == ["notify/m0"]


def test_maintenance_cli_check(fixture_bank, capsys):
    from hark.forge.bank.__main__ import main

    assert main(["check", "notify/m0"]) == 0
    assert "1 / 1 entries re-render as stored" in capsys.readouterr().out
    assert main(["check", "nope/x"]) == 1
    assert capsys.readouterr().err.startswith("bank: ")


# ---------------------------------------------------------------- builders (part 1, 7)


def _source(**kw) -> dict:
    return {"id": "fs-1", "title": "Tick", "author": "me", "license": "CC0 1.0", "url": "https://x", **kw}


def test_add_fit_writes_entry(fixture_bank, tmp_path):
    target = tmp_path / "fit_new.json"
    out = bank.add_fit({"kind": "test_sine", "f0": 3000.0, "report": {"chosen": "a", "class": "struck"}},
                       "fit/tick/new_one", _source(), bank=str(target), vary=0.3)
    doc = json.loads(target.read_text(encoding="utf-8"))
    entry = doc["entries"]["fit/tick/new_one"]
    assert doc["version"] == "fit1h" and doc["categories"]["fit"]["recipe"] is None
    assert entry["kinds"] == ["tick"] and entry["fit"]["chosen"] == "a" and "report" not in entry["model"]
    assert entry["notes"] == "fitted from Tick (CC0 1.0)" and entry["meta"]["dur_s"] > 0
    assert out["id"] == "fit/tick/new_one"
    with pytest.raises(bank.BankError, match="already in"):
        bank.add_fit({"kind": "test_sine", "f0": 3000.0}, "fit/tick/new_one", _source(), bank=str(target))


@pytest.mark.parametrize("sound_id, source, match", [
    ("fit/Tick/x", _source(), "must look like"),
    ("fit/tick/x", _source(url=""), "misses url"),
    ("fit/tick/x", _source(license="CC BY-NC 4.0"), "forbids"),
    ("fit/tick/x", _source(license="CC BY 4.0"), "not CC0"),
])
def test_add_fit_refusals(fixture_bank, tmp_path, sound_id, source, match):
    with pytest.raises(bank.BankError, match=match):
        bank.add_fit({"kind": "test_sine", "f0": 3000.0}, sound_id, source, bank=str(tmp_path / "f.json"))


def test_add_fit_vary_needs_a_vary_knob(fixture_bank, tmp_path):
    with pytest.raises(bank.BankError, match="no vary knob"):
        bank.add_fit({"kind": "test_sine", "f0": 3000.0, "tones": False}, "fit/air/x", _source(),
                     bank=str(tmp_path / "f.json"), vary=0.3)
    bank.add_fit({"kind": "test_sine", "f0": 3000.0}, "fit/air/y", _source(license="Some licence"),
                 bank=str(tmp_path / "f.json"), allow_license=True)


def test_build_from_forge_finals(fixture_bank, tmp_path):
    from hark.forge.bank import _build

    folder = tmp_path / "finals" / "notify" / "final"
    folder.mkdir(parents=True)
    picks = []
    for member in ("m0", "low"):
        entry = bank.get(f"notify/{member}")
        block = {k: entry[k] for k in ("recipe", "fn", "params", "seed", "peak_db", "gain_db")} | {"member": member}
        from hark.forge.engine import schema

        import soundfile

        soundfile.write(str(folder / f"{member}.wav"), schema.rerender(block), SR, subtype="FLOAT")
        picks.append({"name": member, "file": f"{member}.wav", "render": block})
    (folder / "report.json").write_text(json.dumps({"target": "a bell", "picks": picks}), encoding="utf-8")
    out = tmp_path / "forge_v2.json"
    doc = bank.build([str(tmp_path / "finals" / "*" / "final")], str(out))
    assert sorted(doc["entries"]) == ["notify/low", "notify/m0"] and doc["version"] == "forge_v2h1"
    assert doc["categories"]["notify"]["target"] == "a bell"
    picks[0].pop("render")
    (folder / "report.json").write_text(json.dumps({"target": "a bell", "picks": picks}), encoding="utf-8")
    with pytest.raises(bank.BankError, match="1 pick\\(s\\) refused, nothing written"):
        _build.build([str(folder)], str(tmp_path / "other.json"))
    assert not (tmp_path / "other.json").exists()


def test_build_pluck_with_real_recipe(tmp_path, hark_home):
    pytest.importorskip("hark.forge.recipes.impacts.pluck")
    doc = bank.build_pluck(str(tmp_path / "pluck.json"))
    assert sorted(doc["entries"]) == ["pluck/kalimba", "pluck/kalimba_ring", "pluck/kalimba_soft"]
    assert all(e["meta"]["dur_s"] == pytest.approx(3.0, abs=0.01) for e in doc["entries"].values())


def test_block_digest_is_stable():
    from hark.forge.bank import _build

    e = {"recipe": "r", "fn": "render", "params": {"b": 1, "a": 2.0}, "seed": 3, "meta": {"x": 1}}
    assert _build.block_digest(e) == _build.block_digest({**e, "meta": None}) and len(_build.block_digest(e)) == 10


def _catalog_with(monkeypatch, forge04: dict, targets: dict) -> None:
    cat = bank.catalog()
    monkeypatch.setitem(cat, "forge04", forge04)
    monkeypatch.setitem(cat, "targets", targets)
    monkeypatch.setitem(cat, "categories", {**cat["categories"], "notify": {**cat["categories"]["notify"],
                                                                         "recipe": "t_bell"}})


def test_build_forge04_from_palette(fixture_bank, tmp_path, monkeypatch):
    row = {"recipe": "t_bell", "fn": "render", "seed": 40, "kinds": ["bell"], "notes": "a test member"}
    _catalog_with(monkeypatch, {"notify/m9": row}, {})
    doc = bank.build_forge04(str(tmp_path / "forge04.json"))
    entry = doc["entries"]["notify/m9"]
    assert entry["params"] == {"f0": 880.0, "tau": 0.1, "length": 0.4} and entry["kinds"] == ["bell"]
    assert entry["audit"]["seeds"] == [40, 45] and entry["audit"]["clean"] >= 5 and entry["audit"]["role"] == "bell"
    assert entry["original"] == {"made": "recipe", "max_ref_clap": None, "ref": "none"}
    fresh = bank.build_forge04(str(tmp_path / "f2.json"), ref_clap={"notify/m9": 0.7})
    assert fresh["entries"]["notify/m9"]["original"] == {"made": "recipe", "max_ref_clap": 0.7}
    with pytest.raises(bank.BankError, match="over the limit"):
        bank.build_forge04(str(tmp_path / "f3.json"), ref_clap={"notify/m9": 0.97})
    _catalog_with(monkeypatch, {"notify/nope": row}, {})
    with pytest.raises(bank.BankError, match="no palette member 'nope'"):
        bank.build_forge04(str(tmp_path / "f4.json"))


def test_build_forge04_leaves_out_audit_failures(fixture_bank, test_recipes, tmp_path, monkeypatch):
    from hark.forge.bank import _build

    row = {"recipe": "t_bell", "fn": "render", "seed": 40, "kinds": ["bell"], "notes": "a test member"}
    _catalog_with(monkeypatch, {"notify/m9": row, "notify/m9b": {**row, "seed": 50}}, {})
    families = test_recipes["t_bell"].FAMILIES
    monkeypatch.setitem(families, "notify", [*families["notify"], ("m9b", {"f0": 990.0})])
    real_audit = _build._take_audit

    def audit(entry, sound_id, seed):
        got = real_audit(entry, sound_id, seed)
        return {**got, "clean": 2, "own_ok": False, "warnings": ["ring"]} if sound_id == "notify/m9b" else got

    monkeypatch.setattr(_build, "_take_audit", audit)
    lines: list[str] = []
    doc = bank.build_forge04(str(tmp_path / "forge04.json"), log=lines.append)
    assert list(doc["entries"]) == ["notify/m9"]
    assert "refused notify/m9b: take audit: 2 / 6 clean on seeds 50-55, own take not clean (ring)" in lines
    assert any(line.startswith("warning: 1 member(s) failed the take audit") for line in lines)
    target = tmp_path / "strict.json"
    with pytest.raises(bank.BankError, match="notify/m9b: take audit"):
        bank.build_forge04(str(target), strict=True)
    assert not target.exists()


def test_build_derive_logs_target_misses(fixture_bank, tmp_path, monkeypatch):
    target = {"dur_s": 2.0, "peak_s": 0.005, "centroid_hz": 880.0, "lufs_m_max": -20.0}
    _catalog_with(monkeypatch, {}, {"notify/m9": target, "fit/x/y": {}, "pluck/kalimba": {}})
    lines: list[str] = []
    doc = bank.build_derive(str(tmp_path / "forge_v2.json"), log=lines.append)
    entry = doc["entries"]["notify/m9"]
    assert list(doc["entries"]) == ["notify/m9"] and entry["seed"] == 0 and entry["mode"] == "designed"
    assert any(line.startswith("notify/m9: ") and "misses dur" in line for line in lines)
    assert lines[-1].startswith("wrote ") and "1 entries (forge_v2h1)" in lines[-1]


def test_build_derive_drops_an_id_with_no_passing_render(fixture_bank, tmp_path, monkeypatch):
    # T-41: every render fails sfx check, so the id is left out and listed under dropped with its reason
    from hark.forge.bank import _build

    target = {"dur_s": 2.0, "peak_s": 0.005, "centroid_hz": 880.0, "lufs_m_max": -20.0}
    _catalog_with(monkeypatch, {}, {"notify/m9": target})
    monkeypatch.setattr(_build, "_derived", lambda *a: None)
    lines: list[str] = []
    doc = bank.build_derive(str(tmp_path / "forge_v2.json"), log=lines.append)
    assert doc["entries"] == {} and "passes sfx check" in doc["dropped"]["notify/m9"]
    assert any(line.startswith("notify/m9: dropped: ") for line in lines)
    assert json.loads((tmp_path / "forge_v2.json").read_text())["dropped"] == doc["dropped"]


def test_derive_entry_takes_the_next_closest_passing_render(monkeypatch):
    # T-41: the first render fails sfx check; the nearest candidate that passes stands in
    from hark.forge.bank import _build

    pool = {"near": ({"p": 1}, {}), "mid": ({"p": 2}, {}), "far": ({"p": 3}, {})}
    monkeypatch.setattr(_build, "_derive_fn", lambda recipe, category: "render")
    monkeypatch.setattr(_build, "_first_source", lambda *a: ("near", {"p": 1}))
    monkeypatch.setattr(_build, "_candidates", lambda *a: pool)
    monkeypatch.setattr(_build, "_ranked", lambda *a: ["near", "mid", "far"])
    monkeypatch.setattr(_build, "_derived", lambda sid, r, fn, params: {"params": params} if params["p"] >= 2 else None)
    used: set = set()
    lines: list[str] = []
    entry = _build._derive_entry("notify/m9", {}, used, lines.append)
    assert entry["params"] == {"p": 2} and ("notify", "mid") in used
    assert lines == ["notify/m9: first render fails sfx check; next-closest passing render is notify/mid"]


def test_at_34_dropped_ids_listed_shipped(shipped, open_ids):
    doc = json.loads((shipped.PKG_DIR / "data" / "forge_v2.json").read_text(encoding="utf-8"))
    assert set(doc["dropped"]) == open_ids["dropped"] and all(doc["dropped"].values())
    assert not set(doc["dropped"]) & set(shipped.ids())


def test_maintenance_cli_build_error(fixture_bank, tmp_path, monkeypatch, capsys):
    from hark.forge.bank.__main__ import main

    _catalog_with(monkeypatch, {}, {"notify/bad": {}})
    assert main(["build", "--derive", "-o", str(tmp_path / "x.json")]) == 1
    err = capsys.readouterr().err
    assert err.startswith("bank: 1 pick(s) refused, nothing written:") and not (tmp_path / "x.json").exists()
