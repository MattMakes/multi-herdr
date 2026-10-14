# Perf research

## Research to plans

Use this when a team must find frame-rate wins before any change to shipping code, or someone
gives you a public write-up of perf techniques and asks which ones apply.

1. Measure where the time goes first. Write the cost map.
2. For an outside write-up, get the source text. List each technique with its claimed gain.
3. Map the matching systems in the project (storage, movement, collision, rendering). Classify each
   technique as done, not applicable, or candidate.
4. For each candidate, list the measured or estimated gain, the build cost, the risk, the look
   change, the files, and conflicts with work in progress.
5. Prototype the best ideas on a branch in a second worktree that never merges. For any look
   change, get before-and-after evidence with `godot-visual-verification`. Keep prototype-only commits
   (for example step timers) off the main branch.
6. Build only the safest change for real, with a test. Prove identity and gain with a paired A/B.
7. Rank the top 5 to 8. Write a plan skeleton per top idea: goal, files, steps with checks.
8. Cite each web source with its URL. Never send project code or unreleased names to a web service.

## External source research

Use this when you must learn a technique or an exact engine behaviour from an outside source
(engine source, a public demo, a repository, a post, a video) before you change your own code.

1. Read the open questions and the project files that the answer touches.
2. Check the reuse terms first. Record the source revision, the date and the reuse terms. Read the source; do not
   run its scripts.
3. Fetch only the files you need: engine source at your exact engine version tag, or a depth-1
   clone in a temp folder. Generate class docs with the engine `--doctool` if you need API detail.
4. When a page blocks the fetch, try its RSS feed or a cached copy. When nothing works, ask a human
   to save the text into a file. For a video, read frames every 2 to 3 s and the captions.
5. Read the reference files next to the project files. Map conventions (for example axis order and
   handedness) before you copy any math.
6. Check each claim and code sketch in a scratch project in a temp folder. Parse-check every sketch.
   Check each API you propose against the docs of your engine version.
7. Tell the people who need it early when a fact affects work in progress.
8. Write the report: source, reuse terms, a technique table, ranked recommendations with the change each
   needs, and the tested code. Put any project defect you found first.
9. For a port, record baseline frame times first. Port with a source comment and a third-party
   notice. Then run an interleaved A/B. For the look of the port, use `godot-visual-verification`: it
   owns the fixed-state before-and-after captures and the pixel-diff noise floor.
