const native = globalThis.__uiPropTesterNative;

const FORMULA = Symbol("ui-prop-tester.formula");
const GENERATOR = Symbol("ui-prop-tester.generator");
const CELL = Symbol("ui-prop-tester.cell");
const NONE = Symbol("ui-prop-tester.none");

const registry = {
  cells: [],
  adapters: [],
  customActions: [],
  loading: true,
  extracting: false,
  activity: null,
  reads: null,
  stateIndex: -1,
  properties: [],
  categories: [],
  cellNames: new Map(),
};

function cellName(cell) {
  return registry.cellNames.get(cell.id) ?? `cell#${cell.id}`;
}

function freeze(value) {
  if (value !== null && typeof value === "object") {
    Object.freeze(value);
    for (let cursor = 0, entries = Object.keys(value); cursor < entries.length; cursor += 1) {
      const key = entries[cursor];
      freeze(value[key]);
    }
  }
  return value;
}

function snapshot(value) {
  if (value === undefined) {
    return null;
  }
  const text = JSON.stringify(value);
  if (text === undefined) {
    return null;
  }
  return freeze(JSON.parse(text));
}

class Cell {
  constructor(fn) {
    this[CELL] = true;
    this.id = registry.cells.length;
    this.fn = fn;
    this.value = undefined;
    this.ready = false;
  }

  get current() {
    if (registry.extracting) {
      throw new Error("an extractor cannot read another extractor's cell");
    }
    if (!this.ready) {
      throw new Error(`the cell ${cellName(this)} has no value yet; read cells inside formulas and generators`);
    }
    if (registry.reads !== null) {
      registry.reads.push({ name: cellName(this), index: registry.stateIndex, value: this.value });
    }
    return this.value;
  }
}

export function extract(fn) {
  if (typeof fn !== "function") {
    throw new TypeError("extract() needs a function that maps the state to a value");
  }
  if (!registry.loading) {
    throw new Error("extract() must be called while the specification module loads");
  }
  const cell = new Cell(fn);
  registry.cells.push(cell);
  return cell;
}

export function isCell(value) {
  return value !== null && typeof value === "object" && value[CELL] === true;
}

export function isFormula(value) {
  return value !== null && typeof value === "object" && value[FORMULA] === true;
}

export function isGenerator(value) {
  return value !== null && typeof value === "object" && value[GENERATOR] === true;
}

const UNITS = { milliseconds: 1, millisecond: 1, ms: 1, seconds: 1000, second: 1000, s: 1000 };

class Formula {
  constructor(op, args, bound) {
    this[FORMULA] = true;
    this.op = op;
    this.args = args;
    this.bound = bound ?? null;
    Object.freeze(this);
  }

  within(amount, unit) {
    if (this.op !== "always" && this.op !== "eventually") {
      throw new TypeError(".within() applies to always(...) and eventually(...) only");
    }
    if (typeof amount !== "number" || !(amount >= 0)) {
      throw new TypeError(".within() needs a nonnegative number");
    }
    const scale = UNITS[unit];
    if (scale === undefined) {
      throw new TypeError(`.within() unit must be "milliseconds" or "seconds", not ${JSON.stringify(unit)}`);
    }
    return new Formula(this.op, this.args, { ms: amount * scale, amount, unit });
  }
}

function toFormula(value, where) {
  if (isFormula(value)) {
    return value;
  }
  if (typeof value === "function") {
    return new Formula("now", [value]);
  }
  throw new TypeError(`${where} needs a formula or a function, got ${describe(value)}`);
}

function describe(value) {
  if (value === null) {
    return "null";
  }
  if (Array.isArray(value)) {
    return "an array";
  }
  return typeof value;
}

export function now(fn) {
  if (typeof fn !== "function") {
    throw new TypeError("now() needs a function");
  }
  return new Formula("now", [fn]);
}

export function always(f) {
  return new Formula("always", [toFormula(f, "always()")]);
}

export function eventually(f) {
  return new Formula("eventually", [toFormula(f, "eventually()")]);
}

export function next(f) {
  return new Formula("next", [toFormula(f, "next()")]);
}

export function and(...fs) {
  return new Formula("and", fs.map((f) => toFormula(f, "and()")));
}

export function or(...fs) {
  return new Formula("or", fs.map((f) => toFormula(f, "or()")));
}

