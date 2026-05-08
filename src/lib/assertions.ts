import type { ResponseData } from "./types";

export interface Assertion {
  raw: string;
  target: AssertionTarget;
  op: AssertionOp;
  expected: string | null;
}

export type AssertionTarget =
  | { kind: "status" }
  | { kind: "elapsed" }
  | { kind: "body"; path: string[] }
  | { kind: "header"; name: string };

export type AssertionOp =
  | "=="
  | "!="
  | "<"
  | ">"
  | "<="
  | ">="
  | "exists"
  | "!exists"
  | "contains"
  | "!contains"
  | "matches";

export interface AssertionResult {
  raw: string;
  pass: boolean;
  reason?: string;
  actual?: string;
}

const OPS: AssertionOp[] = [
  "<=",
  ">=",
  "==",
  "!=",
  "<",
  ">",
  "exists",
  "!exists",
  "contains",
  "!contains",
  "matches",
];

export function parseAssertions(text: string): Assertion[] {
  const lines = text.split(/\r?\n/);
  const out: Assertion[] = [];
  let inTests = false;
  for (const rawLine of lines) {
    const line = rawLine.trim();
    if (!line) continue;
    if (/^###\s*tests\b/i.test(line)) {
      inTests = true;
      continue;
    }
    if (line.startsWith("###")) {
      inTests = false;
      continue;
    }
    if (!inTests) continue;
    if (line.startsWith("#")) continue;
    const parsed = parseLine(line);
    if (parsed) out.push(parsed);
  }
  return out;
}

function parseLine(line: string): Assertion | null {
  let opFound: AssertionOp | null = null;
  let opIndex = -1;

  for (const op of OPS) {
    const idx = findOp(line, op);
    if (idx !== -1 && (opIndex === -1 || idx < opIndex)) {
      opIndex = idx;
      opFound = op;
    }
  }

  if (!opFound) return null;

  const targetStr = line.slice(0, opIndex).trim();
  const expectedStr = line.slice(opIndex + opFound.length).trim();
  const target = parseTarget(targetStr);
  if (!target) return null;

  let expected: string | null = null;
  if (opFound !== "exists" && opFound !== "!exists") {
    expected = stripQuotes(expectedStr);
  }

  return { raw: line, target, op: opFound, expected };
}

function findOp(line: string, op: AssertionOp): number {
  const tokens: string[] = [op === "==" ? "==" : op];
  for (const t of tokens) {
    let pos = 0;
    while (pos < line.length) {
      const idx = line.indexOf(t, pos);
      if (idx === -1) break;
      const before = idx > 0 ? line[idx - 1] : " ";
      const after = idx + t.length < line.length ? line[idx + t.length] : " ";
      if (op === "exists" || op === "!exists" || op === "contains" || op === "!contains" || op === "matches") {
        if (/\s/.test(before) && /\s/.test(after)) return idx;
      } else {
        return idx;
      }
      pos = idx + t.length;
    }
  }
  return -1;
}

function parseTarget(s: string): AssertionTarget | null {
  const trimmed = s.trim();
  if (!trimmed) return null;
  if (trimmed === "status") return { kind: "status" };
  if (trimmed === "elapsed") return { kind: "elapsed" };
  if (trimmed.startsWith("body")) {
    const path = trimmed.slice(4);
    return { kind: "body", path: parsePath(path) };
  }
  if (trimmed.startsWith("header ")) {
    return { kind: "header", name: trimmed.slice(7).trim() };
  }
  return null;
}

function parsePath(s: string): string[] {
  if (!s) return [];
  if (s.startsWith(".")) s = s.slice(1);
  const out: string[] = [];
  const parts = s.split(/\.|\[|\]/).filter((p) => p.length > 0);
  for (const p of parts) out.push(p);
  return out;
}

function stripQuotes(s: string): string {
  if (s.length >= 2) {
    const f = s[0];
    const l = s[s.length - 1];
    if ((f === '"' && l === '"') || (f === "'" && l === "'")) {
      return s.slice(1, -1);
    }
  }
  return s;
}

export function evaluateAssertions(
  assertions: Assertion[],
  response: ResponseData,
): AssertionResult[] {
  return assertions.map((a) => evaluate(a, response));
}

