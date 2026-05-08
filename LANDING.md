# Dante — Landing Page Content

Copy-paste-ready blocks for the Dante landing page. Each section is standalone — drop in whichever ones the page needs.

---

## Hero

**Headline:**
> Paste a curl. Hit Run.

**Sub-headline (one of these):**
> The local-first HTTP client that doesn't ask you to sign up, sync to a cloud, or fill out a form.

> Postman power. Bruno's locality. Notepad++'s simplicity.

> Native, ~10 MB, plain-text storage, zero account.

**Primary CTA:** `Download for Windows / macOS / Linux`
**Secondary CTA:** `View on GitHub →` (links to `https://github.com/monk0062006/dante`)

---

## One-liner pitch (for OG tags, search snippets, hero alt-text)

> A native, local-first API client. Paste any curl, get a saved, named, runnable request — with auth, scripts, monitors, and mock server. Stores everything as plain `.http` files. No account.

---

## What it is (paragraph form)

Dante is an HTTP client for developers who want Postman's power without Postman's ceremony. You paste a curl command — Dante parses it, names it sensibly (`users-create.http`), saves it as plain text in your project folder, and offers to extract any hardcoded tokens into reusable variables. Hit Run, see the response, write assertions, schedule monitors, generate language-specific code, replay against a local mock server. Everything is a file on disk you can git-commit. There's no account, no cloud, no sync. The whole app is a 10 MB native binary.

---

## Why we built it (pain-point copy)

| Tool | Pain |
|---|---|
| **Postman** | Account wall. Slow. Cloud-syncs your workspace whether you want that or not. ~200 MB Electron app. |
| **Bruno** | Better — but `.bru` proprietary format, limited scripting, no AI features, no AWS sigv4, no OAuth device flow. |
| **Curl + a text editor** | Fine until you need auth flows, assertions, monitoring, or want to share with a non-CLI teammate. |
| **Insomnia** | Acquired by Kong, drifting toward enterprise; opinionated about its DB. |

**Dante's bet**: store everything as plain `.http` files (the same format VS Code REST Client and JetBrains HTTP Client read), make every action sub-second, never require an account.

---

## Feature grid (tag each as Core / Power / Smart)

### Core (the boring stuff most people need)

- **Paste-curl-and-go** — curl in, request out. Multi-line, escaped quotes, `--data-binary @file`, all the weird shapes.
- **Plain `.http` files on disk** — same format as VS Code REST Client / JetBrains. Git-commitable.
- **Folders, sub-folders, drag-drop** — organize requests like files because they *are* files.
- **Environment variables** — `{{token}}`, `{{baseUrl}}`. Multiple env files (dev/staging/prod). Templates substitute at run time.
- **Request history** — every response saved automatically as a JSONL log next to the request. Browse past runs, diff responses.
- **Light & dark themes**, resizable split, customizable layout.
- **Cookie jar** — persists across requests within a session, can be cleared/edited.
- **CodeMirror 6 editor** — syntax highlighting, search, JSON formatter, autocomplete.

### Auth (every flow, no pop-up walls)

- **Bearer / Basic / API key headers** — auto-detected on paste.
- **Digest auth (RFC 2617)** — verified byte-exact against the spec test vector.
- **AWS Signature v4** — manual implementation; signatures verified byte-exact against an independent reference. SES, S3, Lambda — all signed inline.
- **OAuth 2.0 — five flows**:
  - Client credentials
  - Authorization code with PKCE
  - Resource owner password
  - Device authorization (for CLIs and TVs)
  - Refresh token
- **Cookie auth, OAuth1, custom signers via scripts** — all supported.

### Power (the stuff Postman charges for)

