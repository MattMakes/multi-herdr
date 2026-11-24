import argparse
import hashlib
import json
import math
import re
import sys

METHOD = "seeded-tone-ramps"
METHOD_VERSION = "1"
BUILD_ID = "palette.py-1"
RECEIPT_SCHEMA = 1

DEFAULT_STOPS = list(range(0, 101, 5))
ID_PATTERN = re.compile(r"^[a-z][a-z0-9-]*$")
HEX_PATTERN = re.compile(r"^#?([0-9a-fA-F]{3}|[0-9a-fA-F]{6})$")
TINTS = {"none": (0.0, 0.0), "warm": (0.018, 75.0), "cool": (0.018, 250.0)}
STATUS_ROLES = {"danger": "destructive", "success": "success", "warning": "warning", "info": "info"}


class PaletteError(ValueError):
    pass


def to_linear(c):
    c = c / 255.0
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def from_linear(c):
    c = max(0.0, c)
    c = c * 12.92 if c <= 0.0031308 else 1.055 * c ** (1 / 2.4) - 0.055
    return min(255, max(0, round(c * 255)))


def normalize_hex(value):
    m = HEX_PATTERN.match(str(value).strip())
    if not m:
        raise PaletteError("not a hex colour: %r" % (value,))
    digits = m.group(1).lower()
    if len(digits) == 3:
        digits = "".join(ch * 2 for ch in digits)
    return "#" + digits


def hex_to_rgb(value):
    h = normalize_hex(value)
    return tuple(int(h[i:i + 2], 16) for i in (1, 3, 5))


def rgb_to_hex(rgb):
    return "#%02x%02x%02x" % tuple(rgb)


def hex_to_linear(value):
    return tuple(to_linear(c) for c in hex_to_rgb(value))


def luminance(value):
    r, g, b = hex_to_linear(value)
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def contrast_ratio(a, b):
    hi, lo = sorted((luminance(a), luminance(b)), reverse=True)
    return (hi + 0.05) / (lo + 0.05)


def truncate_ratio(ratio):
    return math.floor(ratio * 100 + 1e-9) / 100


def composite(top, alpha, under):
    t = hex_to_rgb(top)
    u = hex_to_rgb(under)
    return rgb_to_hex([max(0, min(255, round(t[i] * alpha + u[i] * (1 - alpha)))) for i in range(3)])


def linear_to_oklab(r, g, b):
    l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b
    m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b
    s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b
    l_, m_, s_ = (math.copysign(abs(x) ** (1 / 3), x) for x in (l, m, s))
    return (
        0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_,
        1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_,
        0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_,
    )


def oklab_to_linear(L, a, b):
    l_ = L + 0.3963377774 * a + 0.2158037573 * b
    m_ = L - 0.1055613458 * a - 0.0638541728 * b
    s_ = L - 0.0894841775 * a - 1.2914855480 * b
    l, m, s = l_ ** 3, m_ ** 3, s_ ** 3
    return (
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    )


def oklch_to_linear(L, C, h):
    rad = math.radians(h)
    return oklab_to_linear(L, C * math.cos(rad), C * math.sin(rad))


def hex_to_oklch(value):
    L, a, b = linear_to_oklab(*hex_to_linear(value))
    C = math.hypot(a, b)
    h = math.degrees(math.atan2(b, a)) % 360 if C > 1e-9 else 0.0
    return L, C, h


def oklch_to_hex(L, C, h):
    lin = oklch_to_linear(L, C, h)
    return rgb_to_hex([from_linear(c) for c in lin])


def in_gamut(lin):
    return all(-0.001 <= c <= 1.001 for c in lin)


def max_chroma(L, h):
    lo, hi = 0.0, 0.4
    if in_gamut(oklch_to_linear(L, hi, h)):
        return hi
    for _ in range(20):
        mid = (lo + hi) / 2
        if in_gamut(oklch_to_linear(L, mid, h)):
            lo = mid
        else:
            hi = mid
    return lo


def delta_e(a, b):
    la = linear_to_oklab(*hex_to_linear(a))
    lb = linear_to_oklab(*hex_to_linear(b))
    return 100 * math.dist(la, lb)


def smoothstep(x):
    x = min(1.0, max(0.0, x))
    return x * x * (3 - 2 * x)


def tone_lightness(t):
    if t <= 0:
        return 0.0
    if t >= 100:
        return 1.0
    y = ((t + 16) / 116) ** 3 if t > 8 else t / 903.3
    return y ** (1 / 3)


