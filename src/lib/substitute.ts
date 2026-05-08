import type { RequestSpec } from "./types";

const PLACEHOLDER = /\{\{\s*([\w.-]+)\s*\}\}/g;
const FUNC_CALL = /\{\{\s*\$([\w.]+)(?:\(([^)]*)\))?\s*\}\}/g;

export interface Substitution {
  spec: RequestSpec;
  resolved: string[];
  unresolved: string[];
}

export function substitute(
  spec: RequestSpec,
  vars: Map<string, string>,
): Substitution {
  const resolved: string[] = [];
  const unresolved: string[] = [];

  const apply = (s: string | null): string | null => {
    if (s === null) return null;
    let out = s.replace(FUNC_CALL, (match, fn, args) => {
      const result = callBuiltin(fn, args ?? "");
      if (result !== null) {
        if (!resolved.includes(`$${fn}`)) resolved.push(`$${fn}`);
        return result;
      }
      return match;
    });
    out = out.replace(PLACEHOLDER, (match, name) => {
      if (name.startsWith("$")) return match;
      const value = vars.get(name);
      if (value !== undefined) {
        if (!resolved.includes(name)) resolved.push(name);
        return value;
      }
      if (!unresolved.includes(name)) unresolved.push(name);
      return match;
    });
    return out;
  };

  return {
    spec: {
      method: spec.method,
      url: apply(spec.url) ?? spec.url,
      headers: spec.headers.map(([k, v]) => [
        apply(k) ?? k,
        apply(v) ?? v,
      ]) as Array<[string, string]>,
      body: apply(spec.body),
    },
    resolved,
    unresolved,
  };
}

function callBuiltin(name: string, argsRaw: string): string | null {
  const args = parseArgs(argsRaw);
  switch (name) {
    case "uuid":
    case "random.uuid":
      return uuidV4();
    case "randomEmail":
      return `${pick(FIRST_NAMES).toLowerCase()}.${pick(LAST_NAMES).toLowerCase()}${randInt(1, 999)}@${pick(EMAIL_DOMAINS)}`;
    case "randomFirstName":
      return pick(FIRST_NAMES);
    case "randomLastName":
      return pick(LAST_NAMES);
    case "randomFullName":
      return `${pick(FIRST_NAMES)} ${pick(LAST_NAMES)}`;
    case "randomPhoneNumber":
      return `+1-${randInt(200, 999)}-${randInt(200, 999)}-${randInt(1000, 9999)}`;
    case "randomIPv4":
      return `${randInt(1, 254)}.${randInt(0, 255)}.${randInt(0, 255)}.${randInt(1, 254)}`;
    case "randomURL":
      return `https://${pick(URL_HOSTS)}/${pick(URL_PATHS)}`;
    case "randomUserAgent":
      return pick(USER_AGENTS);
    case "randomCity":
      return pick(CITIES);
    case "randomCountry":
      return pick(COUNTRIES);
    case "randomColor":
      return `#${randInt(0, 0xffffff).toString(16).padStart(6, "0")}`;
    case "timestamp":
      return Math.floor(Date.now() / 1000).toString();
    case "isoTimestamp":
      return new Date().toISOString();
    case "now":
    case "now.iso":
      return new Date().toISOString();
    case "now.unix":
      return Math.floor(Date.now() / 1000).toString();
    case "now.unix.ms":
      return Date.now().toString();
    case "random.int": {
      const min = Number(args[0] ?? "0");
      const max = Number(args[1] ?? "100");
      if (!Number.isFinite(min) || !Number.isFinite(max)) return null;
      return Math.floor(min + Math.random() * (max - min + 1)).toString();
    }
    case "random.alphanum": {
      const n = Number(args[0] ?? "12");
      if (!Number.isInteger(n) || n < 1 || n > 256) return null;
      const chars = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
      let out = "";
      for (let i = 0; i < n; i++) {
        out += chars[Math.floor(Math.random() * chars.length)];
      }
      return out;
    }
    case "base64": {
      const text = args[0] ?? "";
      try {
        return btoa(unescape(encodeURIComponent(text)));
      } catch {
        return null;
      }
    }
    case "base64url": {
      const text = args[0] ?? "";
      try {
        const std = btoa(unescape(encodeURIComponent(text)));
        return std.replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
      } catch {
        return null;
      }
    }
    case "hmac.sha256": {
      // Note: synchronous, can't await crypto.subtle. Return null and let user know.
      return null;
    }
    default:
      return null;
  }
}

