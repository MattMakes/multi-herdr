---
name: godot-perf-measure
description: >-
  Measure and fix frame cost in a Godot game with numbers you can trust on a shared, noisy dev
  machine. Covers interleaved A/B pairs against a base build, per-part milliseconds instead of FPS,
  hitch maps and cost maps, stress scenarios and leak hunts, per-feature cost budget tests, shader
  warm-up lists with a guard test, MultiMesh batching, physics interpolation, pixel-art camera
  judder, view-edge fill and seeded chunk streaming. Use it whenever someone wants to know why a
  Godot game lags, stutters, hitches or gets slow over time, asks if a change makes frames faster
  or slower, wants a draw-call or frame budget, wants to batch many sprites, sees a stall the first
  time an effect draws, turns on physics interpolation, or wants to rank speedups from a public
  write-up, even if nobody says "profiling" or "A/B".
---

# Godot Perf

Target engine: **Godot 4.7**. The Python block in the references uses the standard library only.

A frame-time number from one run on a busy machine is noise, not evidence. Vsync, the display cap,
cold shader caches, other processes and a background window all move frame time more than most
code changes do. The method measures milliseconds per part in a real window, against a base build
at the same merge base, in short interleaved A/B pairs with the machine load recorded. It works
because pairs taken back to back see the same machine state, so the gap between A and B survives
the noise even when the absolute numbers do not.

## When to use

- You must prove that a change makes frames faster, or does not make them slower.
- A new effect or system must stay under a per-frame cost or draw-call budget.
- Players report lag, a hitch at one event, or play that gets slow over time.
- The first draw of a material stalls the game.
- Many similar nodes cost too many draw calls.
- You turn on physics interpolation, or a low-resolution pixel-art camera judders.
- An endless or chunked world must stream content without hitches.
- Someone hands you a public perf write-up and asks which ideas apply.

## When not to use

- The question is about game balance or bot fight results. Use `godot-bot-balance-loop`.
- You only need to prove a refactor keeps behaviour. Use `godot-safe-refactor`.
- You need a visual check, not a time. Use `godot-visual-verification`.
- The problem is a headless tool that hangs or writes files. Use `godot-headless-runs`.

## Method

The main procedure is an A/B measure. Every other section feeds it or ends in it.

1. **Measure milliseconds per part, not FPS.** Record process time, physics time, render CPU and
   GPU time, draw calls, primitives and static memory. FPS hides where the time goes and caps at
   the display rate. Check: your log has one column per part.
2. **Sum every viewport.** Turn on render-time measurement for the root viewport and for each
   SubViewport, and add them. A root-only number can miss most of the 3D cost. Check: the summed
   render time is larger than or equal to the root number.
3. **Uncap the frame.** Turn vsync off and run with `--max-fps 0` for timing runs. Write these into
   a scratch settings file for each run, because saved user settings can force vsync back on.
   Check: frame time is not stuck at the display period (for example 8.33 ms at 120 Hz).
4. **Use a driver with GPU timers.** Some drivers return 0 for every GPU timer (for example Metal on
   macOS). Switch the driver (for example `--rendering-driver vulkan` through MoltenVK). On a
   renderer with no GPU timer, compare wall time and render CPU time. Check: the GPU column is not
   all zeros, or you state why it is.
5. **Run in a real window, in the foreground.** Headless has no draw, uses a dummy renderer, and
   shows false memory and object growth. Some desktops throttle a background window. Check: the
   run log names the renderer and window mode.
6. **Keep a base build at the same merge base.** Use one of these:
   - a detached worktree of the base commit in a temp folder, imported once;
   - an exported base build;
   - a probe flag that switches the change on and off in the same binary.
   Move the base to the new merge base after each merge. Check: base and branch share the merge
   base commit.
7. **Take the machine.** On a shared machine, hold a perf lock (and a window lock if other agents
   run windowed captures). Keep each hold under about 3 minutes. Run no other batch job at the
   same time. Check: the lock owner file names you for the whole take.
8. **Warm up, then interleave.** Run one untimed warm-up run first, because the first run carries
   cold caches. Then run A and B in back-to-back pairs, 3 to 5 pairs, and alternate which goes
   first. When noise stays high, switch the variant window by window inside one process. Check:
   you have at least 3 valid pairs.
9. **Record the load with every take.** Log the load average and the count of engine processes.
   Mark a take invalid above a load limit you set before the run. Check: every row has a load
   value and a valid flag.
10. **Report the gap, not the absolute.** Report the median gap and its share of the frame. Under
    load, report the best median too, because load inflates the best value least. Mark a result
    weak when the per-pair gaps straddle 0 or the A time drifts across the take. Repeat a run with
    one big spike before you report it. Check: the A/B report script ([script-ab_report.md](references/script-ab_report.md)) prints the gap, the share and
    the weak flag.
11. **Measure through the real setting.** For a graphics setting, switch it through the settings
    API the player uses, not through a direct engine call. Only then does the measure prove the
    setting. Check: the probe sets the setting key.
12. **Prove behaviour and gain for a speedup.** Write value tests that pass on the old code and a
    speed test that fails on it. Then:
    - prove equal behaviour: the same game result in every A/B run, and, for a draw change,
      identical pixels by the `godot-visual-verification` method (fixed state, 1x, noise floor);
    - prove the gain with the pairs above;
    - break one key call in the new code and check that a test fails, then restore it;
    - run the A/B again on the final code before you report.
    Check: every number in the notes comes from a measure, not an estimate. Record negative
    results too.
