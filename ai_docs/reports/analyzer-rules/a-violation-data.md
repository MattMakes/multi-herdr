# Complexity findings and rules for prevention

Local research for T-45. Horch source revision `d01cb8aeb7de51814543274e03105eb725a414a4`. Evidence and Git history stay local.

The raw evidence is dominated by obsolete Rust parser output. The ledger's retained gate messages show a different pattern. Scope depth leads with 161 of 335 visible findings, followed by maintainability index with 53. The most useful observed fixes separate callbacks, setup, cleanup, and per-item work at existing responsibility boundaries. Python also has confirmed parser false positives in comments and expressions.

## Evidence and reproduction

`crates/horch-core/src/taskledger/finish/complexity.rs:12` lists JS, TS and Python extensions. The finish step groups changed files by nearest config and runs the analyzer with JSON output and threshold action disabled. The analyzer supplies `violations[]` with `filePath`, `functionName`, `line`, `metric`, `value`, and `threshold`. Count that array directly. The raw corpus includes 28 values equal to their threshold and 5 maintainability scores below their threshold. A strict greater-than filter would discard both kinds. The gate maps findings to diff hunks using the next reported function start as the boundary (`complexity.rs:70–119, 430–497`). Rust now goes to Clippy's cognitive-complexity lint, recorded in separate `*-clippy.json` evidence (`complexity.rs:160–200, 482–488`). ADR 0086 documents the legacy parser's fictitious Rust functions and the nearest-config policy. The implementation deduplicates Clippy spans by file and line.

Scratch scripts and commands:

```sh
cd ~/.local/state/horch/researcher-8-scratch
python3 mine.py rows.json
python3 events.py events.json
python3 trace_history.py
python3 examples.py
```

The first command reads every `~/.horch/evidence/*/*/*complexity*.json`. It found 459 reports, all valid JSON, and 73 raw findings across 70 distinct project/path/function/line/metric keys. The second command reads the local SQLite ledger in read-only mode. It recovers 335 displayed findings from 67 failure events that advertised 1,338 findings in total. Sixty-four failure events have a later zero-violation pass for the same task and worker. The gate displays at most 10 findings per event (`complexity.rs:343–365`), and repeated attempts count again. The reports are mutable by task and sequence. A passing retry can replace its predecessor's raw JSON, so the raw and gate columns describe different populations and should never be added. Clippy counts in the table come solely from the visible gate messages.

| Metric | Raw JSON | Visible gate |
| --- | ---: | ---: |
| scopeDepthRatio | 20 | 161 |
| maintainabilityIndex | 5 | 53 |
| peakLiveVariables | 15 | 29 |
| halsteadEffort | 3 | 20 |
| abcScore | 1 | 16 |
| variableSpan | 1 | 16 |
| scopeDepthWeightedLoc | 1 | 11 |
| globalDataComplexity | 24 | 10 |
| cognitiveComplexity | 0 | 5 |
| halsteadVolume | 0 | 5 |
| essentialComplexity | 1 | 4 |
| cyclomaticComplexity | 1 | 3 |
| halsteadDifficulty | 1 | 2 |
| moduleDesignComplexity | 0 | 0 |
| specifiedDataComplexity | 0 | 0 |
| **Total** | **73** | **335** |

| Language | Raw JSON | Visible gate |
| --- | ---: | ---: |
| TypeScript | 0 | 214 |
| JavaScript including MJS | 11 | 94 |
| Python | 0 | 9 |
| Rust | 62 legacy | 13 legacy and 5 Clippy |

| Local project | Raw JSON | Visible gate | Most frequent observed limits, with hit counts |
| --- | ---: | ---: | --- |
| `~/infra/pi-worker` | 0 | 214 | Scope ratio 1.5 (84) and 1.3 (26), maintainability 60 (48), ABC 20 (11) |
| `~/games/infinite-ruins` | 0 | 50 | Scope ratio 1.5 (21) and 1.3 (6), live variables 7 (8), Halstead effort 5,000 (3) and 10,000 (2) |
| `~/games/monuments` | 0 | 41 | Scope ratio 1.5 (15) and 1.3 (5), live variables 7 (5) and 10 (3), effort 5,000 (4) and 10,000 (2) |
| `~/repo/multi-herdr` | 65 | 30 | Clippy cognitive 25 (5). Raw legacy Rust global-data 3 (22) and 6 (2), live variables 7 (12) and 10 (2). |
| `~/games/grid-defense` | 8 | 0 | Raw scope ratio 1.5 (3) and 1.3 (1). Live variables 7, difficulty 30, effort 10,000 and maintainability 60 (1 each). |

