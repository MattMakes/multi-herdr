# Palette generation

`scripts/palette.py` makes colour ramps from seed colours, derives the roles, and checks contrast. It uses python3 3.10 or later and the standard library only. Run it as `python3 -I skills/design-system/scripts/palette.py <command>`. Same input gives the same output, byte for byte.

## When to run each command

| Step | Command | Use |
|---|---|---|
| Pick colours | `ramp` | You have 1 or more seed colours (brand or a row of `palettes.md`) and need ramps and roles |
| Probe a pair | `contrast` | You need the ratio of 2 colours, for example a hand-picked brand pair |
| Verify | `check` | A token JSON file exists and every role pair must pass before you ship |

## ramp

```
palette.py ramp --seed accent=#2563eb --seed danger=#dc2626 --seed success=#15803d \
  --seed warning=#b45309 --seed info=#0d9488 --css tokens.generated.css --json palette.json
```

| Input | Meaning |
|---|---|
| `--seed ID=HEX` | A family. The ids `accent`, `danger`, `success`, `warning`, `info` also derive roles. Other ids make ramps only. Hex has 3 or 6 digits. |
| `--neutral ID=HEX` | The neutral family. Default: a gray family named `neutral`. |
| `--gray-tint` | `none` (default), `warm`, `cool` or `from-seed`. Tints the neutral ramp. |
| `--vividness` | 0 to 100, default 50. 0 is gray, 50 is the seed's own weight, 100 is about 1.5 times. |
| `--modes` | `light`, `dark` or `both` (default). |
| `--stops` | Tones, default `0,5,...,100`. Use `0,10,...,100` for a compact ramp. |
| `--mute-keep` with `--mute-full-until` and `--mute-recover-at` | Optional. Dark ramps only. Keeps a share of chroma at dark stops so vivid hues do not glare. |
| `--json PATH`, `--css PATH` | Output files. With neither option, JSON goes to stdout. |

A tone is a CIE L* value. Stop 50 has the same lightness in every family, so 2 families at the same stop have the same contrast against white or black. Only chroma differs by mode.

### Output

- JSON key `families`: `<id>.<mode>.<stop>` is a hex colour. Primitive layer.
- JSON key `roles`: `light` and `dark` maps of semantic tokens, with shadcn names (`background`, `primary`, `primary-foreground`, `muted-foreground`, `destructive`, ...). Status roles add `<intent>-foreground`, `<intent>-muted` (pale fill), `<intent>-muted-foreground`, `<intent>-text` and `<intent>-border`. Intents: `destructive` (from `danger`), `success`, `warning`, `info`.
- JSON key `diagnostics`: per family and mode: `monotonic` (must be true), `min_delta_e` and `max_delta_e` between neighbour stops, `max_hue_drift` in degrees, `risks` (`blue-to-purple`, `yellow-to-brown`), `mapped_stops` (stops that were pulled into the sRGB gamut).
- JSON key `balance`: the 2 closest families (a small `closest_delta_e`, under about 15, means they look the same) and `chroma_ratio` (above about 2 means out of balance).
- JSON key `receipt`: method name and version, build id, `sha256` of `families` and `roles`, and the full normalized request. Store it in the theme record. Regenerating from the receipt gives the same `sha256`.
- CSS: `:root` holds light, `.dark` holds dark. Primitives are `--color-<id>-<stop>`. Roles are bare shadcn names.

### Read warnings

Warnings go to stderr, 1 per line.

- `light <intent> fill ... is under 3:1 against a surface`: the seed fails the fill boundary rule. The line names the first darker colour that passes. Use it as the seed and run again.
- `no stop reaches N:1`: a searched role has no passing stop. The ramp is too light or too dark for that ground. Change the seed or the gray tint.
- `ramp is not monotonic`: a ramp loses lightness order. Do not ship it. Lower `--vividness`.

## Derivation rules in short

Surfaces use fixed neutral stops (light / dark): page 95 / 10, surface 100 / 15, card 100 / 10. Primary text is stop 0 / 100. Roles that need a ratio (secondary text, form edge, role text, focus ring, status border) start at a stop and move to the nearest stop that passes, preferring darker in light and lighter in dark. Light fills use the seed exactly. Dark fills use the seed when it passes, else the nearest stop to 70 that passes. `tokens.md` lists every rule.

## contrast

`palette.py contrast <foreground> <background>` prints 1 ratio, truncated to 2 decimals (4.499 prints 4.49 and fails 4.5). Hex has 3 or 6 digits. For a translucent colour, composite it over the real ground first, then measure.

## check

`palette.py check <file>` reads the `ramp` JSON, or a file of the form `{"light": {...}, "dark": {...}}` with role names as keys. It checks each known pair that has both tokens, prints `<mode> <fg> on <bg> <ratio> need <n> pass|FAIL`, and exits 1 on any FAIL. 8-digit hex colours (with alpha) are composited over the ground first.

| Need | Pairs |
|---|---|
| 4.5 | each `X-foreground` on `X`; `foreground` on `background` and `card`; `muted-foreground` on `background` and `muted`; `<intent>-text` on `background`, `card` and `<intent>-muted`; `<intent>-muted-foreground` on `<intent>-muted` |
| 3.0 | `primary` and each status fill on `background`; `input` on `background` and `card`; `ring` on `background` and `card`; `<intent>-border` on `background` |

Run `check` after every change to a seed, a role or a mode. A hand-edited token file works the same way: it only needs role names from the list.

## Exact brand colours

The script does not pin a brand colour at an arbitrary stop. A seed is used exactly as the light fill. To use another brand value for a role, edit the output token and run `check`: the result is the proof.
