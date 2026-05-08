import type { Dataset } from "./dataset";
import type { RequestSpec } from "./types";

export type Language = "curl" | "python" | "js" | "k6" | "shell";

export const LANGUAGES: { id: Language; label: string }[] = [
  { id: "curl", label: "curl" },
  { id: "python", label: "Python" },
  { id: "js", label: "JavaScript" },
  { id: "k6", label: "k6" },
  { id: "shell", label: "shell" },
];

export function generate(
  lang: Language,
  spec: RequestSpec,
  dataset: Dataset | null = null,
): string {
  if (dataset && dataset.rows.length > 0) {
    switch (lang) {
      case "curl": return toCurlData(spec, dataset);
      case "python": return toPythonData(spec, dataset);
      case "js": return toJsData(spec, dataset);
      case "k6": return toK6Data(spec, dataset);
      case "shell": return toShellData(spec, dataset);
    }
  }
  switch (lang) {
    case "curl": return toCurl(spec);
    case "python": return toPython(spec);
    case "js": return toJs(spec);
    case "k6": return toK6(spec);
    case "shell": return toShell(spec);
  }
}

// ---- single-shot generators ----

function toCurl(spec: RequestSpec): string {
  const lines: string[] = [`curl -X ${spec.method} ${shellQuote(spec.url)}`];
  for (const [k, v] of spec.headers) {
    lines.push(`  -H ${shellQuote(`${k}: ${v}`)}`);
  }
  if (spec.body) {
    lines.push(`  --data ${shellQuote(spec.body)}`);
  }
  return lines.join(" \\\n");
}

function toShell(spec: RequestSpec): string {
  return `#!/usr/bin/env bash\nset -euo pipefail\n\n${toCurl(spec)}\n`;
}

function toPython(spec: RequestSpec): string {
  const lines: string[] = ["import requests", "", `url = ${jsonStr(spec.url)}`];
  if (spec.headers.length > 0) {
    lines.push("headers = {");
    for (const [k, v] of spec.headers) {
      lines.push(`    ${jsonStr(k)}: ${jsonStr(v)},`);
    }
    lines.push("}");
  } else {
    lines.push("headers = {}");
  }
  if (spec.body) {
    if (looksLikeJson(spec)) {
      lines.push(`data = ${spec.body}`);
      lines.push("");
      lines.push(`response = requests.${spec.method.toLowerCase()}(url, headers=headers, json=data)`);
    } else {
      lines.push(`data = ${jsonStr(spec.body)}`);
      lines.push("");
      lines.push(`response = requests.${spec.method.toLowerCase()}(url, headers=headers, data=data)`);
    }
  } else {
    lines.push("");
    lines.push(`response = requests.${spec.method.toLowerCase()}(url, headers=headers)`);
  }
  lines.push("response.raise_for_status()");
  lines.push("print(response.text)");
  return lines.join("\n");
}

function toJs(spec: RequestSpec): string {
  const lines: string[] = [];
  lines.push("const url = " + jsonStr(spec.url) + ";");
  if (spec.headers.length > 0) {
    lines.push("const headers = {");
    for (const [k, v] of spec.headers) {
      lines.push(`  ${jsonStr(k)}: ${jsonStr(v)},`);
    }
    lines.push("};");
  } else {
    lines.push("const headers = {};");
  }
  const init: string[] = [`method: ${jsonStr(spec.method)}`, "headers"];
  if (spec.body) {
    if (looksLikeJson(spec)) {
      lines.push(`const body = ${spec.body};`);
      init.push("body: JSON.stringify(body)");
    } else {
      lines.push(`const body = ${jsonStr(spec.body)};`);
      init.push("body");
    }
  }
  lines.push("");
  lines.push(`const res = await fetch(url, { ${init.join(", ")} });`);
  lines.push("if (!res.ok) throw new Error(`HTTP ${res.status}`);");
  lines.push("console.log(await res.text());");
  return lines.join("\n");
}

