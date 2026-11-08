# bank-kit-sfx, part 4: the catalogue (categories and all 252 sounds)

Part of the spec `docs/specs/hark/bank-kit-sfx.md`. Read the index first.
This part lists every category and every shipped sound with the numbers a
regenerated sound must reach. The numbers in the tables are measurements of the
source's shipped bank files (data), made with the measures of part 1, section 6,
on each entry's render at -1 dBFS peak. They are targets for our regeneration
(index, "Data assets"), not values our files must copy.

Column meanings, used by every table in this part:

| column | meaning |
|---|---|
| `dur s` | the length of the default take, in seconds (part 1, 6.5). |
| `onset s`, `peak s`, `end s` | the take times (part 1, 6.1). |
| `attack ms` | onset-to-peak rise time, as the forge-core attack measure gives it. |
| `centroid Hz` | the spectral centroid of the whole take. |
| `LUFS-M max` | the largest momentary loudness (400 ms window) at -1 dBFS peak. |
| `note Hz` | the stored heard pitch (part 1, 8.2 and 8.3). `none` = measured as unpitched. `-` = no pitch stored. |
| `judged as` | the forge critic mode the pick was judged in (`designed`, `real`, `-` = none). Information only. |
| `ch` | channels (2 = stereo, 1 = mono). |

## 1. Categories

There are 43 categories: 34 in the forge file, 7 new ones in the forge04 file,
`pluck` and `fit`. This table is the built-in category table of part 1, 4.5:
`category_info` uses its `kinds` and `use` when a bank file gives none.

- `kinds` are the cue kinds and kit roles the category serves (part 3, section
  2). The order is data: keep it.
