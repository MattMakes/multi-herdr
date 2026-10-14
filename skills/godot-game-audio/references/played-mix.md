# Measure the played mix

File numbers do not prove the mix. Buses, ducks, a limiter and overlapping cues change what
the player hears. Measure the mix from a recording of the real game.

1. **Write an audio contract.** It holds file names, the bus list, a loudness target per bus,
   a size budget and the one loop rule. Ask the owner for each bus target; do not invent it.
2. **Validate the meter first.** A 997 Hz sine at 0 dBFS reads about -3.01 LUFS. Cross-check
   your meter against ffmpeg `ebur128` within 0.1 LU. A meter that fails this makes every
   later number useless.
3. **Record with Movie Maker mode.** Run the game with `--write-movie <path>` and a fixed frame
   rate at a small window resolution. Movie Maker mode mixes audio offline, so the recording
   holds the real mix at a steady rate. A record effect on the master bus can record silence on
   some multi-channel devices. Record several fights or scenes and normal play, with and
   without music. Record single buses (solo each bus) when you tune one bus.
4. **Vary the second run.** A deterministic autoplay gives identical runs. Use a second level
   or field for the second normal run.
5. **Meter only the play part.** Movie Maker time keeps running on menus and results screens.
   Cut the recording to the play part by the game's event log. Use a streaming meter or
   ffmpeg for long files.
6. **Print a rule table.** For each recording print integrated LUFS, true peak, momentary max,
   per-event levels, the level gap between 2 key sounds, and the duck depth. Mark each row
   PASS or FAIL against its rule.
7. **A/B one setting at a time.** Change 1 bus setting. Record and meter again. Save the chosen
   configuration before an A/B and restore it from the copy after.
8. **Check loops after encoding.** Follow `loop-seam.md` on the decoded file
   (for example the `.ogg`), not only on the source WAV. A codec can add error at the edges.
9. **Pin the result.** Add a test that pins the chosen bus values. Commit the bus layout file
   in the form the editor rewrites it, with its `.uid` file.
10. **Re-check after fixes.** When review fixes land, measure on the base branch with no local
    edits. Write a recheck file: each row that failed before gets a new result of pass, fail
    or not measured. Measure every value a fix note says it did not measure.

Reward cues need care. Use the maximum momentary loudness, not gated integrated loudness, to
rank short reward tiers. Normalize them by loudness, not by peak, so a bigger reward is
louder.
