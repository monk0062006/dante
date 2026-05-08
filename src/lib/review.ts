import type { RequestSpec } from "./types";

export type FindingKind =
  | "bearer-token"
  | "basic-auth"
  | "api-key-header"
  | "api-key-query"
  | "cookie";

export interface Finding {
  kind: FindingKind;
  description: string;
  suggestedName: string;
  rawValue: string;
  apply: (spec: RequestSpec) => RequestSpec;
}

const HEADER_API_KEY_NAMES = [
  "x-api-key",
  "x-apikey",
  "api-key",
  "apikey",
  "x-auth-token",
  "x-access-token",
  "authorization-token",
];

const QUERY_API_KEY_NAMES = [
  "api_key",
  "apikey",
  "access_token",
  "auth_token",
  "key",
  "token",
];

export function reviewRequest(spec: RequestSpec): Finding[] {
  const findings: Finding[] = [];

  for (let i = 0; i < spec.headers.length; i++) {
    const [k, v] = spec.headers[i];
    const key = k.toLowerCase().trim();
    const value = v.trim();
    if (!value || isPlaceholder(value)) continue;

    if (key === "authorization") {
      const bearer = /^bearer\s+(\S.+)$/i.exec(value);
      if (bearer && !isPlaceholder(bearer[1])) {
        findings.push(makeHeaderFinding(i, "bearer-token", "Bearer token", "apiToken", bearer[1], (val) => `Bearer ${val}`));
        continue;
      }
      const basic = /^basic\s+(\S.+)$/i.exec(value);
      if (basic && !isPlaceholder(basic[1])) {
        findings.push(makeHeaderFinding(i, "basic-auth", "Basic auth credentials", "basicAuth", basic[1], (val) => `Basic ${val}`));
        continue;
      }
    }

    if (key === "cookie" && !containsOnlyPlaceholders(value)) {
      findings.push(makeHeaderFinding(i, "cookie", "Cookie header", "cookie", value, (val) => val));
      continue;
    }

    if (HEADER_API_KEY_NAMES.includes(key) && !containsOnlyPlaceholders(value)) {
      findings.push(makeHeaderFinding(i, "api-key-header", `${k} header value`, suggestNameFromHeader(k), value, (val) => val));
      continue;
    }
  }

  const queryFindings = scanQuery(spec.url);
  for (const q of queryFindings) findings.push(q);

  return dedupeFindings(findings);
}

function makeHeaderFinding(
  index: number,
  kind: FindingKind,
  description: string,
  suggestedName: string,
  rawValue: string,
  build: (val: string) => string,
): Finding {
  return {
    kind,
    description,
    suggestedName,
    rawValue,
    apply(spec: RequestSpec): RequestSpec {
      const headers = spec.headers.map(([k, v], i) =>
        i === index ? [k, build(`{{${suggestedName}}}`)] : [k, v],
      ) as Array<[string, string]>;
      return { ...spec, headers };
    },
  };
}

function scanQuery(url: string): Finding[] {
  let parsed: URL | null = null;
  try {
    parsed = new URL(url);
  } catch {
    return [];
  }

  const findings: Finding[] = [];
  const params = parsed.searchParams;
  const seenKeys = new Set<string>();

  for (const [key, value] of params.entries()) {
    if (seenKeys.has(key)) continue;
    seenKeys.add(key);
    if (!value || isPlaceholder(value)) continue;
    if (!QUERY_API_KEY_NAMES.includes(key.toLowerCase())) continue;

    const suggestedName = suggestNameFromQuery(key);
    findings.push({
      kind: "api-key-query",
      description: `?${key}= in URL`,
      suggestedName,
      rawValue: value,
      apply(spec) {
        const u = new URL(spec.url);
        u.searchParams.set(key, `__DANTE_PLACEHOLDER__${suggestedName}__`);
        const rebuilt = u
          .toString()
          .replace(
            `__DANTE_PLACEHOLDER__${suggestedName}__`,
            `{{${suggestedName}}}`,
          );
        return { ...spec, url: rebuilt };
      },
    });
  }

  return findings;
}

function isPlaceholder(value: string): boolean {
  return /^\{\{[\w.-]+\}\}$/.test(value.trim());
}

function containsOnlyPlaceholders(value: string): boolean {
  const stripped = value.replace(/\{\{[\w.-]+\}\}/g, "").trim();
  return stripped === "";
}

function suggestNameFromHeader(header: string): string {
  const stripped = header
    .toLowerCase()
    .replace(/^x-/, "")
    .replace(/-/g, "_");
  return camelize(stripped);
}

function suggestNameFromQuery(name: string): string {
  return camelize(name.replace(/[-_]+/g, "_"));
}

function camelize(snake: string): string {
  const parts = snake.split("_").filter(Boolean);
  if (parts.length === 0) return "value";
  return (
    parts[0] +
    parts
      .slice(1)
      .map((p) => p[0].toUpperCase() + p.slice(1))
      .join("")
  );
}

function dedupeFindings(findings: Finding[]): Finding[] {
  const seen = new Set<string>();
  return findings.filter((f) => {
    const key = `${f.kind}:${f.suggestedName}:${f.rawValue}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}
