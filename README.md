# Dante

> Paste a curl. Get a runnable, named, versioned, organized request — without filling out a form, signing up, or syncing to a cloud.

Local-first HTTP client for developers. Native Tauri 2 binary, ~10 MB. Zero account, zero cloud.

```
curl -X POST https://api.example.com/users \
  -H "Authorization: Bearer abc" \
  -d '{"name":"Alice"}'
```

→ paste in Dante → it parses, names it `users-create.http`, saves to `~/Documents/Dante/api.example.com/`, offers to extract the bearer token to `{{apiToken}}`. Hit Run.

## Why

Postman is bloated, account-walled, and slow. Bruno is closer but Electron and `.bru`-format. Dante is **native, sub-second, and writes plain `.http` files** that work in VS Code REST Client / JetBrains HTTP Client too.

## Install

### From GitHub Releases (recommended)

Grab the installer for your OS from the [latest release](https://github.com/monk0062006/dante/releases/latest):

- **Windows**: `Dante_x.y.z_x64-setup.exe` or `.msi`
- **macOS**: `Dante_x.y.z_universal.dmg`
- **Linux**: `dante_x.y.z_amd64.AppImage`, `.deb`, or `.rpm`

### CLI for CI

```bash
# Download the dante-run binary from the same release
curl -L https://github.com/monk0062006/dante/releases/latest/download/dante-run-linux > dante-run
chmod +x dante-run
./dante-run --reporter junit requests/ > junit.xml
```

## Build from source

Requires **Node 20+**, **Rust stable** (rustup), and on **Windows** the MSVC C++ Build Tools.

```bash
git clone https://github.com/monk0062006/dante.git
cd dante
npm install
npm run tauri dev      # development
npm run tauri build    # release installers in src-tauri/target/release/bundle/
```

## What it does

### Core flow

1. Open Dante. Cold start <1 s.
2. Paste a curl. It parses, auto-names the file, autosaves to a folder under your project root keyed by URL host.
3. Hit Run (or `⌘/Ctrl+Enter`). Response shows status, headers, body. Every run is appended to a per-request `.history.jsonl`.
4. Click any saved request in the sidebar to load it. Edit; autosave kicks in after 400ms.

### Storage format — plain `.http` files

```http
# create a new user
POST https://api.example.com/users
Content-Type: application/json
Authorization: Bearer {{apiToken}}

{"name": "{{name}}", "email": "{{email}}"}

### tests
status == 201
body.id exists
elapsed < 1000

### extract
newUserId = body.id

### data
name,email
Alice,alice@x.com
Bob,bob@y.com

### pre-request
const ts = Date.now();
dante.headers.set("X-Timestamp", String(ts));

### post-request
const data = JSON.parse(dante.response.body);
console.log("created", data.id);
```

The whole file is text. Diffable in git. Openable in VS Code's REST Client / JetBrains' HTTP Client. No proprietary blob.

### Variables

`.env` files for active config, `.env.global` for shared. `{{varName}}` placeholders. Plus 14 built-in template functions: `{{$uuid}}`, `{{$now}}`, `{{$timestamp}}`, `{{$randomEmail}}`, `{{$randomFirstName}}`, `{{$randomFullName}}`, `{{$randomPhoneNumber}}`, `{{$randomIPv4}}`, `{{$randomColor}}`, `{{$base64("text")}}`, `{{$random.int(1,100)}}`, `{{$random.alphanum(8)}}`, etc.

### Auth

Bearer · Basic · Digest · AWS sigv4 · OAuth 2.0 (client_credentials, auth-code with PKCE, password, device flow) · auto-token refresh.

### Smart features

- **Review on paste** — detects Bearer tokens, API keys in URL/headers/cookies, offers one-click extraction to env vars
- **Auto-regression detection** — every run compared to previous; banner if status, latency >2×, or JSON shape drifted
- **AI review (BYOK)** — Claude or any free OpenAI-compatible provider (xAI Grok, Groq, Gemini, OpenRouter) suggests names, tests, extracts, security flags
- **Differential testing** — `Run vs all envs` button fires the same request against every env, side-by-side comparison
- **Data-driven runs** — `### data` block (CSV) → `Run × N` runs once per row → code-generation produces the equivalent loop in Python/JS/k6/shell
- **Workflows** — folder `▶▶ chain` runs requests in alphabetical order; extracted vars from request N feed request N+1
- **Mock server** — toggle in sidebar; spawns a local HTTP listener that replays each saved request's last response on its path
- **Monitors** — `### schedule\nevery: 5m` → background task runs the request on schedule, alerts on failures
- **Load tests** — `Load` button → run N requests with M concurrent workers, p50/p95/p99 latency histograms
- **WebSocket** — paste any `ws://` or `wss://` URL → connect, persistent message log, send box

### Imports

OpenAPI 3.x (JSON / YAML, with `$ref` resolution) · Postman v2.1 collections · Insomnia v4 exports · HAR files (browser DevTools recordings).

### Exports

Markdown documentation (`README.md`) · OpenAPI YAML · Postman v2.1 JSON.

### Code generation

Per request: curl, Python (`requests`), JavaScript (`fetch`), k6, shell. With a `### data` block, generators produce loops over the rows.

### Editor

CodeMirror 6: line numbers, JSON syntax highlight, custom highlight for `{{vars}}`, request lines, header keys, JS-like highlighting in `### pre-request` / `### post-request` blocks. Find/replace, header autocomplete, content-type value autocomplete.

### CLI

`dante-run` is a standalone binary that takes `.http` files or folders, executes them, exits 0 if all 2xx else 1. Reporters: `--reporter stdout|json|junit`. Drop into any CI.

```bash
dante-run --reporter junit requests/ > junit.xml
```

## Keyboard shortcuts

`⌘/Ctrl+?` opens the full list. The big ones:

- `⌘/Ctrl + Enter` — Run
- `⌘/Ctrl + F` — Find in editor
- `⌘/Ctrl + B` — Toggle sidebar
- `⌘/Ctrl + Shift + P` — Focus search
- `⌘/Ctrl + Shift + N` — New request
- `⌘/Ctrl + ↑/↓` — Navigate sidebar

## How does it compare?

| | Postman | Bruno | Dante |
|---|---|---|---|
| Install size | ~600 MB | ~150 MB | **~10 MB** |
| Cold start | ~3 s | ~3 s | **<1 s** |
| Storage format | proprietary JSON | `.bru` (custom) | **`.http` (standard)** |
| Account required | yes | no | **no** |
| Cloud sync | yes | no | **no** (non-goal) |
| JS scripts | yes | yes (vm2) | yes (boa) |
| `pm.*` API | native | partial | shim |
| WebSocket | yes | yes | **yes** |
| gRPC | yes | yes | not yet |
| Mock server | hosted | none | local |
| Monitors | hosted | none | **local** |
| AI review | no | no | **yes (BYOK + free providers)** |
| Auto-regression | no | no | **yes** |
| Diff across envs | no | no | **yes** |
| Data-driven runs | yes | yes | **yes** + script export |

## Project layout

```
src/                       Svelte 5 frontend
  App.svelte                main UI
  lib/Editor.svelte         CodeMirror 6 wrapper
  lib/curl.ts               curl parser
  lib/substitute.ts         {{var}} + template fns
  lib/assertions.ts         test DSL
  lib/extract.ts            ### extract block
  lib/dataset.ts            ### data block (CSV)
  lib/script-blocks.ts      ### pre/post-request parsers
  lib/generate.ts           code generators (curl/python/js/k6/shell)
  lib/review.ts             secret-detection heuristics
  lib/api.ts                Tauri command bindings
src-tauri/                 Rust backend
  src/lib.rs                Tauri commands (HTTP, storage, OAuth, etc.)
  src/ai.rs                 AI review (Claude + OpenAI-compat)
  src/script.rs             boa_engine JS sandbox + pm.* shim
  src/sigv4.rs              AWS sigv4 signing
  src/mock.rs               local mock server (tiny_http)
  src/monitors.rs           scheduled run daemon
  src/ws.rs                 WebSocket connection manager
  src/bin/dante-run.rs      CLI binary
```

## License

MIT. See [LICENSE](LICENSE).

## Status

Active development. v0.1 covers the full feature set described above. Notably missing vs Postman: gRPC, Postman Flows visual editor. By design (non-goals): cloud sync, team workspaces.

Issues + PRs welcome.
