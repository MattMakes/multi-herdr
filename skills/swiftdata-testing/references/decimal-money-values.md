# Testing Decimal money values

Money-typed `@Model` properties should be `Decimal`, never `Double` — binary floating point cannot represent most decimal fractions exactly, and a `Double`-typed balance will eventually drift by a cent under repeated arithmetic. Test code has to respect that same precision, or it'll pass against `Double`-shaped bugs it should be catching.

## Construct Decimal values precisely

```swift
// A fractional literal is NOT exact: Decimal's float-literal initializer
// receives a Double, so `let price: Decimal = 19.99` goes through binary
// floating point first. Integer literals are exact.
let whole: Decimal = 50

// Build fractional values from integers or from a string with a fixed locale.
let price = Decimal(sign: .plus, exponent: -2, significand: 1999)   // 19.99
let fromString = Decimal(string: "19.99", locale: Locale(identifier: "en_US_POSIX"))!

// At a Double boundary (e.g. parsing legacy JSON), go through the string
// initializer, not Decimal(someDouble), which keeps the binary error.
```

```swift
@Test func splittingTransactionPreservesTotalExactly() throws {
    let total: Decimal = 100
    let splits = try splitEvenly(total, into: 3)

    // Decimal division is NOT exact: 100 / 3 has no finite decimal form, so
    // three raw quotients re-sum to 99.99...9. splitEvenly must round each
    // part to the minor unit and give the remainder to one part
    // (33.34 + 33.33 + 33.33). The exact re-sum proves that it does.
    #expect(splits.reduce(0, +) == total)
    #expect(splits.sorted(by: >) == [3334, 3333, 3333].map { Decimal(sign: .plus, exponent: -2, significand: $0) })
}
```

## Rules

- Compare `Decimal` values with `==`, never with an epsilon tolerance. Unlike `Double`, exact equality is the correct comparison — if a test needs a tolerance to pass, that's evidence a `Double` leaked into the calculation somewhere upstream, not a reason to loosen the assertion.
- When a test seeds a model with a money value, use an integer literal (`50`) typed as `Decimal`, or build a fractional value with `Decimal(sign:exponent:significand:)` or `Decimal(string:locale:)`. Don't write `Decimal(50.0)` or `let x: Decimal = 19.99` — both route through `Double` first and can carry over the wrong value.
- If a calculation must divide a `Decimal` (e.g. splitting a transaction N ways), test that partial results re-sum to the original total exactly. `Decimal` division is not exact, so the code must round to the minor unit and distribute the remainder; the type does not guarantee this.
- Test currency *formatting* (`Decimal.formatted(.currency(code:))`) separately from currency *arithmetic*. A locale/formatting bug and a precision bug produce similar-looking symptoms in a UI screenshot but are unrelated and shouldn't share a test.
- If the codebase has a lint or grep-based gate for `Double` in money-typed properties (`CLAUDE.md`-style layer rules are a common place for this), add a test fixture that would fail if a future refactor reintroduces `Double` — a model with an intentionally wrong `Double` property, guarded by `#if compiler` or a comment explaining it exists to prove the gate still fires.

## Common mistakes

| Mistake | Symptom | Fix |
|---|---|---|
| `Decimal(someDouble)` to convert a legacy value | Precision loss carried over from the `Double` | `Decimal(string: String(someDouble))` or fix the source type |
| Epsilon-based comparison (`abs(a - b) < 0.01`) | Masks real off-by-a-cent bugs | Exact `==` — `Decimal` doesn't need tolerance |
| Splitting a `Decimal` total N ways without a re-sum assertion | Rounding-loss bugs ship silently | Assert `splits.reduce(0, +) == total` |