- **Pre/post-request scripts** — JavaScript sandbox (boa engine) with full `pm.*` Postman compatibility. Existing Postman scripts run as-is.
- **Assertion DSL** — `status == 200`, `body.data exists`, `header content-type contains "json"`, `elapsed < 1000`. Plus operators for regex match, type coercion.
- **Variable extraction** — capture from `body.path`, headers, cookies, status into env vars for chained requests. UI: click a value in the JSON tree, pick a variable name.
- **Code generation** — Python (requests + httpx), Node (fetch + axios), Go, Rust, cURL, plus async variants. One click.
- **Data-driven testing** — feed a CSV, run the same request once per row with cell values substituted into vars. Aggregate pass/fail report.
- **Local mock server** — point at a folder of `.http` files; Dante replays the most recent recorded response for each route. Run integration tests against the mock instead of the real API.
- **Monitors** — schedule any request to run on an interval (`every: 30s`, `every: 5m`). Logs results to JSONL, alerts on assertion failure.
- **WebSocket client** — connect, send, receive, close. Message log with timestamps, in/out direction. Echo round-trip verified end-to-end.
- **GraphQL** — paste a query, Dante introspects the schema for autocomplete and type-checking the response.

### Smart (the AI features — opt-in)

- **AI request review** — paste a curl, Dante asks Claude/GPT for: a sensible name, a one-line summary, 3-5 assertion lines, variables to extract, and any security smells (hardcoded secrets, PII in body). Secrets are **redacted before they leave your machine** — verified at the wire by automated tests.
- **Bring your own key** — Anthropic, OpenAI, Groq, any OpenAI-compat endpoint. Local Ollama too. Your key, your billing, your model choice.
- **Test-data generation** — feed a request, Dante generates 5-20 plausible variations (different user IDs, edge cases, malformed inputs). Run them all, see which ones break.

### Import / Export (don't make people start over)

- **Import**: Postman v2.1 collections, Bruno `.bru` folders, Insomnia exports, HAR (browser DevTools recording), OpenAPI / Swagger (JSON or YAML — `{id}` paths converted to `{{id}}` Dante vars).
- **Export**: Postman v2.1 collections, OpenAPI 3 spec, Markdown documentation.

---

## Quality signals (use as a "trust" section)

> Dante is open-source and tested like infrastructure code — because that's what an HTTP client is.

- **92 automated tests** covering every module that touches the network, runs scripts, signs requests, or talks to AI.
- **Cryptographic correctness verified against published reference vectors:**
  - AWS sigv4 signatures match byte-for-byte against an independent spec implementation.
  - HTTP Digest auth matches the RFC 2617 §3.5 worked example exactly.
- **Secret redaction verified at the wire** — when you use AI review, the actual request body that hits Anthropic/OpenAI is captured by the test and inspected to confirm no `Authorization: Bearer <secret>` value leaks.
- **End-to-end network tests** — real WebSocket echo round-trips, real local mock servers, real OAuth token endpoints (mocked locally for hermeticity).
- **Cross-platform CI** — every release is built on actual Windows, macOS Apple Silicon, macOS Intel, and Ubuntu runners.
- **MIT licensed**, ~10 MB binary, source on GitHub.

---

## Comparison table

| | **Dante** | Postman | Bruno | Insomnia |
|---|:---:|:---:|:---:|:---:|
| Local-first / no account | ✅ | ❌ | ✅ | ⚠️ |
| Plain-text storage | ✅ `.http` | ❌ JSON DB | ⚠️ `.bru` | ❌ NeDB |
| Native binary (not Electron) | ✅ Tauri | ❌ | ❌ | ❌ |
| Bundle size | ~10 MB | ~200 MB | ~80 MB | ~150 MB |
| Paste-curl one-shot | ✅ | ✅ | ⚠️ | ⚠️ |
| Pre/post scripts | ✅ JS | ✅ | ✅ | ✅ |
| Postman script compat (`pm.*`) | ✅ | ✅ | ❌ | ❌ |
| AWS sigv4 | ✅ | ✅ | ❌ | ⚠️ |
| OAuth device flow | ✅ | ✅ | ❌ | ❌ |
| Local mock server | ✅ built-in | ❌ paid | ❌ | ❌ |
| Monitors / scheduling | ✅ built-in | ❌ paid | ❌ | ❌ |
| WebSocket | ✅ | ✅ | ⚠️ | ✅ |
| GraphQL introspection | ✅ | ✅ | ✅ | ✅ |
| AI review (BYO key) | ✅ | ⚠️ proprietary | ❌ | ⚠️ |
| Code generation | ✅ 5+ langs | ✅ | ✅ | ✅ |
| CSV-driven testing | ✅ | ⚠️ paid | ❌ | ❌ |
| CLI for CI | ✅ `dante-run` | ✅ Newman | ✅ | ⚠️ |
| Import: Postman / HAR / OpenAPI / Insomnia | ✅ all | ⚠️ partial | ⚠️ partial | ⚠️ partial |
| Export: Postman / OpenAPI / Markdown | ✅ all | ⚠️ | ⚠️ | ⚠️ |