These are limits attached to observed findings, including older config versions and directory-specific limits. They do not inventory every configured metric. Across the 335 visible findings, ratio 1.5 leads with 120 hits, followed by maintainability 60 with 52, ratio 1.3 with 41, and live variables 7 with 25. The label `max` in the gate also appears for the lower-bound maintainability metric.

The full raw census has 50 named and 23 anonymous parser labels. The gate census has 182 other named, 112 anonymous, 14 JS/TS private-method, 9 Python-labelled, 13 legacy Rust, and 5 Clippy findings. Those counts use parser labels. Structural counts below use inspection of the Git samples. In the raw data, the three highest Rust pairs are global data (24), scope ratio (15), and live variables (14). Labels such as `VALUES` in `taskledger/tasks.rs:360`, `fmt` in `cmd/recipes.rs:165`, and `read_dir` in `vcs/landing.rs:370` illustrate why their counts cannot establish Rust coding rules. The raw JS leader is scope ratio (5), with 4 hits from `grid-defense/scripts/regen-golden.mjs` and 1 from `multi-herdr/skills/video-render/scripts/render_frames.mjs`. The raw JS counts for maintainability and Halstead effort are 2 each, and live variables and Halstead difficulty have 1 each.

## Historical patterns

The top 10 pairs use visible gate messages and combine JS and MJS. Each entry cites its ledger event and an inspected `git show <revision>:<path>` source. The failing tree was not preserved by each event, so the before revision is a source match by path, line, symbol and time. The after revision is the recorded landed commit. Later zero-violation passes corroborate the task result without isolating each refactor's effect. Each snippet is an excerpt from the local Git history.

### 1. scopeDepthRatio, TS (110 findings)

**acquireProcessLock** · resource lifetime · 1.533 / 1.5. `pi-worker 358cbeb930:src/lock.ts:29` (event 20164; pass event 20295).

```ts
export function acquireProcessLock(lockPath: string): ProcessLock {
  const db = new DatabaseSync(lockPath, { timeout: 0 });
  try {
    db.exec("PRAGMA locking_mode = EXCLUSIVE");
    db.exec("BEGIN EXCLUSIVE");
```

Landed `55a6e2414f`. Split lock acquisition and release ownership into lockExclusive and heldLock.

**kubectlExecSandboxCommand** · nested callback · 2 / 1.5. `pi-worker 358cbeb930:src/sandbox/transport.ts:11` (event 20255; pass event 20295).

```ts
export function kubectlExecSandboxCommand(namespace: string = DEFAULT_SANDBOX_NAMESPACE): SandboxCommandFactory {
  return (podName, container) => [
    "kubectl",
    "exec",
    "-i",
```

Landed `55a6e2414f`. Bind the namespace to the named kubectlExecArgv helper.

**close** · resource lifetime · 1.429 / 1.3. `pi-worker 358cbeb930:src/sandbox/transport.ts:61` (event 20255; pass event 20295).

```ts
    async close() {
      if (closed) return;
      closed = true;
      try {
        await env.cleanup(BACKGROUND_CONTEXT);
```

Landed `55a6e2414f`. Cache the cleanup promise and move finalization to a named callback.

### 2. scopeDepthRatio, JS (49 findings)

**open** · nested callback · 1.571 / 1.5. `infinite-ruins ce71a33f84:design/prototype/feel/tools/measure.js:17` (event 10735; pass event 10781).

```js
async function open(browser, errors, seed, opts = {}) {
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 860 }, deviceScaleFactor: 2, ...opts });
  const page = await ctx.newPage();
  page.on("console", m => { if (m.type() === "error") errors.push(m.text()); });
  page.on("pageerror", e => errors.push(String(e)));
```

Landed `5d2302185b`. Bind named console and page-error handlers.

**installAuto** · nested callback · 3.341 / 1.5. `infinite-ruins ce71a33f84:design/prototype/feel/tools/measure.js:37` (event 10735; pass event 10781).