export function not(f) {
  return new Formula("not", [toFormula(f, "not()")]);
}

export function implies(a, b) {
  return new Formula("implies", [toFormula(a, "implies()"), toFormula(b, "implies()")]);
}

class Trail {
  constructor(path, reads) {
    this.path = path;
    this.reads = reads;
  }

  push(label) {
    return new Trail([...this.path, label], this.reads);
  }

  withReads(reads) {
    if (reads.length === 0) {
      return this;
    }
    return new Trail(this.path, [...this.reads, ...reads]);
  }
}

const ROOT = new Trail([], []);

function holds(trail) {
  return { verdict: true, trail };
}

function fails(trail, ctx, message, extraReads) {
  const reads = extraReads ? [...trail.reads, ...extraReads] : trail.reads;
  return { verdict: false, failure: { path: trail.path.join(" > "), reads, index: ctx.index, message } };
}

function pending(residual) {
  return { verdict: null, residual };
}

function callPredicate(fn, ctx) {
  const outer = registry.reads;
  const reads = [];
  registry.reads = reads;
  registry.stateIndex = ctx.index;
  const result = fn();
  registry.reads = outer;
  return { result, reads };
}

function evaluate(formula, ctx, trail) {
  switch (formula.op) {
    case "now": {
      const here = trail.push("now");
      const { result, reads } = callPredicate(formula.args[0], ctx);
      const after = here.withReads(reads);
      if (result === true) {
        return holds(after);
      }
      if (result === false) {
        return fails(after, ctx, "the predicate returned false");
      }
      if (isFormula(result) || typeof result === "function") {
        return evaluate(toFormula(result, "now()"), ctx, after);
      }
      throw new TypeError(`a now() callback must return a boolean or a formula, got ${describe(result)}`);
    }
    case "next":
      return pending(new NextObligation(formula.args[0], trail.push("next")));
    case "always": {
      const obligation = new AlwaysObligation(formula.args[0], trail.push(label(formula)), deadline(formula, ctx), formula.bound);
      return obligation.step(ctx);
    }
    case "eventually": {
      const obligation = new EventuallyObligation(formula.args[0], trail.push(label(formula)), deadline(formula, ctx), formula.bound);
      return obligation.step(ctx);
    }
    case "and": {
      const residuals = [];
      for (let i = 0; i < formula.args.length; i += 1) {
        const outcome = evaluate(formula.args[i], ctx, trail.push(`and[${i}]`));
        if (outcome.verdict === false) {
          return outcome;
        }
        if (outcome.verdict === null) {
          residuals.push(outcome.residual);
        }
      }
      return residuals.length === 0 ? holds(trail) : pending(new AndObligation(residuals, trail));
    }
    case "or": {
      const residuals = [];
      const failures = [];
      for (let i = 0; i < formula.args.length; i += 1) {
        const outcome = evaluate(formula.args[i], ctx, trail.push(`or[${i}]`));
        if (outcome.verdict === true) {
          return outcome;
        }
        if (outcome.verdict === false) {
          failures.push(outcome.failure);
        } else {
          residuals.push(outcome.residual);
        }
      }
      if (residuals.length === 0) {
        return combineFailures(failures, trail, ctx);
      }
      return pending(new OrObligation(residuals, failures, trail));
    }
    case "not": {
      const here = trail.push("not");
      const outcome = evaluate(formula.args[0], ctx, here);
      return negate(outcome, here, ctx);
    }
    case "implies": {
      const here = trail.push("implies");
      const antecedent = evaluate(formula.args[0], ctx, here.push("if"));
      if (antecedent.verdict === false) {
        return holds(trail);
      }
      const thenTrail = antecedent.verdict === true ? here.withReads(antecedent.trail.reads.slice(here.reads.length)) : here;
      const consequent = evaluate(formula.args[1], ctx, thenTrail.push("then"));
      if (antecedent.verdict === true || consequent.verdict === true) {
        return consequent;
      }
      const residuals = [new NotObligation(antecedent.residual, here.push("if"))];
      const failures = [];
      if (consequent.verdict === null) {
        residuals.push(consequent.residual);
      } else {
        failures.push(consequent.failure);
      }
      return pending(new OrObligation(residuals, failures, here));
    }
    default:
      throw new TypeError(`unknown formula operator ${formula.op}`);
  }
}

