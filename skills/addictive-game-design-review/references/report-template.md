# The report

Write 1 Markdown file. Keep the order of the sections. A reader who reads only sections 1 and 2 must know what to do
first and what to leave alone. Delete a row that does not apply; do not leave an empty placeholder.

````markdown
# Gameplay review: <game>

Reviewed: <the folders>, at <commit or date>. Engine: <engine and version>.
Times are: <measured | simulated | estimated>, from <the source>.

## 1. Verdict

<3 to 5 sentences. Where the loop is solved or empty. The feeling that is missing: progress, agency or discovery.
The 1 change to make if only 1 is possible.>

The promise of the game:
- <line 1, in the words of the pitch, or "assumption:" with your reading>
- <line 2>

## 2. Plan

| Order | Finding | Kind | Cost | Score | Owner |
| --- | --- | --- | --- | --- | --- |
| Do first | F-03 <title> | REMOVE | S | 9.0 | <seat> |
| Do first | ... | | | | |
| Next | ... | | | | |
| Later | ... | | | | |

Dependencies: <F-05 needs F-02>. 

### Keep

| What works | Evidence | Why it must not change |
| --- | --- | --- |
| <thing> | `<file>:<line>` | <the check it meets> |

## 3. The map

### Loop map

| Stage | What the game has | Evidence |
| --- | --- | --- |
| Act | | `<file>:<line>` |
| Gain | | |
| Choose | | |
| Transform | | |
| Discover | | |

### Upgrade classes

| Class | Count | Share | Examples |
| --- | --- | --- | --- |
| number | | | |
| behaviour | | | |
| interaction | | | |
| constraint | | | |
| reveal | | | |

### Production chain

`<the formula of useful output per second>` at `<file>:<line>`.
<1 product of multipliers | a chain of N stages>. The stage that limits: <by time>.

### Decision list

| Decision | When | Options | What the player gives up | 1 option always best? |
| --- | --- | --- | --- | --- |

## 4. The timeline

| Time | Event | Target range | Status | How known |
| --- | --- | --- | --- | --- |
| <0:03> | First action with a visible result | 0 to 5 s | in range | estimated |
| | First affordable improvement | 10 to 30 s | | |
| | First automation | 30 to 90 s | | |
| | First meaningful trade-off | 2 to 4 min | | |
| | First new mechanic | 5 to 10 min | | |
| | Visible transformation | 10 to 20 min | | |
| | New strategic responsibility | 20 to 30 min | | |
| | <each later reveal> | - | | |
| | End of content | - | | |

Dead time:

| Start | Length | Cause | Evidence |
| --- | --- | --- | --- |

## 5. The checks

| Check | Status | Evidence or what is missing | Findings |
| --- | --- | --- | --- |
| R-1 Solved loop | finding | <4 of 5 points have 1 option best by x2> | F-01 |
| R-2 Upgrade quality | | | |
| ... | | | |
| R-18 Instrumentation | | | |

## 6. Findings

### F-01 <title> - <KIND>

- **Evidence:** `<file>:<line>`: <the value or the formula>.
- **Fault:** <what the player does or cannot do>.
- **Change:** <1 concrete change>.
- **Pattern and grade:** <P-n or F-n>, grade <E | T | D | H>.
- **Test:** <what a play test or a simulation observes>.
- **Variants:** <for ADD and large REDO: this is variant B; variant A is ...>.
- **Cost:** <S | M | L>. **Impact:** <1-3>. **Confidence:** <1-3>. **Score:** <n>.
- **Risk:** <what it can break; what must stay the same>.
- **Owner:** <seat or skill>.

## 7. Not audited, assumptions, and what a play test must confirm

- Not audited: <check> - <what is missing>.
- Assumption: <text>.
- A play test must confirm: <text>.
````

## Rules of the report

1. Each claim about the game has a file and a line. A claim with none is an assumption and goes in section 7.
2. State each time and each band as "about X" with its range, and say how the time was known.
3. Write a `REBALANCE` as a direction and a size class ("the cost of the 3rd producer, down by about half"). Say that
   the new numbers need a proof before the build.
4. Do not write a finding to fill a check. A check with status `ok` and its evidence is a result.
5. Keep section 6 in the order of the score, the highest first. The identifiers F-01, F-02 stay stable when you
   reorder, so that the plan and the hand-offs can point at them.
6. The words of the report are for a designer. Say what the player experiences. Give the name of a pattern after the
   plain sentence, not in place of it.