```js
function installAuto(opts) {
  const P = window.__proto, S = P.S;
  const SCALE = new Set(["stylus", "servo", "torque", "spindle", "metal", "smelter"]);
  window.__log = { nonInt: [], popups: new Set(), stopAt: opts.stopAt || 0, shopEmptyAt: null };
  function relGain(u) {
```

Landed `5d2302185b`. Move the autoplayer to its own script and keep a small browser entry point.

**specialGain** · policy dispatch · 1.6 / 1.5. `infinite-ruins 1afc3a8103:design/prototype/feel/tools/autoplayer.js:22` (event 10760; pass event 10781).

```js
function specialGain(u, total) {   // nodes the sim values by a rule, not by the income change
  const P = P_();
  if (u.id.startsWith("com-")) return 0.7 * oneMoreBotGain(total);
  if (u.id === "uplink") return P.S.bots.length < P.fleetCap() ? 0.02 : 0.9 * oneMoreBotGain(total);
  if (u.id === "smelter") return P.alloyRated() > 0 ? 0.5 * 0.5 : 0;
```

Landed `5d2302185b`. Map distinct item IDs to scoring functions and dispatch through one lookup.

### 3. maintainabilityIndex, TS (48 findings)

**createOmlxModels** · setup and orchestration · 49.65 / 60. `pi-worker 358cbeb930:src/models.ts:28` (event 20164; pass event 20295).

```ts
export function createOmlxModels(env: OmlxEnv = process.env): MutableModels {
  const baseUrl = env.LITELLM_BASE_URL ?? DEFAULT_LITELLM_BASE_URL;
  const provider = createProvider<"openai-completions">({
    id: OMLX_PROVIDER_ID,
    name: "oMLX via LiteLLM",
```

Landed `55a6e2414f`. Move the provider definition into omlxProvider.

**openHarness** · setup and orchestration · 57.61 / 60. `pi-worker dc0dbb2da2:test/gpu-gate.test.ts:235` (event 20971; pass event 21084).

```ts
async function openHarness(clock: FakeClock, extension: Extension, fake: FakeModels = createFakeModels([])): Promise<Harness> {
  const registry = createRegistry();
  registry.install(extension);
  state.harness = await Harness.open(
    await openNodeSqliteStorage(join(state.root, "pi-durable.sqlite")),
```

Landed `f7d5f0933a`. Extract harnessOptions and give opening the storage its own statement.

**onUpstreamConnected** · resource lifetime · 59.78 / 60. `pi-worker e6edda1eeb:src/egress-proxy/server.ts:187` (event 21487; pass event 21494).

```ts
function onUpstreamConnected({ client, upstream, head, address, log }: Tunnel): void {
  upstream.setTimeout(0);
  upstream.removeAllListeners("error");
  upstream.on("error", destroyStream.bind(undefined, client));
  client.on("error", destroyStream.bind(undefined, upstream));
```

Landed `0362717d5a`. Extract the bidirectional stream listeners into linkStreams.

### 4. abcScore, TS (13 findings)

**expectResumed** · multi-phase test · 27.17 / 20. `pi-worker 56bf292c9b:test/harness-pienv.test.ts:128` (event 20220; pass event 20295).

```ts
  harness.resume();
  const conversation = await harness.root(context);
  expect(conversation.id).toBe(conversationId);
  expect((await harness.inspect(context)).tasks).toHaveLength(0);

```

Landed `55a6e2414f`. Split restored-state assertions from the assertions on a new run.

**tunnelsBothWays** · multi-phase test · 20.62 / 20. `pi-worker e6edda1eeb:test/egress-proxy.test.ts:142` (event 21487; pass event 21494).

```ts
async function tunnelsBothWays(target: string): Promise<void> {
  const proxy = await startProxy(await startEchoServer());
  const { socket, head } = await rawRequest(proxy.proxyPort, `CONNECT ${target} HTTP/1.1`);
  expect(head.startsWith("HTTP/1.1 200")).toBe(true);
  const reply = readOnce(socket);
```

Landed `0362717d5a`. Extract pingPong and the allowed-tunnel assertion.

**<anonymous>** · multi-phase test · 36.14 / 30. `pi-worker 49ad95675b:test/config-and-skills.test.ts:38` (event 20979; pass event 21187).