function label(formula) {
  if (formula.bound === null) {
    return formula.op;
  }
  return `${formula.op} within ${formula.bound.amount} ${formula.bound.unit}`;
}

function deadline(formula, ctx) {
  return formula.bound === null ? null : ctx.timestamp + formula.bound.ms;
}

function negate(outcome, here, ctx) {
  if (outcome.verdict === true) {
    return fails(outcome.trail, ctx, "the negated formula held");
  }
  if (outcome.verdict === false) {
    return holds(here);
  }
  return pending(new NotObligation(outcome.residual, here));
}

function combineFailures(failures, trail, ctx) {
  const last = failures[failures.length - 1];
  const reads = [];
  for (let cursor = 0, entries = failures; cursor < entries.length; cursor += 1) {
    const failure = entries[cursor];
    reads.push(...failure.reads);
  }
  return {
    verdict: false,
    failure: {
      path: last ? last.path : trail.path.join(" > "),
      reads,
      index: ctx.index,
      message: failures.length > 1 ? "every alternative failed" : last ? last.message : "no alternative held",
    },
  };
}

class NextObligation {
  constructor(formula, trail) {
    this.formula = formula;
    this.trail = trail;
  }

  step(ctx) {
    return evaluate(this.formula, ctx, this.trail);
  }

  end(ctx) {
    return ctx.negated ? fails(this.trail, ctx, "the trace ended before the next state") : holds(this.trail);
  }
}

class AlwaysObligation {
  constructor(formula, trail, deadlineMs, bound) {
    this.formula = formula;
    this.trail = trail;
    this.deadline = deadlineMs;
    this.bound = bound;
    this.instances = [];
  }

  step(ctx) {
    const kept = [];
    for (let cursor = 0, entries = this.instances; cursor < entries.length; cursor += 1) {
      const instance = entries[cursor];
      const outcome = instance.step(ctx);
      if (outcome.verdict === false) {
        return outcome;
      }
      if (outcome.verdict === null) {
        kept.push(outcome.residual);
      }
    }
    if (this.deadline === null || ctx.timestamp <= this.deadline) {
      const outcome = evaluate(this.formula, ctx, this.trail);
      if (outcome.verdict === false) {
        return outcome;
      }
      if (outcome.verdict === null) {
        kept.push(outcome.residual);
      }
    } else if (kept.length === 0) {
      return holds(this.trail);
    }
    this.instances = kept;
    return pending(this);
  }

  end(ctx) {
    for (let cursor = 0, entries = this.instances; cursor < entries.length; cursor += 1) {
      const instance = entries[cursor];
      const outcome = instance.end(ctx);
      if (outcome.verdict === false) {
        return outcome;
      }
    }
    return holds(this.trail);
  }
}

class EventuallyObligation {
  constructor(formula, trail, deadlineMs, bound) {
    this.formula = formula;
    this.trail = trail;
    this.deadline = deadlineMs;
    this.bound = bound;
    this.instances = [];
    this.lastReads = [];
  }

  step(ctx) {
    const kept = [];
    for (let cursor = 0, entries = this.instances; cursor < entries.length; cursor += 1) {
      const instance = entries[cursor];
      const outcome = instance.step(ctx);
      if (outcome.verdict === true) {
        return outcome;
      }
      if (outcome.verdict === null) {
        kept.push(outcome.residual);
      } else {
        this.lastReads = outcome.failure.reads;
      }
    }
    if (this.deadline === null || ctx.timestamp <= this.deadline) {
      const outcome = evaluate(this.formula, ctx, this.trail);
      if (outcome.verdict === true) {
        return outcome;
      }
      if (outcome.verdict === null) {
        kept.push(outcome.residual);
      } else {
        this.lastReads = outcome.failure.reads;
      }
    } else if (kept.length === 0) {
      return fails(this.trail, ctx, `did not hold within ${this.bound.amount} ${this.bound.unit}`, this.lastReads);
    }
    this.instances = kept;
    return pending(this);
  }

  end(ctx) {
    for (let cursor = 0, entries = this.instances; cursor < entries.length; cursor += 1) {
      const instance = entries[cursor];
      const outcome = instance.end(ctx);
      if (outcome.verdict === true) {
        return outcome;
      }
      if (outcome.verdict === false) {
        this.lastReads = outcome.failure.reads;
      }
    }
    return fails(this.trail, ctx, "still pending when the trace ended", this.lastReads);
  }
}