✅ = built in   ⚠️ = partial / paid / community plugin   ❌ = not available

---

## Install (per-platform copy blocks)

### Windows
```
Download Dante_x.y.z_x64-setup.exe → run → done.
```
Or `winget install monk.dante` *(coming soon — once v0.1 is stable)*.

### macOS
```
Download Dante_x.y.z_aarch64.dmg (Apple Silicon) or _x64.dmg (Intel) → drag to Applications → done.
```
Or `brew install --cask dante` *(coming soon)*.

### Linux
```
curl -L https://github.com/monk0062006/dante/releases/latest/download/Dante_amd64.AppImage \
  -o dante && chmod +x dante && ./dante
```
Or grab the `.deb` / `.rpm` from the release page.

### CI / headless (the `dante-run` CLI)
```bash
curl -L https://github.com/monk0062006/dante/releases/latest/download/dante-run > dante-run
chmod +x dante-run
./dante-run --reporter junit requests/ > junit.xml
```
JSON, JUnit, and human reporters. Exit code is non-zero if any assertion fails.

---

## Tech stack (for the "how it's built" section)

- **Shell**: Tauri 2 (native window, IPC, OS integration)
- **Backend**: Rust — `reqwest` for HTTP, `tokio-tungstenite` for WebSocket, `boa_engine` for the JS sandbox, `tiny_http` for the mock server, manual sigv4 + digest auth implementations
- **Frontend**: Svelte 5 with runes, CodeMirror 6
- **Storage**: plain `.http` text files + `.history.jsonl` for response logs + `cookies.json` for the cookie jar — every artifact is human-readable and git-friendly
- **MIT licensed**, GitHub: `monk0062006/dante`

---

## Footer / FAQ candidates

**Q: Where does Dante store my requests?**
A: In `~/Documents/Dante/` by default (configurable). Each request is a `.http` file. Each response history is `.http.history.jsonl` next to it.

**Q: Does it sync between machines?**
A: No — by design. Use git, Dropbox, Syncthing, or whatever you already use to move files around.

**Q: Is my data sent anywhere?**
A: Only when you explicitly hit a URL. AI review only fires when you click "Review with AI" and only sends the request shape (with secrets redacted) to *your* AI provider using *your* key.

**Q: Can I migrate from Postman?**
A: Yes — File → Import → Postman Collection. Your folder structure, request bodies, and `pm.*` scripts come across.

**Q: Is the format compatible with VS Code REST Client?**
A: Yes — Dante's `.http` files open directly in VS Code REST Client and JetBrains HTTP Client. The reverse is also true.

**Q: How do I extend it?**
A: Pre/post scripts in JavaScript today. A plugin system is on the roadmap.

---

## Tagline candidates (pick one or two for hero / OG)

1. *Paste a curl. Hit Run. Done.*
2. *The HTTP client that respects your time, your filesystem, and your secrets.*
3. *Local-first. Plain-text. Native. No account.*
4. *Postman power. Bruno's locality. Notepad++'s simplicity.*
5. *Test APIs the way you write code: in files, on disk, under git.*

---

## Numbers worth featuring

- **~10 MB** — installer size (Tauri, not Electron)
- **<1s** — cold start to ready
- **92** — automated tests
- **4 platforms** — Windows, macOS arm64, macOS Intel, Linux x64 (deb/rpm/AppImage)
- **$0** — to use, ever
- **MIT** — license
