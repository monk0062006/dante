import { parseDataset, rowToVars } from "../src/lib/dataset";

const httpFile = `POST https://api.example.com/users
Content-Type: application/json

{"name": "{{name}}", "email": "{{email}}"}

### tests
status == 200

### data
name,email,role
Alice,alice@x.com,admin
Bob,bob@y.com,user
"Carol O'Hara","carol@z.io",guest
`;

const ds = parseDataset(httpFile);
console.log("Dataset:", JSON.stringify(ds, null, 2));

if (ds) {
  for (let i = 0; i < ds.rows.length; i++) {
    const vars = rowToVars(ds, i);
    console.log(`Row ${i}:`, Object.fromEntries(vars));
  }
}
