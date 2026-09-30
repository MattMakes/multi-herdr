# Expected totals (the oracle)

Computed by hand from the fixture lines. Prices are the built-in table in
`crates/horch-core/src/usage.rs` (USD per million tokens; cache write 5m =
1.25 x input, 1h = 2 x input unless the table says otherwise):

| model | input | output | cache read | cache write 5m | cache write 1h |
|---|---|---|---|---|---|
| claude-opus-5-5 | 4 | 20 | 0.20 | 5 | 8 |
| claude-sonnet-5 | 2 | 10 | 0.20 | 2.5 | 4 |
| claude-haiku-4-5 | 1 | 5 | 0.10 | 1.25 | 2 |
| gpt-5.6-sol | 4 | 20 | 0.40 | 5 | 8 |
| gpt-5.6-terra | 2 | 12 | 0.20 | 2.5 | 4 |
| gpt-5.6-luna | 0.20 | 1.20 | 0.02 | 0.25 | 0.40 |
| ollama/*, opencode/*-free | 0 | 0 | 0 | 0 | 0 |

Token order in every row: input / cw5m / cw1h / cache read / output / reasoning.

## Per record, whole files (stage 2)

### rec-o1 orchestrator (claude, session 11111111-...)

Main file. `msg_a` twice (identical, counted once), `msg_b` once, the
`<synthetic>` line skipped, `msg_c` twice (no `cache_creation` object, so all
400 written tokens are 5m):

| id | in | cw5m | cw1h | c.read | out | reas |
|---|---|---|---|---|---|---|
| msg_a | 10 | 0 | 2000 | 50000 | 300 | 120 |
| msg_b | 5 | 1000 | 0 | 52000 | 200 | 0 |
| msg_c | 2 | 400 | 0 | 53000 | 100 | 0 |
| sum | 17 | 1400 | 2000 | 155000 | 600 | 120 |

Cost (opus): 17x4 + 1400x5 + 2000x8 + 155000x0.2 + 600x20 = 68 + 7000 + 16000 + 31000 + 12000 = 66068 -> **$0.066068**.

Subagents (haiku). `msg_s1` is seen with output 20, then again with 60: 20 is
emitted, then a correction of +40. `msg_s2` once. `msg_s3` in the nested dir:

| id | in | cw5m | cw1h | c.read | out |
|---|---|---|---|---|---|
| msg_s1 | 100 | 0 | 0 | 0 | 20 + 40 = 60 |
| msg_s2 | 50 | 500 | 0 | 1000 | 30 |
| msg_s3 | 10 | 0 | 0 | 0 | 5 |
| sum | 160 | 500 | 0 | 1000 | 95 |

Cost (haiku): 160x1 + 500x1.25 + 1000x0.1 + 95x5 = 160 + 625 + 100 + 475 = 1360 -> **$0.001360**.

rec-o1 total: 177 / 1900 / 2000 / 156000 / 695 / 120, **$0.067428**. Events: 3 main + 4 subagent (msg_s1, msg_s1 correction, msg_s2, msg_s3) = 7.

### rec-a2 sonnet-1 (claude, session 22222222-...)

| id | in | cw5m | cw1h | c.read | out | reas |
|---|---|---|---|---|---|---|
| msg_d | 60 | 0 | 2502 | 91363 | 1210 | 300 |
| msg_e | 40 | 0 | 0 | 93000 | 500 | 0 |
| sum | 100 | 0 | 2502 | 184363 | 1710 | 300 |

Cost: 100x2 + 2502x4 + 184363x0.2 + 1710x10 = 200 + 10008 + 36872.6 + 17100 = 64180.6 -> **$0.0641806**.

### rec-a3 codex-sol-1 (codex 0.157.1, records)

Input is `input_tokens - cached_input_tokens`.

| response | in | c.read | out | reas |
|---|---|---|---|---|
| resp_1 | 1000-800 = 200 | 800 | 200 | 50 |
| resp_2 | 500-400 = 100 | 400 | 100 | 20 |
| sum | 300 | 1200 | 300 | 70 |

Cost (sol): 300x4 + 1200x0.4 + 300x20 = 1200 + 480 + 6000 = 7680 -> **$0.007680**.
The last `token_count` total is 1200 tokens; the records sum to 1800 = the last
`thread_token_usage` (TEL-05). The old reader priced only resp_1's totals.

### rec-a4 codex-terra-1 (codex 0.149.0, fallback)

One running total (written twice): in 4000-3000 = 1000, c.read 3000, out 600, reas 200.
Cost (terra): 1000x2 + 3000x0.2 + 600x12 = 2000 + 600 + 7200 = 9800 -> **$0.009800**.

### rec-a5 codex-luna-1 (codex 0.150.0, fallback)

Two running totals, each written twice. Events are the differences:

| total_tokens | in | c.read | out | reas |
|---|---|---|---|---|
| 2300 | 2000-1500 = 500 | 1500 | 300 | 100 |
| 3450 | (3000-2200) - 500 = 300 | 2200-1500 = 700 | 150 | 50 |
| sum | 800 | 2200 | 450 | 150 |

Cost (luna): 800x0.2 + 2200x0.02 + 450x1.2 = 160 + 44 + 540 = 744 -> **$0.000744**.

### rec-a6 sonnet-2: unread, `no session id yet`.

### rec-b1 pi-1 (pi)

| id | model | in | cw5m | cw1h | c.read | out | reas | note |
|---|---|---|---|---|---|---|---|---|
| m2 | ollama/qwen3.8 | 900 | 0 | 0 | 0 | 150 | 0 | free |
| m3 | ollama/qwen3.8 (record model) | 30 | 0 | 0 | 0 | 10 | 0 | toolResult, tool_nested |
| m4 | anthropic/claude-sonnet-5 | 100 | 800-300 = 500 | 300 | 2000 | 50 | 10 | |
| sum | | 1030 | 500 | 300 | 2000 | 210 | 10 | |

Cost: m4 only: 100x2 + 500x2.5 + 300x4 + 2000x0.2 + 50x10 = 200 + 1250 + 1200 + 400 + 500 = 3550 -> **$0.003550**.

### rec-b2 prime-1 (prime)

| id | in | cw5m | c.read | out |
|---|---|---|---|---|
| p1 | 50 | 200 | 1000 | 40 |
| p2 | 20 | 0 | 1200 | 30 |
| sum | 70 | 200 | 2200 | 70 |

Cost (opus): 70x4 + 200x5 + 2200x0.2 + 70x20 = 280 + 1000 + 440 + 1400 = 3120 -> **$0.003120**.

### rec-b3 opencode-ultra-1 (opencode, needs `sqlite3`)

`msg_o1` and `msg_o4` are completed assistant messages. `msg_o2` has no
`time.completed`, `msg_u1` is a user message, `msg_x1` is another session.

| id | in | c.read | out | reas |
|---|---|---|---|---|
| msg_o1 | 1000 | 0 | 300 | 40 |
| msg_o4 | 500 | 200 | 100 | 0 |
| sum | 1500 | 200 | 400 | 40 |

Cost: free, **$0**. Without `sqlite3`: unread, `sqlite3 not found`.

### rec-b4 smoke-1: unread, `agent none is not read`.

## Totals (stage 2)

| | in | cw5m | cw1h | c.read | out | reas | cost |
|---|---|---|---|---|---|---|---|
| without opencode | 3477 | 2600 | 4802 | 350963 | 4035 | 850 | $0.1565026 |
| opencode | 1500 | 0 | 0 | 200 | 400 | 40 | $0 |

In: 177 + 100 + 300 + 1000 + 800 + 1030 + 70 = 3477. Cache read: 156000 + 184363 + 1200 + 3000 + 2200 + 2000 + 2200 = 350963.
Cost sum: 0.067428 + 0.0641806 + 0.00768 + 0.0098 + 0.000744 + 0.00355 + 0.00312 = **0.1565026**.

Rollups at `HORCH_NOW=2026-09-28T18:00:00Z`:

- `by_kind`: orchestrator $0.067428; worker $0.0890746 (+ opencode $0).
- `7d` (since 2026-09-21T18:00:00Z) excludes rec-a4 (09-12) and rec-a5 (09-11): 0.1565026 - 0.0098 - 0.000744 = $0.1459586.
- `5h` (since 13:00:00Z) holds the claude events and prime `p1`/`p2` (13:00, 13:01): $0.067428 + $0.0641806 + $0.00312 = $0.1347286.

## Stage 1 (the first floor(n/2) lines of every file)

| file | lines kept | events |
|---|---|---|
| rec-o1 main (8) | 4 | msg_a |
| agent-a1 (3) | 1 | msg_s1 at output 20 |
| agent-a2 (1) | 0 | - |
| rec-a2 (4) | 2 | msg_d |
| rec-a3 (7) | 3 | resp_1 |
| rec-a4 (4) | 2 | - |
| rec-a5 (6) | 3 | total 2300 |
| rec-b1 (5), rec-b2 (3) | 2, 1 | - |
| opencode | whole | msg_o1, msg_o4 |

Stage 1 costs: msg_a 10x4 + 2000x8 + 50000x0.2 + 300x20 = 32040 -> $0.032040;
msg_s1 100x1 + 20x5 = 200 -> $0.000200; msg_d 60x2 + 2502x4 + 91363x0.2 + 1210x10 = 40500.6 -> $0.0405006;
resp_1 200x4 + 800x0.4 + 200x20 = 5120 -> $0.005120; total 2300 500x0.2 + 1500x0.02 + 300x1.2 = 490 -> $0.000490.
Stage 1 total **$0.0783506**.

After stage 2 the totals equal the whole-file totals above, with no duplicate
event key: the growth of `msg_s1` arrives as a correction (`msg_s1+1`, +40 output).
