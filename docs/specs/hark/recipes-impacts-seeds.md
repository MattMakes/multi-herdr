# recipes-impacts, part seeds: the seed files, the pluck bank file and the reference clips

This part belongs to the spec `recipes-impacts.md` (the index). It describes the data assets of this unit: their
format, their meaning, how the source made them, and how hark gets them. It does not copy the data.

Marks:
- REGENERATE: hark makes the asset again with its own tools (a forge run, a render). The values will differ from
  the source's values, and that is acceptable.
- DERIVE: the asset follows from numbers that this spec gives (a formula or a table), plus a hark render.
- NEEDS-DATA: the asset holds information that hark cannot make from this spec. Each one has a fallback.

## S1. The seed store

### S1.1 Role

A seed file keeps good parameter sets that forge runs found for 1 recipe. A later forge run of the same recipe uses
them as extra starting points of its search (generation 0, next to the recipe DEFAULTS and any explicit start
points). A seed file never changes how a render sounds. The forge-core unit owns the load and save logic (its search
module). This unit owns the 8 files listed in S2.

The file name is `<recipe>.json`, where recipe is the last dotted part of the recipe module name. So the typed run
object of the `typing` module writes to the same file as the single key, and the keypress category `typing_key`
writes to `keypress.json`.

### S1.2 Format

A JSON object with 2 fields:

| Field | Type | Meaning |
| --- | --- | --- |
| `recipe` | string | the recipe name (the file name without `.json`) |
| `entries` | list of objects | the parameter sets, pinned entries first (S1.4) |

Fields of 1 entry:

| Field | Type | Present | Meaning |
| --- | --- | --- | --- |
| `params` | object | always | a full or partial parameter dict of the recipe; floats rounded to 6 significant digits |
| `rank_score` | number or null | always | the robust mean when one was measured, else the critic total; 3 decimals; null only for a pinned entry without a finite score |
| `context` | string | always | the target text and the reference set of the run (S1.3) |
| `total` | number or null | always | the critic total of the scored render |
| `tone_db` | number or null | always | the critic tonal-balance distance in dB (null when the critic had no tone target) |
| `passes` | bool | always | the critic pass flag of the scored render |
| `source` | string | always | free text: which run made the entry (round, category, report path) |
| `robust_mean` | number or null | when a robust check ran | mean critic total over extra seeds |
| `robust_pass_rate` | number | when a robust check ran | share of the extra seeds that pass (0-1) |
| `delivered` | string | pinned entries only | the category tag of the delivered family (for example `pro_click`) |
| `member` | string | pinned entries, when known | the member name inside the delivered family |
| `seed` | integer | pinned entries, when known | the render seed of the delivered take |
| `was_delivered` | string | an entry that lost its pin | the tag it had before a newer delivery replaced it |

The critic fields (`total`, `tone_db`, `passes`, robust values) come from the listen and forge-core critic. They are
only true for the critic, references and code version that scored them.

### S1.3 Context string

context = target text + ` | ` + the base names of the reference files, sorted, joined by `,`. An empty reference list
gives the target text followed by ` | ` and nothing.

### S1.4 Load and save rules (owned by forge-core, summarised here)

Load (`n` = 6 by default): sort the entries by (context differs from the run's context, not delivered, minus the rank
value), take the first n, return their `params`. A missing or non-numeric rank counts as -1e9. A missing file gives an
empty list.

Save: new picks become entries (a pick without a finite rank is dropped unless it is delivered). A delivery tag pins
its entries and unpins the older entries of the same tag (they become ordinary entries with `was_delivered`).
Ordinary entries are kept per context, at most 6 per context, best rank first, skipping exact parameter duplicates
of entries already kept. The file keeps every pinned entry plus ordinary entries up to a cap of 48 in all, newest
contexts first. The write is atomic (a temporary file, then a rename). JSON indent 1.

## S2. The 8 seed files of this unit

