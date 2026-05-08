import { evaluateAssertions, parseAssertions } from "../src/lib/assertions";
import type { ResponseData } from "../src/lib/types";

const httpFile = `GET https://api.example.com/users
Authorization: Bearer xxx

### tests
status == 200
elapsed < 1000
body.id exists
body.user.name == "Alice"
header content-type contains json
`;

const assertions = parseAssertions(httpFile);
console.log("Parsed assertions:");
for (const a of assertions) {
  console.log("  ", JSON.stringify(a));
}

const response: ResponseData = {
  status: 200,
  status_text: "OK",
  headers: [["Content-Type", "application/json"]],
  body: '{"id":"u_1","user":{"name":"Alice","email":"a@x"}}',
  elapsed_ms: 145,
};

console.log("\nResults vs response:");
const results = evaluateAssertions(assertions, response);
for (const r of results) {
  console.log(r.pass ? "  PASS" : "  FAIL", r.raw, r.pass ? "" : `→ ${r.reason}`);
}

const failResp: ResponseData = {
  ...response,
  status: 500,
  body: '{"id":"u_1","user":{"name":"Bob"}}',
  elapsed_ms: 2000,
};

console.log("\nResults vs failing response:");
for (const r of evaluateAssertions(assertions, failResp)) {
  console.log(r.pass ? "  PASS" : "  FAIL", r.raw, r.pass ? "" : `→ ${r.reason}`);
}
