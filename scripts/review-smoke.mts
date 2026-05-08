import { reviewRequest } from "../src/lib/review";

const cases = [
  { name: "bearer token", spec: { method: "GET" as const, url: "https://api.example.com/me", headers: [["Authorization", "Bearer eyJhbGciOiJIUzI1NiJ9.abc.def"] as [string, string]], body: null }, expectKind: "bearer-token" as const },
  { name: "basic auth", spec: { method: "GET" as const, url: "https://api.example.com/me", headers: [["Authorization", "Basic dXNlcjpwYXNz"] as [string, string]], body: null }, expectKind: "basic-auth" as const },
  { name: "x-api-key header", spec: { method: "POST" as const, url: "https://api.example.com/users", headers: [["X-API-Key", "sk_test_abc123"] as [string, string], ["Content-Type", "application/json"] as [string, string]], body: '{"a":1}' }, expectKind: "api-key-header" as const },
  { name: "api_key in URL", spec: { method: "GET" as const, url: "https://api.example.com/users?api_key=sk_live_xyz", headers: [], body: null }, expectKind: "api-key-query" as const },
  { name: "already substituted", spec: { method: "GET" as const, url: "https://api.example.com/me", headers: [["Authorization", "Bearer {{apiToken}}"] as [string, string]], body: null }, expectKind: null },
];

let pass = 0;
let fail = 0;
for (const c of cases) {
  const findings = reviewRequest(c.spec);
  if (c.expectKind === null) {
    if (findings.length === 0) {
      console.log("PASS:", c.name);
      pass++;
    } else {
      console.log("FAIL:", c.name, "expected 0 findings, got", findings.map((f) => f.kind).join(","));
      fail++;
    }
  } else {
    const found = findings.find((f) => f.kind === c.expectKind);
    if (found) {
      const applied = found.apply(c.spec);
      const auth =
        applied.headers.find((h) => h[0].toLowerCase() === "authorization") ||
        applied.headers.find((h) => h[0].toLowerCase() === "x-api-key");
      const url = applied.url;
      console.log("PASS:", c.name, "→", found.suggestedName);
      console.log("       header:", auth ? auth.join(": ") : "(none)");
      console.log("       url:   ", url);
      pass++;
    } else {
      console.log("FAIL:", c.name, "expected", c.expectKind, "got", findings.map((f) => f.kind).join(","));
      fail++;
    }
  }
}
console.log(`\n${pass}/${pass + fail} passed`);
process.exit(fail > 0 ? 1 : 0);