- `use` is the one-line text that `hark sfx list` prints. The text here is ours.
- `target` is the short text description of the category that a bank file
  carries for text-audio matching (the listen unit's match). The text here is
  ours. Regenerated files carry it.
- `recipe` names the recipe that renders the category, by what it makes. The
  recipes-impacts (RI) and recipes-motion (RM) specs own the module names and
  the parameters.
- `n` is the member count: forge file + forge04 file.

### 1.1 Forge file categories (34)

| category | kinds | use | target | recipe (unit) | n |
|---|---|---|---|---|---|
| `click` | click | button presses and cursor clicks: a short tuned tick with a little body | a clean, bright interface click | click (RI) | 5 |
| `pro_click` | click, tick.crisp | bright, designed interface ticks | a sharp, high-pitched digital tick for a product demo | click (RI) | 5 |
| `hover_tick` | tick | cursor hover, rows going past, very small ticks | a tiny, soft, high tick as a cursor moves over a button | hover tick (RM) | 5 |
| `pro_hover_tick` | tick, tick.crisp | light, breathy digital ticks for counters and blinking cursors | a very small, breathy digital tick for an animated interface | hover tick (RM) | 5 |
| `tap` | tock, land.light, click, tick | a finger on a screen or a light touch on a surface | a soft, low thump of a finger on a screen | pop-in (RI) | 5 |
| `pro_tap` | land.light, tock | one touch on glass: a soft, low thump | a soft, low thump of a finger on a screen | pop-in (RI) | 5 |
| `toggle` | snap, click | switches, settings toggles, things that lock in (a press, then a latch) | a small switch that turns on with two fast clicks | toggle (RI) | 5 |
| `pop` | pop | bubble pops and cork pops (the bubble's pitch goes up) | a small bubble that pops | pop (RI) | 5 |
| `pro_pop` | pop | tuned pops for a word or a badge that appears | a short, tuned pop | pop, second-version render (RI) | 6 |
| `pro_pop_in` | pop, land, land.light, snap | icons or elements popping onto the screen: a round, low thump | a gentle, round, low pop when an icon shows up | pop-in (RI) | 5 |
| `notify` | bell, hit, text, click.confirm | chimes of a struck bell, A4 to E6: success, a message, a lockup | a struck bell chime for a notification | notify chime (RM) | 6 + 2 |
| `pro_notify` | bell, click.confirm | short, clean dings, one semitone apart | a short, bright notification ding | notify chime (RM) | 5 |
| `shimmer` | shimmer | chimes that ring out, for end-card sparkle | bar chimes and wind chimes that ring out | shimmer (RM) | 6 |
| `pro_shimmer` | shimmer | grainy, designed sparkle | a magic, sparkling shimmer | shimmer (RM) | 5 |
| `whoosh_fast` | whoosh.short, suck | fast camera whips, quick fly-ins, moving elements | a fast, crisp whoosh of air | whoosh (RM); `whip` = bright swish (RM) | 6 + 1 |
| `whoosh_big` | whoosh.long, swell | large scene transitions, large objects that go past | a large, deep whoosh for a transition | whoosh (RM); `thump*` = thump whoosh (RI) | 6 + 3 |
| `pro_whoosh_big` | whoosh.long, swell | a designed swell of low sub energy that leads into a cut | a deep, low, designed whoosh swell | whoosh (RM) | 6 |
| `swish` | whoosh.short | thin, fast flicks, text that slides, small interface moves | a thin, fast swish of a cane in the air | swish (RM); `bright`, `flick` = bright swish (RM) | 5 + 2 |
| `swipe` | whoosh.short | swipes of cards and panels, page turns (cloth, paper) | a soft swipe of cloth | swipe (RM) | 5 |
| `riser` | riser | a build up to a reveal. The forge members stop on their loudest sample. | a film riser that builds tension up to a hit | riser (RM); `dark`, `build`, `snatch` = dark riser (RM) | 5 + 3 |
| `pro_riser` | riser | a short, stuttering riser into a cut | a brief digital riser that stutters up to a cut | riser (RM) | 5 |
| `downlifter` | drop | energy that falls away after a hit, a scene exit | a downlifter: a filtered sweep that sinks | downlifter (RM) | 6 |
| `pro_zip` | zip | tuned sweeps across a cut (the `up`, `up_hi` and `arc` members go up in pitch) | a fast, tuned zip: a quick synth pitch sweep | zip (RM) | 5 |
| `impact` | hit, land | a film hit for a reveal or a title slam. The tails are 3-6 s long: trim with `dur` or `max_s`. | a deep film boom impact | impact (RI) | 6 |
| `pro_impact` | hit, land | a designed sub impact with a gated end (0.7-1.3 s) | a deep, designed sub impact with a grainy rumble tail, gated | impact (RI) | 5 |
| `sub_drop` | drop | low weight beneath a hit, or a drop in the music | a deep sub-bass drop | sub drop (RI) | 5 |
| `pro_sub_drop` | drop | a designed low drop that swells after the cut | a designed sub drop: a low sine that falls under a bright click | sub drop (RI) | 5 |
| `logo_sting` | sting | a logo reveal in layers (a lead-in, a hit, a chime) | a short, bright logo sting: a whoosh that lands on a hit, then a shimmering chime | sting (RM) | 5 |
| `pro_logo_sting` | sting | designed launch gestures that land on a tone | a designed logo sting: a falling chirp that lands on a low hum | sting (RM) | 5 |
| `glitch` | glitch | glitch transitions and error states | a short digital glitch: crushed noise bursts that get faster and then stop | glitch (RM); `burst`, `mid`, `chirp` = burst glitch (RM) | 6 + 3 |
| `pro_glitch` | glitch | a designed stutter that ends on the cut | a short digital glitch: crushed noise bursts that get faster and then stop | glitch (RM) | 6 |
| `typing_key` | key, key.space, key.return | real-sounding mechanical keys, one take for each character | one mechanical key press: a crisp click and a short thock | key press (RI) | 6 |
| `pro_typing_key` | key | designed, high-passed ticks for animated type | one brief digital key tick, high-passed and crisp, for a motion-graphics title | typing (RI) | 6 |
| `pro_typing_run` | key.run | a full typed run in one file | a quick burst of high-passed digital key ticks for a title that types itself | typing, typed-run render (RI) | 6 |

### 1.2 Forge04 file categories (7 new)

| category | kinds | use | target | recipe (unit) | n |
|---|---|---|---|---|---|
| `cut_hit` | hit, drop, land | on a cut, either a brief sub hit with a kick or a brief punchy hit (0.2-0.5 s) | a brief, deep sub-bass hit layered with a kick drum | cut hit (RI) | 5 |
| `reverse_swell` | swell | a reverse cymbal or reverse swell with its peak just after a cut (one per film at most) | a reverse cymbal swell | reverse cymbal (RM) | 5 |
| `ui_pop` | pop, land.light | soft, round, mid-register pops for things that arrive: chips, bubbles, rows | a soft, round pop, a small interface bubble | UI pop (RI) | 6 |
| `knock` | tock, land.light | wood and plastic knocks in the mid band (0.1-0.2 s) | a knock on a wood block | knock (RI) | 5 |
| `mallet` | bell, text | brief gated notes struck with a mallet (glockenspiel, celesta, marimba), a soft pluck, quick chime runs | a short glockenspiel note | mallet (RI) | 6 |
| `drip` | pop | drops of water: a bubble whose pitch goes up (the audit lets a drip climb, part 2, 3.9) | a drop of water that falls into water | drip (RI) | 5 |
| `shutter` | snap, click | camera shutters: a mechanical click in two stages, for snap zooms and whip cuts | a camera shutter click | shutter (RI) | 5 |

### 1.3 The other two

| category | kinds | use | target | recipe (unit) | n |
|---|---|---|---|---|---|
| `pluck` | bell, text, hit | tuned kalimba-tine notes for bells, motifs and a tonal lockup | one plucked kalimba tine (a thumb piano) | pluck (RI) | 3 |
| `fit` | none (each entry has its own `kinds`) | a sound made by fitting a model to a public-domain recording | none (null) | the fit renderer (fit unit) | 15 |

Notes:

- For a fit id, the role in the id (`fit/<role>/...`) is the first kind of the
  entry. The entry's own `kinds` list is the full set.
- Riser direction differs by file. The forge file risers stop on their loudest
  sample. The forge04 riser members fall again after their peak, so a kit
  should align them on `peak` (part 3).
- Per category, the pitch knob ranges and the `dur` modes are in part 2,
  section 4.

## 2. The forge file members (183)

File facts, as shipped by the source:

- 183 entries, 34 categories, 4-6 named members each. Every entry is stereo.
- Render function: 171 use the main render, 6 (`pro_pop/*`) use the
  second-version pop render, 6 (`pro_typing_run/*`) use the typed-run render.
- Critic mode: 138 `designed`, 39 `real`, 6 none (all of `whoosh_fast`).
- Level: every entry has `peak_db` -1. 120 have `gain_db` 0. The others carry a
  palette gain between -0.08 dB and -7.34 dB.
- 12 entries also carry the linear `gain` field (part 1, 3.2): `glitch/stutter`,
  `glitch/crush`, `glitch/data`, `glitch/dropout`, `glitch/scramble`,
  `glitch/hit` and all 6 `pro_pop` members. The linear value equals the dB
  value (`glitch/crush` and `pro_pop/thup` are 1.0, that is 0 dB).
- Stored pitch: only `click`, `pro_click` and `toggle` members store a heard
  pitch (part 1, 8.2). `click/thock` and `toggle/micro` store `none`.

A regenerated forge file must keep every id in this table. Each regenerated
member should match its row: `dur s` and `peak s` within the fingerprint
tolerance of part 1, section 9, when the regeneration aims for a faithful
re-make, or else within the looser bounds the index gives in "Data assets".

| id | dur s | onset s | peak s | end s | attack ms | centroid Hz | LUFS-M max | note Hz | judged as | ch |
|---|---|---|---|---|---|---|---|---|---|---|
| click/double | 0.103 | 0.0012 | 0.0045 | 0.087 | 0.5 | 1772 | -25.2 | 2913.51 | designed | 2 |
| click/snap | 0.062 | 0.0015 | 0.0045 | 0.058 | 0.75 | 3487 | -23.5 | 2206.13 | designed | 2 |
| click/soft | 0.094 | 0.0012 | 0.0045 | 0.088 | 0.5 | 2574 | -21.8 | 2903.41 | designed | 2 |
| click/thock | 0.103 | 0.0012 | 0.0045 | 0.093 | 0.75 | 462 | -25.6 | none | designed | 2 |
| click/tink | 0.070 | 0.0012 | 0.0045 | 0.065 | 0.5 | 7282 | -23.2 | 5373.64 | designed | 2 |
| downlifter/air_fall | 2.060 | 0.0150 | 0.1485 | 1.826 | 101.25 | 4056 | -10.0 | - | designed | 2 |
| downlifter/flange_air | 2.954 | 0.0325 | 0.2205 | 2.655 | 130 | 2442 | -10.3 | - | designed | 2 |
| downlifter/shepard_fall | 4.019 | 0.0295 | 0.2795 | 3.482 | 145.25 | 930 | -10.0 | - | designed | 2 |
| downlifter/sub_sink | 1.787 | 0.0062 | 0.1525 | 1.674 | 16 | 81 | -8.3 | - | designed | 2 |
| downlifter/synth_fall | 4.039 | 0.0232 | 0.3305 | 3.109 | 63.5 | 706 | -6.9 | - | designed | 2 |
| downlifter/tape | 1.630 | 0.0045 | 0.1035 | 1.368 | 48.25 | 619 | -8.3 | - | designed | 2 |
| glitch/crush | 0.540 | 0.0040 | 0.4015 | 0.523 | 2.25 | 1959 | -16.6 | - | designed | 2 |
| glitch/data | 0.490 | 0.0000 | 0.3905 | 0.475 | 1 | 2936 | -11.1 | - | designed | 2 |
| glitch/dropout | 0.740 | 0.0000 | 0.6495 | 0.724 | 1 | 2157 | -10.9 | - | designed | 2 |
| glitch/hit | 0.340 | 0.0000 | 0.2515 | 0.325 | 1.25 | 1290 | -12.1 | - | designed | 2 |
| glitch/scramble | 0.590 | 0.0165 | 0.5365 | 0.581 | 4.25 | 1445 | -12.8 | - | designed | 2 |
| glitch/stutter | 0.640 | 0.0198 | 0.3745 | 0.622 | 2.75 | 2490 | -12.8 | - | designed | 2 |
| hover_tick/air | 0.074 | 0.0012 | 0.0045 | 0.069 | 0.5 | 6517 | -23.6 | - | designed | 2 |
| hover_tick/detent | 0.084 | 0.0015 | 0.0045 | 0.083 | 0.75 | 7533 | -22.1 | - | designed | 2 |
| hover_tick/high | 0.047 | 0.0012 | 0.0045 | 0.044 | 0.75 | 7906 | -24.5 | - | designed | 2 |
| hover_tick/mid | 0.089 | 0.0012 | 0.0045 | 0.078 | 0.75 | 4295 | -23.8 | - | designed | 2 |
| hover_tick/ping | 0.050 | 0.0012 | 0.0045 | 0.050 | 0.75 | 7775 | -20.4 | - | designed | 2 |
| impact/blast | 5.759 | 0.0000 | 0.0235 | 4.283 | 1.5 | 274 | -13.3 | - | real | 2 |
| impact/classic | 4.797 | 0.0000 | 0.1165 | 3.535 | 3.25 | 205 | -11.2 | - | real | 2 |
| impact/deep | 6.205 | 0.0000 | 0.0835 | 4.620 | 4.5 | 95 | -11.7 | - | real | 2 |
| impact/metal | 5.022 | 0.0000 | 0.1165 | 3.610 | 28.75 | 346 | -10.9 | - | real | 2 |
| impact/punch | 2.845 | 0.0000 | 0.0655 | 2.188 | 33.5 | 201 | -12.2 | - | real | 2 |
| impact/swell | 6.232 | 0.2968 | 0.6155 | 4.811 | 7.25 | 198 | -10.8 | - | real | 2 |
| logo_sting/reverse_bloom | 2.127 | 0.1512 | 0.3755 | 1.699 | 131.25 | 3504 | -11.0 | - | designed | 2 |
| logo_sting/soft_bloom | 1.850 | 0.1393 | 0.2985 | 1.415 | 43.25 | 3631 | -10.1 | - | designed | 2 |
| logo_sting/spark | 1.840 | 0.0000 | 0.0105 | 1.459 | 6.75 | 5371 | -11.1 | - | designed | 2 |
| logo_sting/whoosh_glass | 1.912 | 0.1060 | 0.2005 | 1.468 | 16.75 | 2696 | -9.9 | - | designed | 2 |
| logo_sting/zip_ping | 1.656 | 0.0068 | 0.1755 | 1.270 | 87.5 | 2611 | -10.0 | - | designed | 2 |
| notify/m0 | 2.300 | 0.0000 | 0.0045 | 2.157 | 2.75 | 568 | -11.1 | - | designed | 2 |
| notify/m1 | 2.000 | 0.0000 | 0.0045 | 1.860 | 1.5 | 1196 | -12.4 | - | designed | 2 |
| notify/m2 | 1.800 | 0.0000 | 0.0035 | 1.670 | 1.75 | 970 | -12.1 | - | designed | 2 |
| notify/m3 | 2.500 | 0.0000 | 0.0065 | 2.389 | 5.75 | 663 | -12.4 | - | designed | 2 |
| notify/m4 | 1.800 | 0.0000 | 0.0045 | 1.674 | 2 | 635 | -10.2 | - | designed | 2 |
| notify/m5 | 1.700 | 0.0000 | 0.0035 | 1.573 | 1.5 | 1466 | -9.9 | - | designed | 2 |
| pop/0 | 0.200 | 0.0000 | 0.0035 | 0.115 | 3.75 | 707 | -18.0 | - | real | 2 |
| pop/1 | 0.150 | 0.0000 | 0.0035 | 0.097 | 1.75 | 1316 | -17.8 | - | real | 2 |
| pop/2 | 0.250 | 0.0000 | 0.0095 | 0.198 | 6.75 | 553 | -14.6 | - | real | 2 |
| pop/3 | 0.160 | 0.0000 | 0.0035 | 0.085 | 1.5 | 857 | -18.7 | - | real | 2 |
| pop/4 | 0.220 | 0.0000 | 0.0035 | 0.128 | 1.5 | 499 | -19.4 | - | real | 2 |
| pro_click/glass | 0.062 | 0.0012 | 0.0045 | 0.035 | 0.75 | 8256 | -19.0 | 8146.55 | designed | 2 |
| pro_click/mouse | 0.054 | 0.0012 | 0.0045 | 0.029 | 1 | 1134 | -22.8 | 3130.72 | designed | 2 |
| pro_click/snap | 0.047 | 0.0012 | 0.0045 | 0.039 | 0.5 | 7264 | -25.4 | 7268.6 | designed | 2 |
| pro_click/soft | 0.053 | 0.0012 | 0.0045 | 0.024 | 0.75 | 4357 | -25.0 | 5470.21 | designed | 2 |
| pro_click/tink | 0.037 | 0.0012 | 0.0045 | 0.036 | 1 | 7094 | -18.1 | 7506.72 | designed | 2 |
| pro_glitch/crunchy | 0.710 | 0.0078 | 0.6525 | 0.705 | 2.25 | 8494 | -18.9 | - | designed | 2 |
| pro_glitch/data | 0.670 | 0.0572 | 0.6195 | 0.666 | 1.75 | 8149 | -16.4 | - | designed | 2 |
| pro_glitch/fastroll | 0.550 | 0.0055 | 0.5045 | 0.545 | 1 | 8151 | -16.7 | - | designed | 2 |
| pro_glitch/framelock | 0.730 | 0.0530 | 0.6625 | 0.726 | 2.75 | 8138 | -16.8 | - | designed | 2 |
| pro_glitch/sparse | 0.750 | 0.0762 | 0.7035 | 0.746 | 1.75 | 8263 | -19.0 | - | designed | 2 |
| pro_glitch/standard | 0.710 | 0.0085 | 0.6595 | 0.705 | 0.75 | 7867 | -15.2 | - | designed | 2 |
| pro_hover_tick/air | 0.055 | 0.0012 | 0.0045 | 0.050 | 0.5 | 9227 | -29.0 | - | designed | 2 |
| pro_hover_tick/detent | 0.036 | 0.0012 | 0.0045 | 0.036 | 0.5 | 8822 | -28.0 | - | designed | 2 |
| pro_hover_tick/glass | 0.044 | 0.0012 | 0.0045 | 0.042 | 0.5 | 9439 | -29.8 | - | designed | 2 |
| pro_hover_tick/ping | 0.039 | 0.0012 | 0.0045 | 0.038 | 0.25 | 8140 | -32.0 | - | designed | 2 |
| pro_hover_tick/soft | 0.032 | 0.0012 | 0.0045 | 0.032 | 0.5 | 8253 | -28.2 | - | designed | 2 |
| pro_impact/air | 0.848 | 0.0000 | 0.1725 | 0.819 | 3 | 96 | -12.8 | - | designed | 2 |
| pro_impact/deep | 1.315 | 0.0048 | 0.2135 | 1.284 | 3.5 | 151 | -15.4 | - | designed | 2 |
| pro_impact/rumble | 0.993 | 0.0000 | 0.6185 | 0.961 | 1.75 | 303 | -18.0 | - | designed | 2 |
| pro_impact/short | 0.667 | 0.0000 | 0.1815 | 0.637 | 8.25 | 190 | -14.8 | - | designed | 2 |
| pro_impact/swell | 1.118 | 0.0928 | 0.4555 | 1.088 | 2.5 | 215 | -13.7 | - | designed | 2 |
| pro_logo_sting/chirp_hum | 0.985 | 0.0002 | 0.0825 | 0.843 | 7 | 335 | -15.1 | - | designed | 2 |
| pro_logo_sting/sweep_hit | 1.070 | 0.1420 | 0.3375 | 0.953 | 3 | 408 | -13.5 | - | designed | 2 |
| pro_logo_sting/swell_sting | 1.300 | 0.1635 | 0.4655 | 1.220 | 1 | 504 | -11.3 | - | designed | 2 |
| pro_logo_sting/taps_tone | 0.950 | 0.0000 | 0.1565 | 0.883 | 5 | 585 | -10.8 | - | designed | 2 |
| pro_logo_sting/zap_bloom | 0.960 | 0.0000 | 0.0765 | 0.894 | 11.75 | 577 | -11.6 | - | designed | 2 |
| pro_notify/ding_1822 | 0.260 | 0.0000 | 0.0055 | 0.239 | 1.25 | 2012 | -11.6 | - | designed | 2 |
| pro_notify/ding_1930 | 0.270 | 0.0000 | 0.0125 | 0.250 | 8 | 2099 | -11.2 | - | designed | 2 |
| pro_notify/ding_2045 | 0.360 | 0.0000 | 0.0055 | 0.340 | 2 | 2248 | -11.1 | - | designed | 2 |
| pro_notify/ding_2167 | 0.300 | 0.0000 | 0.0135 | 0.272 | 7.25 | 2490 | -11.3 | - | designed | 2 |
| pro_notify/ding_2296 | 0.260 | 0.0000 | 0.0065 | 0.247 | 3 | 2456 | -11.4 | - | designed | 2 |
| pro_pop/bloop | 0.123 | 0.0000 | 0.0065 | 0.102 | 2.5 | 342 | -19.1 | - | designed | 2 |
| pro_pop/boop | 0.114 | 0.0000 | 0.0035 | 0.101 | 1 | 680 | -19.4 | - | designed | 2 |
| pro_pop/chirp | 0.162 | 0.0000 | 0.0375 | 0.146 | 8.5 | 2934 | -15.4 | - | designed | 2 |
| pro_pop/double | 0.157 | 0.0000 | 0.0025 | 0.131 | 0.75 | 1219 | -18.9 | - | designed | 2 |
| pro_pop/thup | 0.099 | 0.0000 | 0.0045 | 0.077 | 4 | 333 | -19.9 | - | designed | 2 |
| pro_pop/tink | 0.093 | 0.0000 | 0.0025 | 0.081 | 0.5 | 2354 | -18.5 | - | designed | 2 |
| pro_pop_in/airy | 0.170 | 0.0015 | 0.0175 | 0.126 | 5.25 | 153 | -18.5 | - | designed | 2 |
| pro_pop_in/deep | 0.176 | 0.0012 | 0.0195 | 0.143 | 2.25 | 101 | -18.7 | - | designed | 2 |
| pro_pop_in/double | 0.165 | 0.0012 | 0.0145 | 0.128 | 5.5 | 135 | -15.6 | - | designed | 2 |
| pro_pop_in/knock | 0.152 | 0.0015 | 0.0115 | 0.063 | 5 | 174 | -19.7 | - | designed | 2 |
| pro_pop_in/round | 0.172 | 0.0012 | 0.0195 | 0.166 | 5.25 | 122 | -19.1 | - | designed | 2 |
| pro_riser/air_gate | 1.200 | 0.0030 | 1.1935 | 1.200 | 31.75 | 4660 | -8.3 | - | designed | 2 |
| pro_riser/chop | 1.000 | 0.0015 | 0.9945 | 1.000 | 7.5 | 6750 | -10.5 | - | designed | 2 |
| pro_riser/nblips | 1.000 | 0.0012 | 0.9945 | 1.000 | 2 | 6068 | -9.1 | - | designed | 2 |
| pro_riser/nblips_hi | 0.850 | 0.0028 | 0.8445 | 0.850 | 32.5 | 7769 | -10.2 | - | designed | 2 |
| pro_riser/roll | 1.300 | 0.0020 | 1.2775 | 1.300 | 3.5 | 3426 | -9.5 | - | designed | 2 |
| pro_shimmer/m0 | 1.950 | 0.1172 | 0.8525 | 1.813 | 5 | 3006 | -12.6 | - | designed | 2 |
| pro_shimmer/m1 | 1.400 | 0.0752 | 0.3625 | 1.151 | 0.75 | 4836 | -17.7 | - | designed | 2 |
| pro_shimmer/m2 | 1.650 | 0.3160 | 1.0635 | 1.643 | 293.25 | 4995 | -7.4 | - | designed | 2 |
| pro_shimmer/m3 | 1.150 | 0.0000 | 0.1095 | 1.140 | 94.75 | 3657 | -8.1 | - | designed | 2 |
| pro_shimmer/m4 | 2.400 | 0.2547 | 1.1595 | 1.995 | 1.25 | 1845 | -13.9 | - | designed | 2 |
| pro_sub_drop/bright | 1.250 | 0.0000 | 0.3345 | 1.184 | 10.25 | 113 | -8.5 | - | designed | 2 |
| pro_sub_drop/low | 1.139 | 0.0520 | 0.4675 | 1.108 | 2.25 | 54 | -8.9 | - | designed | 2 |
| pro_sub_drop/quick | 1.000 | 0.0000 | 0.2495 | 0.953 | 10.5 | 102 | -8.0 | - | designed | 2 |
| pro_sub_drop/ref | 1.445 | 0.0000 | 0.4165 | 1.367 | 10.5 | 78 | -9.0 | - | designed | 2 |
| pro_sub_drop/soft | 1.482 | 0.0002 | 0.4935 | 1.432 | 4.75 | 53 | -6.5 | - | designed | 2 |
| pro_tap/double | 0.118 | 0.0015 | 0.0095 | 0.100 | 6 | 162 | -19.0 | - | designed | 2 |
| pro_tap/finger | 0.089 | 0.0015 | 0.0135 | 0.045 | 4.75 | 151 | -19.1 | - | designed | 2 |
| pro_tap/glass | 0.098 | 0.0012 | 0.0095 | 0.062 | 7 | 225 | -19.1 | - | designed | 2 |
| pro_tap/knock | 0.086 | 0.0012 | 0.0115 | 0.048 | 8.25 | 237 | -19.6 | - | designed | 2 |
| pro_tap/soft | 0.098 | 0.0015 | 0.0185 | 0.058 | 5.25 | 136 | -17.4 | - | designed | 2 |
| pro_typing_key/blip | 0.081 | 0.0010 | 0.0075 | 0.072 | 1.5 | 7963 | -23.3 | - | designed | 2 |
| pro_typing_key/crackle | 0.072 | 0.0000 | 0.0075 | 0.069 | 0.5 | 9604 | -27.5 | - | designed | 2 |
| pro_typing_key/double | 0.092 | 0.0000 | 0.0085 | 0.084 | 1 | 10079 | -24.8 | - | designed | 2 |
| pro_typing_key/glass | 0.065 | 0.0005 | 0.0075 | 0.060 | 1.75 | 8614 | -24.5 | - | designed | 2 |
| pro_typing_key/soft | 0.072 | 0.0062 | 0.0105 | 0.069 | 2 | 8925 | -23.9 | - | designed | 2 |
| pro_typing_key/tick | 0.061 | 0.0000 | 0.0075 | 0.059 | 1 | 7076 | -27.7 | - | designed | 2 |
| pro_typing_run/blip | 1.507 | 0.0025 | 1.0675 | 1.493 | 3 | 7216 | -18.4 | - | designed | 2 |
| pro_typing_run/double | 1.593 | 0.0005 | 0.0085 | 1.584 | 6.5 | 8446 | -19.0 | - | designed | 2 |
| pro_typing_run/glass | 1.474 | 0.0018 | 0.6865 | 1.472 | 3.75 | 8374 | -18.1 | - | designed | 2 |
| pro_typing_run/soft | 1.755 | 0.0048 | 0.5285 | 1.749 | 2.25 | 8098 | -16.9 | - | designed | 2 |
| pro_typing_run/standard | 1.444 | 0.0032 | 0.9755 | 1.429 | 2.25 | 7918 | -19.8 | - | designed | 2 |
| pro_typing_run/tick | 1.266 | 0.0035 | 0.9495 | 1.243 | 2 | 7382 | -21.2 | - | designed | 2 |
| pro_whoosh_big/gritty | 0.931 | 0.0662 | 0.3225 | 0.721 | 3.75 | 61 | -15.5 | - | designed | 2 |
| pro_whoosh_big/long | 1.120 | 0.0678 | 0.5195 | 0.994 | 4.25 | 49 | -12.7 | - | designed | 2 |
| pro_whoosh_big/reverse | 1.055 | 0.0345 | 0.7045 | 0.939 | 3 | 53 | -15.4 | - | designed | 2 |
| pro_whoosh_big/swell | 0.805 | 0.0520 | 0.3545 | 0.746 | 8 | 62 | -13.8 | - | designed | 2 |
| pro_whoosh_big/tonal | 0.806 | 0.0700 | 0.3505 | 0.722 | 2.75 | 72 | -14.9 | - | designed | 2 |
| pro_whoosh_big/windy | 0.770 | 0.0745 | 0.3395 | 0.636 | 1.25 | 116 | -15.5 | - | designed | 2 |
| pro_zip/arc | 0.459 | 0.0025 | 0.0955 | 0.379 | 64 | 2586 | -3.0 | - | designed | 2 |
| pro_zip/down | 0.287 | 0.0008 | 0.0595 | 0.256 | 46.25 | 2197 | -7.5 | - | designed | 2 |
| pro_zip/down_warm | 0.372 | 0.0008 | 0.0345 | 0.335 | 34.25 | 1017 | -6.5 | - | designed | 2 |
| pro_zip/up | 0.350 | 0.0060 | 0.1685 | 0.285 | 1.25 | 971 | -6.0 | - | designed | 2 |
| pro_zip/up_hi | 0.344 | 0.0038 | 0.1355 | 0.300 | 11.25 | 2008 | -3.8 | - | designed | 2 |
| riser/air | 2.400 | 0.0240 | 2.3885 | 2.400 | 300.75 | 4987 | -6.0 | - | designed | 2 |
| riser/rush | 1.756 | 0.0765 | 1.7465 | 1.756 | 197 | 4884 | -8.7 | - | designed | 2 |
| riser/shepard | 3.708 | 0.0048 | 3.6535 | 3.708 | 243.25 | 1266 | -5.5 | - | designed | 2 |
| riser/swell | 2.200 | 1.3075 | 2.1925 | 2.200 | 250.5 | 6210 | -9.8 | - | designed | 2 |
| riser/tension | 2.800 | 0.0050 | 2.7705 | 2.800 | 323.75 | 2979 | -4.7 | - | designed | 2 |
| shimmer/m0 | 4.200 | 0.0000 | 0.7455 | 3.625 | 34.75 | 4230 | -6.1 | - | real | 2 |
| shimmer/m1 | 4.000 | 0.0000 | 0.7045 | 3.390 | 490.25 | 4632 | -6.7 | - | real | 2 |
| shimmer/m2 | 4.200 | 0.0000 | 0.0285 | 3.584 | 27.75 | 4313 | -9.9 | - | real | 2 |
| shimmer/m3 | 3.600 | 0.0000 | 0.5515 | 3.013 | 300.25 | 2276 | -7.3 | - | real | 2 |
| shimmer/m4 | 4.200 | 0.0955 | 0.7865 | 4.053 | 111.5 | 2956 | -8.9 | - | real | 2 |
| shimmer/m5 | 4.400 | 0.0000 | 0.6825 | 3.883 | 680 | 4081 | -6.5 | - | real | 2 |
| sub_drop/808 | 2.054 | 0.0000 | 0.0505 | 1.760 | 2.5 | 61 | -9.4 | - | designed | 2 |
| sub_drop/gritty | 2.150 | 0.0002 | 0.1335 | 1.670 | 2.25 | 83 | -7.5 | - | designed | 2 |
| sub_drop/long | 2.088 | 0.0000 | 0.2735 | 1.990 | 3.25 | 71 | -4.5 | - | designed | 2 |
| sub_drop/pro | 2.097 | 0.0000 | 0.4175 | 1.618 | 3.75 | 59 | -6.7 | - | designed | 2 |
| sub_drop/punch | 1.004 | 0.0000 | 0.0065 | 0.698 | 4 | 104 | -13.6 | - | designed | 2 |
| swipe/cloth | 0.676 | 0.0230 | 0.1765 | 0.529 | 3.75 | 561 | -15.2 | - | real | 2 |
| swipe/flick | 0.568 | 0.0107 | 0.1305 | 0.382 | 3.5 | 615 | -18.8 | - | real | 2 |
| swipe/glide | 0.736 | 0.0278 | 0.2575 | 0.567 | 6.5 | 525 | -15.3 | - | real | 2 |
| swipe/paper | 0.770 | 0.0462 | 0.1295 | 0.515 | 3 | 701 | -17.7 | - | real | 2 |
| swipe/velvet | 1.150 | 0.0480 | 0.2645 | 0.895 | 5.75 | 227 | -16.2 | - | real | 2 |
| swish/blade | 0.368 | 0.0078 | 0.0595 | 0.287 | 12.25 | 902 | -15.9 | - | real | 2 |
| swish/cane | 0.700 | 0.0258 | 0.0725 | 0.602 | 5.5 | 461 | -17.3 | - | real | 2 |
| swish/soft | 0.883 | 0.0205 | 0.1095 | 0.653 | 7.5 | 680 | -20.7 | - | real | 2 |
| swish/stick | 0.527 | 0.0240 | 0.0975 | 0.458 | 0.75 | 392 | -17.8 | - | real | 2 |
| swish/zing | 0.535 | 0.0152 | 0.0645 | 0.437 | 10.5 | 598 | -17.9 | - | real | 2 |
| tap/finger | 0.120 | 0.0012 | 0.0065 | 0.117 | 2.75 | 220 | -21.7 | - | designed | 2 |
| tap/glass | 0.152 | 0.0012 | 0.0045 | 0.094 | 1.5 | 466 | -23.8 | - | designed | 2 |
| tap/knuckle | 0.152 | 0.0012 | 0.0055 | 0.130 | 4.25 | 279 | -21.1 | - | designed | 2 |
| tap/soft | 0.130 | 0.0012 | 0.0055 | 0.096 | 1.25 | 163 | -21.3 | - | designed | 2 |
| tap/table | 0.135 | 0.0012 | 0.0055 | 0.103 | 1.25 | 227 | -22.2 | - | designed | 2 |
| toggle/latch | 0.217 | 0.0012 | 0.1385 | 0.177 | 0.5 | 3848 | -22.8 | 3369.76 | designed | 2 |
| toggle/micro | 0.123 | 0.0012 | 0.0045 | 0.066 | 0.5 | 6676 | -25.6 | none | designed | 2 |
| toggle/off | 0.137 | 0.0012 | 0.0045 | 0.121 | 0.5 | 3862 | -21.1 | 2382.64 | designed | 2 |
| toggle/on | 0.167 | 0.0012 | 0.0045 | 0.103 | 0.5 | 3758 | -18.3 | 3935.05 | designed | 2 |
| toggle/rocker | 0.235 | 0.0012 | 0.0825 | 0.160 | 0.75 | 2516 | -19.2 | 2181.14 | designed | 2 |
| typing_key/clicky | 0.194 | 0.0105 | 0.0225 | 0.181 | 0.75 | 4235 | -24.3 | - | real | 2 |
| typing_key/crisp | 0.223 | 0.0088 | 0.0115 | 0.202 | 1 | 3591 | -25.9 | - | real | 2 |
| typing_key/deep | 0.319 | 0.0100 | 0.0135 | 0.283 | 0.5 | 982 | -24.4 | - | real | 2 |
| typing_key/laptop | 0.192 | 0.0088 | 0.0135 | 0.178 | 0.75 | 1632 | -26.3 | - | real | 2 |
| typing_key/light | 0.158 | 0.0090 | 0.0125 | 0.144 | 0.5 | 2867 | -26.1 | - | real | 2 |
| typing_key/thock | 0.337 | 0.0088 | 0.0135 | 0.292 | 0.75 | 3425 | -24.3 | - | real | 2 |
| whoosh_big/cape | 1.327 | 0.0390 | 0.4585 | 1.045 | 2.5 | 312 | -15.3 | - | real | 2 |
| whoosh_big/deep | 2.038 | 0.2532 | 0.7485 | 1.675 | 7.5 | 110 | -14.4 | - | real | 2 |
| whoosh_big/gust | 1.886 | 0.2018 | 0.6025 | 1.604 | 10.25 | 862 | -13.7 | - | real | 2 |
| whoosh_big/jet | 1.649 | 0.0772 | 0.5445 | 1.363 | 11.5 | 626 | -11.9 | - | real | 2 |
| whoosh_big/reverse | 1.505 | 0.1048 | 0.8525 | 1.310 | 2.5 | 239 | -12.7 | - | real | 2 |
| whoosh_big/swell | 1.355 | 0.0592 | 0.4365 | 1.141 | 17.75 | 399 | -14.4 | - | real | 2 |
| whoosh_fast/crisp | 1.056 | 0.0195 | 0.1305 | 0.681 | 0.75 | 498 | -15.1 | - | - | 2 |
| whoosh_fast/heavy | 0.765 | 0.0285 | 0.1345 | 0.572 | 1.5 | 381 | -16.6 | - | - | 2 |
| whoosh_fast/reverse | 0.768 | 0.0295 | 0.2945 | 0.610 | 11.75 | 461 | -16.7 | - | - | 2 |
| whoosh_fast/sleeve | 1.090 | 0.0142 | 0.1565 | 0.717 | 4 | 616 | -18.7 | - | - | 2 |
| whoosh_fast/tight | 0.536 | 0.0485 | 0.1615 | 0.365 | 7.5 | 749 | -16.7 | - | - | 2 |
| whoosh_fast/whistle | 0.997 | 0.0175 | 0.1925 | 0.712 | 8.25 | 573 | -15.4 | - | - | 2 |

## 3. The forge04 file members (51)

File facts, as shipped by the source:

- 51 entries: 37 in the 7 new categories, 14 new members of 6 forge-file
  categories (`whoosh_big` 3, `riser` 3, `swish` 2, `whoosh_fast` 1, `glitch` 3,
  `notify` 2). Every entry is stereo.
- Each entry is one named palette member of its recipe, rendered on its seed.
  49 use the recipe's main render. `mallet/flourish` and `mallet/up4` use the
  mallet run render.
- The audit of an entry's takes uses its first kind as the kit role (part 1,
  7.3). The audit uses seeds n to n+5 (n = the entry seed). 50 members have 6 of
  6 clean takes. `glitch/mid` has 5 of 6 (one take rings).
- The stored originality values (`max ref CLAP`) are between 0.53 and 0.84.
  They hold only for the source's renders and a reference set we do not have
  (index, "Data assets", and Q1).
- Within one new category, no two members go over a CLAP cosine of about 0.93
  (`cut_hit/sub` and `cut_hit/kick`). The two mid dings reach about 0.95. Two
  renders of one sound reach about 0.97. A regenerated set should keep siblings
  under 0.96 to each other, so that each member is a distinct sound.

### 3.1 Members: recipe, seed, kinds and what each one is

The descriptions are ours. A regenerated member must fit its description and
reach the profile in 3.2.

| id | recipe (unit) | seed | kinds | what it is |
|---|---|---|---|---|
| `cut_hit/sub` | cut hit (RI) | 4101 | drop, hit | a short sub hit gliding from 225 Hz to 46 Hz, layered under a kick and a low punch |
| `cut_hit/kick` | cut hit (RI) | 4102 | drop, hit | a sub hit led by its kick. The beater lifts the centroid to about 275 Hz. |
| `cut_hit/punch` | cut hit (RI) | 4103 | hit, land | a short punchy hit for a cut, about 0.28 s, with most energy under 150 Hz |
| `cut_hit/rumble` | cut hit (RI) | 4104 | hit | a kick over a slowly falling sub that opens into a wide, grainy rumble, 1.4 s |
| `cut_hit/air` | cut hit (RI) | 4105 | hit | a fast, wide sub hit with a bright band of air that holds over it |
| `reverse_swell/cymbal` | reverse cymbal (RM) | 4301 | swell | a bright, wide reverse cymbal. After the peak it holds a short plateau, then stops. |
| `reverse_swell/ring` | reverse cymbal (RM) | 4302 | swell | a reverse cymbal with a stronger swell that rings out after the peak |
| `reverse_swell/dark` | reverse cymbal (RM) | 4303 | swell | a darker reverse swell, almost mono, with a low body |
| `reverse_swell/short` | reverse cymbal (RM) | 4304 | swell | a brief, bright reversed cymbal for a quick cut, peaking at 0.33 s |
| `reverse_swell/breath` | reverse cymbal (RM) | 4305 | swell | a reverse breath of noise in the mid band (about 1.3 kHz), half wide |
| `ui_pop/soft` | UI pop (RI) | 1 | pop, land.light | a soft, round mid pop at about 550 Hz: 22 ms to its peak, down 20 dB after 75 ms |
| `ui_pop/step` | UI pop (RI) | 1 | pop | the soft pop one step higher, with a 3 st fall |
| `ui_pop/boop` | UI pop (RI) | 1 | pop | a rounder "boop" that falls 9 st |
| `ui_pop/blip` | UI pop (RI) | 1 | pop | a bright blip at about 890 Hz. The two channels are almost unrelated (L/R correlation about 0.1). |
| `ui_pop/fall` | UI pop (RI) | 1 | pop | an eased fall of 7 st, then a quiet second pop |
| `ui_pop/wide` | UI pop (RI) | 1 | pop | a wide pop at about 620 Hz with a small lip click (correlation about 0.5) |
| `knock/wood` | knock (RI) | 1 | tock, land.light | a mid-band knock on wood: a block at about 400 Hz and a faint rattle |
| `knock/plastic` | knock (RI) | 1 | tock | a plastic click at about 2.4 kHz with a bounce 18 ms after it |
| `knock/snap` | knock (RI) | 1 | tock | a gentle touch followed by a bright snap of plastic (about 3 kHz) that bounces |
| `knock/desk` | knock (RI) | 1 | tock, land.light | knuckles on a desk: a low wooden knock over the body of the table |
| `knock/double` | knock (RI) | 1 | tock | two knocks on wood, 55 ms apart |
| `mallet/glock` | mallet (RI) | 1 | bell, text | a gated glockenspiel note with a second voice one octave lower, about 0.35 s |
| `mallet/pluck` | mallet (RI) | 1 | text, bell | a soft, low pluck, like a felt hammer |
| `mallet/celesta` | mallet (RI) | 1 | bell, text | a wide, gated celesta note at G5 (correlation about 0.5) |
| `mallet/marimba` | mallet (RI) | 1 | bell, text | a short, dry marimba note on wood at G4 |
| `mallet/flourish` | mallet, run render (RI) | 1 | bell | a fast chime figure of three notes that go up and down, almost mono |
| `mallet/up4` | mallet, run render (RI) | 1 | bell | four gated chime notes that skip up a pentatonic scale, 110 ms apart, each note panned to its own place |
| `drip/drop` | drip (RI) | 1 | pop | a drop of water: a bubble at about 306 Hz that climbs 8 st in 40 ms, with a soft start |
| `drip/bubble` | drip (RI) | 1 | pop | a lower bubble (about 255 Hz) that climbs 10 st, followed by a smaller bubble |
| `drip/plink` | drip (RI) | 1 | pop | a small, high plink (about 690 Hz) that climbs 3 st |
| `drip/bloop` | drip (RI) | 1 | pop | the slow bloop of a large drop (about 420 Hz): about 5 st over 0.10-0.15 s. This member is why the drip allowance exists. |
| `drip/tiny` | drip (RI) | 1 | pop | a very small, fast droplet at about 1.15 kHz, 0.1 s |
| `shutter/dslr` | shutter (RI) | 1 | snap, click | a camera shutter: the latch, 39 ms of curtain travel, then the landing on a box tuned near 1.4 kHz |
| `shutter/snap` | shutter (RI) | 1 | snap, click | a dry, bright shutter at about 2 kHz for a snap zoom |
| `shutter/mirror` | shutter (RI) | 1 | snap | a slower mirror-box shutter whose travel you can hear |
| `shutter/film` | shutter (RI) | 1 | snap | a heavier film camera: 95 ms of travel into a dark-sounding box, a body thump, a rattling spring |
| `shutter/quick` | shutter (RI) | 1 | snap, click | two fast clicks: the release first, the curtain 21 ms later |
| `notify/mid_tine` | notify chime (RM) | 1 | click.confirm, bell | a round mid ding: a tine struck with felt, over its octave (about 690 Hz, 32 ms to the peak) |
| `notify/mid_wood` | notify chime (RM) | 1 | click.confirm, bell | a soft wood bar with a body one octave down, one step higher than `mid_tine` |
| `whoosh_big/thump` | thump whoosh (RI) | 4201 | whoosh.long | a large, low whoosh that lands on a thump about 80 ms in, which rings for about 0.35 s |
| `whoosh_big/thump_deep` | thump whoosh (RI) | 4202 | whoosh.long | a deeper, longer version of the thump whoosh |
| `whoosh_big/thump_airy` | thump whoosh (RI) | 4203 | whoosh.long | an airier version of the thump whoosh |
| `riser/dark` | dark riser (RM) | 4401 | riser | a short, dark riser of noise, sub and a stutter that gets faster. It has no synth sweep. |
| `riser/build` | dark riser (RM) | 4402 | riser | a build heavy in sub, with crackle and a gate that gets faster (about 1.1 s) |
| `riser/snatch` | dark riser (RM) | 4403 | riser | a stutter of half a second that pulls into a cut |
| `swish/bright` | bright swish (RM) | 4501 | whoosh.short | a fast, bright swish (centroid about 2.4 kHz) |
| `swish/flick` | bright swish (RM) | 4502 | whoosh.short | a high, wide flick that goes up and stops at once |
| `whoosh_fast/whip` | bright swish (RM) | 4503 | whoosh.short | a designed, bright whip whose wide ring drops about 45 st |
| `glitch/burst` | burst glitch (RM) | 4601 | glitch | crackle, then bright bursts. The first burst is the loudest and the bursts get faster. |
| `glitch/mid` | burst glitch (RM) | 4602 | glitch | a short glitch in the mid band whose first burst is the loudest |
| `glitch/chirp` | burst glitch (RM) | 4603 | glitch | a stutter of tuned data chirps whose first burst is the loudest |

The members of a palette are named by these ids. Where the source recipe found
a member by its palette name, our recipe specs give each palette member the
member part of the id as its name (`soft`, `step` ...). The recipe specs own
the parameters.

### 3.2 Measured profile of each member

`t20 ms` and `t40 ms` are the times from the peak until the level is 20 dB and
40 dB down. `lo150` and `hi4k` are the shares of energy under 150 Hz and over
4 kHz. `tonal` is the share of energy within +-1/4 octave of the strongest
bin. `glide st` is the pitch glide in semitones. `corr` is the L/R correlation.
`audit clean` is the source's take audit (seeds n to n+5). These profile keys
are the forge04 `profile` object of part 1, 3.6.

| id | dur s | onset s | peak s | end s | attack ms | centroid Hz | LUFS-M max | t20 ms | t40 ms | lo150 | hi4k | tonal | glide st | corr | audit clean | max ref CLAP |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| cut_hit/air | 0.549 | 0.0000 | 0.0435 | 0.504 | 1.75 | 645 | -16.5 | 41.0 | 428.0 | 0.898 | 0.011 | 0.47 | 0.0 | 0.18 | 6/6 | 0.697 |
| cut_hit/kick | 0.325 | 0.0000 | 0.0135 | 0.243 | 2.5 | 274 | -16.3 | 83.0 | 185.0 | 0.476 | 0.0 | 0.42 | -9.7 | 1.0 | 6/6 | 0.593 |
| cut_hit/punch | 0.282 | 0.0000 | 0.0575 | 0.278 | 20.75 | 227 | -15.5 | 51.0 | 195.0 | 0.825 | 0.0 | 0.43 | 0.0 | 0.96 | 6/6 | 0.704 |
| cut_hit/rumble | 1.400 | 0.0000 | 0.0725 | 1.396 | 3.25 | 124 | -12.7 | 113.0 | 251.0 | 0.913 | 0.003 | 0.54 | 3.2 | 0.19 | 6/6 | 0.665 |
| cut_hit/sub | 0.286 | 0.0000 | 0.0205 | 0.263 | 15.25 | 220 | -14.9 | 156.0 | 230.0 | 0.546 | 0.0 | 0.48 | -9.7 | 0.98 | 6/6 | 0.674 |
| drip/bloop | 0.372 | 0.0000 | 0.0305 | 0.316 | 7.25 | 755 | -13.9 | 118.0 | 232.0 | 0.001 | 0.001 | 0.77 | 4.1 | 0.96 | 6/6 | 0.657 |
| drip/bubble | 0.289 | 0.0000 | 0.0145 | 0.241 | 6.25 | 483 | -15.1 | 94.0 | 186.0 | 0.006 | 0.0 | 0.67 | 0.9 | 0.91 | 6/6 | 0.63 |
| drip/drop | 0.334 | 0.0038 | 0.0225 | 0.271 | 7.75 | 581 | -12.3 | 112.0 | 199.0 | 0.0 | 0.0 | 0.63 | 0.0 | 0.97 | 6/6 | 0.599 |
| drip/plink | 0.257 | 0.0018 | 0.0305 | 0.213 | 22 | 883 | -13.0 | 83.0 | 145.0 | 0.0 | 0.0 | 0.72 | 0.0 | 0.96 | 6/6 | 0.571 |
| drip/tiny | 0.142 | 0.0000 | 0.0085 | 0.108 | 4.75 | 1449 | -19.6 | 41.0 | 85.0 | 0.0 | 0.022 | 0.78 | 0.0 | 0.94 | 6/6 | 0.561 |
| glitch/burst | 0.869 | 0.3558 | 0.3615 | 0.726 | 4.25 | 8245 | -22.3 | 120.0 | 358.0 | 0.002 | 0.837 | 0.48 | -70.5 | 0.05 | 6/6 | 0.714 |
| glitch/chirp | 0.407 | 0.0720 | 0.0765 | 0.283 | 1.75 | 1118 | -18.0 | 61.0 | 205.0 | 0.0 | 0.034 | 0.55 | 0.5 | 0.87 | 6/6 | 0.688 |
| glitch/mid | 0.245 | 0.0305 | 0.0385 | 0.220 | 6 | 1079 | -18.9 | 19.0 | 183.0 | 0.003 | 0.011 | 0.51 | -0.5 | 0.89 | 5/6 (ring) | 0.75 |
| knock/desk | 0.224 | 0.0000 | 0.0245 | 0.200 | 10 | 530 | -19.1 | 52.0 | 110.0 | 0.806 | 0.003 | 0.63 | 0.0 | 0.72 | 6/6 | 0.749 |
| knock/double | 0.156 | 0.0000 | 0.0575 | 0.111 | 2 | 1187 | -21.9 | 16.0 | 22.0 | 0.012 | 0.009 | 0.47 | 7.0 | 0.86 | 6/6 | 0.821 |
| knock/plastic | 0.153 | 0.0015 | 0.0525 | 0.124 | 1.5 | 2412 | -23.0 | 31.0 | 51.0 | 0.032 | 0.011 | 0.57 | -8.2 | 0.94 | 6/6 | 0.839 |
| knock/snap | 0.134 | 0.0008 | 0.0415 | 0.101 | 1.25 | 2983 | -23.6 | 13.0 | 45.0 | 0.004 | 0.106 | 0.47 | -14.8 | 0.94 | 6/6 | 0.813 |
| knock/wood | 0.154 | 0.0122 | 0.0155 | 0.130 | 1 | 784 | -22.9 | 31.0 | 87.0 | 0.027 | 0.001 | 0.4 | 0.9 | 0.95 | 6/6 | 0.682 |
| mallet/celesta | 0.414 | 0.0005 | 0.0235 | 0.344 | 11.75 | 1054 | -13.1 | 128.0 | 221.0 | 0.0 | 0.004 | 0.8 | 0.5 | 0.48 | 6/6 | 0.625 |
| mallet/flourish | 0.504 | 0.0000 | 0.0505 | 0.428 | 1 | 953 | -12.8 | 155.0 | 292.0 | 0.0 | 0.0 | 0.64 | 0.0 | 0.98 | 6/6 | 0.629 |
| mallet/glock | 0.412 | 0.0000 | 0.0105 | 0.374 | 3 | 1326 | -13.4 | 158.0 | 301.0 | 0.0 | 0.006 | 0.53 | -12.0 | 0.85 | 6/6 | 0.56 |
| mallet/marimba | 0.564 | 0.0000 | 0.0065 | 0.385 | 3.75 | 504 | -13.3 | 95.0 | 219.0 | 0.0 | 0.0 | 0.9 | 0.0 | 0.91 | 6/6 | 0.565 |
| mallet/pluck | 0.494 | 0.0065 | 0.0555 | 0.415 | 19 | 465 | -11.6 | 104.0 | 211.0 | 0.0 | 0.0 | 0.58 | -12.0 | 0.99 | 6/6 | 0.532 |
| mallet/up4 | 0.749 | 0.0000 | 0.3355 | 0.716 | 114.25 | 1975 | -7.4 | 270.0 | 347.0 | 0.0 | 0.054 | 0.85 | 13.8 | 0.66 | 6/6 | 0.586 |
| notify/mid_tine | 0.425 | 0.0035 | 0.0385 | 0.319 | 20.25 | 690 | -14.5 | 88.0 | 233.0 | 0.0 | 0.0 | 0.61 | 0.0 | 0.94 | 6/6 | 0.626 |
| notify/mid_wood | 0.317 | 0.0045 | 0.0385 | 0.294 | 22.25 | 704 | -13.4 | 126.0 | 235.0 | 0.0 | 0.0 | 0.57 | -12.0 | 0.91 | 6/6 | 0.624 |
| reverse_swell/breath | 1.235 | 0.0905 | 0.7325 | 1.167 | 9.5 | 1321 | -13.6 | 170.0 | 335.0 | 0.0 | 0.0 | 0.43 | -0.4 | 0.75 | 6/6 | 0.576 |
| reverse_swell/cymbal | 0.895 | 0.0050 | 0.5185 | 0.856 | 18.75 | 3345 | -13.7 | 219.0 | 297.0 | 0.0 | 0.27 | 0.26 | -4.4 | 0.16 | 6/6 | 0.565 |
| reverse_swell/dark | 0.919 | 0.0032 | 0.5695 | 0.872 | 7.75 | 420 | -14.9 | 128.0 | 242.0 | 0.102 | 0.0 | 0.35 | -9.3 | 0.91 | 6/6 | 0.606 |
| reverse_swell/ring | 1.680 | 0.1715 | 0.6145 | 1.573 | 20 | 3430 | -15.3 | 373.0 | 682.0 | 0.0 | 0.353 | 0.21 | -24.2 | 0.01 | 6/6 | 0.564 |
| reverse_swell/short | 0.651 | 0.0102 | 0.3325 | 0.553 | 11 | 3714 | -19.3 | 63.0 | 156.0 | 0.0 | 0.273 | 0.31 | 1.8 | 0.26 | 6/6 | 0.602 |
| riser/build | 1.758 | 0.5032 | 1.2015 | 1.664 | 5.75 | 930 | -15.2 | 142.0 | 348.0 | 0.827 | 0.065 | 0.42 | 5.8 | 1.0 | 6/6 | 0.542 |
| riser/dark | 0.951 | 0.1345 | 0.6965 | 0.943 | 6 | 451 | -16.4 | 117.0 | 217.0 | 0.124 | 0.003 | 0.41 | 20.4 | 0.98 | 6/6 | 0.583 |
| riser/snatch | 0.635 | 0.0038 | 0.3855 | 0.630 | 6.5 | 463 | -11.8 | 121.0 | 228.0 | 0.121 | 0.01 | 0.4 | 24.0 | 1.0 | 6/6 | 0.608 |
| shutter/dslr | 0.242 | 0.0005 | 0.0415 | 0.190 | 2 | 1086 | -22.5 | 31.0 | 101.0 | 0.078 | 0.021 | 0.3 | -30.7 | 0.97 | 6/6 | 0.66 |
| shutter/film | 0.283 | 0.0000 | 0.0995 | 0.256 | 1.75 | 639 | -19.1 | 54.0 | 127.0 | 0.197 | 0.001 | 0.42 | -34.2 | 0.98 | 6/6 | 0.637 |
| shutter/mirror | 0.331 | 0.0015 | 0.0675 | 0.285 | 2.25 | 1185 | -20.4 | 50.0 | 155.0 | 0.018 | 0.012 | 0.38 | 16.4 | 0.97 | 6/6 | 0.658 |
| shutter/quick | 0.160 | 0.0000 | 0.0235 | 0.121 | 1 | 2169 | -23.2 | 17.0 | 72.0 | 0.024 | 0.037 | 0.33 | -29.0 | 0.99 | 6/6 | 0.705 |
| shutter/snap | 0.137 | 0.0000 | 0.0615 | 0.098 | 0.75 | 2840 | -23.4 | 15.0 | 28.0 | 0.017 | 0.2 | 0.42 | -56.2 | 0.95 | 6/6 | 0.693 |
| swish/bright | 0.345 | 0.0068 | 0.1555 | 0.299 | 38.5 | 2420 | -12.4 | 88.0 | 121.0 | 0.0 | 0.077 | 0.47 | -10.9 | 0.81 | 6/6 | 0.606 |
| swish/flick | 0.320 | 0.0015 | 0.1105 | 0.255 | 6.5 | 4266 | -19.1 | 18.0 | 101.0 | 0.003 | 0.51 | 0.19 | -4.8 | 0.2 | 6/6 | 0.702 |
| ui_pop/blip | 0.118 | 0.0055 | 0.0165 | 0.047 | 6.75 | 1030 | -17.6 | 19.0 | 27.0 | 0.0 | 0.0 | 0.85 | 0.5 | 0.07 | 6/6 | 0.788 |
| ui_pop/boop | 0.195 | 0.0002 | 0.0275 | 0.168 | 9.25 | 740 | -18.6 | 52.0 | 106.0 | 0.036 | 0.0 | 0.68 | -3.0 | 0.95 | 6/6 | 0.712 |
| ui_pop/fall | 0.204 | 0.0042 | 0.0215 | 0.164 | 8.5 | 921 | -18.3 | 31.0 | 46.0 | 0.003 | 0.001 | 0.58 | -1.8 | 0.67 | 6/6 | 0.744 |
| ui_pop/soft | 0.180 | 0.0025 | 0.0415 | 0.157 | 6.5 | 547 | -15.4 | 75.0 | 109.0 | 0.029 | 0.0 | 0.49 | 0.0 | 0.98 | 6/6 | 0.715 |
| ui_pop/step | 0.187 | 0.0025 | 0.0445 | 0.163 | 20.75 | 699 | -15.9 | 59.0 | 96.0 | 0.029 | 0.0 | 0.48 | -0.8 | 0.99 | 6/6 | 0.722 |
| ui_pop/wide | 0.190 | 0.0000 | 0.0445 | 0.143 | 7 | 930 | -15.9 | 58.0 | 96.0 | 0.0 | 0.002 | 0.7 | -0.7 | 0.51 | 6/6 | 0.691 |
| whoosh_big/thump | 1.030 | 0.0462 | 0.1375 | 0.880 | 1 | 268 | -14.3 | 346.0 | 588.0 | 0.64 | 0.015 | 0.39 | 7.0 | 0.93 | 6/6 | 0.531 |
| whoosh_big/thump_airy | 0.915 | 0.0168 | 0.1535 | 0.798 | 5.25 | 220 | -12.3 | 343.0 | 495.0 | 0.625 | 0.006 | 0.4 | -3.2 | 0.97 | 6/6 | 0.581 |
| whoosh_big/thump_deep | 1.360 | 0.0658 | 0.1645 | 1.255 | 1.25 | 336 | -10.7 | 404.0 | 729.0 | 0.798 | 0.029 | 0.5 | 0.0 | 0.93 | 6/6 | 0.53 |
| whoosh_fast/whip | 0.910 | 0.0048 | 0.1155 | 0.862 | 17.5 | 2558 | -14.1 | 264.0 | 454.0 | 0.0 | 0.091 | 0.47 | -42.6 | 0.27 | 6/6 | 0.629 |

## 4. The pluck file members (3)

The edits and seeds are in part 1, 7.2. Every pluck entry renders at C5
(523.25 Hz) and is 3 s long. Each take re-tunes through the pitch knob (part 2,
4.4). The `note Hz` column is `-` because a pluck entry stores no measured
pitch: its pitch is the `f0` parameter.

| id | dur s | onset s | peak s | end s | attack ms | centroid Hz | LUFS-M max | note Hz | judged as | ch |
|---|---|---|---|---|---|---|---|---|---|---|
| pluck/kalimba | 3.000 | 0.0032 | 0.0135 | 1.914 | 9.25 | 550 | -12.0 | - | - | 2 |
| pluck/kalimba_ring | 3.000 | 0.0032 | 0.0135 | 2.747 | 9.25 | 544 | -10.1 | - | - | 2 |
| pluck/kalimba_soft | 3.000 | 0.0032 | 0.0135 | 2.063 | 9.25 | 529 | -8.1 | - | - | 2 |

## 5. The fit file members (15)

Each fit entry is a model that the fit unit made from one public-domain
recording. The entry renders through the fit renderer in take mode, with the
entry seed and `vary` 0.3 (part 1, 3.3). A new seed gives a new take of the same
object. A take rings out past the point where the trimmed recording stopped, so
it is 0.06-0.2 s longer than the recording. A kit trims it with `max_s`.

### 5.1 Members and their sources

All 15 sources are CC0 1.0 recordings on Freesound. The `source` object of each
entry holds these values (part 1, 3.3). `model` is the model kind the fit chose
(`sines`, `modal`, or `hybrid` with its layers). `CLAP` is the cosine of the
fit's render to its recording.

| id | what it is | model | take s | CLAP | kinds | source id | title | author | URL |
|---|---|---|---|---|---|---|---|---|---|
| `fit/click/floss_lid` | a small click of plastic | sines | 0.13 | 0.96 | click, tick.crisp | fs-167829 | Plastic Clicks.flac | qubodup | https://freesound.org/people/qubodup/sounds/167829/ |
| `fit/tick/domino` | domino pieces that knock together | sines | 0.14 | 0.94 | tick | fs-329098 | Domino pieces hit | Macif | https://freesound.org/people/Macif/sounds/329098/ |
| `fit/pop/mouth_plop` | a soft plop made with the mouth | hybrid (sines + modal + spectral) | 0.28 | 0.96 | pop | fs-181907 | Mouth Plops.wav | CapsLok | https://freesound.org/people/CapsLok/sounds/181907/ |
| `fit/pop/lip_pop` | a brief pop made with the lips | modal | 0.22 | 0.95 | pop | fs-202230 | Pop sound | deraj | https://freesound.org/people/deraj/sounds/202230/ |
| `fit/land.light/fingertip_book` | fingers stepping lightly on a book | modal | 0.38 | 0.94 | land.light, tock | fs-760337 | Light Footsteps On Book | IENBA | https://freesound.org/people/IENBA/sounds/760337/ |
| `fit/land.light/finger_wood` | a finger that taps a wood counter | modal | 0.34 | 0.93 | land.light, tock | fs-452621 | finger tap table counter wood various.wav | kyles | https://freesound.org/people/kyles/sounds/452621/ |
| `fit/land.light/finger_phone` | a finger that taps a phone | hybrid (modal + spectral) | 0.33 | 0.93 | land.light, text | fs-704862 | Finger Tapping Phone | jentlemen | https://freesound.org/people/jentlemen/sounds/704862/ |
| `fit/whoosh.short/bamboo_swish` | a brief swish of air | sines | 0.48 | 0.94 | whoosh.short | fs-427979 | Short Whoosh, 13x | Kinoton | https://freesound.org/people/Kinoton/sounds/427979/ |
| `fit/key/medium` | a mechanical key pressed and let go | hybrid (sines + modal + spectral) | 0.43 | 0.94 | key | fs-378084 | mechanical key medium.wav | bigmonmulgrew | https://freesound.org/people/bigmonmulgrew/sounds/378084/ |
| `fit/key/thock` | a low-pitched mechanical key going down | modal | 0.45 | 0.94 | key | fs-789628 | Keychron k10 A to + individual keys | Sadiquecat | https://freesound.org/people/Sadiquecat/sounds/789628/ |
| `fit/key/lubed` | a mechanical key going down | modal | 0.42 | 0.98 | key | fs-665075 | Keyboard typing sounds: Unidentified Technics keyboard | zrrion | https://freesound.org/people/zrrion/sounds/665075/ |
| `fit/key.space/thock_space` | the space bar of a mechanical keyboard | sines | 0.39 | 0.95 | key.space | fs-789630 | Keychron k10 space_bar | Sadiquecat | https://freesound.org/people/Sadiquecat/sounds/789630/ |
| `fit/snap/switch_flip` | a switch that flips | sines | 0.19 | 0.94 | snap, click | fs-278205 | Switch Flip #1 | IanStarGem | https://freesound.org/people/IanStarGem/sounds/278205/ |
| `fit/bell/small_bell` | one strike on a small bell | sines | 1.66 | 0.94 | bell | fs-452379 | Small Bell #2 | steffcaffrey | https://freesound.org/people/steffcaffrey/sounds/452379/ |
| `fit/hit/bamboo_tok` | a hit on bamboo | hybrid (sines + spectral) | 0.17 | 0.97 | hit, tock | fs-386888 | Bamboo hit | michorvath | https://freesound.org/people/michorvath/sounds/386888/ |

Further facts:

- 10 entries are stereo and 5 are mono (`small_bell`, `bamboo_tok`,
  `finger_phone`, `mouth_plop`, `domino`).
- Two shipped sounds, `small_bell` and `lubed`, give the comb-ring audit warning
  on some takes. Their recordings give the same warning. A kit re-rolls such a
  take (part 3, 5.3).
- Four stereo `sines` entries (`floss_lid`, `thock_space`, `switch_flip`,
  `bamboo_swish`) still use an older fit whose side channel is too quiet. The source
  kept them so that the preset takes do not change. Our regeneration may fix the
  side level, and then the preset takes change (index, Q7).
- Each entry's `notes` text says what the sound is, names the source title,
  author and licence, the model kind, the CLAP value, and the `vary` at which its
  takes pass. Our notes use our own words with the same facts.

### 5.2 When a fit can ship

A fitted recording ships in the fit file only when all these are true:

1. The grade is high: CLAP to the recording is 0.92 or more, and the mel
   distance is no more than 2.5 dB above the model's own floor (the mel distance between
   two renders of the model).
