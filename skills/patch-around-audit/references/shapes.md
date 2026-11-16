# Compensating shapes

Code shapes that often mean "something upstream of here is wrong and this
line makes up for it". Each has a look-alike that is honest code. The
shape makes a candidate; the 2 questions in the skill make the verdict.

The examples are short pseudo-code. The shapes are the same in every
language.

## Contents

1. Computed, then overridden
2. Guards stacked before the work
3. A flag that only skips a step
4. The same clean-up at several call sites
5. A twin: `safe_`, `_v2`, `_compat`, `_fixed`
6. Caught and dropped
7. Special case by identity
8. Do, then undo
9. A test that pins the workaround
10. Silenced signals
11. Comments that tell a history

## 1. Computed, then overridden

```
total = price_total(items)
if customer.region == "EU":      # price_total gets VAT wrong for the EU
    total = recompute_with_vat(items)
```

The first function has a defect for some inputs, and the caller replaces
its answer. **Root cause is in** the first function. **Honest
look-alike:** a default followed by a documented business override
("managers may set the price by hand").

## 2. Guards stacked before the work

```
if x is None: return []
if x == "": return []
if len(x) > 4096: x = x[:4096]
if x.startswith(" "): x = x.strip()
# must be called with a normalised key
return lookup(x)
```

Each guard arrived with 1 bug report. Together they say the function's
input contract was never decided. **Root cause is in** the contract: who
normalises, and where. **Honest look-alike:** validation at a trust
boundary (user input, network, file), written once and on purpose.

## 3. A flag that only skips a step

```
def save(doc, skip_validation=False, legacy_mode=False): ...
```

A parameter that exists so that 1 caller can avoid behaviour that is
wrong for it. Look at who passes it: 1 or 2 call sites, added in a fix
commit. **Root cause is in** the step being skipped: it is wrong for a
whole class of inputs. **Honest look-alike:** a real option that many
callers choose between.

## 4. The same clean-up at several call sites

```
name = get_name(u).strip().lower()      # in 6 files
```

Callers repair a value that its producer should have produced correctly.
New callers forget, and the bug returns. **Root cause is in** the
producer. **Honest look-alike:** callers that truly need different forms.

## 5. A twin: `safe_`, `_v2`, `_compat`, `_fixed`

A second function beside the first, because changing the first felt
unsafe. Check who still calls the original, and whether the comment on it
says why it had to stay. **Root cause:** the original was never fixed or
retired. **Honest look-alike:** a published interface with outside
callers and a stated removal date.

## 6. Caught and dropped

```
try: sync(account)
except Exception: pass     # sometimes fails on first run
```

Also: a retry loop or a sleep with a comment such as "give it time".
**Root cause is in** whatever fails or races. **Honest look-alike:** a
bounded retry around a network call, with the error still reported when
the retries run out.

## 7. Special case by identity

```
if tenant.id == 4412 or path.endswith("/old-reports"): ...
```

One customer, file, user or test hit a bug; the code now names them.
**Root cause is in** the general rule that the named case broke.
**Honest look-alike:** configuration that is meant to differ per tenant,
kept in configuration.

## 8. Do, then undo

```
rows = sort(rows)          # must stay sorted for the exporter
...
rows = sort_by_date(rows)  # exporter needs date order
```

A later step reverses or repeats an earlier one: sort then re-sort,
escape then unescape, cache then invalidate at once, convert there and
back. The earlier step is an anchor nobody removed. **Honest
look-alike:** 2 consumers that need 2 orders, each with its own copy.

## 9. A test that pins the workaround

A test named for the patch (`test_handles_eu_override`), a test that
asserts a value the specification calls wrong, or a skipped or
expected-failure test with a comment. The test is an anchor: it will
fail a correct fix and push the next person back to a patch. Check the
commit that added it; a test added with a patch pins that patch.

## 10. Silenced signals

A lint or type suppression, a widened type (`any`, `Object`), a default
that hides a missing value (`or ""`, `unwrap_or_default`), a log line
turned down from error to debug. Each hides a signal instead of fixing
its source. One is not a site; several around 1 function are.

## 11. Comments that tell a history

"Previously this ...", "changed to fix ...", "we used to ...", "after
the incident ...", "don't remove, see bug 812". A history comment marks
a place that was fixed before, and it turns 1 past decision into a
standing rule for every later reader. Find the commit, read what was
decided and why, and check whether the reason still holds.
