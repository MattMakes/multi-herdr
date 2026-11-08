# Make a sound in code: forge and fit

Use this loop when the user wants a sound built, not picked. Every output sample is computed. No recorded audio goes
in, impulse responses included. Real recordings serve only as the references the critic scores against. The
`forge-core`, `recipes-impacts` and `recipes-motion` units own the engine and the recipes.

## Loop 4: forge

1. Pick a recipe or write 1. Recipes live in `hark/forge/recipes/<group>/<name>.py` (groups `ui`, `impacts`,
   `motion`, `risers`). A recipe states its parameter ranges, defaults, target description and plausible confusions.
   Its render function takes parameters and a random generator and returns a 48 kHz array. Build it from the forge DSP
   kit: modal resonators, contact excitations, coloured noise, a state-variable filter that moves over time, a
   computed room, and a microphone model with a noise floor. Base it on the physics of the real source as measured on
   the references: what strikes what, what resonates, for how long, in what space.
2. Run a round:
   `hark forge <recipe> --refs <real refs> --budget 144 --reps 2 --tone 1.5 --keep 4 --robust 8 --dir <round dir>`,
   or a per-round Python script that loads the models once. `--tone` adds tonal balance and tail (model-free) to the
   objective. `--reps 2` scores each candidate on 2 renders so a lucky render cannot win. Budget about 3-5 minutes per
   160 renders on a loaded laptop CPU, plus a 30-60 s model warm-up.
3. Read the round. Each pick's summary says what it resembles, how far its decay lies outside the references, and how
   far each tone band lies from the reference median (with the bands outside every reference). Check each pick from 4
   angles:
   - the model-free tone words (bands outside every reference, energy shares, tail, length): the most reliable signal
     once model scores are high;
   - `robust`: the mean over fresh seeds, not the luckiest render;
   - `alt_zs`: the closest confusion;
   - spectrograms beside the references':
     `ffmpeg -i x.wav -lavfi showspectrumpic=s=900x360:scale=log:legend=0 x.png`.
4. The tagger tags in `report.json` stay a held-out check only while nobody selects or steers by them. Fix the
   selection rule first (for example the robust mean), then look at the tags and report them.
5. When something is wrong (no release tick, a metallic ring, an overlong tail), change how the recipe is built, not
   only its ranges. Run again. Log every change and its numbers for every round.
6. Report the result beside the real references, scored by the same critic (the leave-one-out reference scores in
   `report.json`). Do not overstate it.

## Goodhart risks (state them)

1. The search maximises scores from hark's own models. Built sounds often beat real recordings on those scores. A
   satisfied critic is no evidence of a better sound.
2. p_real is easy to fool. Modal resonators with a room and a microphone noise floor already score as real.
3. Typicality favours a sound that averages the references.
4. Model checks saturate. After a search every model check, held-out phrasings and separate real sets included,
   places the built sound among the real ones. Then only measurements and human listening still tell them apart.
5. Judge by measured distance to the references (tonal balance, bands outside every reference, tails, lengths), by
   robust re-renders and by spectrograms.
6. Call a built sound as good as real only after a human blind test: shuffled order, matched loudness, the key hidden
   until the end.

hark has not run an independent blind trial of its forge output yet. Report hark's own trial once one exists.

## Fit: translate a recording into recipe code

`hark fit <file> [-o recipe.py] [--kind auto] [--takes]` analyses a recording (sinusoidal, modal, spectral, space and
gesture models), picks a model, and writes recipe code that renders without the recording. `--takes` scores each
candidate on seeds 0, 1 and 2 instead of seed 0 alone. `hark fit --morph A B [--amount 0.5]` blends 2 fits.
`hark fit --forge recipe.py [--budget 96]` runs the critic search on a fitted recipe.

To add your own fit to the bank: `python -m hark.forge.bank add-fit MODEL --id ID --source source.json [--vary 0.3]`,
where `source.json` is `{id, title, author, license, url}` of the CC0 recording. The fitted sound is code; the source
is named, never shipped.