def vividness_factor(v):
    if v <= 25:
        return 0.72 * v / 25
    if v <= 50:
        return 0.72 + 0.28 * (v - 25) / 25
    return 1 + 0.0096 * (v - 50)


def stop_hue(hs, t):
    if 40 <= hs < 70 and t < 50:
        return hs - 8 * math.sqrt(min(1.0, (50 - t) / 40))
    return hs


def balance_factor(h):
    if 70 <= h < 115:
        return 0.94
    if 115 <= h < 175:
        return 0.78
    if 175 <= h < 230:
        return 0.82
    if 285 <= h < 340:
        return 0.90
    return 1.0


def pale_factor(h, t):
    if t <= 60:
        return 1.0
    k = smoothstep((t - 60) / 35)
    if 115 <= h < 175:
        return 1 - 0.28 * k
    if 175 <= h < 200:
        return 1 - 0.40 * k
    return 1.0


def tone_envelope(t):
    return 0.18 + 0.82 * math.sqrt(max(0.0, math.sin(math.pi * t / 100)))


def mute_multiplier(t, mute):
    keep, lo, hi = mute["keep"], mute["full_until"], mute["recover_at"]
    if t <= lo:
        return keep
    if t >= hi:
        return 1.0
    return keep + (1 - keep) * smoothstep((t - lo) / (hi - lo))


def family_source(seed, kind, tint):
    L, C, h = hex_to_oklch(seed)
    if kind == "neutral":
        if tint == "from-seed":
            return C, h
        return TINTS[tint]
    return C, h


def stop_color(seed, mode, tone, vividness=50, kind="chromatic", tint="none", mute=None):
    seed = normalize_hex(seed)
    if tone <= 0:
        return {"L": 0.0, "C": 0.0, "h": 0.0, "mapped": False, "hex": "#000000"}
    if tone >= 100:
        return {"L": 1.0, "C": 0.0, "h": 0.0, "mapped": False, "hex": "#ffffff"}
    Cs, hs = family_source(seed, kind, tint)
    L = tone_lightness(tone)
    h = stop_hue(hs, tone)
    base = Cs if kind == "neutral" else 0.35 * Cs + 0.117
    wanted = (
        base
        * vividness_factor(vividness)
        * balance_factor(h)
        * pale_factor(h, tone)
        * tone_envelope(tone)
        * (1.0 if mode == "light" else 0.85)
    )
    cmax = max_chroma(L, h)
    mapped = wanted > cmax and not in_gamut(oklch_to_linear(L, wanted, h))
    C = cmax if mapped else wanted
    if mute and mode == "dark" and kind == "chromatic":
        C = min(wanted, cmax) * mute_multiplier(tone, mute)
    return {"L": L, "C": C, "h": h, "mapped": mapped, "hex": oklch_to_hex(L, C, h)}


def normalize_request(req):
    families = req.get("families") or []
    if not families:
        raise PaletteError("the family list is empty")
    seen = set()
    out_families = []
    for fam in families:
        fid = fam.get("id", "")
        if not ID_PATTERN.match(fid) or fid in ("black", "white"):
            raise PaletteError("bad family id: %r" % (fid,))
        if fid in seen:
            raise PaletteError("duplicate family id: %s" % fid)
        seen.add(fid)
        kind = fam.get("kind", "chromatic")
        if kind not in ("chromatic", "neutral"):
            raise PaletteError("bad kind for %s: %r" % (fid, kind))
        out_families.append({"id": fid, "seed": normalize_hex(fam.get("seed", "")), "kind": kind, "name": fam.get("name", fid)})
    vividness = req.get("vividness", 50)
    if not isinstance(vividness, (int, float)) or not 0 <= vividness <= 100:
        raise PaletteError("vividness must be in [0, 100]")
    tint = req.get("gray_tint", "none")
    if tint not in ("none", "warm", "cool", "from-seed"):
        raise PaletteError("bad gray tint: %r" % (tint,))
    modes = req.get("modes") or ["light", "dark"]
    if any(m not in ("light", "dark") for m in modes):
        raise PaletteError("modes must be light or dark")
    stops = req.get("stops") or DEFAULT_STOPS
    if any(not isinstance(s, (int, float)) or not 0 <= s <= 100 for s in stops):
        raise PaletteError("a stop is outside [0, 100]")
    if any(b <= a for a, b in zip(stops, stops[1:])):
        raise PaletteError("stops must be strictly increasing")
    mute = req.get("mute")
    if mute is not None:
        allowed = {"keep", "full_until", "recover_at", "families"}
        if set(mute) - allowed:
            raise PaletteError("unknown mute field")
        if not 0 <= mute["keep"] <= 1:
            raise PaletteError("mute keep must be in [0, 1]")
        if mute["full_until"] not in stops or mute["recover_at"] not in stops:
            raise PaletteError("mute stops must be in the stop list")
        if mute["recover_at"] <= mute["full_until"]:
            raise PaletteError("recover_at must be after full_until")
        by_id = {f["id"]: f for f in out_families}
        for fid in mute.get("families", {}):
            if fid not in by_id:
                raise PaletteError("mute names unknown family: %s" % fid)
            if by_id[fid]["kind"] == "neutral":
                raise PaletteError("mute names neutral family: %s" % fid)
    return {
        "families": out_families,
        "vividness": vividness,
        "gray_tint": tint,
        "modes": [m for m in ("light", "dark") if m in modes],
        "mute": mute,
        "stops": list(stops),
    }


