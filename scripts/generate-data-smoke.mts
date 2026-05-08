import { generate, LANGUAGES } from "../src/lib/generate";
import type { Dataset } from "../src/lib/dataset";
import type { RequestSpec } from "../src/lib/types";

const spec: RequestSpec = {
  method: "POST",
  url: "https://api.example.com/users",
  headers: [
    ["Content-Type", "application/json"],
    ["Authorization", "Bearer token123"],
  ],
  body: '{"name":"{{name}}","email":"{{email}}","role":"{{role}}"}',
};

const ds: Dataset = {
  columns: ["name", "email", "role"],
  rows: [
    ["Alice", "alice@x.com", "admin"],
    ["Bob", "bob@y.com", "user"],
  ],
};

for (const lang of LANGUAGES) {
  console.log(`=== ${lang.label} ===`);
  console.log(generate(lang.id, spec, ds));
  console.log();
}
