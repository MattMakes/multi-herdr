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

function failType(message) {
  throw new TypeError(message);
}

function freezeObject(value) {
  Object.freeze(value);
  Object.values(value).forEach(freeze);
  return value;
}

function freeze(value) {
  return value !== null && typeof value === "object" ? freezeObject(value) : value;
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

function requireBoundable(op) {
  return op === "always" || op === "eventually" ? op : failType(".within() applies to always(...) and eventually(...) only");
}

function requireAmount(amount) {
  return typeof amount === "number" && amount >= 0 ? amount : failType(".within() needs a nonnegative number");
}

function unitScale(unit) {
  const scale = UNITS[unit];
  return scale === undefined ? failType(`.within() unit must be "milliseconds" or "seconds", not ${JSON.stringify(unit)}`) : scale;
}

class Formula {
  constructor(op, args, bound) {
    this[FORMULA] = true;
    this.op = op;
    this.args = args;
    this.bound = bound ?? null;
    Object.freeze(this);
  }

  within(amount, unit) {
    requireBoundable(this.op);
    requireAmount(amount);
    const scale = unitScale(unit);
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

function andArgument(f) {
  return toFormula(f, "and()");
}

export function and(...fs) {
  return new Formula("and", fs.map(andArgument));
}

function orArgument(f) {
  return toFormula(f, "or()");
}

export function or(...fs) {
  return new Formula("or", fs.map(orArgument));
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

function firstWith(items, verdict, run, visit) {
  let found = null;
  function stops(item, i) {
    const outcome = run(item, i);
    const stop = outcome.verdict === verdict;
    found = stop ? outcome : found;
    stop ? null : visit(outcome);
    return stop;
  }
  items.some(stops);
  return found;
}

function residualsOf(outcome) {
  return outcome.verdict === null ? [outcome.residual] : [];
}

function keeper(kept) {
  function keep(outcome) {
    kept.push(...residualsOf(outcome));
  }
  return keep;
}

function outcomeSorter(residuals, failures) {
  function sortOutcome(outcome) {
    outcome.verdict === false ? failures.push(outcome.failure) : residuals.push(outcome.residual);
  }
  return sortOutcome;
}

function collector(outcomes) {
  function collect(outcome) {
    outcomes.push(outcome);
  }
  return collect;
}

function ignoreOutcome() {
  return undefined;
}

function identity(value) {
  return value;
}

function stepWith(ctx) {
  function stepOne(obligation) {
    return obligation.step(ctx);
  }
  return stepOne;
}

function endWith(ctx) {
  function endOne(obligation) {
    return obligation.end(ctx);
  }
  return endOne;
}

function inWindow(deadlineMs, ctx) {
  return deadlineMs === null || ctx.timestamp <= deadlineMs;
}

function nestedNow(result, ctx, after) {
  const nested = isFormula(result) || typeof result === "function";
  return nested ? evaluate(toFormula(result, "now()"), ctx, after) : failType(`a now() callback must return a boolean or a formula, got ${describe(result)}`);
}

function falseOrNested(result, ctx, after) {
  return result === false ? fails(after, ctx, "the predicate returned false") : nestedNow(result, ctx, after);
}

function evaluateNow(formula, ctx, trail) {
  const here = trail.push("now");
  const { result, reads } = callPredicate(formula.args[0], ctx);
  const after = here.withReads(reads);
  return result === true ? holds(after) : falseOrNested(result, ctx, after);
}

function evaluateNext(formula, ctx, trail) {
  return pending(new NextObligation(formula.args[0], trail.push("next")));
}

function evaluateAlways(formula, ctx, trail) {
  const here = trail.push(label(formula));
  return new AlwaysObligation(formula.args[0], here, deadline(formula, ctx), formula.bound).step(ctx);
}

function evaluateEventually(formula, ctx, trail) {
  const here = trail.push(label(formula));
  return new EventuallyObligation(formula.args[0], here, deadline(formula, ctx), formula.bound).step(ctx);
}

function andResult(kept, trail) {
  return kept.length === 0 ? holds(trail) : pending(new AndObligation(kept, trail));
}

function evaluateAnd(formula, ctx, trail) {
  const kept = [];
  function runArgument(argument, i) {
    return evaluate(argument, ctx, trail.push(`and[${i}]`));
  }
  return firstWith(formula.args, false, runArgument, keeper(kept)) ?? andResult(kept, trail);
}

function orResult(residuals, failures, trail, ctx) {
  return residuals.length === 0 ? combineFailures(failures, trail, ctx) : pending(new OrObligation(residuals, failures, trail));
}

function evaluateOr(formula, ctx, trail) {
  const residuals = [];
  const failures = [];
  function runArgument(argument, i) {
    return evaluate(argument, ctx, trail.push(`or[${i}]`));
  }
  const won = firstWith(formula.args, true, runArgument, outcomeSorter(residuals, failures));
  return won ?? orResult(residuals, failures, trail, ctx);
}

function evaluateNot(formula, ctx, trail) {
  const here = trail.push("not");
  return negate(evaluate(formula.args[0], ctx, here), here, ctx);
}

function pendingImplication(antecedent, consequent, here) {
  const first = new NotObligation(antecedent.residual, here.push("if"));
  const failures = consequent.verdict === null ? [] : [consequent.failure];
  return pending(new OrObligation([first, ...residualsOf(consequent)], failures, here));
}

function thenTrailOf(antecedent, here) {
  return antecedent.verdict === true ? here.withReads(antecedent.trail.reads.slice(here.reads.length)) : here;
}

function impliesConsequent(formula, ctx, here, antecedent) {
  const consequent = evaluate(formula.args[1], ctx, thenTrailOf(antecedent, here).push("then"));
  const decided = antecedent.verdict === true || consequent.verdict === true;
  return decided ? consequent : pendingImplication(antecedent, consequent, here);
}

function evaluateImplies(formula, ctx, trail) {
  const here = trail.push("implies");
  const antecedent = evaluate(formula.args[0], ctx, here.push("if"));
  return antecedent.verdict === false ? holds(trail) : impliesConsequent(formula, ctx, here, antecedent);
}

const EVALUATORS = new Map([
  ["now", evaluateNow],
  ["next", evaluateNext],
  ["always", evaluateAlways],
  ["eventually", evaluateEventually],
  ["and", evaluateAnd],
  ["or", evaluateOr],
  ["not", evaluateNot],
  ["implies", evaluateImplies],
]);

function evaluate(formula, ctx, trail) {
  const run = EVALUATORS.get(formula.op);
  return run ? run(formula, ctx, trail) : failType(`unknown formula operator ${formula.op}`);
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

function readsOf(failure) {
  return failure.reads;
}

function failurePath(last, trail) {
  return last ? last.path : trail.path.join(" > ");
}

function lastMessage(last) {
  return last ? last.message : "no alternative held";
}

function failureMessage(failures, last) {
  return failures.length > 1 ? "every alternative failed" : lastMessage(last);
}

function combineFailures(failures, trail, ctx) {
  const last = failures[failures.length - 1];
  const path = failurePath(last, trail);
  const message = failureMessage(failures, last);
  return { verdict: false, failure: { path, reads: failures.flatMap(readsOf), index: ctx.index, message } };
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
    const failed = firstWith(this.instances, false, stepWith(ctx), keeper(kept));
    return failed ?? this.advance(ctx, kept);
  }

  advance(ctx, kept) {
    return inWindow(this.deadline, ctx) ? this.startInstance(ctx, kept) : this.closeWindow(kept);
  }

  startInstance(ctx, kept) {
    const outcome = evaluate(this.formula, ctx, this.trail);
    kept.push(...residualsOf(outcome));
    return outcome.verdict === false ? outcome : this.keep(kept);
  }

  closeWindow(kept) {
    return kept.length === 0 ? holds(this.trail) : this.keep(kept);
  }

  keep(kept) {
    this.instances = kept;
    return pending(this);
  }

  end(ctx) {
    return firstWith(this.instances, false, endWith(ctx), ignoreOutcome) ?? holds(this.trail);
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
    const won = firstWith(this.instances, true, stepWith(ctx), this.absorber(kept));
    return won ?? this.advance(ctx, kept);
  }

  absorber(kept) {
    const owner = this;
    function absorb(outcome) {
      outcome.verdict === null ? kept.push(outcome.residual) : owner.noteFailure(outcome);
    }
    return absorb;
  }

  noteFailure(outcome) {
    this.lastReads = outcome.failure.reads;
  }

  advance(ctx, kept) {
    return inWindow(this.deadline, ctx) ? this.startInstance(ctx, kept) : this.closeWindow(ctx, kept);
  }

  startInstance(ctx, kept) {
    const outcome = evaluate(this.formula, ctx, this.trail);
    return outcome.verdict === true ? outcome : this.absorbAndKeep(outcome, kept);
  }

  absorbAndKeep(outcome, kept) {
    this.absorber(kept)(outcome);
    return this.keep(kept);
  }

  closeWindow(ctx, kept) {
    const message = `did not hold within ${this.bound.amount} ${this.bound.unit}`;
    return kept.length === 0 ? fails(this.trail, ctx, message, this.lastReads) : this.keep(kept);
  }

  keep(kept) {
    this.instances = kept;
    return pending(this);
  }

  failureNoter() {
    const owner = this;
    function noteIfFailed(outcome) {
      outcome.verdict === false ? owner.noteFailure(outcome) : null;
    }
    return noteIfFailed;
  }

  end(ctx) {
    const won = firstWith(this.instances, true, endWith(ctx), this.failureNoter());
    return won ?? fails(this.trail, ctx, "still pending when the trace ended", this.lastReads);
  }
}

class AndObligation {
  constructor(residuals, trail) {
    this.residuals = residuals;
    this.trail = trail;
  }

  step(ctx) {
    const kept = [];
    return firstWith(this.residuals, false, stepWith(ctx), keeper(kept)) ?? this.keep(kept);
  }

  keep(kept) {
    this.residuals = kept;
    return kept.length === 0 ? holds(this.trail) : pending(this);
  }

  end(ctx) {
    return firstWith(this.residuals, false, endWith(ctx), ignoreOutcome) ?? holds(this.trail);
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
    return firstWith(outcomes, true, identity, outcomeSorter(kept, this.failures)) ?? this.keep(kept, ctx);
  }

  keep(kept, ctx) {
    this.residuals = kept;
    return kept.length === 0 ? combineFailures(this.failures, this.trail, ctx) : pending(this);
  }

  step(ctx) {
    const outcomes = [];
    return firstWith(this.residuals, true, stepWith(ctx), collector(outcomes)) ?? this.settle(outcomes, ctx);
  }

  end(ctx) {
    const outcomes = [];
    return firstWith(this.residuals, true, endWith(ctx), collector(outcomes)) ?? this.settle(outcomes, ctx);
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
  return typeof fn === "function" ? fn : failType(`${where} needs a function`);
}

function requireWeight(n, where) {
  return Number.isInteger(n) && n >= 0 ? n : failType(`${where} needs a nonnegative integer weight, got ${JSON.stringify(n)}`);
}

function requireNumber(value, where) {
  return typeof value === "number" && Number.isFinite(value) ? value : failType(`${where} needs finite numbers`);
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

function isString(entry) {
  return typeof entry === "string";
}

export function chars(characters, options) {
  const isList = Array.isArray(characters) && characters.every(isString);
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

function oneOfChild(g) {
  return requireGenerator(g, "oneOf()");
}

function toChoice(g) {
  return { weight: g.weightValue, generator: g };
}

export function oneOf(...generators) {
  const children = generators.flat().map(oneOfChild);
  return new Generator({ kind: "choice", choices: children.map(toChoice) });
}

function requirePair(pair) {
  return Array.isArray(pair) && pair.length === 2 ? pair : failType("weighted() needs [weight, generator] pairs");
}

function weightedChoice(pair) {
  requirePair(pair);
  requireWeight(pair[0], "weighted()");
  return { weight: pair[0], generator: requireGenerator(pair[1], "weighted()") };
}

export function weighted(pairs) {
  if (!Array.isArray(pairs)) {
    throw new TypeError("weighted() needs an array of [weight, generator] pairs");
  }
  return new Generator({ kind: "choice", choices: pairs.map(weightedChoice) });
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

function drawConstant(impl) {
  return impl.value;
}

function drawFrom(impl) {
  return impl.values.length === 0 ? NONE : impl.values[below(impl.values.length)];
}

function drawInteger(impl) {
  return impl.min + below(impl.max - impl.min + 1);
}

function drawFloat(impl) {
  return impl.min + native.unit() * (impl.max - impl.min);
}

function drawRegex(impl) {
  return native.regex(impl.source);
}

function drawEmail() {
  return native.email();
}

function drawText(impl) {
  return native.text(impl.min, impl.max);
}

function drawChars(impl) {
  const length = impl.min + below(impl.max - impl.min + 1);
  let out = "";
  for (let i = 0; i < length; i += 1) {
    out += impl.characters[below(impl.characters.length)];
  }
  return out;
}

function drawAction(impl) {
  return drawTemplate(impl.template);
}

function drawChoiceValue(impl) {
  return drawChoice(impl.choices).value;
}

function drawOffered(offered) {
  return offered === null ? NONE : draw(offered);
}

function drawDynamic(impl) {
  return drawOffered(offer(impl.fn(), "actions()"));
}

function drawMap(impl) {
  const value = draw(impl.inner);
  return value === NONE ? NONE : impl.fn(value);
}

function filterAttempt(impl) {
  const value = draw(impl.inner);
  return value === NONE || impl.fn(value) ? { value } : null;
}

function drawFilter(impl) {
  let found = null;
  for (let attempt = 0; attempt < 100 && found === null; attempt += 1) {
    found = filterAttempt(impl);
  }
  return found === null ? NONE : found.value;
}

function drawFlatMap(impl) {
  const value = draw(impl.inner);
  return value === NONE ? NONE : drawOffered(offer(impl.fn(value), "flatMap()"));
}

function drawWhen(impl) {
  return conditionHolds(impl.condition) ? draw(impl.inner) : NONE;
}

const DRAWERS = new Map([
  ["constant", drawConstant],
  ["from", drawFrom],
  ["integer", drawInteger],
  ["float", drawFloat],
  ["regex", drawRegex],
  ["email", drawEmail],
  ["text", drawText],
  ["chars", drawChars],
  ["action", drawAction],
  ["choice", drawChoiceValue],
  ["dynamic", drawDynamic],
  ["map", drawMap],
  ["filter", drawFilter],
  ["flatMap", drawFlatMap],
  ["when", drawWhen],
]);

function draw(generator) {
  const impl = generator.impl;
  const run = DRAWERS.get(impl.kind);
  return run ? run(impl) : failType(`unknown generator ${impl.kind}`);
}

function hasWeight(choice) {
  return choice.weight > 0;
}

function addWeight(total, choice) {
  return total + choice.weight;
}

function pickIndex(live, firstTicket) {
  let ticket = firstTicket;
  let picked = 0;
  while (ticket >= live[picked].weight) {
    ticket -= live[picked].weight;
    picked += 1;
  }
  return picked;
}

function drawLive(live) {
  const picked = pickIndex(live, below(live.reduce(addWeight, 0)));
  const value = draw(live[picked].generator);
  return { value, choice: live[picked], picked };
}

function drawChoice(choices) {
  let live = choices.filter(hasWeight);
  let drawn = { value: NONE, choice: null };
  while (drawn.value === NONE && live.length > 0) {
    drawn = drawLive(live);
    live = [...live.slice(0, drawn.picked), ...live.slice(drawn.picked + 1)];
  }
  return drawn.value === NONE ? { value: NONE, choice: null } : drawn;
}

function isPlainTemplate(template) {
  return template !== null && typeof template === "object" && !isCell(template);
}

function drawList(template) {
  const out = [];
  function add(item) {
    const value = drawTemplate(item);
    out.push(value);
    return value !== NONE;
  }
  return Array.from(template).every(add) ? out : NONE;
}

function drawRecord(template) {
  const out = {};
  function add(key) {
    const value = drawTemplate(template[key]);
    out[key] = value;
    return value !== NONE;
  }
  return Object.keys(template).every(add) ? out : NONE;
}

function drawRecordOrValue(template) {
  return isPlainTemplate(template) ? drawRecord(template) : template;
}

function drawStructure(template) {
  return Array.isArray(template) ? drawList(template) : drawRecordOrValue(template);
}

function drawTemplate(template) {
  return isGenerator(template) ? draw(template) : drawStructure(template);
}

const ENUMERATION_LIMIT = 2000;

function enumerateConstant(impl) {
  return [impl.value];
}

function enumerateFrom(impl) {
  return impl.values.slice(0, ENUMERATION_LIMIT);
}

function integerRange(impl) {
  const out = [];
  for (let n = impl.min; n <= impl.max; n += 1) {
    out.push(n);
  }
  return out;
}

function enumerateInteger(impl, generator) {
  return impl.max - impl.min < 64 ? integerRange(impl) : [draw(generator)];
}

function enumerateDrawn(impl, generator) {
  return [draw(generator)];
}

function enumerateAction(impl) {
  return enumerateTemplate(impl.template);
}

function choiceOptions(choice) {
  return choice.weight > 0 ? enumerate(choice.generator) : [];
}

function enumerateChoice(impl) {
  const out = [];
  function add(choice) {
    out.push(...choiceOptions(choice));
    return out.length >= ENUMERATION_LIMIT;
  }
  impl.choices.some(add);
  return out.slice(0, ENUMERATION_LIMIT);
}

function enumerateOffered(offered) {
  return offered === null ? [] : enumerate(offered);
}

function enumerateDynamic(impl) {
  return enumerateOffered(offer(impl.fn(), "actions()"));
}

function enumerateMap(impl) {
  function apply(value) {
    return impl.fn(value);
  }
  return enumerate(impl.inner).map(apply);
}

function enumerateFilter(impl) {
  function apply(value) {
    return impl.fn(value);
  }
  return enumerate(impl.inner).filter(apply);
}

function enumerateFlatMap(impl) {
  const out = [];
  function add(value) {
    out.push(...enumerateOffered(offer(impl.fn(value), "flatMap()")));
    return out.length >= ENUMERATION_LIMIT;
  }
  enumerate(impl.inner).some(add);
  return out.slice(0, ENUMERATION_LIMIT);
}

function enumerateWhen(impl) {
  return conditionHolds(impl.condition) ? enumerate(impl.inner) : [];
}

const ENUMERATORS = new Map([
  ["constant", enumerateConstant],
  ["from", enumerateFrom],
  ["integer", enumerateInteger],
  ["float", enumerateDrawn],
  ["regex", enumerateDrawn],
  ["email", enumerateDrawn],
  ["text", enumerateDrawn],
  ["chars", enumerateDrawn],
  ["action", enumerateAction],
  ["choice", enumerateChoice],
  ["dynamic", enumerateDynamic],
  ["map", enumerateMap],
  ["filter", enumerateFilter],
  ["flatMap", enumerateFlatMap],
  ["when", enumerateWhen],
]);

function enumerate(generator) {
  const impl = generator.impl;
  const run = ENUMERATORS.get(impl.kind);
  return run ? run(impl, generator) : failType(`unknown generator ${impl.kind}`);
}

function appendOption(combo, option) {
  return [...combo, option];
}

function extendList(combos, item) {
  return product(combos, enumerateTemplate(item), appendOption);
}

function enumerateList(template) {
  return Array.from(template).reduce(extendList, [[]]);
}

function enumerateRecord(template) {
  function extend(combos, key) {
    function withKey(combo, option) {
      return { ...combo, [key]: option };
    }
    return product(combos, enumerateTemplate(template[key]), withKey);
  }
  return Object.keys(template).reduce(extend, [{}]);
}

function enumerateRecordOrValue(template) {
  return isPlainTemplate(template) ? enumerateRecord(template) : [template];
}

function enumerateStructure(template) {
  return Array.isArray(template) ? enumerateList(template) : enumerateRecordOrValue(template);
}

function enumerateTemplate(template) {
  return isGenerator(template) ? enumerate(template) : enumerateStructure(template);
}

function product(left, right, join) {
  const out = [];
  function addPairs(a) {
    function addPair(b) {
      out.push(join(a, b));
      return out.length >= ENUMERATION_LIMIT;
    }
    return right.some(addPair);
  }
  left.some(addPairs);
  return out;
}

function checkAction(value, category) {
  if (value === null || typeof value !== "object" || Array.isArray(value) || typeof value.kind !== "string") {
    throw new TypeError(`the generator ${category} produced ${describe(value)}, not an action object with a string kind`);
  }
  const text = JSON.stringify(value);
  return JSON.parse(text);
}

function applyAdapter(state, adapter) {
  const result = adapter(state);
  return result === undefined ? state : result;
}

function adaptState(state) {
  return registry.adapters.reduce(applyAdapter, state);
}

function isReadyEntry([id]) {
  const cell = registry.cells[id];
  return Boolean(cell && cell.ready);
}

function readyCellEntry([id, name]) {
  return [name, registry.cells[id].value];
}

function cellsFor(names) {
  return Object.fromEntries([...names].filter(isReadyEntry).map(readyCellEntry));
}

function readEntry(index, read) {
  const key = read.index === index ? read.name : `${read.name}@${read.index}`;
  return [key, read.value];
}

function readsToCells(reads, index) {
  return Object.fromEntries(reads.map(readEntry.bind(null, index)));
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

function isUnnamedCell(value, exported) {
  return isCell(value) && !exported.has(value.id) && !registry.cellNames.has(value.id);
}

function recordCellName(value, key, exported, isSpec) {
  registry.cellNames.set(value.id, key);
  isSpec ? exported.set(value.id, key) : null;
}

function nameNamespace(spec, exported, namespace) {
  function nameKey(key) {
    const value = namespace[key];
    isUnnamedCell(value, exported) ? recordCellName(value, key, exported, namespace === spec) : null;
  }
  Object.keys(namespace).forEach(nameKey);
}

function addProperty(name, formula) {
  registry.properties.push({ name, formula, residual: null, started: false, done: false });
}

function addCategory(name, generator) {
  registry.categories.push({ name, generator });
}

function generatorEntry(value) {
  return isGenerator(value) ? addCategory : ignoreOutcome;
}

function registerSpecKey(spec, key) {
  const value = spec[key];
  const add = isFormula(value) ? addProperty : generatorEntry(value);
  add(key, value);
}

function nameOf(entry) {
  return entry.name;
}

function extractorInfo(cell) {
  return { id: cell.id, name: cellName(cell), source: cell.fn.toString() };
}

function installSummary() {
  return JSON.stringify({
    properties: registry.properties.map(nameOf),
    categories: registry.categories.map(nameOf),
    extractors: registry.cells.map(extractorInfo),
    customActions: registry.customActions,
  });
}

function markCells(ready) {
  function mark(cell) {
    cell.ready = ready;
  }
  registry.cells.forEach(mark);
}

function extractCell(state, cell) {
  registry.activity = `the extractor ${cellName(cell)}`;
  const value = cell.fn(state);
  cell.value = snapshot(value);
}

function setCellValue(values, cell, i) {
  cell.value = freeze(values[i] ?? null);
  cell.ready = true;
}

function isOpenProperty(property) {
  return !property.done;
}

function isStartedProperty(property) {
  return !property.done && property.started;
}

function firstOutcome(property, ctx) {
  return property.started ? property.residual.step(ctx) : evaluate(property.formula, ctx, ROOT);
}

function stepProperty(ctx, violations, property) {
  registry.activity = `the property ${property.name}`;
  const outcome = firstOutcome(property, ctx);
  property.started = true;
  settle(property, outcome, violations);
}

function finishProperty(ctx, violations, property) {
  registry.activity = `the property ${property.name}`;
  settle(property, property.residual.end(ctx), violations);
}

function categoryChoice(category) {
  return { weight: category.generator.weightValue, generator: category.generator, name: category.name };
}

function pickedActionText(value, choice) {
  const picked = checkAction(value, choice.name);
  picked.category = choice.name;
  return JSON.stringify(picked);
}

function isOfferedCategory(category) {
  return category.generator.weightValue !== 0;
}

function categoryOffers(category) {
  function tagged(value) {
    const offeredAction = checkAction(value, category.name);
    offeredAction.category = category.name;
    return offeredAction;
  }
  return enumerate(category.generator).map(tagged);
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
    [spec, ...others].forEach(nameNamespace.bind(null, spec, exported));
    registry.exportedCells = exported;
    Object.keys(spec).forEach(registerSpecKey.bind(null, spec));
    return installSummary();
  },

  activity() {
    return registry.activity;
  },

  extractLocal(stateText) {
    const state = adaptState(JSON.parse(stateText));
    markCells(false);
    registry.extracting = true;
    registry.cells.forEach(extractCell.bind(null, state));
    registry.extracting = false;
    registry.activity = null;
    markCells(true);
  },

  setCellValues(valuesText) {
    registry.cells.forEach(setCellValue.bind(null, JSON.parse(valuesText)));
  },

  exportedCells() {
    return JSON.stringify(cellsFor(registry.exportedCells));
  },

  step(index, timestamp) {
    const violations = [];
    registry.properties.filter(isOpenProperty).forEach(stepProperty.bind(null, { index, timestamp }, violations));
    registry.activity = null;
    return JSON.stringify(violations);
  },

  finish(index, timestamp) {
    const violations = [];
    registry.properties.filter(isStartedProperty).forEach(finishProperty.bind(null, { index, timestamp }, violations));
    registry.activity = null;
    return JSON.stringify(violations);
  },

  sample(enabledText) {
    registry.activity = "the action generators";
    const enabled = new Set(JSON.parse(enabledText));
    function isEnabled(category) {
      return enabled.has(category.name);
    }
    const { value, choice } = drawChoice(registry.categories.filter(isEnabled).map(categoryChoice));
    const text = value === NONE ? null : pickedActionText(value, choice);
    registry.activity = null;
    return text;
  },

  offered() {
    registry.activity = "the action generators";
    const out = registry.categories.filter(isOfferedCategory).flatMap(categoryOffers);
    registry.activity = null;
    return JSON.stringify(out);
  },
};

function keepResidual(property, outcome) {
  property.residual = outcome.residual;
}

function failureReports(property, outcome) {
  return outcome.verdict === false ? [report(property.name, outcome.failure)] : [];
}

function closeProperty(property, outcome, violations) {
  property.done = true;
  property.residual = null;
  violations.push(...failureReports(property, outcome));
}

function settle(property, outcome, violations) {
  const close = outcome.verdict === null ? keepResidual : closeProperty;
  close(property, outcome, violations);
}

Object.defineProperty(globalThis, "__uiPropTesterEngine", { value: engine, enumerable: false });