def family_mute(req, fid):
    mute = req["mute"]
    if not mute:
        return None
    keep = mute.get("families", {}).get(fid, mute["keep"])
    return {"keep": keep, "full_until": mute["full_until"], "recover_at": mute["recover_at"]}


def build_ramp(req, fam, mode):
    ramp, mapped = {}, []
    for t in req["stops"]:
        col = stop_color(
            fam["seed"], mode, t, req["vividness"], fam["kind"], req["gray_tint"], family_mute(req, fam["id"])
        )
        ramp[str(t)] = col["hex"]
        if col["mapped"]:
            mapped.append(t)
    return ramp, mapped


def hue_diff(a, b):
    return (a - b + 180) % 360 - 180


def ramp_diagnostics(ramp, mapped, stops):
    lums = [luminance(ramp[str(t)]) for t in stops]
    monotonic = all(b >= a - 1e-6 for a, b in zip(lums, lums[1:]))
    steps = [delta_e(ramp[str(a)], ramp[str(b)]) for a, b in zip(stops, stops[1:])]
    ref = min(stops, key=lambda t: abs(t - 50))
    ref_h = hex_to_oklch(ramp[str(ref)])[2]
    drift, risks = 0.0, []
    for t in stops:
        L, C, h = hex_to_oklch(ramp[str(t)])
        if C < 0.015:
            continue
        d = hue_diff(h, ref_h)
        drift = max(drift, abs(d))
        if 230 <= ref_h < 285 and d > 12 and "blue-to-purple" not in risks:
            risks.append("blue-to-purple")
        if 70 <= ref_h < 115 and d < -12 and "yellow-to-brown" not in risks:
            risks.append("yellow-to-brown")
    return {
        "monotonic": monotonic,
        "min_delta_e": round(min(steps), 2),
        "max_delta_e": round(max(steps), 2),
        "max_hue_drift": round(drift, 2),
        "risks": risks,
        "mapped_stops": mapped,
    }


def balance_report(ramps, req, mode):
    ref = min(req["stops"], key=lambda t: abs(t - 50))
    chroma = {
        f["id"]: hex_to_oklch(ramps[f["id"]][mode][str(ref)])
        for f in req["families"]
        if f["kind"] == "chromatic"
    }
    ids = sorted(chroma)
    if len(ids) < 2:
        return None
    pairs = [
        (round(delta_e(ramps[a][mode][str(ref)], ramps[b][mode][str(ref)]), 2), a, b)
        for i, a in enumerate(ids)
        for b in ids[i + 1:]
    ]
    closest = min(pairs)
    most = max(ids, key=lambda i: chroma[i][1])
    least = min(ids, key=lambda i: chroma[i][1])
    ratio = chroma[most][1] / chroma[least][1] if chroma[least][1] > 1e-9 else None
    return {
        "closest_pair": [closest[1], closest[2]],
        "closest_delta_e": closest[0],
        "most_chromatic": most,
        "least_chromatic": least,
        "chroma_ratio": None if ratio is None else round(ratio, 2),
    }


def nearest_stop(stops, tone, inner=False):
    pool = [s for s in stops if 0 < s < 100] if inner else stops
    return min(pool, key=lambda s: (abs(s - tone), abs(s - 50)))


