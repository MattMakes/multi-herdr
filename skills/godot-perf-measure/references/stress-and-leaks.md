# Stress harness and leak hunt

Use this when you need a repeatable worst case and a baseline per scene and renderer, or a memory
leak across many runs.

1. Build a harness that drives named scenarios through existing seams, with no edit to shipping
   code. Good scenarios: the heaviest levels on a fixed path, the worst fights at 1x, 2x and 4x
   spawn counts (a multiplier knob), a late-game save, a resolution matrix on each renderer, and
   a long endurance run (for example 30 minutes).
2. Add a frame sampler (process, physics, render CPU and GPU per frame, summed over every
   SubViewport), a per-script split timer, a summary CSV and a report script.
3. Build scenario 1 first. Prove that 3 back-to-back runs give medians within about 10 %. Share the
   harness command as soon as it works, so other work can use it.
4. Run the baseline matrix per renderer under the perf lock. Record the load per row.
5. Run the endurance scenario. Log object, node and orphan counts and memory per run. A count that
   grows run after run is a leak.
6. For a leak, run a churn loop that loads, plays and frees the run scene and prints counts per
   cycle. Run a control churn with only idle frames. Bisect by behaviour (idle against active,
   menus against no menus). Report each cause with its call count and its growth per call.
7. Rank the top 10 hotspots by milliseconds and frame share with the split timer. List every error,
   log flood, crash and hang with steps to reproduce.
8. Scale scripted routes to the bot's speed. A fast bot that circles a waypoint it cannot reach
   never triggers a late event.