class AndObligation {
  constructor(residuals, trail) {
    this.residuals = residuals;
    this.trail = trail;
  }

  step(ctx) {
    const kept = [];
    for (let cursor = 0, entries = this.residuals; cursor < entries.length; cursor += 1) {
      const residual = entries[cursor];
      const outcome = residual.step(ctx);
      if (outcome.verdict === false) {
        return outcome;
      }
      if (outcome.verdict === null) {
        kept.push(outcome.residual);
      }
    }
    this.residuals = kept;
    return kept.length === 0 ? holds(this.trail) : pending(this);
  }

  end(ctx) {
    for (let cursor = 0, entries = this.residuals; cursor < entries.length; cursor += 1) {
      const residual = entries[cursor];
      const outcome = residual.end(ctx);
      if (outcome.verdict === false) {
        return outcome;
      }
    }
    return holds(this.trail);
  }
}

class OrObligation {
  constructor(residuals, failures, trail) {
    this.residuals = residuals;
    this.failures = failures;
    this.trail = trail;
  }

  settle(outcomes, ctx) {
    const kept = [];
    for (let cursor = 0, entries = outcomes; cursor < entries.length; cursor += 1) {
      const outcome = entries[cursor];
      if (outcome.verdict === true) {
        return outcome;
      }
      if (outcome.verdict === false) {
        this.failures.push(outcome.failure);
      } else {
        kept.push(outcome.residual);
      }
    }
    this.residuals = kept;
    if (kept.length === 0) {
      return combineFailures(this.failures, this.trail, ctx);
    }
    return pending(this);
  }

  step(ctx) {
    const outcomes = [];
    for (let cursor = 0, entries = this.residuals; cursor < entries.length; cursor += 1) {
      const residual = entries[cursor];
      const outcome = residual.step(ctx);
      if (outcome.verdict === true) {
        return outcome;
      }
      outcomes.push(outcome);
    }
    return this.settle(outcomes, ctx);
  }

  end(ctx) {
    const outcomes = [];
    for (let cursor = 0, entries = this.residuals; cursor < entries.length; cursor += 1) {
      const residual = entries[cursor];
      const outcome = residual.end(ctx);
      if (outcome.verdict === true) {
        return outcome;
      }
      outcomes.push(outcome);
    }
    return this.settle(outcomes, ctx);
  }
}

class NotObligation {
  constructor(residual, trail) {
    this.residual = residual;
    this.trail = trail;
  }

  step(ctx) {
    return negate(this.residual.step(ctx), this.trail, ctx);
  }

  end(ctx) {
    return negate(this.residual.end({ ...ctx, negated: !ctx.negated }), this.trail, ctx);
  }
}

class Generator {
  constructor(impl, weight) {
    this[GENERATOR] = true;
    this.impl = impl;
    this.weightValue = weight ?? 1;
    Object.freeze(this);
  }

  map(fn) {
    requireFunction(fn, "map()");
    return new Generator({ kind: "map", inner: this, fn }, this.weightValue);
  }

  filter(fn) {
    requireFunction(fn, "filter()");
    return new Generator({ kind: "filter", inner: this, fn }, this.weightValue);
  }

  flatMap(fn) {
    requireFunction(fn, "flatMap()");
    return new Generator({ kind: "flatMap", inner: this, fn }, this.weightValue);
  }

  when(condition) {
    if (typeof condition !== "boolean" && typeof condition !== "function" && !isCell(condition)) {
      throw new TypeError("when() needs a boolean, a cell or a function");
    }
    return new Generator({ kind: "when", inner: this, condition }, this.weightValue);
  }

  weight(n) {
    requireWeight(n, "weight()");
    return new Generator(this.impl, n);
  }
}

function requireFunction(fn, where) {
  if (typeof fn !== "function") {
    throw new TypeError(`${where} needs a function`);
  }
}

function requireWeight(n, where) {
  if (!Number.isInteger(n) || n < 0) {
    throw new TypeError(`${where} needs a nonnegative integer weight, got ${JSON.stringify(n)}`);
  }
}

function requireNumber(value, where) {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw new TypeError(`${where} needs finite numbers`);
  }
}

