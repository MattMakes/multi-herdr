# Music sets

A music set is a group of tracks for areas, fights and menus. It can come from a procedural
generator, from edited album tracks, or from an AI music web service.

1. **Settle the scope.** Ask 1 question if the count is ambiguous (for example "3 albums" or
   "3 tracks").
2. **Read the game.** List areas, fights and menus from the game data. Read the music system to
   learn its layers, keys and how it switches cues.
3. **Measure references.** Measure reference tracks into a targets table: tempo, spectral
   centroid, band energy shares and onset rate.
4. **Write the config or brief.** Map each cue to its source, tempo scale and loop start. Give
   each area a shared tempo and key family, so cues inside an area can switch cleanly. For a
   generation service, add shared style text and an exclusion list.
5. **Pilot first.** Render or generate a small set at both ends of the energy range (the
   calmest and the most intense). Measure it against the targets and get approval.
6. **Tune in small steps.** Change one level or pattern at a time. Measure each stem alone
   before a mix change; a stem can be far too quiet inside the mix. An onset detector reacts
   strongly near its threshold, so add or remove single hits.
7. **Render all.** Use a time stretch that keeps pitch, normalize loudness, then encode. Check
   seams, clipping and DC offset on each file. Match loudness to the old set within a set
   tolerance (for example 1.5 LU).
8. **Render again and compare hashes.** Every file must match.
9. **Ship beside the old set.** Add a settings choice that picks the set. An empty slot falls
   back to the original track. This lets a designer A/B the sets in the game.
10. **Import and test.** Check each `.import` file for the loop setting and a uid. Run the
    audio tests with the real files.
11. **Verify the report.** Check every number in the notes with a script before you report.
    Check that edit scripts only read the source folder, and that no source audio is staged
    for commit.

**AI music web service rules.**

- Check the account and credits with a read-only call first.
- Submit 1 cue at a time. Record each take link in an index file as you go.
- Stop at each CAPTCHA and ask the user.
- Stop at any paid step (for example a paid WAV unlock) and ask the user. Use a format the
  plan allows, such as MP3, if the user agrees.
- Close any login window before an automation tool opens the browser profile.
