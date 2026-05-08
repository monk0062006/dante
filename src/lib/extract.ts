import type { ResponseData } from "./types";

export interface ExtractRule {
  raw: string;
  varName: string;
  source: ExtractSource;
}

export type ExtractSource =
  | { kind: "body"; path: string[] }
  | { kind: "header"; name: string }
  | { kind: "cookie"; name: string }
  | { kind: "status" };

export interface ExtractResult {
  rule: ExtractRule;
  value: string | null;
}

export function parseExtracts(text: string): ExtractRule[] {
  const lines = text.split(/\r?\n/);
  const out: ExtractRule[] = [];
  let inExtract = false;
  for (const rawLine of lines) {
    const line = rawLine.trim();
    if (!line) continue;
    if (/^###\s*extract\b/i.test(line)) {
      inExtract = true;
      continue;
    }
    if (line.startsWith("###")) {
      inExtract = false;
      continue;
    }
    if (!inExtract) continue;
    if (line.startsWith("#")) continue;

    const eq = line.indexOf("=");
    if (eq === -1) continue;
    const varName = line.slice(0, eq).trim();
    const sourceStr = line.slice(eq + 1).trim();
    if (!varName) continue;
    const source = parseSource(sourceStr);
    if (!source) continue;
    out.push({ raw: line, varName, source });
  }
  return out;
}

function parseSource(s: string): ExtractSource | null {
  const trimmed = s.trim();
  if (!trimmed) return null;
  if (trimmed === "status") return { kind: "status" };
  if (trimmed.startsWith("body")) {
    return { kind: "body", path: parsePath(trimmed.slice(4)) };
  }
  if (trimmed.startsWith("header ")) {
    return { kind: "header", name: trimmed.slice(7).trim() };
  }
  if (trimmed.startsWith("cookie ")) {
    return { kind: "cookie", name: trimmed.slice(7).trim() };
  }
  return null;
}

function parsePath(s: string): string[] {
  if (!s) return [];
  if (s.startsWith(".")) s = s.slice(1);
  return s.split(/\.|\[|\]/).filter((p) => p.length > 0);
}

export function applyExtracts(
  rules: ExtractRule[],
  response: ResponseData,
): ExtractResult[] {
  return rules.map((rule) => ({ rule, value: extractValue(rule.source, response) }));
}

function extractValue(source: ExtractSource, response: ResponseData): string | null {
  switch (source.kind) {
    case "status":
      return String(response.status);
    case "header": {
      const found = response.headers.find(
        (h) => h[0].toLowerCase() === source.name.toLowerCase(),
      );
      return found ? found[1] : null;
    }
    case "cookie": {
      const cookieHeader =
        response.headers.find((h) => h[0].toLowerCase() === "set-cookie")?.[1] ?? "";
      const m = new RegExp(`(?:^|;\\s*)${escapeRegex(source.name)}=([^;]+)`).exec(
        cookieHeader,
      );
      return m ? m[1] : null;
    }
    case "body": {
      const ct = response.headers.find((h) => h[0].toLowerCase() === "content-type")?.[1] ?? "";
      if (!ct.includes("json")) return null;
      let cur: unknown;
      try {
        cur = JSON.parse(response.body);
      } catch {
        return null;
      }
      for (const part of source.path) {
        if (cur === null || cur === undefined) return null;
        if (Array.isArray(cur)) {
          const idx = Number(part);
          if (!Number.isInteger(idx)) return null;
          cur = cur[idx];
        } else if (typeof cur === "object") {
          cur = (cur as Record<string, unknown>)[part];
        } else {
          return null;
        }
      }
      if (cur === undefined || cur === null) return null;
      return typeof cur === "object" ? JSON.stringify(cur) : String(cur);
    }
  }
}

function escapeRegex(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