13. **State the proxy.** A low-end renderer and a render scale stand in for weak hardware. Label
    those numbers "estimate". Check: the report says which numbers are proxies.

## Other procedures

9 more procedures feed the Method or end in it: cost budgets, hitch and cost maps, stress and leaks, research,
shader warm-up, MultiMesh batching, physics interpolation, pixel-art camera and chunk content.
[References](#references) names the file of each one and when to read it.

## Pitfalls

The 7 most frequent rows. The full table of 56 rows is in [references/pitfalls.md](references/pitfalls.md).

| Symptom | Cause | Fix |
| --- | --- | --- |
| Frame time sits at 8.33 ms (or 16.67 ms) with vsync "off". | Saved user settings force vsync on, or the fps cap is on. | Vsync off and `--max-fps 0` in a scratch settings file for each run. |
| Every GPU timer reads 0. | The driver gives no GPU timestamps (for example Metal). | Use another driver, or compare wall time and render CPU time. |
| Render time is tiny next to the frame (for example 0.15 ms against 1.65 ms). | The measure reads only the root viewport. | Sum every SubViewport. |
| Numbers jump when you switch apps. | The OS throttles a background window. | Run in the foreground. |
| Frame times fall by half between two runs of the same code. | Machine load changed. | Interleaved pairs, load per take, invalid flag above a limit. |
| A cost test fails at 0.1 ms under load and passes at 0.025 ms when idle. | One noisy run. | Best of 3 to 5 runs inside the test. |
| A 60 to 110 ms spike on the first use of an effect. | A shader compiles on first draw. | Add the material to the warm-up; prove with cold runs. |

## Artifacts to build

- **Frame sampler:** a script that logs per-frame process, physics, render CPU and GPU (summed over
  every viewport), draw calls, primitives, node count and static memory to CSV.
- **A/B pair runner:** a shell script that takes the lock, runs the warm-up, runs A and B
  interleaved for N pairs, and writes one CSV with variant, pair, load and process count per row.
- **A/B report:** [references/script-ab_report.md](references/script-ab_report.md). Copy the block into a file. It reads frame CSVs and prints per-take
  stats, medians, the gap, its frame share and a weak flag.
- **Base build helper:** a script that makes, imports and moves the base worktree or export.
- **Hitch map tool:** a windowed drive along a scripted path that logs frames over a threshold with
  context.
- **Cost map tool:** a headless fixed-fps run that times each system per frame, with a plain mode.
- **Event probe:** a scene script that fires one event and logs one row per frame around it.
- **Stress harness:** named scenarios, a sampler, a split timer, a summary CSV and a report.
- **Churn loop:** a script that loads, plays and frees a scene N times and prints object, node and
  orphan counts per cycle.
- **Cost budget tests:** headless tests that drive a worst case and fail above a budget, best of N.
- **Warm-up list and guard test:** one data list of material sources in groups, plus a test that
  scans the project for unlisted materials.
- **Long-run drive:** a bot or camera drive for 10 minutes or more, with a per-minute summary table.
- **Standalone bench project:** a minimal project for architecture choices, with equal work per
  variant.
- **Perf notes table:** a checklist row per number: what, scenario, renderer, lock state, load,
  median, range, valid.

## Related skills

- `godot-bot-balance-loop`: plays the game with bots for balance numbers; this skill reuses its drives
  for perf scenarios only.
- `godot-safe-refactor`: proves that a change keeps behaviour; this skill adds the speed proof.
- `godot-visual-verification`: owns the visual proof. It gives fixed-state window captures, 1x
  comparison, before-and-after pairs and the pixel-diff noise floor that prove "same pixels". This
  skill gives only the paired timing evidence.
- `godot-headless-runs`: tool-script hygiene, flags and process cleanup for the headless probes
  here.
- `godot-test-loop`: the test loop, proof that a test can fail, and the exit leak hunt at suite end.
- `godot-feature-seams-and-saves`: data seams and flags that let a probe switch a variant in one binary.
- `godot-upgrade-tree-design`: design research for questions that are about fun, not frame time.
- `godot-fleet-game-dev`: the mkdir perf and window locks, the owner check, and wave plans from the
  ranked perf ideas.

## References

- [references/script-ab_report.md](references/script-ab_report.md): the A/B report script; read it at step 10 of the Method.
- [references/cost-budgets.md](references/cost-budgets.md): read it before you write a per-frame or draw-call budget test.
- [references/hitch-and-cost-maps.md](references/hitch-and-cost-maps.md): read it when players report lag, a hitch or slow play over time.
- [references/stress-and-leaks.md](references/stress-and-leaks.md): read it to build a stress harness or to hunt a memory leak.
- [references/research.md](references/research.md): read it to rank perf ideas or to study an outside source.
- [references/shader-warm-up.md](references/shader-warm-up.md): read it when the first draw of a material stalls.
- [references/multimesh-batching.md](references/multimesh-batching.md): read it before you batch sprites into a MultiMesh.
- [references/physics-interpolation.md](references/physics-interpolation.md): read it before you turn on physics interpolation.
- [references/pixel-art-camera.md](references/pixel-art-camera.md): read it for pixel-art camera judder or view-edge fill.
- [references/chunk-content.md](references/chunk-content.md): read it for seeded per-chunk content and streaming.
- [references/pitfalls.md](references/pitfalls.md): the full symptom, cause and fix table; read it when a measure looks wrong.
