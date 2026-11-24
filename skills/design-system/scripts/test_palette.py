import json
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import palette

SCRIPT = os.path.join(HERE, "palette.py")

VECTORS = [
    ("TV-01", "#2563eb", "light", 10, {}, 0.2241, 0.1223, 262.88, False, "#001053", 17.51, 1.20),
    ("TV-02", "#2563eb", "light", 50, {}, 0.5690, 0.1923, 262.88, False, "#366ee6", 4.64, 4.53),
    ("TV-03", "#2563eb", "light", 90, {}, 0.9138, 0.0415, 262.88, True, "#d4e3ff", 1.29, 16.23),
    ("TV-04", "#2563eb", "dark", 50, {}, 0.5690, 0.1635, 262.88, False, "#4171d6", 4.61, 4.56),
    ("TV-05", "#dc2626", "light", 40, {}, 0.4828, 0.1884, 27.33, False, "#b01116", 7.15, 2.94),
    ("TV-06", "#dc2626", "light", 95, {}, 0.9569, 0.0215, 27.33, True, "#ffece9", 1.14, 18.43),
    ("TV-07", "#16a34a", "light", 50, {}, 0.5690, 0.1376, 149.21, False, "#2b8d47", 4.20, 5.00),
    ("TV-08", "#16a34a", "light", 80, {}, 0.8276, 0.0924, 149.21, False, "#9cd8a6", 1.64, 12.79),
    ("TV-09", "#d97706", "light", 20, {}, 0.3103, 0.0845, 51.39, True, "#502100", 13.47, 1.56),
    ("TV-10", "#d97706", "dark", 60, {}, 0.6552, 0.1433, 58.32, False, "#d07723", 3.31, 6.35),
    ("TV-11", "#0d9488", "light", 85, {}, 0.8707, 0.0626, 184.70, False, "#a6e2d9", 1.45, 14.50),
    ("TV-12", "#7c3aed", "dark", 30, {}, 0.3966, 0.1427, 293.01, False, "#4c308b", 9.99, 2.10),
    ("TV-13", "#2563eb", "dark", 20, {"mute": dict(keep=0.5, full_until=25, recover_at=60)}, 0.3103, 0.0661, 262.88, False, "#1e2f52", 13.26, 1.58),
    ("TV-14", "#2563eb", "dark", 40, {"mute": dict(keep=0.5, full_until=25, recover_at=60)}, 0.4828, 0.1116, 262.88, False, "#3c5c9d", 6.54, 3.21),
    ("TV-15", "#2563eb", "light", 50, {"vividness": 100}, 0.5690, 0.2367, 262.88, True, "#2267ff", 4.72, 4.45),
    ("TV-16", "#2563eb", "light", 50, {"vividness": 0}, 0.5690, 0.0, None, False, "#777777", 4.48, 4.69),
    ("TV-17", "#777777", "light", 50, {"kind": "neutral"}, 0.5690, 0.0, None, False, "#777777", 4.48, 4.69),
    ("TV-18", "#777777", "light", 95, {"kind": "neutral", "tint": "warm"}, 0.9569, 0.0085, 75.00, False, "#f4f0eb", 1.13, 18.51),
]


def channel_gap(a, b):
    pa = [int(a[i:i + 2], 16) for i in (1, 3, 5)]
    pb = [int(b[i:i + 2], 16) for i in (1, 3, 5)]
    return max(abs(x - y) for x, y in zip(pa, pb))


def run(*args):
    return subprocess.run(
        [sys.executable, "-I", SCRIPT, *args], capture_output=True, text=True
    )


class TestVectors(unittest.TestCase):
    def test_vectors(self):
        for vec in VECTORS:
            name, seed, mode, tone, opts, L, C, h, mapped, hexv, cw, ck = vec
            with self.subTest(vector=name):
                got = palette.stop_color(seed, mode, tone, **opts)
                self.assertAlmostEqual(got["L"], L, delta=0.001)
                self.assertAlmostEqual(got["C"], C, delta=0.002)
                if h is not None:
                    self.assertAlmostEqual(got["h"], h, delta=0.5)
                self.assertEqual(got["mapped"], mapped)
                self.assertLessEqual(channel_gap(got["hex"], hexv), 1)
                self.assertAlmostEqual(palette.contrast_ratio(got["hex"], "#ffffff"), cw, delta=0.03)
                self.assertAlmostEqual(palette.contrast_ratio(got["hex"], "#000000"), ck, delta=0.03)

    def test_tone_to_lightness(self):
        for tone, expected in [(5, 0.1769), (10, 0.2241), (20, 0.3103), (50, 0.5690), (80, 0.8276), (90, 0.9138), (95, 0.9569)]:
            with self.subTest(tone=tone):
                self.assertAlmostEqual(palette.tone_lightness(tone), expected, delta=0.0005)

    def test_ends_are_black_and_white(self):
        self.assertEqual(palette.stop_color("#2563eb", "light", 0)["hex"], "#000000")
        self.assertEqual(palette.stop_color("#2563eb", "dark", 100)["hex"], "#ffffff")

    def test_mute_leaves_light_and_neutral_alone(self):
        mute = dict(keep=0.5, full_until=25, recover_at=60)
        plain = palette.stop_color("#2563eb", "light", 20)
        muted = palette.stop_color("#2563eb", "light", 20, mute=mute)
        self.assertEqual(plain["hex"], muted["hex"])