function evaluate(assertion: Assertion, response: ResponseData): AssertionResult {
  const { actual, exists } = resolveTarget(assertion.target, response);

  if (assertion.op === "exists") {
    return exists
      ? { raw: assertion.raw, pass: true, actual }
      : { raw: assertion.raw, pass: false, reason: "not present", actual };
  }
  if (assertion.op === "!exists") {
    return !exists
      ? { raw: assertion.raw, pass: true }
      : { raw: assertion.raw, pass: false, reason: "is present", actual };
  }

  if (!exists) {
    return { raw: assertion.raw, pass: false, reason: "target missing" };
  }

  const expected = assertion.expected ?? "";
  const a = actual ?? "";

  switch (assertion.op) {
    case "==":
      return compareEq(assertion.raw, a, expected, true);
    case "!=":
      return compareEq(assertion.raw, a, expected, false);
    case "<":
    case ">":
    case "<=":
    case ">=":
      return compareNum(assertion.raw, a, expected, assertion.op);
    case "contains":
      return a.includes(expected)
        ? { raw: assertion.raw, pass: true, actual }
        : { raw: assertion.raw, pass: false, reason: `${a} does not contain ${expected}`, actual };
    case "!contains":
      return !a.includes(expected)
        ? { raw: assertion.raw, pass: true, actual }
        : { raw: assertion.raw, pass: false, reason: `${a} contains ${expected}`, actual };
    case "matches": {
      try {
        const re = new RegExp(expected);
        return re.test(a)
          ? { raw: assertion.raw, pass: true, actual }
          : { raw: assertion.raw, pass: false, reason: `does not match /${expected}/`, actual };
      } catch (e) {
        return { raw: assertion.raw, pass: false, reason: `bad regex: ${e instanceof Error ? e.message : String(e)}` };
      }
    }
    default:
      return { raw: assertion.raw, pass: false, reason: "unsupported op" };
  }
}

function compareEq(raw: string, a: string, expected: string, eq: boolean): AssertionResult {
  const equal = a === expected || (Number.isFinite(+a) && Number.isFinite(+expected) && +a === +expected);
  if (eq) {
    return equal
      ? { raw, pass: true, actual: a }
      : { raw, pass: false, reason: `${a} ≠ ${expected}`, actual: a };
  }
  return !equal
    ? { raw, pass: true, actual: a }
    : { raw, pass: false, reason: `${a} == ${expected}`, actual: a };
}

function compareNum(raw: string, a: string, expected: string, op: "<" | ">" | "<=" | ">="): AssertionResult {
  const av = Number(a);
  const ev = Number(expected);
  if (!Number.isFinite(av) || !Number.isFinite(ev)) {
    return { raw, pass: false, reason: `not numeric: ${a} ${op} ${expected}`, actual: a };
  }
  let pass = false;
  switch (op) {
    case "<": pass = av < ev; break;
    case ">": pass = av > ev; break;
    case "<=": pass = av <= ev; break;
    case ">=": pass = av >= ev; break;
  }
  return pass
    ? { raw, pass: true, actual: a }
    : { raw, pass: false, reason: `${a} ${op === "<" ? "≮" : op === ">" ? "≯" : op === "<=" ? "≰" : "≱"} ${expected}`, actual: a };
}

function resolveTarget(
  target: AssertionTarget,
  response: ResponseData,
): { actual?: string; exists: boolean } {
  switch (target.kind) {
    case "status":
      return { actual: String(response.status), exists: true };
    case "elapsed":
      return { actual: String(response.elapsed_ms), exists: true };
    case "header": {
      const found = response.headers.find(
        (h) => h[0].toLowerCase() === target.name.toLowerCase(),
      );
      return found ? { actual: found[1], exists: true } : { exists: false };
    }
    case "body": {
      const ct = response.headers.find((h) => h[0].toLowerCase() === "content-type")?.[1] ?? "";
      if (target.path.length === 0) {
        return { actual: response.body, exists: true };
      }
      if (!ct.includes("json")) {
        return { actual: response.body, exists: false };
      }
      let cur: unknown;
      try {
        cur = JSON.parse(response.body);
      } catch {
        return { exists: false };
      }
      for (const part of target.path) {
        if (cur === null || cur === undefined) return { exists: false };
        if (Array.isArray(cur)) {
          const idx = Number(part);
          if (!Number.isInteger(idx)) return { exists: false };
          cur = cur[idx];
        } else if (typeof cur === "object") {
          cur = (cur as Record<string, unknown>)[part];
        } else {
          return { exists: false };
        }
      }
      if (cur === undefined) return { exists: false };
      return {
        actual: typeof cur === "object" ? JSON.stringify(cur) : String(cur),
        exists: true,
      };
    }
  }
}