function toK6(spec: RequestSpec): string {
  const lines: string[] = [];
  lines.push("import http from 'k6/http';");
  lines.push("import { check, sleep } from 'k6';");
  lines.push("");
  lines.push("export const options = {");
  lines.push("  vus: 1,");
  lines.push("  duration: '10s',");
  lines.push("};");
  lines.push("");
  lines.push("export default function () {");
  lines.push(`  const url = ${jsonStr(spec.url)};`);
  if (spec.headers.length > 0) {
    lines.push("  const params = {");
    lines.push("    headers: {");
    for (const [k, v] of spec.headers) {
      lines.push(`      ${jsonStr(k)}: ${jsonStr(v)},`);
    }
    lines.push("    },");
    lines.push("  };");
  } else {
    lines.push("  const params = {};");
  }
  if (spec.body) {
    lines.push(`  const body = ${jsonStr(spec.body)};`);
    lines.push(`  const res = http.${k6Method(spec.method)}(url, body, params);`);
  } else {
    lines.push(`  const res = http.${k6Method(spec.method)}(url, null, params);`);
  }
  lines.push("  check(res, { 'status is 2xx': (r) => r.status >= 200 && r.status < 300 });");
  lines.push("  sleep(1);");
  lines.push("}");
  return lines.join("\n");
}

// ---- helpers ----

function k6Method(method: string): string {
  const m = method.toLowerCase();
  if (m === "get") return "get";
  if (m === "post") return "post";
  if (m === "put") return "put";
  if (m === "patch") return "patch";
  if (m === "delete") return "del";
  if (m === "head") return "head";
  if (m === "options") return "options";
  return "request";
}

function looksLikeJson(spec: RequestSpec): boolean {
  if (!spec.body) return false;
  const ct = spec.headers.find(([k]) => k.toLowerCase() === "content-type")?.[1] ?? "";
  if (ct.includes("json")) return tryParseJson(spec.body);
  return false;
}

function tryParseJson(s: string): boolean {
  try {
    JSON.parse(s);
    return true;
  } catch {
    return false;
  }
}