Reference kinds in the contexts: `film` = clips from the licensed film study (private, never shipped); `library` =
clips from a public sound library (individual licences); `kit` = renders of the source's own older kit; `local` =
local recordings or renders under the reference root.

### S2.1 `click.json`

- Recipe `click`. 18 entries. 27 parameter keys (the click space).
- Contexts: `a clean bright user interface click` with 8 references (4 library, 4 kit); `a crisp high digital
  interface tick for a product video` with 1 film reference. 9 entries each.
- Delivered palettes: `click` (members soft, thock, tink, snap, double; 3 of 5 pass) and `pro_click` (members tink,
  snap, mouse, glass, soft; 4 of 5 pass). Each pinned entry has its member name and seed.
- 8 ordinary entries from family runs, rounds 1-3.

### S2.2 `impact.json`

- Recipe `impact`. 14 entries. 59 parameter keys (the version 3 space; part impact).
- Contexts: `a deep cinematic boom impact` with 2 kit references (9 entries); `a deep designed sub impact with a
  granular rumble tail, gated` with 2 film references (5 entries).
- Delivered palettes: `impact` (classic, deep, punch, metal, swell, blast; 1 of 6 passes) and `pro_impact` (rumble,
  air, short, deep, swell; 3 of 5 pass).
- 3 ordinary entries from a round 2 family run.

### S2.3 `keypress.json`

- Recipe `keypress`. 9 entries. 51 parameter keys.
- Contexts: `a single mechanical keyboard key press with a crisp click and a short thock` with 15 references (6
  library, 9 kit), 6 entries; `a single deep thocky mechanical keyboard key press` with the 6 local keyboard
  references of the recipe REFS, 3 entries.
- Delivered palette: `typing_key`, members `typing_key_01` .. `typing_key_06` (3 of 6 pass).
- This file is the only home of the `typing_key` palette. The recipe has no family table (part keys).

### S2.4 `pop.json`

- Recipe `pop`. 26 entries. 121 parameter keys over all entries. Both delivered palettes are version 2 (they carry
  `style`): the `pro_pop` members have 83 keys (style chirp), the `pop` members 45 keys (style bubble).
- Contexts: `a short tonal pop` with 2 film references (12 entries); `a small bubble pop` with 7 references (2
  library, 4 kit, 1 local), 11 entries; `a small bubble pop` with the 4 local pop references of the recipe REFS, 3
  entries.
- Delivered palettes: `pro_pop` (chirp, boop, bloop, double, tink, thup; none pass) and `pop` (pop_01 .. pop_05; none
  pass). The `pop` members have only numbered names (index, Q-POP-1).
- 2 entries carry `was_delivered`.

### S2.5 `popin.json`

- Recipe `popin`. 25 entries. 34 parameter keys (some delivered members have 33).
- Contexts: `a soft low thump of a finger touching a screen` with 7 references (6 library, 1 kit), 11 entries; the
  same text with 1 film reference, 8 entries; `a soft round low pop as an icon appears on screen` with 10 film
  references, 6 entries.
- Delivered palettes: `tap` (finger, glass, table, knuckle, soft), `pro_tap` (finger, glass, soft, knock, double),
  `pro_pop_in` (round, double, knock, deep, airy). No entry passes the critic.
- The delivered `tap` members are not the family boxes of the recipe (part pop, popin section). They are later edits.
- 5 pinned entries have no robust values. 2 entries carry `was_delivered`.

### S2.6 `subdrop.json`

- Recipe `subdrop`. 13 entries. 17 parameter keys (the full subdrop space).
- Contexts: `a deep sub bass drop` (8 entries) and `a designed sub drop: a low sine falling under a bright click` (5
  entries). Both have no references (the critic ran on the text embedding alone).
- Delivered palettes: `sub_drop` (long, 808, gritty, pro, punch; 4 of 5 pass) and `pro_sub_drop` (soft, ref, quick,
  low, bright; 3 of 5 pass). The `tone_db` field is null in every entry.
