import { applyExtracts, parseExtracts } from "../src/lib/extract";
import type { ResponseData } from "../src/lib/types";

const httpFile = `POST https://auth.example.com/login
Content-Type: application/json

{"user":"a","pass":"b"}

### extract
token = body.access_token
userId = body.user.id
sessionId = cookie session
rateLimit = header X-RateLimit-Remaining
status = status

### tests
status == 200
`;

const rules = parseExtracts(httpFile);
console.log("Parsed extracts:");
for (const r of rules) {
  console.log("  ", JSON.stringify(r));
}

const response: ResponseData = {
  status: 200,
  status_text: "OK",
  headers: [
    ["Content-Type", "application/json"],
    ["Set-Cookie", "session=abc123; Path=/; HttpOnly"],
    ["X-RateLimit-Remaining", "97"],
  ],
  body: '{"access_token":"jwt-xyz","user":{"id":"u_42","name":"A"}}',
  elapsed_ms: 122,
};

console.log("\nApplied:");
for (const r of applyExtracts(rules, response)) {
  console.log("  ", r.rule.varName, "=", r.value);
}