function shellQuote(s: string): string {
  if (!/['\\$`"\s]/.test(s)) return s;
  return "'" + s.replace(/'/g, "'\\''") + "'";
}

function jsonStr(s: string): string {
  return JSON.stringify(s);
}

// ---- data-driven generators ----

type SegPart = { type: "literal" | "expr"; value: string };

function tokenizeForData(s: string, columns: string[]): { parts: SegPart[]; hasExpr: boolean } {
  const parts: SegPart[] = [];
  let hasExpr = false;
  let lastIdx = 0;
  const re = /\{\{(\w+)\}\}/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(s)) !== null) {
    if (m.index > lastIdx) {
      parts.push({ type: "literal", value: s.slice(lastIdx, m.index) });
    }
    if (columns.includes(m[1])) {
      parts.push({ type: "expr", value: m[1] });
      hasExpr = true;
    } else {
      parts.push({ type: "literal", value: m[0] });
    }
    lastIdx = m.index + m[0].length;
  }
  if (lastIdx < s.length) {
    parts.push({ type: "literal", value: s.slice(lastIdx) });
  }
  return { parts, hasExpr };
}

function templateForPython(s: string, columns: string[]): string {
  const { parts, hasExpr } = tokenizeForData(s, columns);
  if (!hasExpr) return JSON.stringify(s);
  const out: string[] = [];
  for (const p of parts) {
    if (p.type === "literal") {
      out.push(
        p.value
          .replace(/\\/g, "\\\\")
          .replace(/"/g, '\\"')
          .replace(/\n/g, "\\n")
          .replace(/\{/g, "{{")
          .replace(/\}/g, "}}"),
      );
    } else {
      out.push(`{row['${p.value}']}`);
    }
  }
  return `f"${out.join("")}"`;
}

function templateForJs(s: string, columns: string[]): string {
  const { parts, hasExpr } = tokenizeForData(s, columns);
  if (!hasExpr) return JSON.stringify(s);
  const out: string[] = [];
  for (const p of parts) {
    if (p.type === "literal") {
      out.push(
        p.value
          .replace(/\\/g, "\\\\")
          .replace(/`/g, "\\`")
          .replace(/\$\{/g, "\\${"),
      );
    } else {
      out.push(`\${row[${JSON.stringify(p.value)}]}`);
    }
  }
  return `\`${out.join("")}\``;
}

function templateForShell(s: string, columns: string[]): string {
  const { parts, hasExpr } = tokenizeForData(s, columns);
  if (!hasExpr) {
    if (!/['\\$`"\s]/.test(s)) return s;
    return `"${s.replace(/\\/g, "\\\\").replace(/"/g, '\\"').replace(/\$/g, "\\$")}"`;
  }
  const out: string[] = [];
  for (const p of parts) {
    if (p.type === "literal") {
      out.push(p.value.replace(/\\/g, "\\\\").replace(/"/g, '\\"').replace(/\$/g, "\\$"));
    } else {
      out.push(`\${${p.value}}`);
    }
  }
  return `"${out.join("")}"`;
}

function pythonRowsLiteral(dataset: Dataset): string {
  const items = dataset.rows.map((row) => {
    const obj: Record<string, string> = {};
    for (let i = 0; i < dataset.columns.length; i++) {
      obj[dataset.columns[i]] = row[i] ?? "";
    }
    return JSON.stringify(obj);
  });
  return `[\n  ${items.join(",\n  ")}\n]`;
}

function jsRowsLiteral(dataset: Dataset): string {
  return JSON.stringify(
    dataset.rows.map((row) => {
      const obj: Record<string, string> = {};
      for (let i = 0; i < dataset.columns.length; i++) {
        obj[dataset.columns[i]] = row[i] ?? "";
      }
      return obj;
    }),
    null,
    2,
  );
}

function toPythonData(spec: RequestSpec, ds: Dataset): string {
  const cols = ds.columns;
  const lines: string[] = [];
  lines.push("import requests");
  lines.push("");
  lines.push(`DATA = ${pythonRowsLiteral(ds)}`);
  lines.push("");
  lines.push("for row in DATA:");
  lines.push(`    url = ${templateForPython(spec.url, cols)}`);
  if (spec.headers.length > 0) {
    lines.push("    headers = {");
    for (const [k, v] of spec.headers) {
      lines.push(
        `        ${templateForPython(k, cols)}: ${templateForPython(v, cols)},`,
      );
    }
    lines.push("    }");
  } else {
    lines.push("    headers = {}");
  }
  if (spec.body) {
    if (looksLikeJson(spec)) {
      lines.push("    import json");
      lines.push(`    body = json.loads(${templateForPython(spec.body, cols)})`);
      lines.push(
        `    response = requests.${spec.method.toLowerCase()}(url, headers=headers, json=body)`,
      );
    } else {
      lines.push(`    data = ${templateForPython(spec.body, cols)}`);
      lines.push(
        `    response = requests.${spec.method.toLowerCase()}(url, headers=headers, data=data)`,
      );
    }
  } else {
    lines.push(
      `    response = requests.${spec.method.toLowerCase()}(url, headers=headers)`,
    );
  }
  lines.push("    print(row, response.status_code)");
  return lines.join("\n");
}

function toJsData(spec: RequestSpec, ds: Dataset): string {
  const cols = ds.columns;
  const lines: string[] = [];
  lines.push(`const DATA = ${jsRowsLiteral(ds)};`);
  lines.push("");
  lines.push("for (const row of DATA) {");
  lines.push(`  const url = ${templateForJs(spec.url, cols)};`);
  if (spec.headers.length > 0) {
    lines.push("  const headers = {");
    for (const [k, v] of spec.headers) {
      lines.push(
        `    ${templateForJs(k, cols)}: ${templateForJs(v, cols)},`,
      );
    }
    lines.push("  };");
  } else {
    lines.push("  const headers = {};");
  }
  const init: string[] = [`method: ${jsonStr(spec.method)}`, "headers"];
  if (spec.body) {
    if (looksLikeJson(spec)) {
      lines.push(`  const body = JSON.parse(${templateForJs(spec.body, cols)});`);
      init.push("body: JSON.stringify(body)");
    } else {
      lines.push(`  const body = ${templateForJs(spec.body, cols)};`);
      init.push("body");
    }
  }
  lines.push(`  const res = await fetch(url, { ${init.join(", ")} });`);
  lines.push("  console.log(row, res.status);");
  lines.push("}");
  return lines.join("\n");
}

function toCurlData(spec: RequestSpec, ds: Dataset): string {
  const lines: string[] = [];
  lines.push("#!/usr/bin/env bash");
  lines.push("set -euo pipefail");
  lines.push("");
  lines.push(`COLS=(${ds.columns.map((c) => `"${c}"`).join(" ")})`);
  lines.push("ROWS=(");
  for (const row of ds.rows) {
    const tsv = row.map((c) => c.replace(/\t/g, " ").replace(/"/g, '\\"')).join("\t");
    lines.push(`  "${tsv}"`);
  }
  lines.push(")");
  lines.push("");
  lines.push('for ROW in "${ROWS[@]}"; do');
  lines.push(`  IFS=$'\\t' read -ra VALS <<< "$ROW"`);
  lines.push('  for i in "${!COLS[@]}"; do');
  lines.push('    declare "${COLS[$i]}=${VALS[$i]:-}"');
  lines.push("  done");
  lines.push(`  curl -X ${spec.method} ${templateForShell(spec.url, ds.columns)} \\`);
  for (const [k, v] of spec.headers) {
    lines.push(
      `    -H ${templateForShell(`${k}: ${v}`, ds.columns)} \\`,
    );
  }
  if (spec.body) {
    lines.push(`    --data ${templateForShell(spec.body, ds.columns)}`);
  } else {
    const last = lines.pop();
    if (last) lines.push(last.replace(/ \\$/, ""));
  }
  lines.push("done");
  return lines.join("\n");
}

function toShellData(spec: RequestSpec, ds: Dataset): string {
  return toCurlData(spec, ds);
}

function toK6Data(spec: RequestSpec, ds: Dataset): string {
  const cols = ds.columns;
  const lines: string[] = [];
  lines.push("import http from 'k6/http';");
  lines.push("import { check } from 'k6';");
  lines.push("import { SharedArray } from 'k6/data';");
  lines.push("");
  lines.push(`const DATA = new SharedArray('rows', () => ${jsRowsLiteral(ds)});`);
  lines.push("");
  lines.push("export const options = {");
  lines.push("  iterations: DATA.length,");
  lines.push("  vus: 1,");
  lines.push("};");
  lines.push("");
  lines.push("export default function () {");
  lines.push("  const row = DATA[__ITER % DATA.length];");
  lines.push(`  const url = ${templateForJs(spec.url, cols)};`);
  if (spec.headers.length > 0) {
    lines.push("  const params = { headers: {");
    for (const [k, v] of spec.headers) {
      lines.push(
        `    ${templateForJs(k, cols)}: ${templateForJs(v, cols)},`,
      );
    }
    lines.push("  } };");
  } else {
    lines.push("  const params = {};");
  }
  if (spec.body) {
    lines.push(`  const body = ${templateForJs(spec.body, cols)};`);
    lines.push(`  const res = http.${k6Method(spec.method)}(url, body, params);`);
  } else {
    lines.push(`  const res = http.${k6Method(spec.method)}(url, null, params);`);
  }
  lines.push("  check(res, { 'status is 2xx': (r) => r.status >= 200 && r.status < 300 });");
  lines.push("}");
  return lines.join("\n");
}
