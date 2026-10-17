# Hitch maps and cost maps

Use this when players report lag or a hitch at one event, or a scene has a high steady cost. Get
numbers before you change code.

1. Read the player's own perf overlay or log first. Do not change it. Compare code time per frame
   with total frame time. When code time is small, look at the GPU, data transfer and the vsync
   wait. Count the other processes on the machine.
2. Write a **hitch map**: drive the real flow with real input along a scripted path. Log each frame
   over a threshold (for example 20 ms) with its context (scene, event, position).
3. Write a **cost map**: a headless run at a fixed fps that times each system's process call per
   frame. Add a plain mode with no timers. Check that both modes give the same run outcome, so the
   timers do not change the game.
4. For an event hitch, list every listener of the event signal. Write a probe that triggers the
   event and logs one row per frame around it. Smoke it headless for a parse check. Then take
   windowed takes, one per lock hold, on each renderer. Split each slow frame into process time,
   render time and draw calls.
5. Take a warm-cache run. When the spike goes away, it is a shader compile. Go to
   [Shader warm-up](shader-warm-up.md).
6. For a steady cost on one renderer, bench each scene on each renderer with vsync off. Skip the
   first frames, sample a fixed count, and print average, p50 and p99. Hide one node or one set of
   nodes per run to find the cost. Sweep a constant in a temp copy of the project.
7. For "slow over time", run a long windowed bot run (for example 10 minutes). Log frame time, draw
   calls, primitives, node count and stored history once per second. Make a table at fixed minutes
   and find what grows. A bounded pool grows and flattens; a leak keeps its slope.
8. Run each scenario 3 times and take the median per bucket, with the load recorded.
9. Time the parts of the largest spike with a probe or a micro-benchmark. Measure the dispatch cost
   before you move work to threads; a small loop gains nothing.
10. Build each fix on a prototype branch, one commit per idea, with the full test suite after each.
    Give each fix one owner. Prove each fix with the [Method](../SKILL.md#method) pairs.
11. When the vsync wait is the cause, offer a vsync mode setting. Godot reports the requested vsync
    mode, not the mode the driver uses, so let the player test it.
12. Write a research doc with the numbers, the fixes and the open risks. When the bot does not
    reproduce a player's spikes, say so, and fix the spikes you measured.