def seed_stop(req, fam):
    L, C, h = hex_to_oklch(fam["seed"])
    best, best_de = None, None
    for t in req["stops"]:
        if t <= 0 or t >= 100:
            continue
        col = stop_color(fam["seed"], "light", t, req["vividness"], fam["kind"], req["gray_tint"])
        de = delta_e(col["hex"], fam["seed"])
        if best_de is None or de < best_de:
            best, best_de = t, de
    return best


class RoleBuilder:
    def __init__(self, req, ramps, mode, neutral_id, seed_stops):
        self.req = req
        self.ramps = ramps
        self.mode = mode
        self.stops = req["stops"]
        self.neutral = ramps[neutral_id][mode]
        self.seed_stops = seed_stops
        self.warnings = []
        self.checks = []
        self.pick = self.pick_mode

    def pick_mode(self, light, dark):
        return light if self.mode == "light" else dark

    def n(self, light, dark=None):
        tone = self.pick_mode(light, light if dark is None else dark)
        return self.neutral[str(nearest_stop(self.stops, tone))]

    def surfaces(self):
        return {
            "background": self.n(95, 10),
            "surface": self.n(100, 15),
            "card": self.n(100, 10),
        }

    def grounds_for(self, extra=None):
        base = list(self.surfaces().values())
        if extra is None:
            return base
        return [composite(extra[0], extra[1], g) if extra[1] < 1 else extra[0] for g in base]

    def search(self, ramp, start_tone, need, grounds, token, preferred=None):
        order = sorted(
            self.stops,
            key=lambda s: (abs(s - start_tone), 0 if (s < start_tone) == (self.mode == "light") else 1),
        )
        best, best_ratio = None, -1.0
        for s in order:
            ratio = min(contrast_ratio(ramp[str(s)], g) for g in grounds)
            if ratio >= need:
                return ramp[str(s)], s, ratio, True
            if ratio > best_ratio:
                best, best_ratio = s, ratio
        self.warnings.append("%s %s: no stop reaches %.1f:1, best %.2f" % (self.mode, token, need, best_ratio))
        return ramp[str(best)], best, best_ratio, False

    def content_on(self, fill, prefer_dark_first=False):
        dark_first = prefer_dark_first or self.mode == "dark"
        first_tone, second_tone = (5, 100) if dark_first else (100, 5)
        first = self.neutral[str(nearest_stop(self.stops, first_tone))]
        second = self.neutral[str(nearest_stop(self.stops, second_tone))]
        r1, r2 = contrast_ratio(first, fill), contrast_ratio(second, fill)
        if r1 >= 4.5 or r1 >= r2:
            best, best_r = first, r1
        else:
            best, best_r = second, r2
        if best_r < 4.5:
            end = self.neutral[str(self.stops[0])] if luminance(fill) > 0.18 else self.neutral[str(self.stops[-1])]
            if contrast_ratio(end, fill) > best_r:
                best = end
        return best

    def darker_fill(self, fill, surfaces):
        r, g, b = hex_to_rgb(fill)
        for i in range(1, 101):
            cand = rgb_to_hex([round(c * (1 - i / 100)) for c in (r, g, b)])
            if all(contrast_ratio(cand, s) >= 3 for s in surfaces):
                return cand
        return "#000000"

    def family_roles(self, fid, intent, is_accent):
        ramp = self.ramps[fid][self.mode]
        surfaces = list(self.surfaces().values())
        seed = next(f["seed"] for f in self.req["families"] if f["id"] == fid)
        seed_t = self.seed_stops[fid]
        prefer_dark = fid == "warning"
        if self.mode == "light":
            fill, exact = seed, True
        else:
            on_seed = self.content_on(seed, prefer_dark)
            if contrast_ratio(on_seed, seed) >= 4.5 and all(contrast_ratio(seed, s) >= 3 for s in surfaces):
                fill, exact = seed, True
            else:
                exact = False
                fill = ramp[str(nearest_stop(self.stops, 70, inner=True))]
                inner = sorted(
                    [t for t in self.stops if 0 < t < 100],
                    key=lambda s: (abs(s - 70), -s),
                )
                for t in inner:
                    cand = ramp[str(t)]
                    if all(contrast_ratio(cand, s) >= 3 for s in surfaces) and contrast_ratio(self.content_on(cand, prefer_dark), cand) >= 4.5:
                        fill = cand
                        break
                else:
                    self.warnings.append("dark %s fill: no stop passes, using stop 70" % intent)
        if self.mode == "light" and any(contrast_ratio(fill, s) < 3 for s in surfaces):
            self.warnings.append(
                "light %s fill %s is under 3:1 against a surface; first darker colour that passes: %s"
                % (intent, fill, self.darker_fill(fill, surfaces))
            )
        on_fill = self.content_on(fill, prefer_dark)
        if is_accent:
            tint_tone = self.pick_mode(85, 30)
            tint_stop = nearest_stop(self.stops, tint_tone)
            tint = ramp[str(tint_stop)]
            tint_over = {"background": tint}
        else:
            if self.mode == "light":
                tint_stop = nearest_stop(self.stops, 85)
                alpha = 1.0
            else:
                tint_stop = nearest_stop(self.stops, 75)
                alpha = 0x3d / 255
            tint_src = ramp[str(tint_stop)]
            tint = composite(tint_src, alpha, self.surfaces()["background"]) if alpha < 1 else tint_src
        tint_grounds = [tint] if is_accent else [
            composite(ramp[str(tint_stop)], 0x3d / 255, s) if self.mode == "dark" else ramp[str(tint_stop)] for s in surfaces
        ]
        text_grounds = surfaces + tint_grounds
        start_tone = seed_t if exact else self.pick_mode(45, 80)
        start_hex = fill if exact else None
        if start_hex and all(contrast_ratio(start_hex, g) >= 4.5 for g in text_grounds):
            text = start_hex
        else:
            text, _, _, _ = self.search(ramp, start_tone, 4.5, text_grounds, "%s text" % intent)
        on_tint, _, _, _ = self.search(ramp, self.pick_mode(25, 80), 4.5, tint_grounds, "%s on tint" % intent)
        ring, _, _, _ = self.search(ramp, seed_t, 3.0, surfaces, "%s ring" % intent)
        border, _, _, _ = self.search(ramp, self.pick_mode(80, 65), 3.0, surfaces, "%s border" % intent)
        return {
            "fill": fill,
            "on_fill": on_fill,
            "tint": tint,
            "text": text,
            "on_tint": on_tint,
            "ring": ring,
            "border": border,
        }

    def build(self, accent_id, status_ids):
        roles = {}
        s = self.surfaces()
        roles["background"] = s["background"]
        roles["card"] = s["card"]
        roles["popover"] = self.n(100, 10)
        roles["muted"] = self.n(95, 10)
        roles["secondary"] = roles["muted"]
        roles["foreground"] = self.n(0, 100)
        roles["card-foreground"] = roles["foreground"]
        roles["popover-foreground"] = roles["foreground"]
        roles["secondary-foreground"] = roles["foreground"]
        surfaces = list(s.values())
        sec, _, _, _ = self.search(self.neutral, self.pick_mode(30, 65), 4.5, surfaces + [roles["muted"]], "muted-foreground")
        roles["muted-foreground"] = sec
        roles["border"] = composite(
            self.n(0, 100), 0x14 / 255 if self.mode == "light" else 0x1a / 255, roles["background"]
        )
        edge, _, _, _ = self.search(self.neutral, self.pick_mode(60, 50), 3.0, surfaces, "input")
        roles["input"] = edge
        roles["inverted"] = self.n(10, 95)
        roles["inverted-foreground"] = self.n(95, 10)
        if accent_id:
            r = self.family_roles(accent_id, "primary", True)
            roles["primary"] = r["fill"]
            roles["primary-foreground"] = r["on_fill"]
            roles["accent"] = r["tint"]
            roles["accent-foreground"] = r["on_tint"]
            roles["primary-text"] = r["text"]
            roles["ring"] = r["ring"]
        else:
            roles["ring"] = edge
        for fid, intent in status_ids.items():
            r = self.family_roles(fid, intent, False)
            roles[intent] = r["fill"]
            roles[intent + "-foreground"] = r["on_fill"]
            roles[intent + "-muted"] = r["tint"]
            roles[intent + "-muted-foreground"] = r["on_tint"]
            roles[intent + "-text"] = r["text"]
            roles[intent + "-border"] = r["border"]
        return roles