class TestContrast(unittest.TestCase):
    def test_known_ratio(self):
        self.assertAlmostEqual(palette.contrast_ratio("#2563eb", "#ffffff"), 5.17, delta=0.01)
        self.assertAlmostEqual(palette.contrast_ratio("#000000", "#ffffff"), 21.0, delta=0.001)

    def test_report_truncates(self):
        self.assertEqual(palette.truncate_ratio(4.499), 4.49)
        self.assertEqual(palette.truncate_ratio(4.5), 4.5)

    def test_composite(self):
        self.assertEqual(palette.composite("#000000", 0.5, "#ffffff"), "#808080")


class TestValidation(unittest.TestCase):
    def good(self, **over):
        req = {"families": [{"id": "accent", "seed": "#2563eb"}]}
        req.update(over)
        return req

    def test_rejects(self):
        bad = [
            {"families": []},
            {"families": [{"id": "Accent", "seed": "#2563eb"}]},
            {"families": [{"id": "black", "seed": "#2563eb"}]},
            {"families": [{"id": "white", "seed": "#2563eb"}]},
            {"families": [{"id": "a", "seed": "#2563eb"}, {"id": "a", "seed": "#dc2626"}]},
            {"families": [{"id": "a", "seed": "#25"}]},
            {"families": [{"id": "a", "seed": "#2563eb"}], "vividness": 101},
            {"families": [{"id": "a", "seed": "#2563eb"}], "stops": [0, 50, 101]},
            {"families": [{"id": "a", "seed": "#2563eb"}], "stops": [0, 50, 50]},
        ]
        for req in bad:
            with self.subTest(req=req):
                with self.assertRaises(palette.PaletteError):
                    palette.normalize_request(req)

    def test_normalizes(self):
        req = palette.normalize_request(self.good())
        self.assertEqual(req["families"][0]["seed"], "#2563eb")
        self.assertEqual(req["vividness"], 50)
        self.assertEqual(req["modes"], ["light", "dark"])
        self.assertEqual(req["stops"], list(range(0, 101, 5)))
        short = palette.normalize_request({"families": [{"id": "a", "seed": "ABC"}]})
        self.assertEqual(short["families"][0]["seed"], "#aabbcc")

    def test_deterministic(self):
        a = json.dumps(palette.generate(self.good()), sort_keys=True)
        b = json.dumps(palette.generate(self.good()), sort_keys=True)
        self.assertEqual(a, b)


class TestGenerate(unittest.TestCase):
    SEEDS = ["--seed", "accent=#2563eb", "--seed", "danger=#dc2626", "--seed", "success=#15803d", "--seed", "warning=#b45309", "--seed", "info=#0d9488"]

    def test_ramp_is_monotonic(self):
        out = palette.generate({"families": [{"id": "accent", "seed": "#2563eb"}]})
        ramp = out["families"]["accent"]["light"]
        lums = [palette.luminance(ramp[str(t)]) for t in sorted(int(k) for k in ramp)]
        self.assertEqual(lums, sorted(lums))
        self.assertTrue(out["diagnostics"]["accent"]["light"]["monotonic"])

    def test_roles_cover_shadcn_names(self):
        req = {"families": [
            {"id": "accent", "seed": "#2563eb"},
            {"id": "danger", "seed": "#dc2626"},
            {"id": "success", "seed": "#16a34a"},
            {"id": "warning", "seed": "#d97706"},
            {"id": "info", "seed": "#0d9488"},
        ]}
        out = palette.generate(req)
        for mode in ("light", "dark"):
            roles = out["roles"][mode]
            for name in ("background", "foreground", "card", "muted", "muted-foreground", "border", "input", "ring",
                         "primary", "primary-foreground", "destructive", "destructive-foreground",
                         "success", "success-foreground", "warning", "warning-foreground", "info", "info-foreground"):
                self.assertIn(name, roles)

    def test_generated_roles_pass_check(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "ramp.json")
            res = run("ramp", *self.SEEDS, "--json", path)
            self.assertEqual(res.returncode, 0, res.stderr)
            checked = run("check", path)
            self.assertEqual(checked.returncode, 0, checked.stdout)
            self.assertIn("pass", checked.stdout)
            self.assertNotIn("FAIL", checked.stdout)


class TestCli(unittest.TestCase):
    def test_help_exits_zero(self):
        for sub in ("ramp", "contrast", "check"):
            with self.subTest(sub=sub):
                self.assertEqual(run(sub, "--help").returncode, 0)

    def test_contrast_command(self):
        res = run("contrast", "#2563eb", "#ffffff")
        self.assertEqual(res.returncode, 0)
        self.assertEqual(res.stdout.strip(), "5.16")

    def test_ramp_css_output(self):
        with tempfile.TemporaryDirectory() as tmp:
            css = os.path.join(tmp, "p.css")
            res = run("ramp", "--seed", "accent=#2563eb", "--css", css)
            self.assertEqual(res.returncode, 0, res.stderr)
            text = open(css).read()
            self.assertIn(":root", text)
            self.assertIn(".dark", text)
            self.assertIn("--color-accent-50:", text)

    def test_ramp_rejects_bad_seed(self):
        res = run("ramp", "--seed", "accent=nothex")
        self.assertNotEqual(res.returncode, 0)

    def test_check_fails_on_low_contrast(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "tokens.json")
            with open(path, "w") as fh:
                json.dump({"light": {"background": "#ffffff", "foreground": "#cccccc"}}, fh)
            res = run("check", path)
            self.assertEqual(res.returncode, 1)
            self.assertIn("FAIL", res.stdout)

    def test_check_passes_on_good_pair(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "tokens.json")
            with open(path, "w") as fh:
                json.dump({"light": {"background": "#ffffff", "foreground": "#111111"}}, fh)
            self.assertEqual(run("check", path).returncode, 0)


if __name__ == "__main__":
    unittest.main()
