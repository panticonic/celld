// Expectation matching with normalisation of ports and volatile values.
//
// Plain values in an expectation are compared exactly (deep equality for
// objects and arrays). Matchers from `M` loosen that where a value is
// legitimately volatile (timestamps, ids, ports).

const MATCHER = Symbol("matcher");
const matcher = (name, test) => ({ [MATCHER]: name, test });
const isMatcher = (x) => x !== null && typeof x === "object" && MATCHER in x;

export const M = {
  /** Anything at all (but the key must exist in an object). */
  any: () => matcher("any", () => null),
  /** typeof check: "string" | "number" | "boolean" | "object" */
  type: (t) =>
    matcher(`type ${t}`, (v) =>
      (t === "array" ? Array.isArray(v) : typeof v === t) ? null : `expected ${t}, got ${JSON.stringify(v)}`,
    ),
  int: () => M.type("number"),
  /** String matching a regexp. */
  re: (re) =>
    matcher(`re ${re}`, (v) => (typeof v === "string" && re.test(v) ? null : `${JSON.stringify(v)} does not match ${re}`)),
  /** ISO-8601 UTC timestamp string. */
  isoDate: () => M.re(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?Z$/),
  /** Epoch milliseconds within `windowMs` of now (default one hour either way). */
  epochMs: (windowMs = 3600_000) =>
    matcher("epochMs", (v) =>
      typeof v === "number" && Math.abs(v - Date.now()) <= windowMs ? null : `${v} is not within ${windowMs}ms of now`,
    ),
  /** Number within [lo, hi]. */
  between: (lo, hi) =>
    matcher(`between ${lo}..${hi}`, (v) => (typeof v === "number" && v >= lo && v <= hi ? null : `${v} not in [${lo}, ${hi}]`)),
  /** Number within `tol` of `x`. */
  near: (x, tol) =>
    matcher(`near ${x}`, (v) => (typeof v === "number" && Math.abs(v - x) <= tol ? null : `${v} not within ${tol} of ${x}`)),
  /** Objects: only the listed keys are compared; others are ignored. */
  partial: (obj) => ({ [MATCHER]: "partial", obj, test: null }),
  /** Always matches and stores the value as `{$name}` for later steps. */
  capture: (name, inner) => ({ [MATCHER]: "capture", name, inner }),
  /** Value is absent (undefined) or null. */
  absent: () => matcher("absent", (v) => (v === undefined || v === null ? null : `expected absent, got ${JSON.stringify(v)}`)),
  /** Custom predicate. */
  pred: (fn, desc = "predicate") => matcher(desc, (v) => (fn(v) ? null : `${desc} failed for ${JSON.stringify(v)}`)),
  /** One of several alternatives. */
  oneOf: (...alts) =>
    matcher("oneOf", (v) => {
      for (const a of alts) if (match(v, a, "$", {}).length === 0) return null;
      return `${JSON.stringify(v)} is none of ${JSON.stringify(alts)}`;
    }),
};

/** Replace the volatile origin of the running node with stable placeholders. */
export function normString(s, ctx) {
  if (typeof s !== "string" || !ctx?.port) return s;
  const p = String(ctx.port);
  return s
    .replaceAll(`http://127.0.0.1:${p}`, "{origin}")
    .replaceAll(`ws://127.0.0.1:${p}`, "{wsorigin}")
    .replaceAll(`http://localhost:${p}`, "{origin}")
    .replaceAll(`127.0.0.1:${p}`, "{host}")
    .replaceAll(`localhost:${p}`, "{host}");
}

export function normValue(v, ctx) {
  if (typeof v === "string") return normString(v, ctx);
  if (Array.isArray(v)) return v.map((x) => normValue(x, ctx));
  if (v && typeof v === "object") return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, normValue(x, ctx)]));
  return v;
}

/**
 * Substitute `{$name}` captures. A string that is exactly one capture keeps
 * the captured value's type (number, object); otherwise it is interpolated.
 */
export function subst(v, vars) {
  if (typeof v === "string") {
    const whole = v.match(/^\{\$([A-Za-z0-9_]+)\}$/);
    if (whole) {
      if (!(whole[1] in vars)) throw new Error(`unknown capture {$${whole[1]}}`);
      return vars[whole[1]];
    }
    return v.replace(/\{\$([A-Za-z0-9_]+)\}/g, (_, n) => {
      if (!(n in vars)) throw new Error(`unknown capture {$${n}}`);
      return String(vars[n]);
    });
  }
  if (Array.isArray(v)) return v.map((x) => subst(x, vars));
  if (v && typeof v === "object" && !(v instanceof Buffer) && !(v instanceof RegExp) && !isMatcher(v))
    return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, subst(x, vars)]));
  return v;
}

