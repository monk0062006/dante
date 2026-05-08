import type { HttpMethod, RequestSpec } from "./types";

const KNOWN_METHODS: HttpMethod[] = [
  "GET",
  "POST",
  "PUT",
  "PATCH",
  "DELETE",
  "HEAD",
  "OPTIONS",
];

export function looksLikeCurl(input: string): boolean {
  return /^\s*curl\b/i.test(input);
}

export function parseCurl(input: string): RequestSpec {
  const tokens = tokenize(stripLineContinuations(input.trim()));
  if (tokens.length === 0 || tokens[0].toLowerCase() !== "curl") {
    throw new Error("not a curl command");
  }

  let method: HttpMethod | null = null;
  let url: string | null = null;
  const headers: Array<[string, string]> = [];
  const data: string[] = [];
  let isMultipart = false;
  let basicAuth: string | null = null;

  for (let i = 1; i < tokens.length; i++) {
    const tok = tokens[i];
    const next = (): string => {
      const v = tokens[++i];
      if (v === undefined) throw new Error(`flag ${tok} missing argument`);
      return v;
    };

    switch (tok) {
      case "-X":
      case "--request":
        method = next().toUpperCase() as HttpMethod;
        break;
      case "-H":
      case "--header": {
        const [k, ...rest] = next().split(":");
        headers.push([k.trim(), rest.join(":").trim()]);
        break;
      }
      case "-d":
      case "--data":
      case "--data-raw":
      case "--data-binary":
      case "--data-ascii":
        data.push(next());
        break;
      case "--data-urlencode":
        data.push(encodeURIComponent(next()));
        break;
      case "-F":
      case "--form":
        isMultipart = true;
        data.push(next());
        break;
      case "-u":
      case "--user":
        basicAuth = next();
        break;
      case "-b":
      case "--cookie":
        headers.push(["Cookie", next()]);
        break;
      case "-A":
      case "--user-agent":
        headers.push(["User-Agent", next()]);
        break;
      case "-e":
      case "--referer":
        headers.push(["Referer", next()]);
        break;
      case "--url":
        url = next();
        break;
      case "-G":
      case "--get":
        method = method ?? "GET";
        break;
      case "-I":
      case "--head":
        method = "HEAD";
        break;
      // Flags we silently ignore (don't take an argument)
      case "-v":
      case "--verbose":
      case "-s":
      case "--silent":
      case "-S":
      case "--show-error":
      case "-k":
      case "--insecure":
      case "-L":
      case "--location":
      case "-i":
      case "--include":
      case "-f":
      case "--fail":
      case "-#":
      case "--progress-bar":
      case "--compressed":
      case "--http1.1":
      case "--http2":
      case "--http3":
        break;
      // Flags that take an argument we don't need
      case "-o":
      case "--output":
      case "-O":
      case "--remote-name":
      case "--max-time":
      case "--connect-timeout":
      case "--retry":
      case "-w":
      case "--write-out":
      case "-D":
      case "--dump-header":
      case "-c":
      case "--cookie-jar":
      case "-T":
      case "--upload-file":
      case "--cacert":
      case "--cert":
      case "--key":
      case "-x":
      case "--proxy":
      case "--resolve":
      case "--limit-rate":
        next();
        break;
      default:
        if (tok.startsWith("--data") || tok.startsWith("-d")) {
          data.push(next());
        } else if (!tok.startsWith("-")) {
          if (url === null) url = tok;
        }
        break;
    }
  }

  if (!url) throw new Error("curl missing URL");

  if (basicAuth !== null) {
    headers.push(["Authorization", `Basic ${btoa(basicAuth)}`]);
  }

  let body: string | null = null;
  if (data.length > 0) {
    if (isMultipart) {
      body = data.join("&");
    } else {
      body = data.join("&");
    }
    method = method ?? "POST";
    if (!headers.some(([k]) => k.toLowerCase() === "content-type")) {
      headers.push([
        "Content-Type",
        isMultipart
          ? "multipart/form-data"
          : "application/x-www-form-urlencoded",
      ]);
    }
  }

  if (!method) method = "GET";
  if (!KNOWN_METHODS.includes(method)) method = "GET";

  return { method, url, headers, body };
}

function stripLineContinuations(s: string): string {
  return s.replace(/\\\r?\n/g, " ").replace(/\^\r?\n/g, " ");
}

function tokenize(s: string): string[] {
  const tokens: string[] = [];
  let i = 0;
  while (i < s.length) {
    const ch = s[i];
    if (ch === " " || ch === "\t" || ch === "\n" || ch === "\r") {
      i++;
      continue;
    }
    if (ch === "'") {
      let end = s.indexOf("'", i + 1);
      if (end === -1) end = s.length;
      tokens.push(s.slice(i + 1, end));
      i = end + 1;
      continue;
    }
    if (ch === '"') {
      let buf = "";
      i++;
      while (i < s.length && s[i] !== '"') {
        if (s[i] === "\\" && i + 1 < s.length) {
          buf += s[i + 1];
          i += 2;
        } else {
          buf += s[i];
          i++;
        }
      }
      i++;
      tokens.push(buf);
      continue;
    }
    let buf = "";
    while (
      i < s.length &&
      s[i] !== " " &&
      s[i] !== "\t" &&
      s[i] !== "\n" &&
      s[i] !== "\r"
    ) {
      if (s[i] === "\\" && i + 1 < s.length) {
        buf += s[i + 1];
        i += 2;
      } else {
        buf += s[i];
        i++;
      }
    }
    tokens.push(buf);
  }
  return tokens;
}

export function toHttpFile(spec: RequestSpec): string {
  const lines: string[] = [];
  lines.push(`${spec.method} ${spec.url}`);
  for (const [k, v] of spec.headers) {
    lines.push(`${k}: ${v}`);
  }
  if (spec.body) {
    lines.push("");
    lines.push(spec.body);
  }
  return lines.join("\n") + "\n";
}

export function autoName(spec: RequestSpec): string {
  let path = "/";
  try {
    path = new URL(spec.url).pathname || "/";
  } catch {
    path = spec.url.replace(/^https?:\/\/[^/]+/, "") || "/";
  }
  return `${spec.method} ${path}`;
}