```ts
  it("adds, updates, disables and removes repos while keeping the header comments", async () => {
    const header = (await readFile(join(REPO_ROOT, ALLOWLIST_PATH), "utf8")).split("repos:")[0]!;
    let entries: AllowlistEntry[] = [];

    entries = [...entries, { repo: "MattMakes/alpha", enabled: true, notes: "small TS lib" }];
```

Landed `890119e6a1`. Split the add, update, disable and remove cases and share roundTrip.

### 5. halsteadEffort, JS (12 findings)

**bestCell** · iteration and aggregation · 1.1e+04 / 10000. `infinite-ruins 1afc3a8103:design/prototype/feel/tools/autoplayer.js:64` (event 10760; pass event 10781).

```js
  const P = P_(), S = P.S;
  const r0 = Math.max(0, Math.ceil(S.camY / P.BW)), r1 = Math.floor((S.camY + P.H) / P.BW) - 1;
  let best = null, bv = 0;
  for (let r = r0; r <= r1; r++) for (let c = 0; c < P.COLS; c++) {
    const v = cellValue(c, r);
```

Landed `5d2302185b`. Extract rowCells and better, and keep the fallback at the selection boundary.

**candidate** · policy dispatch · 5388 / 5000. `infinite-ruins f27fab0d62:design/prototype/feel/tools/autoplayer.js:109` (event 10764; pass event 10781).

```js
function candidate(u, inc) {   // null when the sim buyer would skip it
  const P = P_(), cur = P.currency(u), g = relGain(u);
  const skip = g <= 0.004 || (SCALE_NODES.has(u.id) && g < 0.15) || !(inc[cur] > 0);
  return skip ? null : { v: Math.log10(1 + g) / Math.max(1, P.cost(u) / inc[cur]), u, g, cur };
}
```

Landed `5d2302185b`. Move eligibility to worth and name the income rate once.

**longestGap** · iteration and aggregation · 8756 / 5000. `infinite-ruins 4a1831afe9:design/prototype/feel/tools/measure.js:37` (event 10769; pass event 10781).

```js
function longestGap(times) {
  const t = [0, ...times.filter(x => x < WINDOW), WINDOW];
  let gap = 0, at = "";
  for (let k = 1; k < t.length; k++) if (t[k] - t[k - 1] > gap) { gap = t[k] - t[k - 1]; at = t[k - 1].toFixed(1) + "-" + t[k].toFixed(1); }
  return { gap, at };
```

Landed `5d2302185b`. Separate time-window selection, gap calculation and widest-gap selection.

### 6. variableSpan, TS (11 findings)

**createOmlxModels** · setup and orchestration · 32 / 20. `pi-worker 358cbeb930:src/models.ts:28` (event 20164; pass event 20295).

```ts
export function createOmlxModels(env: OmlxEnv = process.env): MutableModels {
  const baseUrl = env.LITELLM_BASE_URL ?? DEFAULT_LITELLM_BASE_URL;
  const provider = createProvider<"openai-completions">({
    id: OMLX_PROVIDER_ID,
    name: "oMLX via LiteLLM",
```

Landed `55a6e2414f`. Build the provider separately and register it next to model creation.

**createFakeModels** · setup and orchestration · 116 / 35. `pi-worker 358cbeb930:test/fakes/fake-models.ts:55` (event 20255; pass event 20295).

```ts
export function createFakeModels(scriptedTurns: readonly ScriptedTurn[]): FakeModels {
  const queue = [...scriptedTurns];
  const requests: TranscriptContext[] = [];
  let toolCallCount = 0;

```

Landed `55a6e2414f`. Move scripted state into FakeScript and provider metadata into fakeProvider.

**stream** · nested callback · 63 / 35. `pi-worker 358cbeb930:test/fakes/fake-models.ts:73` (event 20255; pass event 20295).

```ts
    const index = requests.length;
    requests.push(context);
    const step = queue.shift();
    const turn: FakeTurn =
      step === undefined
```

Landed `55a6e2414f`. Move turn selection and emission behind #nextTurn and #emit.

### 7. peakLiveVariables, Python (9 findings)

**draw_spray** · parser artifact · 7 / 7. `infinite-ruins 64fda7c425:design/board/vfx/generate.py:1473` (event 10284; pass event 10291).

