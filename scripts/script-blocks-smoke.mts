import { parseScriptBlocks } from "../src/lib/script-blocks";

const httpFile = `POST https://api.example.com/login
Content-Type: application/json

{"user":"alice","pass":"hunter2"}

### pre-request
const ts = Date.now();
dante.headers.set("X-Timestamp", String(ts));
console.log("pre-request set timestamp", ts);

### post-request
if (dante.response && dante.response.status === 200) {
  const data = JSON.parse(dante.response.body);
  dante.env.set("token", data.access_token);
  console.log("token saved");
} else {
  console.error("login failed");
}

### tests
status == 200
`;

const blocks = parseScriptBlocks(httpFile);
console.log("--- pre-request ---");
console.log(blocks.preScript);
console.log("--- post-request ---");
console.log(blocks.postScript);