const FIRST_NAMES = [
  "Alice", "Bob", "Carol", "David", "Eve", "Frank", "Grace", "Heidi", "Ivan",
  "Judy", "Kim", "Liam", "Maya", "Noah", "Olivia", "Pat", "Quinn", "Riya",
  "Sam", "Tara", "Umar", "Vera", "Will", "Xena", "Yuki", "Zara",
];

const LAST_NAMES = [
  "Smith", "Johnson", "Williams", "Brown", "Jones", "Miller", "Davis",
  "Garcia", "Rodriguez", "Wilson", "Martinez", "Anderson", "Taylor", "Thomas",
  "Hernandez", "Moore", "Martin", "Jackson", "Thompson", "White",
];

const EMAIL_DOMAINS = ["example.com", "test.io", "dev.local", "mail.test", "demo.app"];

const URL_HOSTS = ["api.example.com", "service.dev", "data.io", "cloud.app"];
const URL_PATHS = ["users", "posts", "items", "products", "orders/123", "v1/data"];

const USER_AGENTS = [
  "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
  "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15",
  "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36",
  "curl/8.4.0",
  "Dante/0.1.0",
];

const CITIES = [
  "New York", "London", "Tokyo", "Paris", "Sydney", "Berlin", "Toronto",
  "São Paulo", "Mumbai", "Cape Town",
];

const COUNTRIES = [
  "USA", "UK", "Japan", "France", "Australia", "Germany", "Canada",
  "Brazil", "India", "South Africa",
];

function pick<T>(arr: T[]): T {
  return arr[Math.floor(Math.random() * arr.length)];
}

function randInt(min: number, max: number): number {
  return Math.floor(min + Math.random() * (max - min + 1));
}

function parseArgs(s: string): string[] {
  if (!s.trim()) return [];
  const out: string[] = [];
  let buf = "";
  let inStr: string | null = null;
  let escape = false;
  for (const ch of s) {
    if (escape) {
      buf += ch;
      escape = false;
    } else if (inStr) {
      if (ch === "\\") escape = true;
      else if (ch === inStr) inStr = null;
      else buf += ch;
    } else if (ch === '"' || ch === "'") {
      inStr = ch;
    } else if (ch === ",") {
      out.push(buf.trim());
      buf = "";
    } else {
      buf += ch;
    }
  }
  if (buf.trim()) out.push(buf.trim());
  return out;
}

function uuidV4(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  const bytes = new Uint8Array(16);
  if (typeof crypto !== "undefined" && typeof crypto.getRandomValues === "function") {
    crypto.getRandomValues(bytes);
  } else {
    for (let i = 0; i < 16; i++) bytes[i] = Math.floor(Math.random() * 256);
  }
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex: string[] = [];
  for (const b of bytes) hex.push(b.toString(16).padStart(2, "0"));
  return `${hex.slice(0, 4).join("")}-${hex.slice(4, 6).join("")}-${hex.slice(6, 8).join("")}-${hex.slice(8, 10).join("")}-${hex.slice(10, 16).join("")}`;
}

export function findPlaceholders(spec: RequestSpec): string[] {
  const found = new Set<string>();
  const scan = (s: string | null) => {
    if (!s) return;
    let m: RegExpExecArray | null;
    PLACEHOLDER.lastIndex = 0;
    while ((m = PLACEHOLDER.exec(s)) !== null) {
      found.add(m[1]);
    }
  };
  scan(spec.url);
  for (const [k, v] of spec.headers) {
    scan(k);
    scan(v);
  }
  scan(spec.body);
  return [...found];
}