def generate(request):
    req = normalize_request(request)
    families = list(req["families"])
    ids = {f["id"] for f in families}
    neutral_id = next((f["id"] for f in families if f["kind"] == "neutral"), None)
    if neutral_id is None:
        neutral_id = "neutral"
        while neutral_id in ids:
            neutral_id += "-0"
        families.append({"id": neutral_id, "seed": "#808080", "kind": "neutral", "name": neutral_id})
        req["families"] = families
    ramps, mapped, diagnostics = {}, {}, {}
    for fam in families:
        ramps[fam["id"]], diagnostics[fam["id"]] = {}, {}
        for mode in req["modes"]:
            ramp, m = build_ramp(req, fam, mode)
            ramps[fam["id"]][mode] = ramp
            diagnostics[fam["id"]][mode] = ramp_diagnostics(ramp, m, req["stops"])
    balance = {m: balance_report(ramps, req, m) for m in req["modes"]}
    seed_stops = {f["id"]: seed_stop(req, f) for f in families if f["kind"] == "chromatic"}
    accent_id = "accent" if "accent" in ids else None
    status_ids = {fid: STATUS_ROLES[fid] for fid in STATUS_ROLES if fid in ids}
    roles, warnings, ramp_report = {}, [], {}
    for mode in req["modes"]:
        builder = RoleBuilder(req, ramps, mode, neutral_id, seed_stops)
        roles[mode] = builder.build(accent_id, status_ids)
        warnings.extend(builder.warnings)
    for fid, per_mode in diagnostics.items():
        for mode, diag in per_mode.items():
            if not diag["monotonic"]:
                warnings.append("%s %s ramp is not monotonic" % (fid, mode))
    body = {"families": ramps, "roles": roles}
    digest = hashlib.sha256(json.dumps(body, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
    return {
        "receipt": {
            "schema": RECEIPT_SCHEMA,
            "method": METHOD,
            "method_version": METHOD_VERSION,
            "build": BUILD_ID,
            "sha256": digest,
            "request": req,
        },
        "families": ramps,
        "seed_stops": seed_stops,
        "roles": roles,
        "diagnostics": diagnostics,
        "balance": balance,
        "warnings": warnings,
    }


def css_block(selector, lines):
    return "%s {\n%s\n}\n" % (selector, "\n".join("  %s" % l for l in lines))


def to_css(out):
    light = out["receipt"]["request"]["modes"]
    blocks = []
    for mode in light:
        selector = ":root" if mode == "light" else ".dark"
        lines = []
        for fid in sorted(out["families"]):
            ramp = out["families"][fid][mode]
            for t in sorted(ramp, key=int):
                lines.append("--color-%s-%s: %s;" % (fid, t, ramp[t]))
        for name in sorted(out["roles"][mode]):
            lines.append("--%s: %s;" % (name, out["roles"][mode][name]))
        blocks.append(css_block(selector, lines))
    return "\n".join(blocks)


ROLE_PAIRS = [
    ("foreground", "background", 4.5),
    ("foreground", "card", 4.5),
    ("card-foreground", "card", 4.5),
    ("popover-foreground", "popover", 4.5),
    ("muted-foreground", "background", 4.5),
    ("muted-foreground", "muted", 4.5),
    ("secondary-foreground", "secondary", 4.5),
    ("primary-foreground", "primary", 4.5),
    ("accent-foreground", "accent", 4.5),
    ("primary-text", "background", 4.5),
    ("inverted-foreground", "inverted", 4.5),
    ("primary", "background", 3.0),
    ("input", "background", 3.0),
    ("input", "card", 3.0),
    ("ring", "background", 3.0),
    ("ring", "card", 3.0),
]
for _intent in ("destructive", "success", "warning", "info"):
    ROLE_PAIRS += [
        (_intent + "-foreground", _intent, 4.5),
        (_intent + "-muted-foreground", _intent + "-muted", 4.5),
        (_intent + "-text", "background", 4.5),
        (_intent + "-text", "card", 4.5),
        (_intent + "-text", _intent + "-muted", 4.5),
        (_intent + "-border", "background", 3.0),
        (_intent, "background", 3.0),
    ]


def resolve(value, ground):
    if value is None:
        return None
    digits = value.lstrip("#")
    if len(digits) == 8:
        alpha = int(digits[6:], 16) / 255
        return composite("#" + digits[:6], alpha, ground)
    return normalize_hex(value)


def check_tokens(tokens):
    results = []
    for mode in ("light", "dark"):
        set_ = tokens.get(mode)
        if not set_:
            continue
        page = resolve(set_.get("background"), "#ffffff") if set_.get("background") else "#ffffff"
        for fg, bg, need in ROLE_PAIRS:
            if fg not in set_ or bg not in set_:
                continue
            ground = resolve(set_[bg], page)
            color = resolve(set_[fg], ground)
            ratio = truncate_ratio(contrast_ratio(color, ground))
            results.append((mode, fg, bg, ratio, need, ratio >= need))
    return results


def load_tokens(path):
    with open(path) as fh:
        data = json.load(fh)
    if "roles" in data:
        return data["roles"]
    return data


def cmd_ramp(args):
    families = []
    for item in args.seed:
        if "=" not in item:
            raise PaletteError("seed must be id=hex: %r" % item)
        fid, value = item.split("=", 1)
        families.append({"id": fid, "seed": value})
    for item in args.neutral or []:
        if "=" not in item:
            raise PaletteError("neutral must be id=hex: %r" % item)
        fid, value = item.split("=", 1)
        families.append({"id": fid, "seed": value, "kind": "neutral"})
    request = {
        "families": families,
        "vividness": args.vividness,
        "gray_tint": args.gray_tint,
        "modes": ["light", "dark"] if args.modes == "both" else [args.modes],
    }
    if args.stops:
        request["stops"] = [int(s) for s in args.stops.split(",")]
    if args.mute_keep is not None:
        request["mute"] = {
            "keep": args.mute_keep,
            "full_until": args.mute_full_until,
            "recover_at": args.mute_recover_at,
        }
    out = generate(request)
    text = json.dumps(out, indent=2, sort_keys=True)
    if args.json:
        with open(args.json, "w") as fh:
            fh.write(text + "\n")
    if args.css:
        with open(args.css, "w") as fh:
            fh.write(to_css(out))
    if not args.json and not args.css:
        print(text)
    for w in out["warnings"]:
        print("warning: " + w, file=sys.stderr)
    return 0


def cmd_contrast(args):
    ratio = contrast_ratio(args.foreground, args.background)
    print("%.2f" % truncate_ratio(ratio))
    return 0


def cmd_check(args):
    results = check_tokens(load_tokens(args.tokens))
    if not results:
        print("no checkable role pairs found")
        return 1
    failed = 0
    for mode, fg, bg, ratio, need, ok in results:
        print("%s %s on %s %.2f need %.1f %s" % (mode, fg, bg, ratio, need, "pass" if ok else "FAIL"))
        failed += 0 if ok else 1
    return 1 if failed else 0


def build_parser():
    p = argparse.ArgumentParser(prog="palette.py", description="Seeded palettes, contrast and role checks.")
    sub = p.add_subparsers(dest="command", required=True)
    r = sub.add_parser("ramp", help="generate ramps and roles from seed colours")
    r.add_argument("--seed", action="append", required=True, metavar="ID=HEX", help="family seed, repeatable; ids accent, danger, success, warning, info also set roles")
    r.add_argument("--neutral", action="append", metavar="ID=HEX", help="neutral family seed; default is a gray family named neutral")
    r.add_argument("--vividness", type=float, default=50, help="0 to 100, default 50")
    r.add_argument("--gray-tint", default="none", choices=["none", "warm", "cool", "from-seed"])
    r.add_argument("--modes", default="both", choices=["light", "dark", "both"])
    r.add_argument("--stops", help="comma list of tones, default 0,5,...,100")
    r.add_argument("--mute-keep", type=float, help="dark-edge mute: share of chroma kept")
    r.add_argument("--mute-full-until", type=int, default=25)
    r.add_argument("--mute-recover-at", type=int, default=60)
    r.add_argument("--json", metavar="PATH", help="write JSON here; stdout when no output option is given")
    r.add_argument("--css", metavar="PATH", help="write CSS custom properties here")
    r.set_defaults(func=cmd_ramp)
    c = sub.add_parser("contrast", help="WCAG ratio of 2 colours")
    c.add_argument("foreground")
    c.add_argument("background")
    c.set_defaults(func=cmd_contrast)
    k = sub.add_parser("check", help="check the role pairs of a token JSON file; exit 1 on a fail")
    k.add_argument("tokens", help="JSON from ramp, or {light: {...}, dark: {...}}")
    k.set_defaults(func=cmd_check)
    return p


def main(argv=None):
    args = build_parser().parse_args(argv)
    try:
        return args.func(args)
    except PaletteError as err:
        print("error: %s" % err, file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
