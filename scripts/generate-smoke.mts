import { generate, LANGUAGES } from "../src/lib/generate";
import type { RequestSpec } from "../src/lib/types";

const spec: RequestSpec = {
  method: "POST",
  url: "https://api.example.com/users?limit=10",
  headers: [
    ["Content-Type", "application/json"],
    ["Authorization", "Bearer test-token"],
  ],
  body: '{"name":"Jane","email":"j@x.com"}',
};

for (const lang of LANGUAGES) {
  console.log(`=== ${lang.label} ===`);
  console.log(generate(lang.id, spec));
  console.log();
}