2. Five takes (seeds 0 to 4, made the same way the bank makes takes, at the
   shipped `vary`) pass the take check:
   - no hard artefact that the recording does not have (the forge-core polish
     detectors, with the recording as the reference);
   - every audit warning of the take is also a warning on the recording;
   - CLAP of the take to the fit's render is 0.90 or more, and differs by at most 0.06 from the
     fit's CLAP to the recording. Measure both over the length of the recording:
     CLAP pads a short clip by repeating it, so a different length changes the
     rhythm it hears;
   - the excess energy of the take is within 1.5 dB of the fit's;
   - the loudness spread of the five takes is 2 LU or less.
3. The take check tries `vary` 0.3; when that fails, 0.15; last, a run that
   only renews the noise. A fit that passes only on that last run does not
   ship: its takes hardly differ.
4. The recording is CC0 1.0, and its title, author and page mention no
   commercial sound library. Someone who re-uploads a library sound has no
   right to release it.

Source result: 46 CC0 recordings were fitted for the film roles and 15 passed,
all at `vary` 0.3. Of the 31 that failed, 20 had a good or fair grade. Of the 11
with a high grade, 7 made takes that moved away from the recording (by CLAP),
added an artefact that the recording does not have (for example a comb ring
or a rising glide) or had a loudness spread over 2 LU; 3 were clean only with fresh noise; 1 was a
re-upload.