```python
        draw_spray(c, sparks, f, cx, top, [VOLT_CORE, lt(VOLT, 0.5), VOLT, dk(VOLT, 0.4)])
        frames.append(c)
    return frames, bore_context(SEED + 423), {"key_frame": 4 * N // 8}


```

Landed `5d08a62f6e`. In the final fix the hot call site passes TESLA_SPARKS instead of constructing its palette inline. Earlier attempts split particle math.

**#** · parser artifact · 9 / 7. `infinite-ruins 35e5bfe038:tools/audio/music/track_cinder.py:104` (event 44070; pass event 44168).

```python
    # bell: bell semitone shift; cut: low-pass scale; roar: roar level; ir: reverb shape
    # (MR 4: B is lower, closed filters, longer reverb). thump: sub thump level.
    "a": {"bell": 0, "cut": 1.0, "roar": 1.0, "ir": {"rt60": 3.0, "length": 2.8}, "thump": 0.20},
    "b": {"bell": -12, "cut": 0.6, "roar": 1.3, "ir": {"rt60": 4.6, "length": 4.4}, "thump": 0.18},
}
```

Landed `f1acbf8cc0`. Move dictionary metadata into prose comments above the declarations.

**enumerate** · parser artifact · 7 / 7. `multi-herdr 5a8b455afb:skills/incremental-game-design/scripts/scan_godot.py:95` (event 68134; pass event 68293).

```python
NOT_BEAT = re.compile(r"request|pressed|clicked|refused|denied|failed|hovered|selected|focus")
KIND_ORDER = {k: i for i, k in enumerate(["purchase", "unlock", "ladder step", "level up", "milestone", "reward", "reset"])}
TOOLING = re.compile(r"(?:^|[_\-./])(?:sim|sims|simulator|simulation|simulate|balance|balancing|diag|diagnostics?|"
                     r"tune|tuning|tuner|calibrat\w*|polic(?:y|ies)|sweeps?|autoplay\w*|playstyle\w*|headless|"
                     r"full_run|fun_targets|(?:\w+_)?targets|parity|fixtures?)(?:[_\-./]|$)", re.I)
```

Landed `3de4341298`. Name KINDS and construct the rank dictionary separately with zip.

### 8. peakLiveVariables, JS (9 findings)

**pace** · iteration and aggregation · 9 / 7. `infinite-ruins 4a1831afe9:design/prototype/feel/tools/measure.js:45` (event 10769; pass event 10781).

```js
async function pace(browser, errors) {
  const runs = Number(ARG || 13), out = [];
  for (let i = 0; i < runs; i++) {
    const seed = SEED0 + i, before = errors.length;
    const page = await open(browser, errors, seed);
```

Landed `5d2302185b`. Extract one pace run and then aggregate sequentially.

**bestCell** · iteration and aggregation · 7 / 7. `infinite-ruins 1afc3a8103:design/prototype/feel/tools/autoplayer.js:64` (event 10760; pass event 10781).

```js
  const P = P_(), S = P.S;
  const r0 = Math.max(0, Math.ceil(S.camY / P.BW)), r1 = Math.floor((S.camY + P.H) / P.BW) - 1;
  let best = null, bv = 0;
  for (let r = r0; r <= r1; r++) for (let c = 0; c < P.COLS; c++) {
    const v = cellValue(c, r);
```

Landed `5d2302185b`. Move row expansion and best-score selection into existing-domain helpers.

**installAuto** · nested callback · 7 / 7. `infinite-ruins ce71a33f84:design/prototype/feel/tools/measure.js:37` (event 10735; pass event 10781).

```js
function installAuto(opts) {
  const P = window.__proto, S = P.S;
  const SCALE = new Set(["stylus", "servo", "torque", "spindle", "metal", "smelter"]);
  window.__log = { nonInt: [], popups: new Set(), stopAt: opts.stopAt || 0, shopEmptyAt: null };
  function relGain(u) {
```

Landed `5d2302185b`. Separate the browser bridge from the autoplayer and its nested work.

### 9. scopeDepthWeightedLoc, TS (9 findings)

**createFakeModels** · setup and orchestration · 248 / 70. `pi-worker 358cbeb930:test/fakes/fake-models.ts:55` (event 20255; pass event 20295).

```ts
  let toolCallCount = 0;

  const model: Model<Api> = {
    id: FAKE_MODEL_ID,
    name: "Fake scripted model",
```