- The delivered members keep the character keys of the family tables (part hits, section 2.3) and differ only in
  the fine keys that the search may move (harm, harm_lp, click, click_f, click_ms, length).

### S2.7 `toggle.json`

- Recipe `toggle`. 10 entries. 33 parameter keys (1 member has 32).
- Context: `a small switch flipped on with two quick clicks` with 4 references (2 library, 2 kit).
- Delivered palette: `toggle` (on, off, latch, micro, rocker; 1 of 5 passes).
- 5 ordinary entries from family runs, rounds 2-4.

### S2.8 `typing.json`

- Recipe `typing`. 12 entries, all pinned.
- Contexts: `a short crisp high-passed digital typing tick for a motion graphics title` with 6 film references;
  `a quick run of crisp high-passed digital typing ticks for an animated title` with 1 film reference.
- Delivered palettes: `pro_typing_key` (pro_typing_key_01 .. _06; 22 parameter keys; none pass) and `pro_typing_run`
  (pro_typing_run_01 .. _06; 31 parameter keys, the key keys plus the run keys; 3 of 6 pass).
- The key and run entries share the file. Their contexts differ, so a load for one never prefers the other.
- This file is the only home of the 2 typing palettes. The recipe has no family table (part keys).

### S2.9 Marks and fallbacks

| Asset | Mark | Fallback |
| --- | --- | --- |
| The ordinary entries of all 8 files | REGENERATE | Start with no seed file. A forge run then starts from DEFAULTS and the family members, and writes its own entries. |
| Pinned `click`, `pro_click`, `toggle`, `impact`, `pro_impact`, `sub_drop`, `pro_sub_drop`, `pro_pop` entries | DERIVE | Use the family tables of the parts as the delivered parameters, with seeds 0 .. n-1. The fine keys then differ from the source's tuned values. |
| Pinned `tap`, `pro_tap`, `pro_pop_in` entries | DERIVE | As above, from the popin family boxes. The source's later edits of `tap` are lost. |
| Pinned `pop` entries (pop_01 .. pop_05) | NEEDS-DATA | Render the pop version 2 DEFAULTS with style `bubble` and 5 different seeds, or run a hark forge of category `pop` and deliver 5 members. |
| Pinned `typing_key` entries (keypress) | NEEDS-DATA | Render the keypress DEFAULTS with 6 different `key` values (each `key` is a different board layout), or run a hark forge of category `typing_key`. |
| Pinned `pro_typing_key` and `pro_typing_run` entries | NEEDS-DATA | Render the typing key and run DEFAULTS with 6 different seeds each, or run a hark forge of the 2 categories. |
| Critic fields (`total`, `tone_db`, `passes`, robust values, `rank_score`) | REGENERATE | Recompute with the hark critic. Never copy them: they only hold for the critic that made them. |

## S3. The pluck bank file `bank/pluck.json`

### S3.1 Role

A small sound bank of the recipe `pluck`: 3 tuned kalimba voices. The note is C5. A take re-tunes through `f0`. The
bank format and its measurement fields belong to the unit bank-kit-sfx. This part gives the content.

### S3.2 Format

Top level:

| Field | Value or type | Meaning |
| --- | --- | --- |
| `bank` | 1 | bank format version |
| `name` | `pluck` | bank name |
| `version` | `pluck1` | content version |
| tool version | a version string | the version of the tool that wrote the file. The source names this field after its own project; hark names it `hark` |
| `note` | text | tuned kalimba voices from the pluck recipe (the launch film pick, a soft edit and a long-ring edit), native pitch C5, takes re-tune through f0 |
| `categories` | object | 1 category, `pluck` (below) |
| `entries` | object | key `pluck/<member>` -> entry (below) |

Category `pluck`:

| Field | Value |
| --- | --- |
| `recipe` | `pluck` |
| `target` | `a kalimba note, a thumb piano pluck` |
| `kinds` | `bell`, `text`, `hit` (the roles the category serves) |
| `use` | `a kalimba tine: tuned melodic notes (bells, motifs, a tonal lockup)` |