### 5.3 Measured rows

| id | dur s | onset s | peak s | end s | attack ms | centroid Hz | LUFS-M max | note Hz | judged as | ch |
|---|---|---|---|---|---|---|---|---|---|---|
| fit/bell/small_bell | 1.660 | 0.0012 | 0.0625 | 0.890 | 48.5 | 4171 | -9.4 | 4037.63 | - | 1 |
| fit/click/floss_lid | 0.132 | 0.0040 | 0.0075 | 0.069 | 0.75 | 4884 | -25.2 | none | - | 2 |
| fit/hit/bamboo_tok | 0.170 | 0.0070 | 0.0115 | 0.102 | 2 | 6159 | -27.8 | none | - | 1 |
| fit/key.space/thock_space | 0.390 | 0.0065 | 0.0205 | 0.277 | 2 | 1091 | -24.2 | none | - | 2 |
| fit/key/lubed | 0.424 | 0.0622 | 0.0755 | 0.323 | 1 | 1818 | -22.6 | none | - | 2 |
| fit/key/medium | 0.426 | 0.0932 | 0.1035 | 0.372 | 0.75 | 2204 | -24.9 | none | - | 2 |
| fit/key/thock | 0.454 | 0.0048 | 0.0145 | 0.310 | 0.75 | 2024 | -26.4 | none | - | 2 |
| fit/land.light/finger_phone | 0.331 | 0.0082 | 0.0125 | 0.155 | 1.5 | 95 | -24.3 | none | - | 1 |
| fit/land.light/finger_wood | 0.339 | 0.0050 | 0.0095 | 0.222 | 5.25 | 141 | -20.0 | none | - | 2 |
| fit/land.light/fingertip_book | 0.375 | 0.0045 | 0.0095 | 0.192 | 3.5 | 240 | -19.8 | 199.46 | - | 2 |
| fit/pop/lip_pop | 0.221 | 0.0052 | 0.0095 | 0.166 | 1.5 | 1200 | -17.6 | none | - | 2 |
| fit/pop/mouth_plop | 0.281 | 0.0080 | 0.0145 | 0.192 | 1.75 | 815 | -22.0 | 858.85 | - | 1 |
| fit/snap/switch_flip | 0.185 | 0.0008 | 0.0355 | 0.107 | 4.25 | 10014 | -22.3 | none | - | 2 |
| fit/tick/domino | 0.139 | 0.0042 | 0.0075 | 0.077 | 2.25 | 5270 | -26.9 | none | - | 1 |
| fit/whoosh.short/bamboo_swish | 0.477 | 0.0870 | 0.1295 | 0.390 | 26 | 535 | -14.2 | 231.94 | - | 2 |