/** Returns a list of mismatch descriptions (empty = match). */
export function match(actual, expected, at = "$", vars = {}) {
  const errs = [];
  const fail = (m) => errs.push(`${at}: ${m}`);
  if (isMatcher(expected)) {
    if (expected[MATCHER] === "capture") {
      const sub = expected.inner === undefined ? [] : match(actual, expected.inner, at, vars);
      if (sub.length) return sub;
      vars[expected.name] = actual;
      return [];
    }
    if (expected[MATCHER] === "partial") {
      if (actual === null || typeof actual !== "object") {
        fail(`expected an object, got ${JSON.stringify(actual)}`);
        return errs;
      }
      for (const [k, ev] of Object.entries(expected.obj)) errs.push(...match(actual[k], ev, `${at}.${k}`, vars));
      return errs;
    }
    const r = expected.test(actual);
    if (r) fail(r);
    return errs;
  }
  if (expected instanceof RegExp) {
    if (typeof actual !== "string" || !expected.test(actual)) fail(`${JSON.stringify(actual)} does not match ${expected}`);
    return errs;
  }
  if (Array.isArray(expected)) {
    if (!Array.isArray(actual)) {
      fail(`expected array, got ${JSON.stringify(actual)}`);
      return errs;
    }
    if (actual.length !== expected.length) fail(`array length ${actual.length}, expected ${expected.length} (${JSON.stringify(actual)})`);
    for (let i = 0; i < Math.min(actual.length, expected.length); i++)
      errs.push(...match(actual[i], expected[i], `${at}[${i}]`, vars));
    return errs;
  }
  if (expected !== null && typeof expected === "object") {
    if (actual === null || typeof actual !== "object" || Array.isArray(actual)) {
      fail(`expected object, got ${JSON.stringify(actual)}`);
      return errs;
    }
    const ak = Object.keys(actual).sort();
    const ek = Object.keys(expected).sort();
    for (const k of ek) {
      if (!(k in actual) && !(isMatcher(expected[k]) && expected[k][MATCHER] === "absent")) fail(`missing key ${k} in ${JSON.stringify(actual)}`);
    }
    for (const k of ak) if (!ek.includes(k)) fail(`unexpected key ${k}=${JSON.stringify(actual[k])}`);
    for (const k of ek) if (k in actual || (isMatcher(expected[k]) && expected[k][MATCHER] === "absent")) errs.push(...match(actual[k], expected[k], `${at}.${k}`, vars));
    return errs;
  }
  if (actual !== expected) fail(`got ${JSON.stringify(actual)}, expected ${JSON.stringify(expected)}`);
  return errs;
}

/**
 * Check an HTTP response `{status, headers, text, json, bytes}` against
 * `{status?, headers?, text?, json?, bytes?}`. Bodies are normalised first.
 * Returns mismatches.
 */
export function matchResponse(res, expect, ctx, vars) {
  const errs = [];
  if (typeof expect === "number") expect = { status: expect };
  if (expect.status !== undefined) {
    const sub = match(res.status, expect.status, "status", vars);
    errs.push(...sub);
  }
  for (const [h, ev] of Object.entries(expect.headers ?? {})) {
    const av = res.headers[h.toLowerCase()];
    errs.push(...match(av === undefined ? undefined : normString(av, ctx), ev === null ? M.absent() : ev, `header ${h}`, vars));
  }
  if (expect.text !== undefined) errs.push(...match(normString(res.text, ctx), expect.text, "text", vars));
  if (expect.json !== undefined) {
    if (res.json === undefined) errs.push(`json: body is not JSON: ${JSON.stringify(res.text.slice(0, 200))}`);
    else errs.push(...match(normValue(res.json, ctx), expect.json, "json", vars));
  }
  if (expect.bytes !== undefined) {
    if (!Buffer.from(res.bytes).equals(Buffer.from(expect.bytes))) errs.push(`bytes: ${res.bytes.length} bytes differ from expected ${expect.bytes.length}`);
  }
  return errs;
}