function lengths(options, where) {
  const min = options?.min ?? 1;
  const max = options?.max ?? Math.max(min, 16);
  if (!Number.isInteger(min) || !Number.isInteger(max) || min < 0 || max < min) {
    throw new TypeError(`${where} needs integer lengths with 0 <= min <= max`);
  }
  return { min, max };
}

export function constant(value) {
  return new Generator({ kind: "constant", value });
}

export function from(values) {
  if (!Array.isArray(values)) {
    throw new TypeError("from() needs an array");
  }
  return new Generator({ kind: "from", values: [...values] });
}

export function integer(min, max) {
  requireNumber(min, "integer()");
  requireNumber(max, "integer()");
  if (!Number.isInteger(min) || !Number.isInteger(max) || max < min) {
    throw new TypeError("integer() needs integers with min <= max");
  }
  return new Generator({ kind: "integer", min, max });
}

export function float(min, max) {
  requireNumber(min, "float()");
  requireNumber(max, "float()");
  if (max < min) {
    throw new TypeError("float() needs min <= max");
  }
  return new Generator({ kind: "float", min, max });
}

export function regex(pattern) {
  let source = pattern;
  if (pattern instanceof RegExp) {
    source = pattern.source;
  }
  if (typeof source !== "string") {
    throw new TypeError("regex() needs a string or a RegExp");
  }
  native.checkRegex(source);
  return new Generator({ kind: "regex", source });
}

export function email() {
  return new Generator({ kind: "email" });
}

export function text(options) {
  const { min, max } = lengths(options, "text()");
  return new Generator({ kind: "text", min, max });
}

export function chars(characters, options) {
  const isList = Array.isArray(characters) && characters.every((entry) => typeof entry === "string");
  if ((typeof characters !== "string" && !isList) || characters.length === 0) {
    throw new TypeError("chars() needs a nonempty string or a nonempty array of strings");
  }
  const { min, max } = lengths(options, "chars()");
  return new Generator({ kind: "chars", characters: Array.from(characters), min, max });
}

export function action(template) {
  if (template === null || typeof template !== "object" || Array.isArray(template)) {
    throw new TypeError("action() needs an object with a string kind");
  }
  if (typeof template.kind !== "string") {
    throw new TypeError("action() needs a string kind");
  }
  return new Generator({ kind: "action", template });
}

export function oneOf(...generators) {
  const children = generators.flat().map((g) => requireGenerator(g, "oneOf()"));
  return new Generator({ kind: "choice", choices: children.map((g) => ({ weight: g.weightValue, generator: g })) });
}

export function weighted(pairs) {
  if (!Array.isArray(pairs)) {
    throw new TypeError("weighted() needs an array of [weight, generator] pairs");
  }
  const choices = pairs.map((pair) => {
    if (!Array.isArray(pair) || pair.length !== 2) {
      throw new TypeError("weighted() needs [weight, generator] pairs");
    }
    requireWeight(pair[0], "weighted()");
    return { weight: pair[0], generator: requireGenerator(pair[1], "weighted()") };
  });
  return new Generator({ kind: "choice", choices });
}

export function actions(fn) {
  requireFunction(fn, "actions()");
  return new Generator({ kind: "dynamic", fn });
}

function requireGenerator(value, where) {
  if (!isGenerator(value)) {
    throw new TypeError(`${where} needs generators, got ${describe(value)}`);
  }
  return value;
}

function offer(value, where) {
  if (value === null || value === undefined) {
    return null;
  }
  if (Array.isArray(value)) {
    return value.length === 0 ? null : oneOf(...value);
  }
  return requireGenerator(value, where);
}

function conditionHolds(condition) {
  if (typeof condition === "boolean") {
    return condition;
  }
  if (typeof condition === "function") {
    return Boolean(condition());
  }
  return Boolean(condition.current);
}

function below(n) {
  return native.below(n);
}