Entry fields:

| Field | Value or type | Meaning |
| --- | --- | --- |
| `type` | `forge` | made by a forge recipe |
| `category` | `pluck` | |
| `member` | string | member name |
| `recipe` | `pluck` | |
| `fn` | `render` | the recipe function to call |
| `params` | object | all 30 space keys plus `f0` |
| `seed` | integer | render seed |
| `peak_db` | -1.0 | the take is normalised to this peak |
| `gain_db` | 0.0 | extra gain |
| `notes` | text | what the member is |
| `meta` | object | measurements of the stored take (bank-kit-sfx): `onset_s`, `peak_s`, `end_s`, `dur_s`, `lufs_m_max`, `attack_ms`, `centroid_hz`, `channels`, `fp` = {`hop_s` 0.05, `env_db` = a list of 50 ms RMS frames in dB relative to the loudest frame, cut where the sound stays below the floor} |

### S3.3 Members

Every member starts from the pluck DEFAULTS (part tuned, section 3.2) with f0 = C5 = 440 x 2^(3 / 12) Hz
(523.2511306011972).

| Member | Seed | Edits from the defaults | Notes |
| --- | --- | --- | --- |
| `kalimba` | 8001 | none | the launch film kalimba: a bright steel tine over a small wooden box |
| `kalimba_soft` | 8009 | p2_lvl x 0.75, thumb_ms = 1.4, zing_lvl x 0.5, skin_lvl x 0.4, knock_lvl x 0.5 | a fleshy-thumb pluck: darker and softer at the attack (the film's soft notes) |
| `kalimba_ring` | 8006 | tau1 x 1.5 | a longer ring (the film's chord tones): end cards, a tonal lockup |

The source's stored measurements, for comparison only (hark measures its own renders):

| Member | onset_s | peak_s | end_s | dur_s | lufs_m_max | attack_ms | centroid_hz | channels | fp frames |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `kalimba` | 0.0032 | 0.0135 | 1.914 | 3.0 | -11.97 | 9.25 | 549.9 | 2 | 50 |
| `kalimba_soft` | 0.0032 | 0.0135 | 2.063 | 3.0 | -8.08 | 9.25 | 529.0 | 2 | 52 |
| `kalimba_ring` | 0.0032 | 0.0135 | 2.747 | 3.0 | -10.05 | 9.25 | 544.0 | 2 | 58 |

### S3.4 Mark

| Asset | Mark | How |
| --- | --- | --- |
| Header, category, member params, seeds, peak and gain | DERIVE | from S3.2 and S3.3 |
| `meta` (with `fp`) | REGENERATE | render each member with hark (params, seed), normalise to -1 dB peak, measure with the bank-kit-sfx definitions |

## S4. Reference clips (REFS)

The recipes name reference clips under the reference root (index, Interface). The forge critic compares renders with
them. They are recordings, not code, and the source does not ship them.

| Recipe | Clips | Kind |
| --- | --- | --- |
| `click` | 4 UI clicks, `ui/ui_click_01` .. `04` (wav) | local |
| `impact` | 3 impacts, `impact/impact_01` .. `03` (wav) | local |
| `keypress` | 6 keyboard keys, `keyboard/` (4 thocks, 1 enter, 1 space; wav) | local |
| `pop` | 4 UI pops, `ui/ui_pop_01` .. `04` (wav) | local |
| `pluck` | 5 kalimba notes G4 A4 B4 D5 E5, `kalimba/` (ogg), from a public sound library (items 681787, 681788, 681789, 681791, 681792) | library |

Every film and kit reference named in the seed contexts (S2) is also outside the repo.

| Asset | Mark | Fallback |
| --- | --- | --- |
| All reference clips | NEEDS-DATA | Run the critic without references (text embedding and measured profiles only), as the subdrop seeds were made. The pluck render still works without its clips: REF_F0 is a number table. Hark may record or license its own clips and name them in its own reference root. |