Landed `55a6e2414f`. Move persistent fake state and its methods into FakeScript.

**stream** · nested callback · 140 / 70. `pi-worker 358cbeb930:test/fakes/fake-models.ts:73` (event 20255; pass event 20295).

```ts
    requests.push(context);
    const step = queue.shift();
    const turn: FakeTurn =
      step === undefined
        ? { error: `fake model: no scripted turn left for request ${index}` }
```

Landed `55a6e2414f`. Move script-selection and emission branches out of the stream callback.

**<anonymous>** · multi-phase test · 171 / 70. `pi-worker 358cbeb930:test/harness-pienv.test.ts:66` (event 20164; pass event 20295).

```ts
describe("pi-durable Harness with pi-env over the local sandboxCommand seam", () => {
  let lock: ProcessLock | undefined;
  let sandbox: SandboxExecutionEnv | undefined;

  afterEach(async () => {
```

Landed `55a6e2414f`. Flatten the describe wrapper and move process details into firstProcess and secondProcess.

### 10. halsteadEffort, TS (8 findings)

**expectResumed** · multi-phase test · 6787 / 5000. `pi-worker 56bf292c9b:test/harness-pienv.test.ts:128` (event 20220; pass event 20295).

```ts
  expect(toolResults(view.messages)).toHaveLength(2);
  expect(view.messages.at(-1)?.role).toBe("assistant");

  // A retried request ID finds the stored submission instead of running again.
  expect((await conversation.submit(FIRST_INPUT, context)).id).toBe(submissionId);
```

Landed `55a6e2414f`. Separate the stored conversation checks from the new-run checks.

**#listAll** · iteration and aggregation · 7232 / 5000. `pi-worker 7135a0efe4:src/gpu-gate/probe.ts:149` (event 21055; pass event 21084).

