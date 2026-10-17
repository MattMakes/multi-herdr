# Pitfalls: the full table

| Symptom | Cause | Fix |
| --- | --- | --- |
| A 48 kHz file reports 9 % too long | The report tool assumed 44.1 kHz | Read the rate from the header, or pass it |
| A one-shot feels late | Leading silence (for example 36 ms) in the recipe | Trim the start under 1 ms |
| A loop clicks once per cycle | Seam step above the 99th-percentile step | Render as a circular buffer; match the end values |
| WAV seams pass but the game clicks | Codec error at the edges of the encoded file | Measure the decoded file; render again |
| An 8 s loop stops at about 3 s | A fixed loop end after a compressed import | Set the loop end from the stream length at run time |
| Each new loop file breaks tests | Hard-coded loop tables in tests | One name rule for loops |
| A focused file test passes, the suite fails | Rules in other audio tests | Run the whole audio folder |
| A new file fails to load | First import pass | Import twice; judge the second pass |
| An audio timer runs in pause | Audio nodes process in pause | Stop the timer on pause |
| A duck still fires with its source muted | A muted bus still feeds a sidechain | Bypass the compressor to switch a duck off |
| A cue sounds harsh | Too much high energy | Compare the energy share above 3 kHz with a known soft cue |
| The mix recording is all zeros | A real-time record effect on a multi-channel device | Use Movie Maker mode |
| A recording runs far too long | Movie Maker time keeps the results screen open | Meter only the play part |
| Session WAV too big to load | A whole-file meter | Use a streaming meter or ffmpeg |
| The meter is off by more than 0.1 LU | A filter shape that does not match BS.1770 | Use the bilinear K-weighting form; check with the sine |
| Reward tiers do not rise in loudness | Gated loudness on short cues; peak normalization | Use momentary max; normalize by loudness |
| A latency probe passes in CI | Headless gives a dummy 0 ms latency; some drivers read 0.0 | Make the probe fail in headless and on 0.0 |
| The cue log misses physics-step cues | Entries keyed by frame number | Key log entries by identity |
| A scripted fight never reaches later phases | The scripted player deals too little damage | Lower boss health at set times in the recorder |
| A capture writes no result once | A flaky capture run | Retry once and record the retry |
| A connect with `unbind(0)` prints an error | The signal has no arguments | Handle 0-argument signals in a separate path |
| A typed read fails on event data | The field holds a different type | Read it untyped; report the contract mismatch |
| The bus file shows a large diff | The editor rewrote it in its own format | Commit the rewritten file with its `.uid` |
| Reported folder size disagrees with `du` | `du` counts disk blocks | Report the apparent size |
| Notes state wrong seam results | Written from memory | Check each number with a script |
| A drum stem disappears in the mix | Stem never measured alone | Measure each stem alone first |
| An in-place `sed -i` edit fails on macOS | BSD `sed` needs a suffix argument | Use a Python edit or the editor tool |
