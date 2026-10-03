# gsap.utils

Pure helpers; no registration. Most take the input value last. Omit it to get a reusable function, which is the efficient form in handlers that run often:

```javascript
gsap.utils.clamp(0, 100, 150);        // 100
const clamp = gsap.utils.clamp(0, 100);
clamp(-10);                           // 0
```

Exception: `random()` returns a function only when you pass `true` as the last argument.

| Util | Example | Result |
|---|---|---|
| `clamp(min, max, v?)` | `clamp(0, 100, 150)` | `100` |
| `mapRange(inMin, inMax, outMin, outMax, v?)` | `mapRange(0, 1, 0, 360, 0.5)` | `180` |
| `normalize(min, max, v?)` | `normalize(100, 300, 200)` | `0.5` |
| `interpolate(a, b, p?)` | `interpolate("#f00", "#00f", 0.5)`; also numbers and objects with matching keys | mid value |
| `snap(inc \| array \| {values, radius}, v?)` | `snap(10, 23)`; `snap([0, 100, 200], 140)` | `20`; `100` |
| `wrap(min, max, v?)` | `wrap(0, 360, 370)` | `10` (max is exclusive) |
| `wrap(array, i?)` | `wrap(["a", "b", "c"], 4)` | `"b"` |
| `wrapYoyo(min, max, v?)` | `wrapYoyo(0, 100, 150)` | `50` |
| `random(min, max, inc?, fn?)` | `random(0, 500, 5)`; `random(-1, 1, true)` | a value; a function |
| `random(array, fn?)` | `random(["red", "blue"])` | one item |
| `shuffle(array)` | `shuffle([1, 2, 3])` | same array, shuffled in place |
| `distribute(config)` | see below | a function `(i, el, all)` |
| `getUnit(v)` | `getUnit("50%")` | `"%"` |
| `unitize(fn, unit?)` | `unitize(gsap.utils.clamp(0, 100), "px")(150)` | `"100px"` |
| `splitColor(c, hsl?)` | `splitColor("#6fb936")` | `[111, 185, 54]` |
| `toArray(v, scope?)` | `toArray(".item", container)` | a real array |
| `selector(scope)` | `const q = selector(ref); q(".box")` | elements inside scope |
| `pipe(...fns)` | `pipe(normalize(0, 100), snap(0.1))(47)` | `0.5` |
| `checkPrefix(prop)` | `checkPrefix("filter")` | the supported name |

`mapRange`, `normalize` and `clamp` work on numbers only; strip and restore units with `getUnit` and `unitize`.

## String form in tween vars

```javascript
gsap.to(".dot", { x: "random(-100, 100, 5)", rotation: "random([-10, 0, 10])" });
```

GSAP evaluates the string once per target.

## distribute()

Spreads values across targets by index or grid position.

```javascript
gsap.to(".cell", {
  scale: gsap.utils.distribute({ base: 0.5, amount: 1.5, from: "center", grid: "auto", ease: "power1.inOut" }),
});
```

Config: `base` (start value), `amount` (total spread) or `each` (step per target), `from` (`"start"`, `"center"`, `"edges"`, `"end"`, `"random"`, an index, or `[x, y]` ratios), `grid` (`"auto"` or `[rows, cols]`), `axis` (`"x"` or `"y"`), `ease`.

## Common patterns

```javascript
// Pointer position to rotation, reused on every event
const toRot = gsap.utils.mapRange(0, innerWidth, -15, 15);
const rotTo = gsap.quickTo(".card", "rotationY", { duration: 0.4, ease: "power3" });
addEventListener("pointermove", (e) => rotTo(toRot(e.clientX)));

// Infinite carousel index
const wrapIndex = gsap.utils.wrap(0, slides.length);
goTo(wrapIndex(current + 1));

// Scroll progress to a stepped value
ScrollTrigger.create({ trigger: ".steps", onUpdate: (self) => setStep(gsap.utils.snap(0.25, self.progress)) });
```

In components, prefer `gsap.utils.selector(root)` or `toArray(sel, root)` over global selectors.