```ts
    let token: string | undefined;
    do {
      const url = new URL(`${this.#trueforgeBaseUrl}${path}`);
      url.searchParams.set("limit", String(SESSIONS_PAGE_LIMIT));
      if (token) url.searchParams.set("page_token", token);
```

Landed `f7d5f0933a`. Extract page URL construction and token normalization from traversal. The landed patch uses async recursion.

**<anonymous>** · multi-phase test · 1.329e+04 / 10000. `pi-worker 49ad95675b:test/config-and-skills.test.ts:38` (event 20979; pass event 21187).

```ts

    entries = [...entries, { repo: "MattMakes/alpha", enabled: true, notes: "small TS lib" }];
    entries = [...entries, { repo: "MattMakes/beta.js", enabled: true }];
    let raw = serializeAllowlistYaml(entries);
    expect(raw.startsWith(header)).toBe(true);
```

Landed `890119e6a1`. Extract YAML round trips and fixtures, and use one case per mutation.

Sample shape counts across the 30 pair entries. A function can appear under multiple metrics.

- Nested Callback — 6
- Multi-Phase Test — 6
- Setup And Orchestration — 5
- Iteration And Aggregation — 5
- Resource Lifetime — 3
- Parser Artifact — 3
- Policy Dispatch — 2


## Ranked rules for agent instruction files

Evidence counts below refer to entries in the inspected sample unless a gate count is explicitly named. Several entries describe the same function under different metrics. Verified means a comparable change appears in a landed task with a zero-violation gate event. The short illustrative examples were not independently analyzed.

| Rank | Rule | Metric and evidence | Bad example → good example | Target and verification |
| ---: | --- | --- | --- | --- |
| 1 | Move a callback with its own condition or error handling into a named function. | Scope ratio or weighted LOC. 6 nested-callback entries across 2 projects. | `page.on("console", m => { if (isError(m)) log(m); })` → `page.on("console", logErrors)` | `.claude/rules/complexity-js-ts.md`, paths `**/*.{js,mjs,cjs,jsx,ts,mts,cts,tsx}`. Verified by `open` and `stream`. |
| 2 | Split a test into independently meaningful scenarios or name the phases of a single scenario. | ABC, Halstead effort or weighted LOC. 6 multi-phase test entries in 1 project. | `test("all mutations", async () => { add(); update(); remove(); })` → `test("adds a record", addCase); test("removes a record", removeCase)` | `.claude/rules/complexity-tests.md`, paths `**/*.{test,spec}.{js,ts,mjs}`. Verified by the allowlist tests and `expectResumed`. |
| 3 | Build configuration in its own named operation when it interrupts resource setup or orchestration. | Maintainability, variable span or weighted LOC. 5 setup entries in 1 project. | `open(storage, { registry: buildRegistry(), models: buildModels(), hooks: buildHooks() })` → `open(storage, harnessOptions(inputs))` | `.claude/rules/complexity-js-ts.md`, same JS/TS paths. Verified by `openHarness` and `createOmlxModels`. |
| 4 | Separate one item's work from traversal and aggregation when each has its own state. | Halstead effort or live variables. 5 iteration and aggregation entries across 2 projects. | `for (const row of rows) for (const cell of row) { if (cell.score > best.score) best = cell; }` → `rows.flatMap(rowCells).reduce(better, fallback)` | `.claude/rules/complexity-js-ts.md`, same JS/TS paths. Verified by `bestCell` and `pace`. |
| 5 | Move resource acquisition, coupling, or release behind a named operation when it has separate failure handling. | Scope ratio or maintainability. 3 resource-lifetime entries in 1 project. | `open(); try { lock(); } catch (error) { close(); throw error; }` → `const db = open(); lockExclusive(db); return heldLock(db)` | Project `CLAUDE.md` for `pi-worker`. Verified by `acquireProcessLock`, `close` and `onUpstreamConnected`. |
| 6 | Declare a value in the phase that consumes it, or pass just that phase's inputs to a named helper. | Variable span. 3 entries in 1 project. | `const provider = buildProvider(env); prepareRegistry(); register(provider)` → `prepareRegistry(); register(buildProvider(env))` | `.claude/rules/complexity-js-ts.md`, same JS/TS paths. Verified by the provider and fake stream changes. |
| 7 | Put finite policy dispatch in a lookup when each key selects an independent rule. | Scope ratio or Halstead effort. 2 policy-dispatch entries in 1 project. | `if (id === "a") return a(x); if (id === "b") return b(x)` → `return strategies[id]?.(x) ?? null` | Project `CLAUDE.md` for `infinite-ruins`. Verified by `specialGain`. |
| 8 | Confirm that the reported Python line defines a function before restructuring a function for a live-variable finding. | Peak live variables. 3 inspected parser artifacts across 2 projects, plus the independently inspected `stack` comment in `monuments`. | `# bell: shift; cut: scale` inside a dictionary → `# Bell sets the shift. Cut scales the filter.` above the dictionary. | `.claude/rules/complexity-python.md`, paths `**/*.py`. Verified by `track_cinder.py` and `make_timeline.py`; their rewrites change only comments. |
| 9 | Separate Rust test assertions into named helpers when each helper checks a distinct invariant. | Clippy cognitive complexity. 1 directly traced finding in 1 project and 5 visible Rust findings overall. | `fn env_case() { /* setup and several groups of checks */ }` → `fn env_case() { let ctx = fixture(); assert_paths(&ctx); assert_settings(&ctx); }` | `.claude/rules/complexity-rust.md`, paths `**/*.rs`. Verified by event 68278 (65 / 25), revision `40e144ac` and pass event 68293. |
| 10 | Check the nearest applicable analyzer config before using a numeric complexity budget. | Scope ratio alone hits 1.3 and 1.5 in 3 projects in the gate sample. | `Assume the ratio limit is always 1.5.` → `Use the nearest config's ratio threshold.` | Project `AGENTS.md`. Config discovery verified in Horch; prevention effect not verified. |

Use the actual threshold and operation semantics to evaluate each extraction. A short linear function may already have a distinct purpose. Do not turn a large loop into recursive calls solely to reduce a score, or replace bounded sequential I/O with unbounded concurrency. The documented `#listAll` change is historical evidence and does not establish either transformation as a general rule.

## Limits

The raw Rust rows describe a retired routing path. The 10 trace groups prioritize findings that reached a real gate and could support rules for current languages. The Python group has fewer than 3 genuine function bodies among the inspected examples. Three inspected labels are a call, a comment, and a comprehension. The source is available, and the gate passes are recorded. Structural counts describe the 30 intentionally selected pair entries. The corpus-wide counts describe parser labels, since a label alone cannot establish structural complexity. The 1,003 findings omitted by the event cap cannot be ranked from the retained event messages.
