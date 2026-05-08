import { substitute } from "../src/lib/substitute";
import type { RequestSpec } from "../src/lib/types";

const spec: RequestSpec = {
  method: "POST",
  url: "https://api.example.com/things/{{$uuid}}?ts={{$now.unix}}",
  headers: [
    ["X-Request-ID", "{{$random.alphanum(8)}}"],
    ["X-Auth", "Bearer {{token}}"],
  ],
  body: '{"id":"{{$uuid}}","when":"{{$now}}","secret":"{{$base64("hi there")}}"}',
};

const env = new Map<string, string>([["token", "real-token-value"]]);
const sub = substitute(spec, env);

console.log("URL:", sub.spec.url);
console.log("Headers:");
for (const [k, v] of sub.spec.headers) {
  console.log(`  ${k}: ${v}`);
}
console.log("Body:", sub.spec.body);
console.log("\nResolved:", sub.resolved.join(", "));
console.log("Unresolved:", sub.unresolved.join(", "));