function draw(generator) {
  const impl = generator.impl;
  switch (impl.kind) {
    case "constant":
      return impl.value;
    case "from":
      return impl.values.length === 0 ? NONE : impl.values[below(impl.values.length)];
    case "integer":
      return impl.min + below(impl.max - impl.min + 1);
    case "float":
      return impl.min + native.unit() * (impl.max - impl.min);
    case "regex":
      return native.regex(impl.source);
    case "email":
      return native.email();
    case "text":
      return native.text(impl.min, impl.max);
    case "chars": {
      const length = impl.min + below(impl.max - impl.min + 1);
      let out = "";
      for (let i = 0; i < length; i += 1) {
        out += impl.characters[below(impl.characters.length)];
      }
      return out;
    }
    case "action":
      return drawTemplate(impl.template);
    case "choice":
      return drawChoice(impl.choices).value;
    case "dynamic": {
      const offered = offer(impl.fn(), "actions()");
      return offered === null ? NONE : draw(offered);
    }
    case "map": {
      const value = draw(impl.inner);
      return value === NONE ? NONE : impl.fn(value);
    }
    case "filter": {
      for (let attempt = 0; attempt < 100; attempt += 1) {
        const value = draw(impl.inner);
        if (value === NONE) {
          return NONE;
        }
        if (impl.fn(value)) {
          return value;
        }
      }
      return NONE;
    }
    case "flatMap": {
      const value = draw(impl.inner);
      if (value === NONE) {
        return NONE;
      }
      const offered = offer(impl.fn(value), "flatMap()");
      return offered === null ? NONE : draw(offered);
    }
    case "when":
      return conditionHolds(impl.condition) ? draw(impl.inner) : NONE;
    default:
      throw new TypeError(`unknown generator ${impl.kind}`);
  }
}

function drawChoice(choices) {
  let live = choices.filter((choice) => choice.weight > 0);
  while (live.length > 0) {
    let total = 0;
    for (let cursor = 0, entries = live; cursor < entries.length; cursor += 1) {
      const choice = entries[cursor];
      total += choice.weight;
    }
    let ticket = below(total);
    let picked = 0;
    while (ticket >= live[picked].weight) {
      ticket -= live[picked].weight;
      picked += 1;
    }
    const value = draw(live[picked].generator);
    if (value !== NONE) {
      return { value, choice: live[picked] };
    }
    live = live.filter((_, i) => i !== picked);
  }
  return { value: NONE, choice: null };
}

function drawTemplate(template) {
  if (isGenerator(template)) {
    return draw(template);
  }
  if (Array.isArray(template)) {
    const out = [];
    for (let cursor = 0, entries = template; cursor < entries.length; cursor += 1) {
      const item = entries[cursor];
      const value = drawTemplate(item);
      if (value === NONE) {
        return NONE;
      }
      out.push(value);
    }
    return out;
  }
  if (template !== null && typeof template === "object" && !isCell(template)) {
    const out = {};
    for (let cursor = 0, entries = Object.keys(template); cursor < entries.length; cursor += 1) {
      const key = entries[cursor];
      const value = drawTemplate(template[key]);
      if (value === NONE) {
        return NONE;
      }
      out[key] = value;
    }
    return out;
  }
  return template;
}

const ENUMERATION_LIMIT = 2000;

function enumerate(generator) {
  const impl = generator.impl;
  switch (impl.kind) {
    case "constant":
      return [impl.value];
    case "from":
      return impl.values.slice(0, ENUMERATION_LIMIT);
    case "integer":
      if (impl.max - impl.min < 64) {
        const out = [];
        for (let n = impl.min; n <= impl.max; n += 1) {
          out.push(n);
        }
        return out;
      }
      return [draw(generator)];
    case "float":
    case "regex":
    case "email":
    case "text":
    case "chars":
      return [draw(generator)];
    case "action":
      return enumerateTemplate(impl.template);
    case "choice": {
      const out = [];
      for (let cursor = 0, entries = impl.choices; cursor < entries.length; cursor += 1) {
        const choice = entries[cursor];
        if (choice.weight > 0) {
          out.push(...enumerate(choice.generator));
        }
        if (out.length >= ENUMERATION_LIMIT) {
          break;
        }
      }
      return out.slice(0, ENUMERATION_LIMIT);
    }
    case "dynamic": {
      const offered = offer(impl.fn(), "actions()");
      return offered === null ? [] : enumerate(offered);
    }
    case "map":
      return enumerate(impl.inner).map((value) => impl.fn(value));
    case "filter":
      return enumerate(impl.inner).filter((value) => impl.fn(value));
    case "flatMap": {
      const out = [];
      for (let cursor = 0, entries = enumerate(impl.inner); cursor < entries.length; cursor += 1) {
        const value = entries[cursor];
        const offered = offer(impl.fn(value), "flatMap()");
        if (offered !== null) {
          out.push(...enumerate(offered));
        }
        if (out.length >= ENUMERATION_LIMIT) {
          break;
        }
      }
      return out.slice(0, ENUMERATION_LIMIT);
    }
    case "when":
      return conditionHolds(impl.condition) ? enumerate(impl.inner) : [];
    default:
      throw new TypeError(`unknown generator ${impl.kind}`);
  }
}

function enumerateTemplate(template) {
  if (isGenerator(template)) {
    return enumerate(template);
  }
  if (Array.isArray(template)) {
    let combos = [[]];
    for (let cursor = 0, entries = template; cursor < entries.length; cursor += 1) {
      const item = entries[cursor];
      const options = enumerateTemplate(item);
      combos = product(combos, options, (combo, option) => [...combo, option]);
    }
    return combos;
  }
  if (template !== null && typeof template === "object" && !isCell(template)) {
    let combos = [{}];
    for (let cursor = 0, entries = Object.keys(template); cursor < entries.length; cursor += 1) {
      const key = entries[cursor];
      const options = enumerateTemplate(template[key]);
      combos = product(combos, options, (combo, option) => ({ ...combo, [key]: option }));
    }
    return combos;
  }
  return [template];
}

function product(left, right, join) {
  const out = [];
  for (let cursor = 0, entries = left; cursor < entries.length; cursor += 1) {
    const a = entries[cursor];
    for (let cursor = 0, entries = right; cursor < entries.length; cursor += 1) {
      const b = entries[cursor];
      out.push(join(a, b));
      if (out.length >= ENUMERATION_LIMIT) {
        return out;
      }
    }
  }
  return out;
}

function checkAction(value, category) {
  if (value === null || typeof value !== "object" || Array.isArray(value) || typeof value.kind !== "string") {
    throw new TypeError(`the generator ${category} produced ${describe(value)}, not an action object with a string kind`);
  }
  const text = JSON.stringify(value);
  return JSON.parse(text);
}

function adaptState(state) {
  let adapted = state;
  for (let cursor = 0, entries = registry.adapters; cursor < entries.length; cursor += 1) {
    const adapter = entries[cursor];
    const result = adapter(adapted);
    if (result !== undefined) {
      adapted = result;
    }
  }
  return adapted;
}

function cellsFor(names) {
  const out = {};
  for (const [id, name] of names) {
    const cell = registry.cells[id];
    if (cell && cell.ready) {
      out[name] = cell.value;
    }
  }
  return out;
}

function readsToCells(reads, index) {
  const out = {};
  for (let cursor = 0, entries = reads; cursor < entries.length; cursor += 1) {
    const read = entries[cursor];
    const key = read.index === index ? read.name : `${read.name}@${read.index}`;
    out[key] = read.value;
  }
  return out;
}

function report(name, failure) {
  return {
    property: name,
    state_index: failure.index,
    path: failure.path,
    message: failure.message,
    cells: readsToCells(failure.reads, failure.index),
  };
}

const engine = {
  registerStateAdapter(fn) {
    requireFunction(fn, "registerStateAdapter()");
    registry.adapters.push(fn);
  },

  registerCustomAction(name, fn) {
    if (typeof name !== "string" || name.length === 0) {
      throw new TypeError("a custom action needs a nonempty name");
    }
    requireFunction(fn, "registerCustomAction()");
    if (registry.customActions.some((entry) => entry.name === name)) {
      throw new Error(`the custom action ${name} is defined twice`);
    }
    registry.customActions.push({ name, source: fn.toString() });
  },

  install(spec, ...others) {
    registry.loading = false;
    const exported = new Map();
    for (let cursor = 0, entries = [spec, ...others]; cursor < entries.length; cursor += 1) {
      const namespace = entries[cursor];
      for (let cursor = 0, entries = Object.keys(namespace); cursor < entries.length; cursor += 1) {
        const key = entries[cursor];
        const value = namespace[key];
        if (isCell(value) && !exported.has(value.id) && !registry.cellNames.has(value.id)) {
          registry.cellNames.set(value.id, key);
          if (namespace === spec) {
            exported.set(value.id, key);
          }
        }
      }
    }
    registry.exportedCells = exported;
    for (let cursor = 0, entries = Object.keys(spec); cursor < entries.length; cursor += 1) {
      const key = entries[cursor];
      const value = spec[key];
      if (isFormula(value)) {
        registry.properties.push({ name: key, formula: value, residual: null, started: false, done: false });
      } else if (isGenerator(value)) {
        registry.categories.push({ name: key, generator: value });
      }
    }
    return JSON.stringify({
      properties: registry.properties.map((p) => p.name),
      categories: registry.categories.map((c) => c.name),
      extractors: registry.cells.map((cell) => ({ id: cell.id, name: cellName(cell), source: cell.fn.toString() })),
      customActions: registry.customActions,
    });
  },

  activity() {
    return registry.activity;
  },

  extractLocal(stateText) {
    const state = adaptState(JSON.parse(stateText));
    for (let cursor = 0, entries = registry.cells; cursor < entries.length; cursor += 1) {
      const cell = entries[cursor];
      cell.ready = false;
    }
    registry.extracting = true;
    for (let cursor = 0, entries = registry.cells; cursor < entries.length; cursor += 1) {
      const cell = entries[cursor];
      registry.activity = `the extractor ${cellName(cell)}`;
      const value = cell.fn(state);
      cell.value = snapshot(value);
    }
    registry.extracting = false;
    registry.activity = null;
    for (let cursor = 0, entries = registry.cells; cursor < entries.length; cursor += 1) {
      const cell = entries[cursor];
      cell.ready = true;
    }
  },

  setCellValues(valuesText) {
    const values = JSON.parse(valuesText);
    registry.cells.forEach((cell, i) => {
      cell.value = freeze(values[i] ?? null);
      cell.ready = true;
    });
  },

  exportedCells() {
    return JSON.stringify(cellsFor(registry.exportedCells));
  },

  step(index, timestamp) {
    const ctx = { index, timestamp };
    const violations = [];
    for (let cursor = 0, entries = registry.properties; cursor < entries.length; cursor += 1) {
      const property = entries[cursor];
      if (property.done) {
        continue;
      }
      registry.activity = `the property ${property.name}`;
      const outcome = property.started ? property.residual.step(ctx) : evaluate(property.formula, ctx, ROOT);
      property.started = true;
      settle(property, outcome, violations);
    }
    registry.activity = null;
    return JSON.stringify(violations);
  },

  finish(index, timestamp) {
    const ctx = { index, timestamp };
    const violations = [];
    for (let cursor = 0, entries = registry.properties; cursor < entries.length; cursor += 1) {
      const property = entries[cursor];
      if (property.done || !property.started) {
        continue;
      }
      registry.activity = `the property ${property.name}`;
      settle(property, property.residual.end(ctx), violations);
    }
    registry.activity = null;
    return JSON.stringify(violations);
  },

  sample(enabledText) {
    registry.activity = "the action generators";
    const enabled = new Set(JSON.parse(enabledText));
    const choices = registry.categories
      .filter((category) => enabled.has(category.name))
      .map((category) => ({ weight: category.generator.weightValue, generator: category.generator, name: category.name }));
    const { value, choice } = drawChoice(choices);
    if (value === NONE) {
      registry.activity = null;
      return null;
    }
    const picked = checkAction(value, choice.name);
    picked.category = choice.name;
    registry.activity = null;
    return JSON.stringify(picked);
  },

  offered() {
    registry.activity = "the action generators";
    const out = [];
    for (let cursor = 0, entries = registry.categories; cursor < entries.length; cursor += 1) {
      const category = entries[cursor];
      if (category.generator.weightValue === 0) {
        continue;
      }
      for (let cursor = 0, entries = enumerate(category.generator); cursor < entries.length; cursor += 1) {
        const value = entries[cursor];
        const offeredAction = checkAction(value, category.name);
        offeredAction.category = category.name;
        out.push(offeredAction);
      }
    }
    registry.activity = null;
    return JSON.stringify(out);
  },
};

function settle(property, outcome, violations) {
  if (outcome.verdict === null) {
    property.residual = outcome.residual;
    return;
  }
  property.done = true;
  property.residual = null;
  if (outcome.verdict === false) {
    violations.push(report(property.name, outcome.failure));
  }
}

Object.defineProperty(globalThis, "__uiPropTesterEngine", { value: engine, enumerable: false });
