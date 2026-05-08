<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { open as openInShell } from "@tauri-apps/plugin-shell";
  import {
    aiReviewRequest,
    aiReviewRequestOpenaiCompat,
    appendHistory,
    clearCookies,
    createEnv,
    deleteCookie,
    defaultProjectFolder,
    deleteRequest,
    duplicateRequest,
    exportMarkdown,
    moveRequest,
    renameFolder,
    deleteFolder,
    renameRequest,
    exportOpenapi,
    exportPostman,
    fetchOauthAuthcode,
    graphqlIntrospect,
    monitorList,
    monitorParseSchedule,
    monitorStart,
    monitorStop,
    readWorkspaceConfig,
    fetchOauthPassword,
    fetchOauthRefresh,
    fetchOauthToken,
    oauthDeviceInit,
    oauthDevicePoll,
    getSettings,
    importHar,
    importInsomnia,
    importOpenapi,
    importPostman,
    listCookies,
    listEnvs,
    listRequests,
    loadCookies,
    loadRequest,
    mockServerStatus,
    readEnv,
    readHistory,
    runRequest,
    runScript,
    wsClose,
    wsConnect,
    wsSend,
    saveCookies,
    saveRequest,
    saveSettings,
    startMockServer,
    stopMockServer,
    writeEnv,
    type AiReview,
    type AwsParams,
    type CookieView,
    type DigestCreds,
    type ImportResult,
    type PmTestResult,
    type WorkspaceConfig,
    type EnvFile,
    type EnvPair,
    type HistoryEntry,
    type MockStatus,
    type RequestEntry,
  } from "./lib/api";
  import {
    evaluateAssertions,
    parseAssertions,
    type AssertionResult,
  } from "./lib/assertions";
  import { autoName, looksLikeCurl, parseCurl, toHttpFile } from "./lib/curl";
  import { parseDataset, rowToVars, type Dataset } from "./lib/dataset";
  import { parseScriptBlocks } from "./lib/script-blocks";
  import {
    applyExtracts,
    parseExtracts,
    type ExtractResult,
  } from "./lib/extract";
  import AiReviewPanel from "./lib/AiReviewPanel.svelte";
  import Editor from "./lib/Editor.svelte";
  import GeneratePanel from "./lib/GeneratePanel.svelte";
  import { reviewRequest, type Finding } from "./lib/review";
  import { findPlaceholders, substitute } from "./lib/substitute";
  import type { RequestSpec, ResponseData } from "./lib/types";

  const SAMPLE_CURL = `curl https://httpbin.org/get -H "Accept: application/json"`;

  let folder = $state<string | null>(null);
  let requests = $state<RequestEntry[]>([]);
  let activePath = $state<string | null>(null);
  let editorText = $state("");
  let parsed = $state<RequestSpec | null>(null);
  let parseError = $state<string | null>(null);
  let response = $state<ResponseData | null>(null);
  let sentSpec = $state<RequestSpec | null>(null);
  let runError = $state<string | null>(null);
  let isRunning = $state(false);
  let bootstrapped = $state(false);
  let workspaceConfig = $state<WorkspaceConfig>({
    default_headers: {},
    base_url: null,
    timeout_secs: null,
  });
  let history = $state<HistoryEntry[]>([]);
  let viewingHistoryTs = $state<number | null>(null);
  let historyOpen = $state(false);
  let diffOpen = $state(false);
  let diffPair = $state<{ a: HistoryEntry; b: HistoryEntry } | null>(null);
  let diffPickA = $state<number | null>(null);

  function pickHistoryForDiff(entry: HistoryEntry) {
    if (diffPickA === null) {
      diffPickA = entry.ts;
      showToast("pick a second run to compare");
    } else if (diffPickA === entry.ts) {
      diffPickA = null;
    } else {
      const a = history.find((h) => h.ts === diffPickA);
      const b = entry;
      if (a) {
        diffPair = { a, b };
        diffOpen = true;
      }
      diffPickA = null;
    }
  }

  function lineDiff(a: string, b: string): Array<{ type: "same" | "add" | "del"; text: string }> {
    const aLines = a.split("\n");
    const bLines = b.split("\n");
    const max = Math.max(aLines.length, bLines.length);
    const out: Array<{ type: "same" | "add" | "del"; text: string }> = [];
    for (let i = 0; i < max; i++) {
      if (aLines[i] === bLines[i]) {
        out.push({ type: "same", text: aLines[i] ?? "" });
      } else {
        if (aLines[i] !== undefined) out.push({ type: "del", text: aLines[i] });
        if (bLines[i] !== undefined) out.push({ type: "add", text: bLines[i] });
      }
    }
    return out;
  }

  function prettyForDiff(entry: HistoryEntry): string {
    const ct = entry.response.headers.find(([k]) => k.toLowerCase() === "content-type")?.[1] ?? "";
    if (ct.includes("json")) {
      try {
        return JSON.stringify(JSON.parse(entry.response.body), null, 2);
      } catch {
        return entry.response.body;
      }
    }
    return entry.response.body;
  }
  let envs = $state<EnvFile[]>([]);
  let activeEnvPath = $state<string | null>(null);
  let activeEnvPairs = $state<EnvPair[]>([]);
  let globalEnvPath = $state<string | null>(null);
  let globalEnvPairs = $state<EnvPair[]>([]);
  let envEditorOpen = $state(false);
  let lastResolved = $state<string[]>([]);
  let lastUnresolved = $state<string[]>([]);
  let generateOpen = $state(false);
  let aiReview = $state<AiReview | null>(null);
  let aiReviewBusy = $state(false);

  type DiffRow = {
    envName: string;
    status: number | null;
    statusText: string;
    elapsedMs: number | null;
    bodyKeys: string[];
    error: string | null;
  };

  let diffRows = $state<DiffRow[] | null>(null);
  let diffRunning = $state(false);

  type FolderRunRow = {
    name: string;
    method: string;
    url: string;
    status: number | null;
    elapsedMs: number | null;
    passed: number;
    total: number;
    error: string | null;
  };

  let folderRunRows = $state<FolderRunRow[] | null>(null);
  let folderRunning = $state(false);

  type LoadStats = {
    total: number;
    completed: number;
    success: number;
    p50: number;
    p95: number;
    p99: number;
    min: number;
    max: number;
    statusBuckets: Record<string, number>;
    errors: number;
  };

  let loadConfig = $state({ runs: 100, concurrency: 10 });
  let loadOpen = $state(false);
  let loadStats = $state<LoadStats | null>(null);
  let loadRunning = $state(false);
  let loadCancel = $state(false);

  function percentile(sorted: number[], p: number): number {
    if (sorted.length === 0) return 0;
    const idx = Math.min(sorted.length - 1, Math.floor((p / 100) * sorted.length));
    return sorted[idx];
  }

  async function runLoadTest() {
    if (!parsed) return;
    loadRunning = true;
    loadCancel = false;
    const total = Math.max(1, Math.floor(loadConfig.runs));
    const concurrency = Math.max(1, Math.floor(loadConfig.concurrency));
    const sub = substitute(parsed, envVarsMap());
    const aws = awsParamsFromEnv();
    const digest = digestCredsFromEnv();

    const elapsed: number[] = [];
    const buckets: Record<string, number> = {};
    let completed = 0;
    let success = 0;
    let errors = 0;

    let inFlight = 0;
    let issued = 0;
    const updateStats = () => {
      const sorted = [...elapsed].sort((a, b) => a - b);
      loadStats = {
        total,
        completed,
        success,
        p50: percentile(sorted, 50),
        p95: percentile(sorted, 95),
        p99: percentile(sorted, 99),
        min: sorted[0] ?? 0,
        max: sorted[sorted.length - 1] ?? 0,
        statusBuckets: { ...buckets },
        errors,
      };
    };

    const worker = async () => {
      while (issued < total && !loadCancel) {
        const _myIdx = issued;
        issued++;
        inFlight++;
        try {
          const result = await runRequest(sub.spec, aws, digest);
          elapsed.push(result.elapsed_ms);
          const bucket = `${Math.floor(result.status / 100)}xx`;
          buckets[bucket] = (buckets[bucket] ?? 0) + 1;
          if (result.status >= 200 && result.status < 400) success++;
        } catch {
          errors++;
        }
        inFlight--;
        completed++;
        if (completed % Math.max(1, Math.floor(total / 50)) === 0 || completed === total) {
          updateStats();
        }
      }
    };

    updateStats();
    await Promise.all(Array.from({ length: concurrency }, () => worker()));
    updateStats();
    loadRunning = false;
  }

  type DataRunRow = {
    rowIndex: number;
    rowSummary: string;
    status: number | null;
    elapsedMs: number | null;
    passed: number;
    total: number;
    error: string | null;
  };

  let dataRunRows = $state<DataRunRow[] | null>(null);
  let dataRunning = $state(false);
  let cookies = $state<CookieView[]>([]);
  let cookiesOpen = $state(false);
  let assertionResults = $state<AssertionResult[]>([]);
  let extractResults = $state<ExtractResult[]>([]);
  let oauthOpen = $state(false);
  let oauthBusy = $state(false);
  let oauthError = $state<string | null>(null);
  let mockStatus = $state<MockStatus>({ running: false, port: null });
  let mockPort = $state("8787");
  let renderHtml = $state(false);
  let renderTable = $state(false);
  let renderTree = $state(false);

  function tryJsonValue(body: string): unknown {
    try {
      return JSON.parse(body);
    } catch {
      return undefined;
    }
  }

  function pathToExpr(path: Array<string | number>): string {
    let out = "body";
    for (const seg of path) {
      if (typeof seg === "number") out += `[${seg}]`;
      else if (/^[a-zA-Z_$][\w$]*$/.test(seg)) out += `.${seg}`;
      else out += `[${JSON.stringify(seg)}]`;
    }
    return out;
  }

  async function extractFromTree(value: unknown, path: Array<string | number>) {
    const suggested = path.length > 0 ? String(path[path.length - 1]) : "value";
    const varName = prompt(`save as env var:`, suggested);
    if (!varName) return;
    const valueStr = typeof value === "string" ? value : JSON.stringify(value);
    await setEnvVar(varName, valueStr);
    showToast(`saved ${varName} = ${valueStr.slice(0, 40)}${valueStr.length > 40 ? "…" : ""}`);
  }

  async function copyPath(path: Array<string | number>) {
    const expr = pathToExpr(path);
    try {
      await navigator.clipboard.writeText(expr);
      showToast(`copied: ${expr}`);
    } catch {
      showToast(expr);
    }
  }

  function tryJsonArray(body: string): Array<Record<string, unknown>> | null {
    try {
      const v = JSON.parse(body);
      if (
        Array.isArray(v) &&
        v.length > 0 &&
        v.every((x) => x && typeof x === "object" && !Array.isArray(x))
      ) {
        return v as Array<Record<string, unknown>>;
      }
      return null;
    } catch {
      return null;
    }
  }

  function jsonTable(rows: Array<Record<string, unknown>>): {
    columns: string[];
    rows: string[][];
  } {
    const cols = new Set<string>();
    for (const r of rows) for (const k of Object.keys(r)) cols.add(k);
    const columns = [...cols];
    const out = rows.map((r) =>
      columns.map((c) => {
        const v = r[c];
        if (v === null || v === undefined) return "";
        if (typeof v === "object") return JSON.stringify(v);
        return String(v);
      }),
    );
    return { columns, rows: out };
  }

  let assertions = $derived(parseAssertions(editorText));
  let extracts = $derived(parseExtracts(editorText));
  let dataset = $derived<Dataset | null>(parseDataset(editorText));
  let scriptBlocks = $derived(parseScriptBlocks(editorText));
  let scriptLogs = $state<string[]>([]);
  let pmTests = $state<PmTestResult[]>([]);

  let monitorIntervalSecs = $state<number | null>(null);
  let activeMonitors = $state<Set<string>>(new Set());

  type MonitorEventPayload = {
    path: string;
    ts_ms: number;
    status: number | null;
    elapsed_ms: number | null;
    ok: boolean;
    error: string | null;
  };

  let lastMonitorEvent = $state<MonitorEventPayload | null>(null);

  $effect(() => {
    if (!editorText) {
      monitorIntervalSecs = null;
      return;
    }
    void monitorParseSchedule(editorText).then((sec) => {
      monitorIntervalSecs = sec;
    });
  });

  let monitoring = $derived(activePath !== null && activeMonitors.has(activePath));

  let graphqlSchema = $state<unknown>(null);
  let graphqlSchemaOpen = $state(false);
  let graphqlBusy = $state(false);

  async function introspectGraphql() {
    if (!parsed) return;
    graphqlBusy = true;
    try {
      const sub = substitute(parsed, envVarsMap());
      graphqlSchema = await graphqlIntrospect(sub.spec.url, sub.spec.headers);
      graphqlSchemaOpen = true;
    } catch (err) {
      showToast(`introspect: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      graphqlBusy = false;
    }
  }

  function summarizeGraphqlSchema(schema: unknown): { types: string[]; queries: string[]; mutations: string[] } {
    const out = { types: [] as string[], queries: [] as string[], mutations: [] as string[] };
    const root = (schema as { data?: { __schema?: unknown } } | null)?.data?.__schema as
      | { types?: Array<{ kind?: string; name?: string; fields?: Array<{ name?: string }> }>; queryType?: { name?: string }; mutationType?: { name?: string } | null }
      | undefined;
    if (!root) return out;
    const queryTypeName = root.queryType?.name;
    const mutationTypeName = root.mutationType?.name;
    for (const t of root.types ?? []) {
      if (!t.name || t.name.startsWith("__")) continue;
      if (t.kind === "OBJECT" || t.kind === "INTERFACE" || t.kind === "INPUT_OBJECT" || t.kind === "ENUM" || t.kind === "SCALAR" || t.kind === "UNION") {
        if (t.name === queryTypeName) {
          for (const f of t.fields ?? []) if (f.name) out.queries.push(f.name);
        } else if (t.name === mutationTypeName) {
          for (const f of t.fields ?? []) if (f.name) out.mutations.push(f.name);
        } else {
          out.types.push(`${t.kind?.toLowerCase()} ${t.name}`);
        }
      }
    }
    return out;
  }

  let isWebSocket = $derived(
    parsed ? /^wss?:\/\//i.test(parsed.url) : false,
  );

  type WsMessage = { direction: "in" | "out"; text: string; ts: number };
  type WsStatus = "idle" | "connecting" | "connected" | "closed" | "error";

  let wsId = $state<string | null>(null);
  let wsMessages = $state<WsMessage[]>([]);
  let wsStatus = $state<WsStatus>("idle");
  let wsSendDraft = $state("");
  let wsError = $state<string | null>(null);

  type WsEventPayload =
    | { kind: "Connected"; id: string }
    | { kind: "Message"; id: string; direction: "in" | "out"; text: string; ts_ms: number }
    | { kind: "Closed"; id: string; reason: string | null }
    | { kind: "Error"; id: string; message: string };

  let paramsOpen = $state(false);
  let urlParams = $derived.by((): Array<[string, string]> => {
    if (!parsed) return [];
    try {
      const u = new URL(parsed.url);
      return [...u.searchParams.entries()];
    } catch {
      return [];
    }
  });

  function setUrlParam(idx: number, key: string, value: string) {
    if (!parsed) return;
    let newUrl: string;
    try {
      const u = new URL(parsed.url);
      const entries = [...u.searchParams.entries()];
      if (idx >= entries.length) entries.push([key, value]);
      else entries[idx] = [key, value];
      u.search = "";
      for (const [k, v] of entries) {
        if (k.trim()) u.searchParams.append(k, v);
      }
      newUrl = u.toString();
    } catch {
      return;
    }
    rewriteUrl(newUrl);
  }

  function deleteUrlParam(idx: number) {
    if (!parsed) return;
    try {
      const u = new URL(parsed.url);
      const entries = [...u.searchParams.entries()];
      entries.splice(idx, 1);
      u.search = "";
      for (const [k, v] of entries) u.searchParams.append(k, v);
      rewriteUrl(u.toString());
    } catch {
      // ignore
    }
  }

  function addUrlParam() {
    if (!parsed) return;
    try {
      const u = new URL(parsed.url);
      u.searchParams.append("key", "value");
      rewriteUrl(u.toString());
    } catch {
      // ignore
    }
  }

  function rewriteUrl(newUrl: string) {
    if (!parsed) return;
    const lines = editorText.split(/\r?\n/);
    for (let i = 0; i < lines.length; i++) {
      const m = lines[i].match(/^(GET|POST|PUT|PATCH|DELETE|HEAD|OPTIONS)\s+/i);
      if (m) {
        lines[i] = `${m[1].toUpperCase()} ${newUrl}`;
        break;
      }
    }
    editorText = lines.join("\n");
    parsed = { ...parsed, url: newUrl };
  }

  let awsActive = $derived(awsParamsFromEnv() !== null);

  let isGraphql = $derived.by(() => {
    if (!parsed) return false;
    if (parsed.method !== "POST") return false;
    const ct = parsed.headers.find(([k]) => k.toLowerCase() === "content-type")?.[1] ?? "";
    if (ct.includes("application/graphql")) return true;
    if (parsed.body) {
      const trimmed = parsed.body.trim();
      if (trimmed.startsWith("{") && /"query"\s*:/.test(trimmed)) return true;
      if (/^(query|mutation|subscription)\b/i.test(trimmed)) return true;
    }
    return false;
  });

  function getEnvVar(key: string): string {
    return activeEnvPairs.find(([k]) => k === key)?.[1] ?? "";
  }

  async function persistOauthToken(tok: { access_token?: string; refresh_token?: string; expires_in?: number; [key: string]: unknown }, varName: string) {
    if (tok.access_token) {
      await setEnvVar(varName, tok.access_token);
    }
    if (tok.refresh_token) {
      await setEnvVar("__oauth_refresh_token", tok.refresh_token);
    }
    if (typeof tok.expires_in === "number" && tok.expires_in > 0) {
      const expiresAt = Math.floor(Date.now() / 1000) + tok.expires_in;
      await setEnvVar("__oauth_expires_at", String(expiresAt));
    }
  }

  let refreshCheckTimer: ReturnType<typeof setInterval> | null = null;

  async function maybeRefreshToken() {
    const m = envVarsMap();
    const refreshToken = m.get("__oauth_refresh_token");
    const expiresAt = parseInt(m.get("__oauth_expires_at") ?? "0", 10);
    const tokenUrl = m.get("__oauth_token_url");
    const clientId = m.get("__oauth_client_id");
    const clientSecret = m.get("__oauth_client_secret");
    const scope = m.get("__oauth_scope");
    const tokenVar = m.get("__oauth_var") || "accessToken";
    if (!refreshToken || !expiresAt || !tokenUrl || !clientId) return;
    const nowSec = Math.floor(Date.now() / 1000);
    if (expiresAt > nowSec + 60) return; // not yet near expiry
    try {
      const tok = await fetchOauthRefresh(tokenUrl, clientId, clientSecret || null, refreshToken, scope || null);
      await persistOauthToken(tok, tokenVar);
      showToast("OAuth token auto-refreshed");
    } catch (err) {
      // Silent — user will see next manual fetch attempt
      console.warn("auto-refresh failed:", err);
    }
  }

  async function setEnvVar(key: string, value: string) {
    const envPath = await ensureActiveEnv();
    if (!envPath) return;
    const idx = activeEnvPairs.findIndex(([k]) => k === key);
    if (idx === -1) {
      activeEnvPairs = [...activeEnvPairs, [key, value]];
    } else {
      activeEnvPairs = activeEnvPairs.map((p, i) => (i === idx ? [p[0], value] : p));
    }
    await writeEnv(envPath, activeEnvPairs);
  }

  let theme = $state<"dark" | "light">(
    typeof window !== "undefined" && localStorage.getItem("dante.theme") === "light"
      ? "light"
      : "dark",
  );

  let sidebarWidth = $state(
    typeof window !== "undefined"
      ? parseInt(localStorage.getItem("dante.sidebarWidth") ?? "240", 10) || 240
      : 240,
  );

  let splitVertical = $state(
    typeof window !== "undefined" && localStorage.getItem("dante.splitVertical") === "true",
  );

  $effect(() => {
    if (typeof localStorage !== "undefined") {
      try { localStorage.setItem("dante.sidebarWidth", String(sidebarWidth)); } catch { /* */ }
    }
  });

  $effect(() => {
    if (typeof localStorage !== "undefined") {
      try { localStorage.setItem("dante.splitVertical", String(splitVertical)); } catch { /* */ }
    }
  });

  let resizing = $state(false);

  function startSidebarResize(ev: MouseEvent) {
    ev.preventDefault();
    resizing = true;
    const startX = ev.clientX;
    const startWidth = sidebarWidth;
    const onMove = (e: MouseEvent) => {
      const dx = e.clientX - startX;
      sidebarWidth = Math.max(160, Math.min(600, startWidth + dx));
    };
    const onUp = () => {
      resizing = false;
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
    };
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
  }

  $effect(() => {
    if (typeof document !== "undefined") {
      document.documentElement.classList.toggle("light", theme === "light");
      try { localStorage.setItem("dante.theme", theme); } catch { /* ignore */ }
    }
  });

  function toggleTheme() {
    theme = theme === "dark" ? "light" : "dark";
  }

  let toast = $state<string | null>(null);
  let toastTimer: ReturnType<typeof setTimeout> | null = null;

  let clipboardCurl = $state<string | null>(null);
  let clipboardDismissed = new Set<string>();

  async function checkClipboardForCurl() {
    if (editorText.trim() !== "" && editorText !== SAMPLE_CURL) return;
    try {
      const text = await navigator.clipboard.readText();
      if (!text) return;
      const trimmed = text.trim();
      if (clipboardDismissed.has(trimmed)) return;
      if (looksLikeCurl(trimmed) && trimmed.length < 8000) {
        clipboardCurl = trimmed;
      }
    } catch {
      // permission denied or no clipboard text — fine
    }
  }

  function applyClipboardCurl() {
    if (!clipboardCurl) return;
    activePath = null;
    lastSavedContent = "";
    editorText = clipboardCurl;
    clipboardCurl = null;
  }

  function dismissClipboardCurl() {
    if (clipboardCurl) clipboardDismissed.add(clipboardCurl);
    clipboardCurl = null;
  }

  function showToast(msg: string) {
    toast = msg;
    if (toastTimer) clearTimeout(toastTimer);
    toastTimer = setTimeout(() => (toast = null), 3000);
  }

  async function attachFile() {
    const path = await openDialog({ multiple: false });
    if (typeof path !== "string") return;
    const lines = editorText.split(/\r?\n/);
    let i = 0;
    while (i < lines.length && (lines[i].trim() === "" || lines[i].startsWith("#"))) i++;
    if (i < lines.length) i++; // request line
    while (i < lines.length && lines[i].trim() !== "") i++; // headers
    while (i < lines.length && lines[i].trim() === "") i++; // blank lines
    const before = lines.slice(0, i);
    const after = lines.slice(i);
    const insert = [`@${path}`];
    const newLines = [...before, ...insert, ...after];
    editorText = newLines.join("\n");
    showToast(`@${path.split(/[\\/]/).pop()} attached as body`);
  }

  async function importSpec() {
    if (!folder) return;
    const path = await openDialog({
      filters: [
        { name: "OpenAPI / Postman / Insomnia / HAR", extensions: ["json", "yaml", "yml", "har"] },
      ],
      multiple: false,
    });
    if (typeof path !== "string") return;
    try {
      const kind = await detectImportKind(path);
      let result: ImportResult;
      switch (kind) {
        case "postman":
          result = await importPostman(folder, path);
          break;
        case "har":
          result = await importHar(folder, path);
          break;
        case "insomnia":
          result = await importInsomnia(folder, path);
          break;
        default:
          result = await importOpenapi(folder, path);
      }
      requests = await listRequests(folder);
      showToast(`imported ${result.created.length} request${result.created.length === 1 ? "" : "s"} (${kind})`);
    } catch (err) {
      showToast(`import failed: ${err instanceof Error ? err.message : String(err)}`);
    }
  }

  async function detectImportKind(
    path: string,
  ): Promise<"postman" | "har" | "insomnia" | "openapi"> {
    if (path.endsWith(".har")) return "har";
    if (path.endsWith(".yaml") || path.endsWith(".yml")) return "openapi";
    try {
      const head = (await loadRequest(path)).slice(0, 4000);
      if (/"_postman_id"|"info"\s*:\s*\{[^}]*"schema"[^}]*postman/i.test(head)) return "postman";
      if (/"_type"\s*:\s*"export"|"__export_format"/i.test(head)) return "insomnia";
      if (/"log"\s*:\s*\{[^}]*"version"|"creator"\s*:\s*\{[^}]*"name"/i.test(head)) return "har";
      return "openapi";
    } catch {
      return "openapi";
    }
  }

  async function exportDocs() {
    if (!folder) return;
    try {
      const path = await exportMarkdown(folder);
      showToast(`docs written to ${path.split(/[\\/]/).pop()}`);
    } catch (err) {
      showToast(`export failed: ${err instanceof Error ? err.message : String(err)}`);
    }
  }

  async function exportSpec() {
    if (!folder) return;
    try {
      const path = await exportOpenapi(folder);
      showToast(`openapi written to ${path.split(/[\\/]/).pop()}`);
    } catch (err) {
      showToast(`export failed: ${err instanceof Error ? err.message : String(err)}`);
    }
  }

  function digestCredsFromEnv(): DigestCreds | null {
    const m = envVarsMap();
    const u = m.get("__digest_user");
    const p = m.get("__digest_pass");
    if (!u || !p) return null;
    return { username: u, password: p };
  }

  let digestActive = $derived(digestCredsFromEnv() !== null);

  type AiProviderPreset = {
    label: string;
    baseUrl: string;
    defaultModel: string;
    supportsJsonMode: boolean;
    freeTier: boolean;
  };

  const AI_PROVIDERS: Record<string, AiProviderPreset> = {
    xai: {
      label: "xAI Grok",
      baseUrl: "https://api.x.ai/v1",
      defaultModel: "grok-2-1212",
      supportsJsonMode: true,
      freeTier: true,
    },
    groq: {
      label: "Groq",
      baseUrl: "https://api.groq.com/openai/v1",
      defaultModel: "llama-3.3-70b-versatile",
      supportsJsonMode: true,
      freeTier: true,
    },
    gemini: {
      label: "Google Gemini",
      baseUrl: "https://generativelanguage.googleapis.com/v1beta/openai",
      defaultModel: "gemini-2.0-flash",
      supportsJsonMode: false,
      freeTier: true,
    },
    openai: {
      label: "OpenAI",
      baseUrl: "https://api.openai.com/v1",
      defaultModel: "gpt-4o-mini",
      supportsJsonMode: true,
      freeTier: false,
    },
    openrouter: {
      label: "OpenRouter",
      baseUrl: "https://openrouter.ai/api/v1",
      defaultModel: "meta-llama/llama-3.3-70b-instruct:free",
      supportsJsonMode: false,
      freeTier: true,
    },
  };

  function pickAiBackend(): {
    kind: "claude" | "openai-compat";
    apiKey: string;
    baseUrl?: string;
    model?: string;
    supportsJsonMode?: boolean;
    label?: string;
  } | null {
    const m = envVarsMap();
    const claudeKey = m.get("__claude_api_key");
    if (claudeKey) {
      return { kind: "claude", apiKey: claudeKey, label: "Claude Sonnet" };
    }
    const aiKey = m.get("__ai_api_key");
    if (!aiKey) return null;

    const providerName = (m.get("__ai_provider") ?? "").toLowerCase();
    const preset = AI_PROVIDERS[providerName];
    const baseUrl =
      m.get("__ai_base_url") ?? preset?.baseUrl ?? null;
    const model =
      m.get("__ai_model") ?? preset?.defaultModel ?? null;
    if (!baseUrl || !model) return null;
    const supportsJsonMode = preset?.supportsJsonMode ?? false;
    const label = preset?.label ?? providerName ?? "OpenAI-compat";

    return {
      kind: "openai-compat",
      apiKey: aiKey,
      baseUrl,
      model,
      supportsJsonMode,
      label,
    };
  }

  let aiBackend = $derived(pickAiBackend());
  let aiKeyAvailable = $derived(aiBackend !== null);

  async function runAiReview() {
    if (!parsed || !aiBackend) {
      showToast("set __claude_api_key OR __ai_api_key + __ai_provider to enable AI review");
      return;
    }
    aiReviewBusy = true;
    try {
      if (aiBackend.kind === "claude") {
        aiReview = await aiReviewRequest(
          aiBackend.apiKey,
          parsed.method,
          parsed.url,
          parsed.headers,
          parsed.body,
        );
      } else {
        aiReview = await aiReviewRequestOpenaiCompat(
          aiBackend.baseUrl!,
          aiBackend.apiKey,
          aiBackend.model!,
          aiBackend.supportsJsonMode!,
          parsed.method,
          parsed.url,
          parsed.headers,
          parsed.body,
        );
      }
    } catch (err) {
      showToast(`AI review (${aiBackend.label}): ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      aiReviewBusy = false;
    }
  }

  function appendToBlock(blockHeader: string, lines: string[]) {
    if (lines.length === 0) return;
    const allLines = editorText.split(/\r?\n/);
    const headerLower = blockHeader.toLowerCase();
    let blockIdx = allLines.findIndex((l) => l.trim().toLowerCase() === headerLower);
    if (blockIdx === -1) {
      // Append the block at end
      if (allLines.length > 0 && allLines[allLines.length - 1].trim() !== "") {
        allLines.push("");
      }
      allLines.push(blockHeader);
      for (const line of lines) allLines.push(line);
    } else {
      let insertIdx = blockIdx + 1;
      while (
        insertIdx < allLines.length &&
        !allLines[insertIdx].trim().startsWith("###")
      ) {
        insertIdx++;
      }
      while (
        insertIdx > blockIdx + 1 &&
        allLines[insertIdx - 1].trim() === ""
      ) {
        insertIdx--;
      }
      allLines.splice(insertIdx, 0, ...lines);
    }
    editorText = allLines.join("\n");
  }

  function addAiTest(line: string) {
    appendToBlock("### tests", [line]);
  }

  function addAiTests(lines: string[]) {
    appendToBlock("### tests", lines);
  }

  function addAiExtract(varName: string, source: string) {
    appendToBlock("### extract", [`${varName} = ${source}`]);
  }

  function addAiExtracts(entries: Array<{ var_name: string; source: string }>) {
    appendToBlock(
      "### extract",
      entries.map((e) => `${e.var_name} = ${e.source}`),
    );
  }

  async function runWithData() {
    if (!parsed || !dataset) return;
    dataRunning = true;
    dataRunRows = [];
    const aws = awsParamsFromEnv();
    const digest = digestCredsFromEnv();
    const baseVars = envVarsMap();

    const rows: DataRunRow[] = [];
    for (let i = 0; i < dataset.rows.length; i++) {
      const rowVars = rowToVars(dataset, i);
      const merged = new Map<string, string>(baseVars);
      for (const [k, v] of rowVars) merged.set(k, v);
      const sub = substitute(parsed, merged);

      const summary = dataset.columns
        .map((c, idx) => `${c}=${dataset.rows[i][idx] ?? ""}`)
        .slice(0, 3)
        .join(", ");

      try {
        const result = await runRequest(sub.spec, aws, digest);
        const tests = evaluateAssertions(assertions, result);
        const passed = tests.filter((t) => t.pass).length;
        rows.push({
          rowIndex: i,
          rowSummary: summary,
          status: result.status,
          elapsedMs: result.elapsed_ms,
          passed,
          total: tests.length,
          error: null,
        });
      } catch (err) {
        rows.push({
          rowIndex: i,
          rowSummary: summary,
          status: null,
          elapsedMs: null,
          passed: 0,
          total: 0,
          error: err instanceof Error ? err.message : String(err),
        });
      }
      dataRunRows = [...rows];
    }
    dataRunning = false;
  }

  async function runFolderChained(folderName: string) {
    if (!folder) return;
    const entries = requests
      .filter((r) => (r.folder ?? "") === folderName)
      .slice()
      .sort((a, b) => a.name.localeCompare(b.name));
    if (entries.length === 0) return;
    folderRunning = true;
    folderRunRows = [];
    const aws = awsParamsFromEnv();
    const digest = digestCredsFromEnv();
    const chainEnv = new Map(envVarsMap());

    const rows: FolderRunRow[] = [];
    for (const entry of entries) {
      try {
        const content = await loadRequest(entry.path);
        const spec = parseHttpFile(content);
        if (!spec) {
          rows.push({
            name: entry.name,
            method: entry.method,
            url: entry.url,
            status: null,
            elapsedMs: null,
            passed: 0,
            total: 0,
            error: "could not parse",
          });
          folderRunRows = [...rows];
          continue;
        }

        const blocks = parseScriptBlocks(content);
        let workingSpec = spec;
        if (blocks.preScript) {
          const outcome = await runScript({
            script: blocks.preScript,
            env: [...chainEnv.entries()],
            method: spec.method,
            url: spec.url,
            headers: spec.headers,
            body: spec.body,
            response: null,
          });
          if (!outcome.error) {
            for (const [k, v] of outcome.env) chainEnv.set(k, v);
            workingSpec = { ...workingSpec, headers: outcome.headers };
          }
        }

        const sub = substitute(workingSpec, chainEnv);
        const result = await runRequest(sub.spec, aws, digest);
        const localAssertions = parseAssertions(content);
        const tests = evaluateAssertions(localAssertions, result);
        const passed = tests.filter((t) => t.pass).length;

        // Apply extracts to the chain env
        const localExtracts = parseExtracts(content);
        const extractRes = applyExtracts(localExtracts, result);
        for (const r of extractRes) {
          if (r.value !== null) chainEnv.set(r.rule.varName, r.value);
        }

        // Post-script
        if (blocks.postScript) {
          const outcome = await runScript({
            script: blocks.postScript,
            env: [...chainEnv.entries()],
            method: sub.spec.method,
            url: sub.spec.url,
            headers: sub.spec.headers,
            body: sub.spec.body,
            response: {
              status: result.status,
              status_text: result.status_text,
              headers: result.headers,
              body: result.body,
              elapsed_ms: result.elapsed_ms,
            },
          });
          if (!outcome.error) {
            for (const [k, v] of outcome.env) chainEnv.set(k, v);
          }
        }

        rows.push({
          name: entry.name,
          method: entry.method,
          url: entry.url,
          status: result.status,
          elapsedMs: result.elapsed_ms,
          passed,
          total: tests.length,
          error: null,
        });

        // If status >= 500 or critical assertion failure, abort chain
        if (result.status >= 500 || (tests.length > 0 && passed === 0)) {
          showToast(`chain aborted at ${entry.name} (${result.status})`);
          folderRunRows = [...rows];
          break;
        }
      } catch (err) {
        rows.push({
          name: entry.name,
          method: entry.method,
          url: entry.url,
          status: null,
          elapsedMs: null,
          passed: 0,
          total: 0,
          error: err instanceof Error ? err.message : String(err),
        });
      }
      folderRunRows = [...rows];
    }
    // Persist any new vars from the chain back to active env
    if (activeEnvPath) {
      let pairs = activeEnvPairs.slice();
      let changed = false;
      for (const [k, v] of chainEnv.entries()) {
        if (k.startsWith("__")) continue; // skip internal
        const idx = pairs.findIndex(([key]) => key === k);
        if (idx >= 0) {
          if (pairs[idx][1] !== v) {
            pairs[idx] = [k, v];
            changed = true;
          }
        } else {
          pairs.push([k, v]);
          changed = true;
        }
      }
      if (changed) {
        activeEnvPairs = pairs;
        await writeEnv(activeEnvPath, pairs);
      }
    }
    folderRunning = false;
  }

  async function runFolder(folderName: string) {
    if (!folder) return;
    const entries = requests.filter((r) => (r.folder ?? "") === folderName);
    if (entries.length === 0) return;
    folderRunning = true;
    folderRunRows = [];
    const aws = awsParamsFromEnv();
    const digest = digestCredsFromEnv();
    const vars = envVarsMap();

    const rows: FolderRunRow[] = [];
    for (const entry of entries) {
      try {
        const content = await loadRequest(entry.path);
        const spec = parseHttpFile(content);
        if (!spec) {
          rows.push({
            name: entry.name,
            method: entry.method,
            url: entry.url,
            status: null,
            elapsedMs: null,
            passed: 0,
            total: 0,
            error: "could not parse",
          });
          folderRunRows = [...rows];
          continue;
        }
        const sub = substitute(spec, vars);
        const result = await runRequest(sub.spec, aws, digest);
        const localAssertions = parseAssertions(content);
        const tests = evaluateAssertions(localAssertions, result);
        const passed = tests.filter((t) => t.pass).length;
        rows.push({
          name: entry.name,
          method: entry.method,
          url: entry.url,
          status: result.status,
          elapsedMs: result.elapsed_ms,
          passed,
          total: tests.length,
          error: null,
        });
      } catch (err) {
        rows.push({
          name: entry.name,
          method: entry.method,
          url: entry.url,
          status: null,
          elapsedMs: null,
          passed: 0,
          total: 0,
          error: err instanceof Error ? err.message : String(err),
        });
      }
      folderRunRows = [...rows];
    }
    folderRunning = false;
  }

  async function runAcrossEnvs() {
    if (!parsed || !folder) return;
    diffRunning = true;
    diffRows = [];
    type Cand = { name: string; pairs: EnvPair[] };
    const candidates: Cand[] = [];
    if (activeEnvPath) {
      const active = envs.find((e) => e.path === activeEnvPath);
      candidates.push({ name: active?.name ?? "(active)", pairs: activeEnvPairs });
    } else {
      candidates.push({ name: "(no env)", pairs: [] });
    }
    for (const e of envs) {
      if (e.path === activeEnvPath) continue;
      try {
        const pairs = await readEnv(e.path);
        candidates.push({ name: e.name, pairs });
      } catch {
        // skip unreadable
      }
    }

    const rows: DiffRow[] = [];
    for (const c of candidates) {
      const m = new Map<string, string>(globalEnvPairs);
      for (const [k, v] of c.pairs) m.set(k, v);
      const sub = substitute(parsed, m);
      try {
        const result = await runRequest(sub.spec, awsParamsFromEnv(), digestCredsFromEnv());
        rows.push({
          envName: c.name,
          status: result.status,
          statusText: result.status_text,
          elapsedMs: result.elapsed_ms,
          bodyKeys: jsonShape(result.headers, result.body) ?? [],
          error: null,
        });
      } catch (err) {
        rows.push({
          envName: c.name,
          status: null,
          statusText: "",
          elapsedMs: null,
          bodyKeys: [],
          error: err instanceof Error ? err.message : String(err),
        });
      }
      diffRows = [...rows];
    }
    diffRunning = false;
  }

  function awsParamsFromEnv(): AwsParams | null {
    const m = envVarsMap();
    const access = m.get("__aws_access_key_id");
    const secret = m.get("__aws_secret_access_key");
    const region = m.get("__aws_region");
    if (!access || !secret || !region) return null;
    let service = m.get("__aws_service") ?? "";
    if (!service && parsed) {
      const host = hostOf(parsed.url);
      const m2 = host.match(/^(?:[\w-]+\.)?([\w-]+)\.amazonaws\.com$/);
      if (m2) service = m2[1];
    }
    if (!service) return null;
    return {
      access_key: access,
      secret_key: secret,
      region,
      service,
      session_token: m.get("__aws_session_token") || null,
    };
  }

  async function toggleMockServer() {
    if (!folder) return;
    try {
      if (mockStatus.running) {
        mockStatus = await stopMockServer();
        showToast("mock server stopped");
      } else {
        const port = parseInt(mockPort, 10);
        if (!port || port < 1 || port > 65535) {
          showToast("invalid port");
          return;
        }
        mockStatus = await startMockServer(folder, port);
        showToast(`mock server on http://localhost:${mockStatus.port}`);
      }
    } catch (err) {
      showToast(`mock server: ${err instanceof Error ? err.message : String(err)}`);
    }
  }

  async function runOauthFetch() {
    const tokenUrl = getEnvVar("__oauth_token_url");
    const clientId = getEnvVar("__oauth_client_id");
    const clientSecret = getEnvVar("__oauth_client_secret");
    const scope = getEnvVar("__oauth_scope");
    const tokenVar = getEnvVar("__oauth_var") || "accessToken";
    if (!tokenUrl || !clientId || !clientSecret) {
      oauthError = "missing __oauth_token_url, __oauth_client_id, or __oauth_client_secret";
      return;
    }
    oauthBusy = true;
    oauthError = null;
    try {
      const tok = await fetchOauthToken(tokenUrl, clientId, clientSecret, scope || null);
      const access = tok.access_token;
      if (!access) {
        oauthError = "response missing access_token: " + JSON.stringify(tok).slice(0, 100);
        return;
      }
      await persistOauthToken(tok, tokenVar);
    } catch (err) {
      oauthError = err instanceof Error ? err.message : String(err);
    } finally {
      oauthBusy = false;
    }
  }

  async function runOauthDevice() {
    const tokenUrl = getEnvVar("__oauth_token_url");
    const clientId = getEnvVar("__oauth_client_id");
    const clientSecret = getEnvVar("__oauth_client_secret");
    const deviceUrl = getEnvVar("__oauth_device_url");
    const scope = getEnvVar("__oauth_scope");
    const tokenVar = getEnvVar("__oauth_var") || "accessToken";
    if (!deviceUrl || !tokenUrl || !clientId) {
      oauthError = "missing __oauth_device_url, __oauth_token_url, or __oauth_client_id";
      return;
    }
    oauthBusy = true;
    oauthError = null;
    try {
      const init = await oauthDeviceInit(deviceUrl, clientId, scope || null);
      showToast(
        `enter code ${init.user_code} at ${init.verification_uri}`,
      );
      const tok = await oauthDevicePoll(
        tokenUrl,
        clientId,
        clientSecret || null,
        init.device_code,
        init.interval,
        init.expires_in,
      );
      const access = tok.access_token;
      if (!access) {
        oauthError = "device flow returned no access_token";
        return;
      }
      await persistOauthToken(tok, tokenVar);
      showToast("OAuth device authorized");
    } catch (err) {
      oauthError = err instanceof Error ? err.message : String(err);
    } finally {
      oauthBusy = false;
    }
  }

  async function runOauthPassword() {
    const tokenUrl = getEnvVar("__oauth_token_url");
    const clientId = getEnvVar("__oauth_client_id");
    const clientSecret = getEnvVar("__oauth_client_secret");
    const username = getEnvVar("__oauth_username");
    const password = getEnvVar("__oauth_password");
    const scope = getEnvVar("__oauth_scope");
    const tokenVar = getEnvVar("__oauth_var") || "accessToken";
    if (!tokenUrl || !clientId || !username || !password) {
      oauthError = "missing __oauth_token_url, __oauth_client_id, __oauth_username, or __oauth_password";
      return;
    }
    oauthBusy = true;
    oauthError = null;
    try {
      const tok = await fetchOauthPassword(
        tokenUrl,
        clientId,
        clientSecret || null,
        username,
        password,
        scope || null,
      );
      const access = tok.access_token;
      if (!access) {
        oauthError = "response missing access_token";
        return;
      }
      await persistOauthToken(tok, tokenVar);
      showToast("OAuth password flow authorized");
    } catch (err) {
      oauthError = err instanceof Error ? err.message : String(err);
    } finally {
      oauthBusy = false;
    }
  }

  async function runOauthAuthcode() {
    const authUrl = getEnvVar("__oauth_auth_url");
    const tokenUrl = getEnvVar("__oauth_token_url");
    const clientId = getEnvVar("__oauth_client_id");
    const clientSecret = getEnvVar("__oauth_client_secret");
    const scope = getEnvVar("__oauth_scope");
    const portStr = getEnvVar("__oauth_redirect_port") || "8788";
    const tokenVar = getEnvVar("__oauth_var") || "accessToken";
    const port = parseInt(portStr, 10);
    if (!authUrl || !tokenUrl || !clientId) {
      oauthError = "missing __oauth_auth_url, __oauth_token_url, or __oauth_client_id";
      return;
    }
    if (!port || port < 1 || port > 65535) {
      oauthError = "invalid __oauth_redirect_port";
      return;
    }
    oauthBusy = true;
    oauthError = null;
    try {
      const tok = await fetchOauthAuthcode(
        authUrl,
        tokenUrl,
        clientId,
        clientSecret,
        scope || null,
        port,
      );
      const access = tok.access_token;
      if (!access) {
        oauthError = "response missing access_token: " + JSON.stringify(tok).slice(0, 100);
        return;
      }
      await persistOauthToken(tok, tokenVar);
      showToast("OAuth authorized");
    } catch (err) {
      oauthError = err instanceof Error ? err.message : String(err);
    } finally {
      oauthBusy = false;
    }
  }

  let saveTimer: ReturnType<typeof setTimeout> | null = null;
  let lastSavedContent = $state("");
  let isDirty = $derived(
    activePath !== null && editorText !== lastSavedContent && editorText.trim() !== "",
  );

  onMount(async () => {
    if (typeof window !== "undefined") {
      window.addEventListener("focus", () => {
        void checkClipboardForCurl();
      });
    }

    refreshCheckTimer = setInterval(() => {
      void maybeRefreshToken();
    }, 30_000);

    listen<MonitorEventPayload>("monitor", (event) => {
      lastMonitorEvent = event.payload;
      if (!event.payload.ok) {
        showToast(
          `monitor ✗ ${event.payload.path.split(/[\\/]/).pop()} — ${event.payload.error ?? `status ${event.payload.status}`}`,
        );
      }
    });

    try {
      const list = await monitorList();
      activeMonitors = new Set(list.map(([p]) => p));
    } catch {
      // ignore
    }

    listen<WsEventPayload>("ws", (event) => {
      const ev = event.payload;
      if (ev.id !== wsId) return;
      switch (ev.kind) {
        case "Connected":
          wsStatus = "connected";
          break;
        case "Message":
          wsMessages = [
            ...wsMessages,
            { direction: ev.direction, text: ev.text, ts: ev.ts_ms },
          ];
          break;
        case "Closed":
          wsStatus = "closed";
          if (ev.reason) wsError = `closed: ${ev.reason}`;
          break;
        case "Error":
          wsStatus = "error";
          wsError = ev.message;
          break;
      }
    });

    try {
      const settings = await getSettings();
      let f = settings.project_folder;
      if (!f) {
        f = await defaultProjectFolder();
        await saveSettings({ project_folder: f, active_env: null });
      }
      folder = f;
      requests = await listRequests(f);
      envs = await listEnvs(f);
      try {
        workspaceConfig = await readWorkspaceConfig(f);
      } catch {
        // ignore
      }
      try {
        await loadCookies(f);
        cookies = await listCookies();
      } catch {
        // cookies file might not exist yet — fine
      }
      try {
        mockStatus = await mockServerStatus();
      } catch {
        // ignore
      }
      const globalEnv = envs.find((e) => e.name === ".env.global");
      if (globalEnv) {
        globalEnvPath = globalEnv.path;
        globalEnvPairs = await readEnv(globalEnv.path);
      }
      const wantedEnv = settings.active_env ?? null;
      const found = wantedEnv ? envs.find((e) => e.path === wantedEnv) : null;
      if (found) {
        activeEnvPath = found.path;
        activeEnvPairs = await readEnv(found.path);
      }
      editorText = SAMPLE_CURL;
      bootstrapped = true;
    } catch (err) {
      parseError = `bootstrap failed: ${err instanceof Error ? err.message : String(err)}`;
      bootstrapped = true;
    }
  });

  async function selectEnv(path: string | null) {
    activeEnvPath = path;
    activeEnvPairs = path ? await readEnv(path) : [];
    if (folder) {
      await saveSettings({ project_folder: folder, active_env: path });
    }
  }

  async function addEnv() {
    if (!folder) return;
    const name = prompt("env name (e.g. staging)");
    if (!name) return;
    const path = await createEnv(folder, name);
    envs = await listEnvs(folder);
    await selectEnv(path);
    envEditorOpen = true;
  }

  function envVarsMap(): Map<string, string> {
    const m = new Map<string, string>(globalEnvPairs);
    for (const [k, v] of activeEnvPairs) m.set(k, v);
    return m;
  }

  let detectedVars = $derived(parsed ? findPlaceholders(parsed) : []);
  let findings = $derived<Finding[]>(parsed ? reviewRequest(parsed) : []);

  async function ensureActiveEnv(): Promise<string | null> {
    if (activeEnvPath) return activeEnvPath;
    if (!folder) return null;
    const path = await createEnv(folder, ".env");
    envs = await listEnvs(folder);
    activeEnvPath = path;
    activeEnvPairs = await readEnv(path);
    await saveSettings({ project_folder: folder, active_env: path });
    return path;
  }

  async function extractFinding(f: Finding) {
    if (!parsed) return;
    const envPath = await ensureActiveEnv();
    if (!envPath) return;

    const idx = activeEnvPairs.findIndex(([k]) => k === f.suggestedName);
    if (idx === -1) {
      activeEnvPairs = [...activeEnvPairs, [f.suggestedName, f.rawValue]];
    } else {
      activeEnvPairs = activeEnvPairs.map((p, i) =>
        i === idx ? [p[0], f.rawValue] : p,
      );
    }
    await writeEnv(envPath, activeEnvPairs);

    const newSpec = f.apply(parsed);
    parsed = newSpec;
    editorText = toHttpFile(newSpec);
  }

  async function extractAllFindings() {
    if (!parsed || findings.length === 0) return;
    for (const f of findings) {
      await extractFinding(f);
    }
  }

  async function saveActiveEnv() {
    if (!activeEnvPath) return;
    await writeEnv(activeEnvPath, activeEnvPairs);
  }

  function addEnvPair() {
    activeEnvPairs = [...activeEnvPairs, ["", ""]];
  }

  async function removeEnvPair(idx: number) {
    activeEnvPairs = activeEnvPairs.filter((_, i) => i !== idx);
    await saveActiveEnv();
  }

  async function clearAllCookies() {
    await clearCookies();
    if (folder) await saveCookies(folder);
    cookies = [];
  }

  async function promoteToActive(globalKey: string, globalValue: string) {
    if (!activeEnvPath) return;
    const idx = activeEnvPairs.findIndex(([k]) => k === globalKey);
    if (idx >= 0) {
      activeEnvPairs = activeEnvPairs.map((p, i) => (i === idx ? [globalKey, globalValue] : p));
    } else {
      activeEnvPairs = [...activeEnvPairs, [globalKey, globalValue]];
    }
    await writeEnv(activeEnvPath, activeEnvPairs);
    showToast(`copied ${globalKey} to active env`);
  }

  async function demoteToGlobal(idx: number) {
    if (!folder) return;
    const [k, v] = activeEnvPairs[idx];
    if (!globalEnvPath) {
      try {
        const path = await createEnv(folder, ".env.global");
        globalEnvPath = path;
        envs = await listEnvs(folder);
      } catch (err) {
        showToast(`create global env: ${err instanceof Error ? err.message : String(err)}`);
        return;
      }
    }
    const gIdx = globalEnvPairs.findIndex(([key]) => key === k);
    if (gIdx >= 0) {
      globalEnvPairs = globalEnvPairs.map((p, i) => (i === gIdx ? [k, v] : p));
    } else {
      globalEnvPairs = [...globalEnvPairs, [k, v]];
    }
    if (globalEnvPath) await writeEnv(globalEnvPath, globalEnvPairs);

    activeEnvPairs = activeEnvPairs.filter((_, i) => i !== idx);
    if (activeEnvPath) await writeEnv(activeEnvPath, activeEnvPairs);
    showToast(`moved ${k} to global env`);
  }

  async function deleteOneCookie(c: CookieView) {
    try {
      await deleteCookie(c.domain, c.path, c.name);
      if (folder) await saveCookies(folder);
      cookies = await listCookies();
    } catch (err) {
      showToast(`delete cookie: ${err instanceof Error ? err.message : String(err)}`);
    }
  }

  async function persistExtractedVars(results: ExtractResult[]) {
    if (results.length === 0) return;
    const envPath = await ensureActiveEnv();
    if (!envPath) return;
    let changed = false;
    let pairs = activeEnvPairs.slice();
    for (const r of results) {
      if (r.value === null) continue;
      const idx = pairs.findIndex(([k]) => k === r.rule.varName);
      if (idx === -1) {
        pairs = [...pairs, [r.rule.varName, r.value]];
      } else if (pairs[idx][1] !== r.value) {
        pairs = pairs.map((p, i) => (i === idx ? [p[0], r.value as string] : p));
      } else {
        continue;
      }
      changed = true;
    }
    if (changed) {
      activeEnvPairs = pairs;
      await writeEnv(envPath, pairs);
    }
  }

  $effect(() => {
    if (!bootstrapped) return;
    parseAndAutosave(editorText);
  });

  function parseAndAutosave(text: string) {
    const trimmed = text.trim();
    if (!trimmed) {
      parsed = null;
      parseError = null;
      return;
    }

    if (looksLikeCurl(trimmed)) {
      try {
        const spec = parseCurl(trimmed);
        editorText = toHttpFile(spec);
        return;
      } catch (err) {
        parsed = null;
        parseError = err instanceof Error ? err.message : String(err);
        return;
      }
    }

    const spec = parseHttpFile(trimmed);
    if (!spec) {
      parsed = null;
      parseError = "Paste a curl command, or write an .http request.";
      return;
    }
    parsed = spec;
    parseError = null;
    scheduleSave();
  }

  function parseHttpFile(text: string): RequestSpec | null {
    const lines = text.split(/\r?\n/);
    let i = 0;
    while (
      i < lines.length &&
      (lines[i].trim() === "" ||
        (lines[i].startsWith("#") && !lines[i].trim().startsWith("###")))
    ) {
      i++;
    }
    if (i >= lines.length) return null;
    if (lines[i].trim().startsWith("###")) return null;
    const requestLine = lines[i].trim();
    const match = requestLine.match(/^(GET|POST|PUT|PATCH|DELETE|HEAD|OPTIONS)\s+(\S+)/i);
    if (!match) return null;
    i++;
    const headers: Array<[string, string]> = [];
    while (i < lines.length && lines[i].trim() !== "") {
      if (lines[i].trim().startsWith("###")) break;
      const colon = lines[i].indexOf(":");
      if (colon === -1) break;
      headers.push([lines[i].slice(0, colon).trim(), lines[i].slice(colon + 1).trim()]);
      i++;
    }
    while (i < lines.length && lines[i].trim() === "") i++;
    const bodyLines: string[] = [];
    while (i < lines.length) {
      if (lines[i].trim().startsWith("###")) break;
      bodyLines.push(lines[i]);
      i++;
    }
    const body = bodyLines.join("\n").trim();
    return {
      method: match[1].toUpperCase() as RequestSpec["method"],
      url: match[2],
      headers,
      body: body || null,
    };
  }

  function scheduleSave() {
    if (!folder || !parsed) return;
    if (editorText === lastSavedContent) return;
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      void doSave();
    }, 400);
  }

  function hostOf(url: string): string {
    try {
      return new URL(url).hostname;
    } catch {
      return "";
    }
  }

  async function doSave() {
    if (!folder || !parsed) return;
    const content = editorText;
    if (content === lastSavedContent) return;
    try {
      const subfolder = activePath ? null : hostOf(parsed.url) || null;
      const path = await saveRequest(
        folder,
        autoName(parsed),
        content,
        activePath,
        subfolder,
      );
      activePath = path;
      lastSavedContent = content;
      requests = await listRequests(folder);
    } catch (err) {
      parseError = `save failed: ${err instanceof Error ? err.message : String(err)}`;
    }
  }

  function jsonShape(headers: Array<[string, string]>, body: string): string[] | null {
    const ct = headers.find(([k]) => k.toLowerCase() === "content-type")?.[1] ?? "";
    if (!ct.includes("json")) return null;
    try {
      const parsed = JSON.parse(body);
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
        return Object.keys(parsed).sort();
      }
      return null;
    } catch {
      return null;
    }
  }

  let regression = $derived.by((): string[] | null => {
    if (!response || viewingHistoryTs === null) return null;
    if (history.length < 2) return null;
    if (history[0].ts !== viewingHistoryTs) return null;
    const prev = history[1];
    if (!prev) return null;

    const issues: string[] = [];
    if (prev.response.status !== response.status) {
      issues.push(`status ${prev.response.status} → ${response.status}`);
    }
    if (
      response.elapsed_ms > prev.response.elapsed_ms * 2 &&
      response.elapsed_ms > 100
    ) {
      const factor = (response.elapsed_ms / Math.max(prev.response.elapsed_ms, 1)).toFixed(1);
      issues.push(`latency ${prev.response.elapsed_ms}ms → ${response.elapsed_ms}ms (${factor}× slower)`);
    }
    const prevKeys = jsonShape(prev.response.headers, prev.response.body);
    const curKeys = jsonShape(response.headers, response.body);
    if (prevKeys && curKeys) {
      const added = curKeys.filter((k) => !prevKeys.includes(k));
      const removed = prevKeys.filter((k) => !curKeys.includes(k));
      if (added.length > 0 || removed.length > 0) {
        const parts: string[] = [];
        if (added.length > 0) parts.push("+" + added.join(", +"));
        if (removed.length > 0) parts.push("−" + removed.join(", −"));
        issues.push(`shape: ${parts.join(" ")}`);
      }
    }
    return issues.length > 0 ? issues : null;
  });

  let searchTerm = $state("");

  let filteredRequests = $derived.by((): RequestEntry[] => {
    const q = searchTerm.trim().toLowerCase();
    if (!q) return requests;
    return requests.filter((r) => {
      return (
        r.name.toLowerCase().includes(q) ||
        r.method.toLowerCase().includes(q) ||
        r.url.toLowerCase().includes(q) ||
        r.folder.toLowerCase().includes(q) ||
        (r.description ?? "").toLowerCase().includes(q)
      );
    });
  });

  let groups = $derived.by(() => {
    const map = new Map<string, RequestEntry[]>();
    for (const r of filteredRequests) {
      const k = r.folder ?? "";
      if (!map.has(k)) map.set(k, []);
      map.get(k)!.push(r);
    }
    return [...map.entries()]
      .sort((a, b) => {
        if (a[0] === "" && b[0] !== "") return -1;
        if (b[0] === "" && a[0] !== "") return 1;
        return a[0].localeCompare(b[0]);
      })
      .map(([folder, entries]) => ({ folder, entries }));
  });

  function handlePaste(event: ClipboardEvent) {
    const pasted = event.clipboardData?.getData("text") ?? "";
    if (!pasted) return;
    if (looksLikeCurl(pasted)) {
      event.preventDefault();
      activePath = null;
      lastSavedContent = "";
      editorText = pasted;
    }
  }

  async function newRequest() {
    if (saveTimer) clearTimeout(saveTimer);
    activePath = null;
    lastSavedContent = "";
    editorText = "";
    parsed = null;
    response = null;
    runError = null;
    history = [];
    viewingHistoryTs = null;
    historyOpen = false;
  }

  async function selectRequest(entry: RequestEntry) {
    if (saveTimer) clearTimeout(saveTimer);
    try {
      const content = await loadRequest(entry.path);
      activePath = entry.path;
      lastSavedContent = content;
      editorText = content;
      runError = null;
      historyOpen = false;
      history = await readHistory(entry.path, 50);
      if (history.length > 0) {
        response = history[0].response;
        viewingHistoryTs = history[0].ts;
      } else {
        response = null;
        viewingHistoryTs = null;
      }
    } catch (err) {
      parseError = `load failed: ${err instanceof Error ? err.message : String(err)}`;
    }
  }

  async function removeRequest(entry: RequestEntry, ev: Event) {
    ev.stopPropagation();
    try {
      await deleteRequest(entry.path);
      if (activePath === entry.path) {
        await newRequest();
      }
      if (folder) requests = await listRequests(folder);
    } catch (err) {
      parseError = `delete failed: ${err instanceof Error ? err.message : String(err)}`;
    }
  }

  async function renameEntry(entry: RequestEntry, ev: Event) {
    ev.stopPropagation();
    const proposed = prompt("new name", entry.name);
    if (!proposed || proposed === entry.name) return;
    try {
      const newPath = await renameRequest(entry.path, proposed);
      if (folder) requests = await listRequests(folder);
      if (activePath === entry.path) {
        activePath = newPath;
      }
      showToast("renamed");
    } catch (err) {
      showToast(`rename failed: ${err instanceof Error ? err.message : String(err)}`);
    }
  }

  let dragSourcePath = $state<string | null>(null);
  let dragTargetFolder = $state<string | null>(null);

  async function renameFolderAction(folderName: string) {
    if (!folder) return;
    const proposed = prompt("rename folder:", folderName);
    if (!proposed || proposed === folderName) return;
    try {
      await renameFolder(folder, folderName, proposed);
      requests = await listRequests(folder);
      showToast(`renamed → ${proposed}`);
    } catch (err) {
      showToast(`rename: ${err instanceof Error ? err.message : String(err)}`);
    }
  }

  async function deleteFolderAction(folderName: string) {
    if (!folder) return;
    const count = requests.filter((r) => r.folder === folderName).length;
    if (!confirm(`Delete folder "${folderName}" and all ${count} request${count === 1 ? "" : "s"} inside?`)) return;
    try {
      await deleteFolder(folder, folderName);
      requests = await listRequests(folder);
      if (activePath?.includes(`/${folderName}/`) || activePath?.includes(`\\${folderName}\\`)) {
        await newRequest();
      }
      showToast(`deleted folder ${folderName}`);
    } catch (err) {
      showToast(`delete: ${err instanceof Error ? err.message : String(err)}`);
    }
  }

  function onDragStart(entry: RequestEntry, ev: DragEvent) {
    dragSourcePath = entry.path;
    if (ev.dataTransfer) {
      ev.dataTransfer.effectAllowed = "move";
      ev.dataTransfer.setData("text/plain", entry.path);
    }
  }

  function onDragOverFolder(folderName: string, ev: DragEvent) {
    if (!dragSourcePath) return;
    ev.preventDefault();
    if (ev.dataTransfer) ev.dataTransfer.dropEffect = "move";
    dragTargetFolder = folderName;
  }

  async function onDropFolder(folderName: string, ev: DragEvent) {
    ev.preventDefault();
    if (!dragSourcePath || !folder) return;
    const src = dragSourcePath;
    dragSourcePath = null;
    dragTargetFolder = null;
    try {
      const newPath = await moveRequest(src, folderName, folder);
      requests = await listRequests(folder);
      if (activePath === src) activePath = newPath;
      showToast(folderName ? `moved to ${folderName}` : "moved to root");
    } catch (err) {
      showToast(`move failed: ${err instanceof Error ? err.message : String(err)}`);
    }
  }

  async function openExternalEditor(entry: RequestEntry, ev: Event) {
    ev.stopPropagation();
    try {
      await openInShell(entry.path);
    } catch (err) {
      showToast(`open failed: ${err instanceof Error ? err.message : String(err)}`);
    }
  }

  async function duplicateEntry(entry: RequestEntry, ev: Event) {
    ev.stopPropagation();
    try {
      const newPath = await duplicateRequest(entry.path);
      if (folder) requests = await listRequests(folder);
      showToast(`duplicated to ${newPath.split(/[\\/]/).pop()}`);
    } catch (err) {
      showToast(`duplicate failed: ${err instanceof Error ? err.message : String(err)}`);
    }
  }

  async function toggleMonitor() {
    if (!activePath || !monitorIntervalSecs) return;
    if (activeMonitors.has(activePath)) {
      try {
        await monitorStop(activePath);
        const next = new Set(activeMonitors);
        next.delete(activePath);
        activeMonitors = next;
        showToast("monitor stopped");
      } catch (err) {
        showToast(`stop failed: ${err instanceof Error ? err.message : String(err)}`);
      }
    } else {
      try {
        await monitorStart(activePath, monitorIntervalSecs);
        const next = new Set(activeMonitors);
        next.add(activePath);
        activeMonitors = next;
        showToast(`monitoring every ${monitorIntervalSecs}s`);
      } catch (err) {
        showToast(`start failed: ${err instanceof Error ? err.message : String(err)}`);
      }
    }
  }

  async function exportPostmanCollection() {
    if (!folder) return;
    try {
      const path = await exportPostman(folder);
      showToast(`postman collection written to ${path.split(/[\\/]/).pop()}`);
    } catch (err) {
      showToast(`export failed: ${err instanceof Error ? err.message : String(err)}`);
    }
  }

  async function wsConnectAction() {
    if (!parsed) return;
    if (wsId) {
      try { await wsClose(wsId); } catch { /* ignore */ }
    }
    const newId = crypto.randomUUID();
    wsId = newId;
    wsMessages = [];
    wsError = null;
    wsStatus = "connecting";
    try {
      await wsConnect(parsed.url, newId);
    } catch (err) {
      wsStatus = "error";
      wsError = err instanceof Error ? err.message : String(err);
    }
  }

  async function wsSendAction() {
    if (!wsId || !wsSendDraft) return;
    try {
      await wsSend(wsId, wsSendDraft);
      wsSendDraft = "";
    } catch (err) {
      wsError = err instanceof Error ? err.message : String(err);
    }
  }

  async function wsCloseAction() {
    if (!wsId) return;
    try {
      await wsClose(wsId);
    } catch { /* ignore */ }
    wsStatus = "closed";
  }

  function applyWorkspaceConfig(spec: RequestSpec, cfg: WorkspaceConfig): RequestSpec {
    let url = spec.url;
    if (cfg.base_url && !/^https?:\/\//i.test(url) && !/^wss?:\/\//i.test(url)) {
      const base = cfg.base_url.replace(/\/$/, "");
      const path = url.startsWith("/") ? url : `/${url}`;
      url = `${base}${path}`;
    }
    const existingHeaderKeys = new Set(spec.headers.map(([k]) => k.toLowerCase()));
    const headers = [...spec.headers];
    for (const [k, v] of Object.entries(cfg.default_headers ?? {})) {
      if (!existingHeaderKeys.has(k.toLowerCase())) {
        headers.push([k, v]);
      }
    }
    return { ...spec, url, headers };
  }

  async function persistEnvFromScript(updatedEnv: Array<[string, string]>) {
    if (!activeEnvPath) return;
    const updates = new Map(updatedEnv);
    let changed = false;
    let pairs = activeEnvPairs.slice();
    const globalMap = new Map(globalEnvPairs);

    for (const [k, v] of updatedEnv) {
      const activeIdx = pairs.findIndex(([key]) => key === k);
      if (activeIdx >= 0) {
        if (pairs[activeIdx][1] !== v) {
          pairs[activeIdx] = [k, v];
          changed = true;
        }
      } else if (globalMap.has(k)) {
        if (globalMap.get(k) !== v) {
          pairs.push([k, v]);
          changed = true;
        }
      } else {
        pairs.push([k, v]);
        changed = true;
      }
    }
    // Detect deletions: any active key no longer in updatedEnv
    const beforeKeys = pairs.map(([k]) => k);
    pairs = pairs.filter(([k]) => updates.has(k) || !beforeKeys.includes(k));
    if (changed) {
      activeEnvPairs = pairs;
      await writeEnv(activeEnvPath, pairs);
    }
  }

  async function run() {
    if (!parsed) return;
    if (saveTimer) {
      clearTimeout(saveTimer);
      await doSave();
    }
    isRunning = true;
    runError = null;
    response = null;
    viewingHistoryTs = null;
    scriptLogs = [];
    pmTests = [];

    let workingSpec: RequestSpec = parsed;
    let workingEnv: Array<[string, string]> = [...envVarsMap().entries()];
    const allLogs: string[] = [];

    if (scriptBlocks.preScript) {
      try {
        const outcome = await runScript({
          script: scriptBlocks.preScript,
          env: workingEnv,
          method: workingSpec.method,
          url: workingSpec.url,
          headers: workingSpec.headers,
          body: workingSpec.body,
          response: null,
        });
        if (outcome.error) {
          runError = `pre-request: ${outcome.error}`;
          allLogs.push(...outcome.logs);
          scriptLogs = allLogs;
          isRunning = false;
          return;
        }
        workingEnv = outcome.env;
        workingSpec = { ...workingSpec, headers: outcome.headers };
        allLogs.push(...outcome.logs.map((l) => `[pre] ${l}`));
        if (outcome.tests) pmTests = [...pmTests, ...outcome.tests];
        await persistEnvFromScript(outcome.env);
      } catch (err) {
        runError = `pre-request: ${err instanceof Error ? err.message : String(err)}`;
        scriptLogs = allLogs;
        isRunning = false;
        return;
      }
    }

    workingSpec = applyWorkspaceConfig(workingSpec, workspaceConfig);

    const sub = substitute(workingSpec, new Map(workingEnv));
    lastResolved = sub.resolved;
    lastUnresolved = sub.unresolved;

    const aws = awsParamsFromEnv();
    const digest = digestCredsFromEnv();
    sentSpec = sub.spec;
    try {
      const result = await runRequest(sub.spec, aws, digest);
      response = result;
      renderHtml = false;
      assertionResults = evaluateAssertions(assertions, result);
      extractResults = applyExtracts(extracts, result);
      await persistExtractedVars(extractResults);

      if (scriptBlocks.postScript) {
        try {
          const outcome = await runScript({
            script: scriptBlocks.postScript,
            env: workingEnv,
            method: sub.spec.method,
            url: sub.spec.url,
            headers: sub.spec.headers,
            body: sub.spec.body,
            response: {
              status: result.status,
              status_text: result.status_text,
              headers: result.headers,
              body: result.body,
              elapsed_ms: result.elapsed_ms,
            },
          });
          if (outcome.error) {
            allLogs.push(`[post] error: ${outcome.error}`);
          } else {
            await persistEnvFromScript(outcome.env);
          }
          allLogs.push(...outcome.logs.map((l) => `[post] ${l}`));
          if (outcome.tests) pmTests = [...pmTests, ...outcome.tests];
        } catch (err) {
          allLogs.push(`[post] error: ${err instanceof Error ? err.message : String(err)}`);
        }
      }
      scriptLogs = allLogs;

      if (activePath) {
        const ts = await appendHistory(activePath, sub.spec, result);
        viewingHistoryTs = ts;
        history = await readHistory(activePath, 50);
      }
      if (folder) {
        try {
          await saveCookies(folder);
          cookies = await listCookies();
        } catch {
          // ignore
        }
      }
    } catch (err) {
      runError = err instanceof Error ? err.message : String(err);
      scriptLogs = allLogs;
    } finally {
      isRunning = false;
    }
  }

  function viewHistoryEntry(entry: HistoryEntry) {
    response = entry.response;
    viewingHistoryTs = entry.ts;
    historyOpen = false;
  }

  function formatTs(ts: number): string {
    const date = new Date(ts);
    const today = new Date();
    const isToday =
      date.getFullYear() === today.getFullYear() &&
      date.getMonth() === today.getMonth() &&
      date.getDate() === today.getDate();
    if (isToday) {
      return date.toLocaleTimeString([], {
        hour: "2-digit",
        minute: "2-digit",
        second: "2-digit",
      });
    }
    return date.toLocaleString([], {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  }

  let shortcutsOpen = $state(false);
  let sidebarHidden = $state(false);

  function handleKey(event: KeyboardEvent) {
    const mod = event.ctrlKey || event.metaKey;
    if (mod && event.key === "Enter") {
      event.preventDefault();
      void run();
      return;
    }
    if (mod && event.key === "/" && event.shiftKey) {
      event.preventDefault();
      shortcutsOpen = true;
      return;
    }
    if (mod && event.key.toLowerCase() === "p" && event.shiftKey) {
      event.preventDefault();
      const input = document.querySelector<HTMLInputElement>(".search-input");
      input?.focus();
      return;
    }
    if (mod && event.key.toLowerCase() === "n" && event.shiftKey) {
      event.preventDefault();
      void newRequest();
      return;
    }
    if (mod && event.key.toLowerCase() === "b") {
      event.preventDefault();
      sidebarHidden = !sidebarHidden;
      return;
    }
    if (mod && (event.key === "ArrowDown" || event.key === "ArrowUp")) {
      event.preventDefault();
      navigateSidebar(event.key === "ArrowDown" ? 1 : -1);
      return;
    }
  }

  async function navigateSidebar(direction: number) {
    if (filteredRequests.length === 0) return;
    const currentIdx = activePath ? filteredRequests.findIndex((r) => r.path === activePath) : -1;
    let nextIdx = currentIdx + direction;
    if (nextIdx < 0) nextIdx = filteredRequests.length - 1;
    if (nextIdx >= filteredRequests.length) nextIdx = 0;
    await selectRequest(filteredRequests[nextIdx]);
  }

  function explainNetworkError(message: string): { headline: string; hint: string | null } {
    const lower = message.toLowerCase();
    if (lower.includes("dns") || lower.includes("name not resolved") || lower.includes("name or service not known")) {
      return { headline: message, hint: "DNS lookup failed — check the hostname and your internet connection" };
    }
    if (lower.includes("connection refused")) {
      return { headline: message, hint: "Server is not accepting connections — is the service running on this port?" };
    }
    if (lower.includes("connection reset")) {
      return { headline: message, hint: "Server closed the connection — could be a TLS mismatch, proxy issue, or rate-limit drop" };
    }
    if (lower.includes("timed out") || lower.includes("timeout")) {
      return { headline: message, hint: "Request timed out — server is slow or unreachable" };
    }
    if (lower.includes("tls") || lower.includes("ssl") || lower.includes("certificate")) {
      return { headline: message, hint: "TLS/certificate error — server cert may be self-signed or expired" };
    }
    if (lower.includes("invalid url") || lower.includes("relative url") || lower.includes("unsupported scheme")) {
      return { headline: message, hint: "URL is malformed — needs http:// or https:// (or set base_url in .dante.config.json for relative paths)" };
    }
    return { headline: message, hint: null };
  }

  function sparklinePath(values: number[], width: number, height: number): string {
    if (values.length === 0) return "";
    const max = Math.max(...values, 1);
    const min = Math.min(...values, 0);
    const range = Math.max(max - min, 1);
    const step = values.length > 1 ? width / (values.length - 1) : 0;
    return values
      .map((v, i) => {
        const x = i * step;
        const y = height - ((v - min) / range) * height;
        return `${i === 0 ? "M" : "L"}${x.toFixed(1)},${y.toFixed(1)}`;
      })
      .join(" ");
  }

  let sparklineValues = $derived.by((): number[] => {
    if (history.length === 0) return [];
    return [...history]
      .reverse()
      .slice(-30)
      .map((h) => h.response.elapsed_ms);
  });

  function statusColor(status: number): string {
    if (status >= 200 && status < 300) return "var(--green)";
    if (status >= 300 && status < 400) return "var(--yellow)";
    return "var(--red)";
  }

  function isHtmlResponse(resp: ResponseData): boolean {
    const ct = resp.headers.find(([k]) => k.toLowerCase() === "content-type")?.[1] ?? "";
    return ct.includes("html");
  }

  function prettyBody(body: string, headers: Array<[string, string]>): string {
    const ct = headers.find(([k]) => k.toLowerCase() === "content-type")?.[1] ?? "";
    if (ct.includes("json")) {
      try {
        return JSON.stringify(JSON.parse(body), null, 2);
      } catch {
        return body;
      }
    }
    return body;
  }

  function pathOf(url: string): string {
    try {
      return new URL(url).pathname || "/";
    } catch {
      return url.replace(/^https?:\/\/[^/]+/, "") || "/";
    }
  }
</script>

{#snippet jsonNode(value: unknown, path: Array<string | number>)}
  {#if value === null}
    <span
      class="leaf null clickable"
      role="button"
      tabindex="0"
      onclick={() => extractFromTree(value, path)}
      onkeydown={(e) => { if (e.key === "Enter") extractFromTree(value, path); }}
    >null</span>
  {:else if typeof value === "string"}
    <span
      class="leaf string clickable"
      role="button"
      tabindex="0"
      title={value}
      onclick={() => extractFromTree(value, path)}
      onkeydown={(e) => { if (e.key === "Enter") extractFromTree(value, path); }}
    >"{value.length > 60 ? value.slice(0, 60) + "…" : value}"</span>
  {:else if typeof value === "number"}
    <span
      class="leaf number clickable"
      role="button"
      tabindex="0"
      onclick={() => extractFromTree(value, path)}
      onkeydown={(e) => { if (e.key === "Enter") extractFromTree(value, path); }}
    >{value}</span>
  {:else if typeof value === "boolean"}
    <span
      class="leaf bool clickable"
      role="button"
      tabindex="0"
      onclick={() => extractFromTree(value, path)}
      onkeydown={(e) => { if (e.key === "Enter") extractFromTree(value, path); }}
    >{value}</span>
  {:else if Array.isArray(value)}
    <details class="tree-node" open={path.length < 2}>
      <summary>
        <span class="dim">[{value.length}]</span>
        {#if path.length > 0}
          <button class="copy-path" onclick={() => copyPath(path)} title="copy path">⎘</button>
        {/if}
      </summary>
      <div class="tree-children">
        {#each value as item, i (i)}
          <div class="kv">
            <span class="k">{i}</span>
            {@render jsonNode(item, [...path, i])}
          </div>
        {/each}
      </div>
    </details>
  {:else if typeof value === "object"}
    {@const obj = value as Record<string, unknown>}
    <details class="tree-node" open={path.length < 2}>
      <summary>
        <span class="dim">{`{${Object.keys(obj).length}}`}</span>
        {#if path.length > 0}
          <button class="copy-path" onclick={() => copyPath(path)} title="copy path">⎘</button>
        {/if}
      </summary>
      <div class="tree-children">
        {#each Object.entries(obj) as [k, v] (k)}
          <div class="kv">
            <span class="k">{k}</span>
            {@render jsonNode(v, [...path, k])}
          </div>
        {/each}
      </div>
    </details>
  {/if}
{/snippet}

<svelte:window onkeydown={handleKey} />

{#if generateOpen && parsed}
  {@const fullEnv = envVarsMap()}
  {@const envForGen = dataset
    ? new Map([...fullEnv].filter(([k]) => !dataset.columns.includes(k)))
    : fullEnv}
  {@const sub = substitute(parsed, envForGen)}
  <GeneratePanel
    spec={sub.spec}
    dataset={dataset}
    onClose={() => (generateOpen = false)}
  />
{/if}

{#if aiReview}
  <AiReviewPanel
    review={aiReview}
    onClose={() => (aiReview = null)}
    onAddTest={addAiTest}
    onAddTests={addAiTests}
    onAddExtract={addAiExtract}
    onAddExtracts={addAiExtracts}
  />
{/if}

{#if diffOpen && diffPair}
  <div class="backdrop" role="presentation" onclick={() => (diffOpen = false)} onkeydown={() => {}}></div>
  <div class="modal diff-modal" role="dialog" aria-modal="true" aria-label="History diff">
    <div class="modal-head">
      <div class="title">
        <span>diff</span>
        <span class="dim">{formatTs(diffPair.a.ts)} → {formatTs(diffPair.b.ts)}</span>
      </div>
      <button class="close" onclick={() => (diffOpen = false)} aria-label="close">×</button>
    </div>
    <div class="diff-body">
      <div class="diff-cols">
        <div class="diff-col">
          <div class="diff-col-head">
            <span style="color: {statusColor(diffPair.a.response.status)}">{diffPair.a.response.status}</span>
            <span class="dim">{diffPair.a.response.elapsed_ms}ms</span>
          </div>
          <pre class="diff-text">{prettyForDiff(diffPair.a)}</pre>
        </div>
        <div class="diff-col">
          <div class="diff-col-head">
            <span style="color: {statusColor(diffPair.b.response.status)}">{diffPair.b.response.status}</span>
            <span class="dim">{diffPair.b.response.elapsed_ms}ms</span>
          </div>
          <pre class="diff-text">{prettyForDiff(diffPair.b)}</pre>
        </div>
      </div>
      <div class="diff-inline">
        <div class="diff-col-head dim">line diff</div>
        {#each lineDiff(prettyForDiff(diffPair.a), prettyForDiff(diffPair.b)) as line, idx (idx)}
          <div class="diff-line diff-{line.type}">
            <span class="diff-marker">{line.type === "add" ? "+" : line.type === "del" ? "-" : " "}</span>
            <span>{line.text}</span>
          </div>
        {/each}
      </div>
    </div>
  </div>
{/if}

{#if shortcutsOpen}
  <div class="backdrop" role="presentation" onclick={() => (shortcutsOpen = false)} onkeydown={() => {}}></div>
  <div class="modal" role="dialog" aria-modal="true" aria-label="Keyboard shortcuts">
    <div class="modal-head">
      <div class="title">⌨ keyboard shortcuts</div>
      <button class="close" onclick={() => (shortcutsOpen = false)} aria-label="close">×</button>
    </div>
    <div class="schema-body">
      <table class="shortcut-table">
        <tbody>
          <tr><td><kbd>⌘/Ctrl</kbd> <kbd>Enter</kbd></td><td>Run request</td></tr>
          <tr><td><kbd>⌘/Ctrl</kbd> <kbd>F</kbd></td><td>Find in editor</td></tr>
          <tr><td><kbd>⌘/Ctrl</kbd> <kbd>H</kbd></td><td>Find &amp; replace</td></tr>
          <tr><td><kbd>⌘/Ctrl</kbd> <kbd>Z</kbd></td><td>Undo</td></tr>
          <tr><td><kbd>⌘/Ctrl</kbd> <kbd>Shift</kbd> <kbd>Z</kbd></td><td>Redo</td></tr>
          <tr><td><kbd>⌘/Ctrl</kbd> <kbd>B</kbd></td><td>Toggle sidebar</td></tr>
          <tr><td><kbd>⌘/Ctrl</kbd> <kbd>Shift</kbd> <kbd>P</kbd></td><td>Focus search</td></tr>
          <tr><td><kbd>⌘/Ctrl</kbd> <kbd>Shift</kbd> <kbd>N</kbd></td><td>New request</td></tr>
          <tr><td><kbd>⌘/Ctrl</kbd> <kbd>Shift</kbd> <kbd>/</kbd></td><td>This help</td></tr>
        </tbody>
      </table>
      <div class="shortcut-section">
        <strong>autosave</strong> · 400ms after you stop typing
      </div>
      <div class="shortcut-section">
        <strong>paste</strong> · curl detected on paste auto-converts to .http
      </div>
      <div class="shortcut-section">
        <strong>focus</strong> · clipboard sniffed when window regains focus
      </div>
    </div>
  </div>
{/if}

{#if loadOpen && parsed}
  <div class="backdrop" role="presentation" onclick={() => (loadOpen = false)} onkeydown={() => {}}></div>
  <div class="modal" role="dialog" aria-modal="true" aria-label="Load test">
    <div class="modal-head">
      <div class="title">⚡ Load test</div>
      <button class="close" onclick={() => { loadCancel = true; loadOpen = false; }} aria-label="close">×</button>
    </div>
    <div class="schema-body">
      <div class="env-pair">
        <span class="oauth-label">total runs</span>
        <input class="env-input" type="number" min="1" bind:value={loadConfig.runs} />
      </div>
      <div class="env-pair">
        <span class="oauth-label">concurrency</span>
        <input class="env-input" type="number" min="1" bind:value={loadConfig.concurrency} />
      </div>
      <div style="display: flex; gap: 6px; margin: 8px 0;">
        <button class="primary" onclick={runLoadTest} disabled={loadRunning}>
          {loadRunning ? `Running (${loadStats?.completed ?? 0}/${loadStats?.total ?? 0})…` : "Start"}
        </button>
        {#if loadRunning}
          <button onclick={() => (loadCancel = true)}>Stop</button>
        {/if}
      </div>
      {#if loadStats}
        <div class="schema-section">
          <h4>Latency (ms)</h4>
          <div class="loadstat-grid">
            <span>p50</span><span>{loadStats.p50}</span>
            <span>p95</span><span>{loadStats.p95}</span>
            <span>p99</span><span>{loadStats.p99}</span>
            <span>min</span><span>{loadStats.min}</span>
            <span>max</span><span>{loadStats.max}</span>
          </div>
        </div>
        <div class="schema-section">
          <h4>Status</h4>
          <div class="loadstat-grid">
            <span>completed</span><span>{loadStats.completed}/{loadStats.total}</span>
            <span>success</span><span>{loadStats.success}</span>
            <span>errors</span><span>{loadStats.errors}</span>
            {#each Object.entries(loadStats.statusBuckets) as [k, v]}
              <span>{k}</span><span>{v}</span>
            {/each}
          </div>
        </div>
      {/if}
    </div>
  </div>
{/if}

{#if graphqlSchemaOpen && graphqlSchema}
  {@const summary = summarizeGraphqlSchema(graphqlSchema)}
  <div class="backdrop" role="presentation" onclick={() => (graphqlSchemaOpen = false)} onkeydown={() => {}}></div>
  <div class="modal" role="dialog" aria-modal="true" aria-label="GraphQL schema">
    <div class="modal-head">
      <div class="title">
        <span class="badge-graphql">GraphQL</span>
        <span>schema</span>
      </div>
      <button class="close" onclick={() => (graphqlSchemaOpen = false)} aria-label="close">×</button>
    </div>
    <div class="schema-body">
      <div class="schema-section">
        <h4>Queries ({summary.queries.length})</h4>
        <ul>
          {#each summary.queries as q}
            <li>{q}</li>
          {/each}
        </ul>
      </div>
      {#if summary.mutations.length > 0}
        <div class="schema-section">
          <h4>Mutations ({summary.mutations.length})</h4>
          <ul>
            {#each summary.mutations as m}
              <li>{m}</li>
            {/each}
          </ul>
        </div>
      {/if}
      <div class="schema-section">
        <h4>Types ({summary.types.length})</h4>
        <ul class="types">
          {#each summary.types as t}
            <li>{t}</li>
          {/each}
        </ul>
      </div>
    </div>
  </div>
{/if}

{#if toast}
  <div class="toast">{toast}</div>
{/if}

<main>
  <header>
    <div class="brand">Dante</div>
    <div class="name-line">
      {#if parsed}
        {#if isDirty}
          <span class="dirty-dot" title="unsaved (autosave in 400ms)">●</span>
        {/if}
        <span class="method method-{parsed.method.toLowerCase()}">{parsed.method}</span>
        <span class="path">{pathOf(parsed.url)}</span>
        {#if isGraphql}
          <span class="badge-graphql">GraphQL</span>
          <button
            class="badge-monitor"
            onclick={introspectGraphql}
            disabled={!parsed || graphqlBusy}
            title="introspect schema"
          >
            {graphqlBusy ? "…" : "introspect"}
          </button>
        {/if}
        {#if awsActive}
          <span class="badge-aws" title="AWS sigv4 will sign this request before sending">sigv4</span>
        {/if}
        {#if digestActive}
          <span class="badge-digest" title="HTTP Digest auth retries on 401">digest</span>
        {/if}
        {#if scriptBlocks.preScript || scriptBlocks.postScript}
          <span class="badge-script" title="JS scripts run pre- and/or post-request">
            JS{scriptBlocks.preScript ? " ↓" : ""}{scriptBlocks.postScript ? " ↑" : ""}
          </span>
        {/if}
        {#if monitorIntervalSecs && activePath}
          <button
            class="badge-monitor"
            class:active={monitoring}
            onclick={toggleMonitor}
            title="schedule: every {monitorIntervalSecs}s"
          >
            {monitoring ? "● mon" : "○ mon"}
          </button>
        {/if}
      {:else}
        <span class="dim">paste a curl to get started</span>
      {/if}
    </div>
    <div class="actions">
      <span class="hint dim">⌘/Ctrl+Enter</span>
      {#if aiKeyAvailable && aiBackend}
        <button
          onclick={runAiReview}
          disabled={!parsed || aiReviewBusy}
          title="AI review via {aiBackend.label}"
        >
          {aiReviewBusy ? "✨ thinking…" : `✨ AI review`}
        </button>
      {/if}
      <button onclick={() => (generateOpen = true)} disabled={!parsed}>
        Generate
      </button>
      {#if dataset}
        <button onclick={runWithData} disabled={!parsed || dataRunning} title="Run once per data row">
          {dataRunning ? "Running…" : `Run × ${dataset.rows.length}`}
        </button>
      {/if}
      {#if envs.length > 1}
        <button onclick={runAcrossEnvs} disabled={!parsed || diffRunning} title="Run against every env, side-by-side">
          {diffRunning ? "Diffing…" : "Run vs all envs"}
        </button>
      {/if}
      {#if !isWebSocket}
        <button onclick={() => (loadOpen = true)} disabled={!parsed} title="Load test: many concurrent runs">
          Load
        </button>
      {/if}
      {#if isWebSocket}
        {#if wsStatus === "connected" || wsStatus === "connecting"}
          <button class="primary" onclick={wsCloseAction} title="Disconnect">
            {wsStatus === "connecting" ? "Connecting…" : "Disconnect"}
          </button>
        {:else}
          <button class="primary" onclick={wsConnectAction} disabled={!parsed}>
            Connect
          </button>
        {/if}
      {:else}
        <button class="primary" onclick={run} disabled={!parsed || isRunning}>
          {isRunning ? "Running…" : "Run"}
        </button>
      {/if}
    </div>
  </header>

  <div
    class="layout"
    class:vertical-split={splitVertical}
    class:resizing={resizing}
    class:sidebar-hidden={sidebarHidden}
    style="--sidebar-width: {sidebarWidth}px;"
  >
    <aside class="sidebar">
      <div class="sidebar-head">
        <button class="new-btn" onclick={newRequest}>+ New</button>
        <div class="head-actions">
          <button class="head-btn" onclick={importSpec} title="import OpenAPI / Postman">📥</button>
          <button class="head-btn" onclick={exportDocs} title="export README.md">📝</button>
          <button class="head-btn" onclick={exportSpec} title="export openapi.yaml">📤</button>
          <button class="head-btn" onclick={exportPostmanCollection} title="export Postman collection">📦</button>
        </div>
      </div>
      {#if requests.length > 5}
        <div class="search-row">
          <input
            class="search-input"
            placeholder="filter…"
            bind:value={searchTerm}
          />
        </div>
      {/if}
      <div class="sidebar-list">
        {#if requests.length === 0}
          <div class="empty dim">
            no saved requests yet — paste a curl to begin
          </div>
        {:else}
          {#each groups as g (g.folder)}
            {#if g.folder !== ""}
              <div
                class="group-label"
                class:drop-target={dragTargetFolder === g.folder}
                ondragover={(e) => onDragOverFolder(g.folder, e)}
                ondragleave={() => (dragTargetFolder = null)}
                ondrop={(e) => onDropFolder(g.folder, e)}
                role="region"
              >
                <span>{g.folder}</span>
                <button
                  class="group-run"
                  onclick={() => runFolder(g.folder)}
                  disabled={folderRunning}
                  title="run all in this folder (parallel results)"
                >▶</button>
                <button
                  class="group-run"
                  onclick={() => runFolderChained(g.folder)}
                  disabled={folderRunning}
                  title="run as chain (sequential, vars flow)"
                >▶▶</button>
                <button
                  class="group-run"
                  onclick={() => renameFolderAction(g.folder)}
                  title="rename folder"
                >✎</button>
                <button
                  class="group-run"
                  onclick={() => deleteFolderAction(g.folder)}
                  title="delete folder"
                >×</button>
              </div>
            {:else if requests.some((r) => (r.folder ?? "") !== "")}
              <div
                class="group-label root-target"
                class:drop-target={dragTargetFolder === ""}
                ondragover={(e) => onDragOverFolder("", e)}
                ondragleave={() => (dragTargetFolder = null)}
                ondrop={(e) => onDropFolder("", e)}
                role="region"
              >
                <span>(root)</span>
              </div>
            {/if}
            {#each g.entries as entry (entry.path)}
              <button
                class="item"
                class:active={entry.path === activePath}
                class:nested={g.folder !== ""}
                draggable="true"
                title={entry.description || `${entry.method} ${entry.url}`}
                onclick={() => selectRequest(entry)}
                ondragstart={(e) => onDragStart(entry, e)}
              >
                <span class="m method-{entry.method.toLowerCase()}">{entry.method}</span>
                <span class="n">{entry.name}</span>
                <span class="entry-actions">
                  <span
                    class="del"
                    role="button"
                    tabindex="0"
                    aria-label="rename"
                    title="rename"
                    onclick={(e) => renameEntry(entry, e)}
                    onkeydown={(e) => {
                      if (e.key === "Enter") renameEntry(entry, e);
                    }}>✎</span>
                  <span
                    class="del"
                    role="button"
                    tabindex="0"
                    aria-label="duplicate"
                    title="duplicate"
                    onclick={(e) => duplicateEntry(entry, e)}
                    onkeydown={(e) => {
                      if (e.key === "Enter") duplicateEntry(entry, e);
                    }}>⎘</span>
                  <span
                    class="del"
                    role="button"
                    tabindex="0"
                    aria-label="open in external editor"
                    title="open in external editor"
                    onclick={(e) => openExternalEditor(entry, e)}
                    onkeydown={(e) => {
                      if (e.key === "Enter") openExternalEditor(entry, e);
                    }}>↗</span>
                  <span
                    class="del"
                    role="button"
                    tabindex="0"
                    aria-label="delete"
                    title="delete"
                    onclick={(e) => removeRequest(entry, e)}
                    onkeydown={(e) => {
                      if (e.key === "Enter" || e.key === " ") removeRequest(entry, e);
                    }}>×</span>
                </span>
              </button>
            {/each}
          {/each}
        {/if}
      </div>
      <div class="sidebar-foot">
        {#if folder}
          <div class="env-row">
            <select
              class="env-select"
              value={activeEnvPath ?? ""}
              onchange={(e) =>
                selectEnv((e.currentTarget as HTMLSelectElement).value || null)}
            >
              <option value="">no env</option>
              {#each envs as e (e.path)}
                <option value={e.path}>{e.name}</option>
              {/each}
            </select>
            {#if activeEnvPath}
              <button
                class="env-edit"
                aria-pressed={envEditorOpen}
                onclick={() => (envEditorOpen = !envEditorOpen)}
                title="edit env vars"
              >⚙</button>
            {/if}
            <button
              class="env-edit"
              onclick={addEnv}
              title="new env file"
            >+</button>
          </div>
          {#if envEditorOpen && activeEnvPath}
            <div class="env-editor">
              {#if globalEnvPairs.length > 0}
                <div class="scope-label dim">global · read-only · click ↓ to copy</div>
                {#each globalEnvPairs as [gk, gv], gIdx (gIdx)}
                  <div class="env-pair">
                    <span class="env-input read-only" title={gk}>{gk}</span>
                    <span class="env-input read-only dim" title={gv}>{gv.slice(0, 30)}{gv.length > 30 ? "…" : ""}</span>
                    <button class="env-del" onclick={() => promoteToActive(gk, gv)} title="copy to active">↓</button>
                  </div>
                {/each}
                <div class="scope-label dim">active env</div>
              {/if}
              {#each activeEnvPairs as pair, idx (idx)}
                <div class="env-pair">
                  <input
                    class="env-input"
                    placeholder="KEY"
                    bind:value={activeEnvPairs[idx][0]}
                    onblur={saveActiveEnv}
                  />
                  <input
                    class="env-input"
                    placeholder="value"
                    bind:value={activeEnvPairs[idx][1]}
                    onblur={saveActiveEnv}
                  />
                  <button
                    class="env-del"
                    onclick={() => demoteToGlobal(idx)}
                    title="move to global"
                  >↑</button>
                  <button
                    class="env-del"
                    onclick={() => removeEnvPair(idx)}
                    title="remove"
                  >×</button>
                </div>
              {/each}
              <button class="env-add" onclick={addEnvPair}>+ var</button>
            </div>
          {/if}
          <div class="cookies-row">
            <button
              class="cookies-toggle"
              onclick={() => (cookiesOpen = !cookiesOpen)}
            >
              🍪 {cookies.length}
            </button>
            {#if cookies.length > 0}
              <button class="cookies-clear" onclick={clearAllCookies} title="clear all cookies">clear</button>
            {/if}
            <button
              class="cookies-toggle"
              onclick={() => (oauthOpen = !oauthOpen)}
              title="OAuth 2.0 client credentials"
            >oauth2</button>
          </div>
          <div class="cookies-row">
            <button
              class="cookies-toggle"
              class:active-mock={mockStatus.running}
              onclick={toggleMockServer}
              title={mockStatus.running ? `serving on :${mockStatus.port}` : "start mock server"}
            >
              {mockStatus.running ? `mock :${mockStatus.port}` : "mock: off"}
            </button>
            {#if !mockStatus.running}
              <input
                class="env-input"
                style="max-width: 60px; font-size: 10px;"
                bind:value={mockPort}
              />
            {/if}
          </div>
          {#if oauthOpen && activeEnvPath}
            <div class="env-editor">
              <div class="oauth-help dim">
                Token stored as <code>{getEnvVar("__oauth_var") || "accessToken"}</code>.
                Reference as <code>{`{{${getEnvVar("__oauth_var") || "accessToken"}}}`}</code> in requests.
              </div>
              <div class="env-pair">
                <span class="oauth-label">authorize URL</span>
                <input
                  class="env-input"
                  placeholder="(auth-code only) https://…/authorize"
                  value={getEnvVar("__oauth_auth_url")}
                  onblur={(e) => setEnvVar("__oauth_auth_url", e.currentTarget.value)}
                />
              </div>
              <div class="env-pair">
                <span class="oauth-label">device URL</span>
                <input
                  class="env-input"
                  placeholder="(device flow only)"
                  value={getEnvVar("__oauth_device_url")}
                  onblur={(e) => setEnvVar("__oauth_device_url", e.currentTarget.value)}
                />
              </div>
              <div class="env-pair">
                <span class="oauth-label">token URL</span>
                <input
                  class="env-input"
                  placeholder="https://…/oauth/token"
                  value={getEnvVar("__oauth_token_url")}
                  onblur={(e) => setEnvVar("__oauth_token_url", e.currentTarget.value)}
                />
              </div>
              <div class="env-pair">
                <span class="oauth-label">client_id</span>
                <input
                  class="env-input"
                  value={getEnvVar("__oauth_client_id")}
                  onblur={(e) => setEnvVar("__oauth_client_id", e.currentTarget.value)}
                />
              </div>
              <div class="env-pair">
                <span class="oauth-label">client_secret</span>
                <input
                  class="env-input"
                  type="password"
                  value={getEnvVar("__oauth_client_secret")}
                  onblur={(e) => setEnvVar("__oauth_client_secret", e.currentTarget.value)}
                />
              </div>
              <div class="env-pair">
                <span class="oauth-label">scope</span>
                <input
                  class="env-input"
                  placeholder="optional"
                  value={getEnvVar("__oauth_scope")}
                  onblur={(e) => setEnvVar("__oauth_scope", e.currentTarget.value)}
                />
              </div>
              <div class="env-pair">
                <span class="oauth-label">redirect port</span>
                <input
                  class="env-input"
                  placeholder="8788"
                  value={getEnvVar("__oauth_redirect_port")}
                  onblur={(e) => setEnvVar("__oauth_redirect_port", e.currentTarget.value)}
                />
              </div>
              <div class="env-pair">
                <span class="oauth-label">store as</span>
                <input
                  class="env-input"
                  placeholder="accessToken"
                  value={getEnvVar("__oauth_var")}
                  onblur={(e) => setEnvVar("__oauth_var", e.currentTarget.value)}
                />
              </div>
              <div style="display: flex; flex-wrap: wrap; gap: 4px;">
                <button class="env-add" onclick={runOauthFetch} disabled={oauthBusy}>
                  {oauthBusy ? "…" : "client_credentials"}
                </button>
                <button class="env-add" onclick={runOauthAuthcode} disabled={oauthBusy}>
                  {oauthBusy ? "…" : "auth-code"}
                </button>
                <button class="env-add" onclick={runOauthPassword} disabled={oauthBusy}>
                  {oauthBusy ? "…" : "password"}
                </button>
                <button class="env-add" onclick={runOauthDevice} disabled={oauthBusy}>
                  {oauthBusy ? "…" : "device"}
                </button>
              </div>
              {#if oauthError}
                <div class="oauth-error">{oauthError}</div>
              {/if}
            </div>
          {/if}
          {#if cookiesOpen}
            <div class="cookies-list">
              {#if cookies.length === 0}
                <div class="empty dim">no cookies yet — run a request that sets one</div>
              {:else}
                {#each cookies as c, idx (idx)}
                  <div class="cookie-row">
                    <span class="cookie-domain">{c.domain}</span>
                    <span class="cookie-name">{c.name}</span>
                    <span class="cookie-value dim" title={c.value}>{c.value.slice(0, 24)}{c.value.length > 24 ? "…" : ""}</span>
                    <button class="cookies-clear" onclick={() => deleteOneCookie(c)} title="delete cookie">×</button>
                  </div>
                {/each}
              {/if}
            </div>
          {/if}
          <div class="folder-line dim">
            <span title={folder}>{folder}</span>
            <button
              class="theme-toggle"
              onclick={() => (splitVertical = !splitVertical)}
              title="toggle split direction"
            >{splitVertical ? "⥯" : "⥋"}</button>
            <button class="theme-toggle" onclick={toggleTheme} title="toggle theme">
              {theme === "dark" ? "☾" : "☀"}
            </button>
          </div>
        {/if}
      </div>
    </aside>

    <div
      class="resize-handle"
      role="separator"
      aria-orientation="vertical"
      onmousedown={startSidebarResize}
      ondblclick={() => (sidebarWidth = 240)}
      title="drag to resize · double-click to reset"
    ></div>

    <section class="editor-pane">
      <div class="pane-label">
        <span>
          Request
          {#if detectedVars.length > 0}
            <span class="vars-line dim">
              uses {detectedVars.map((v) => `{{${v}}}`).join(" ")}
            </span>
          {/if}
        </span>
        {#if parsed}
          <div style="display: flex; gap: 4px;">
            <button class="history-toggle" onclick={attachFile} title="attach file as body (@/path)">📎</button>
            <button class="history-toggle" onclick={() => (paramsOpen = !paramsOpen)}>
              params {urlParams.length > 0 ? `(${urlParams.length})` : ""} ▾
            </button>
          </div>
        {/if}
      </div>
      {#if paramsOpen && parsed}
        <div class="params-panel">
          {#each urlParams as [k, v], idx (idx)}
            <div class="param-row">
              <input
                class="env-input"
                value={k}
                onblur={(e) => setUrlParam(idx, e.currentTarget.value, v)}
              />
              <input
                class="env-input"
                value={v}
                onblur={(e) => setUrlParam(idx, k, e.currentTarget.value)}
              />
              <button class="env-del" onclick={() => deleteUrlParam(idx)} title="remove">×</button>
            </div>
          {/each}
          <button class="env-add" onclick={addUrlParam}>+ param</button>
        </div>
      {/if}
      {#if clipboardCurl}
        <div class="clipboard-banner">
          <span>📋 found a curl in your clipboard</span>
          <button class="clipboard-use" onclick={applyClipboardCurl}>use it</button>
          <button class="clipboard-dismiss" onclick={dismissClipboardCurl} aria-label="dismiss">×</button>
        </div>
      {/if}
      {#if findings.length > 0}
        <div class="review-strip">
          <span class="review-label">⚠ found {findings.length} secret{findings.length === 1 ? "" : "s"}:</span>
          {#each findings as f (f.kind + f.suggestedName + f.rawValue)}
            <button
              class="review-chip"
              onclick={() => extractFinding(f)}
              title={`extract to active env as {{${f.suggestedName}}}`}
            >
              {f.description} → <code>{`{{${f.suggestedName}}}`}</code>
            </button>
          {/each}
          {#if findings.length > 1}
            <button class="review-all" onclick={extractAllFindings}>
              extract all
            </button>
          {/if}
        </div>
      {/if}
      <div class="editor-mount">
        <Editor bind:value={editorText} onPaste={handlePaste} onRun={run} />
      </div>
      {#if parseError}
        <div class="hint error">{parseError}</div>
      {/if}
    </section>

    <section class="response-pane">
      {#if isWebSocket}
        <div class="pane-label">
          WebSocket · <span class:status-connected={wsStatus === "connected"}>{wsStatus}</span>
        </div>
        <div class="ws-pane">
          <div class="ws-messages">
            {#if wsError}
              <div class="hint error">{wsError}</div>
            {/if}
            {#if wsMessages.length === 0 && wsStatus === "idle"}
              <div class="hint dim">click Connect to open the connection</div>
            {/if}
            {#each wsMessages as m, idx (idx)}
              <div class="ws-msg ws-msg-{m.direction}">
                <span class="ts dim">{new Date(m.ts).toLocaleTimeString()}</span>
                <span class="dir">{m.direction === "out" ? "→" : "←"}</span>
                <pre class="ws-text">{m.text}</pre>
              </div>
            {/each}
          </div>
          {#if wsStatus === "connected"}
            <div class="ws-send">
              <textarea
                class="ws-draft"
                placeholder="message to send (Cmd/Ctrl+Enter)"
                bind:value={wsSendDraft}
                onkeydown={(e) => {
                  if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
                    e.preventDefault();
                    void wsSendAction();
                  }
                }}
              ></textarea>
              <button class="primary" onclick={wsSendAction} disabled={!wsSendDraft.trim()}>
                Send
              </button>
            </div>
          {/if}
        </div>
      {:else}
        <div class="pane-label">
          Response
        {#if history.length > 0}
          <button class="history-toggle" onclick={() => (historyOpen = !historyOpen)}>
            {history.length} run{history.length === 1 ? "" : "s"} ▾
          </button>
        {/if}
      </div>
      {#if historyOpen}
        <div class="history-list">
          {#if history.length > 1}
            <div class="diff-hint dim">
              click a run to view, or click ⇄ to mark for diff{diffPickA !== null ? " (1 picked, click another)" : ""}
            </div>
          {/if}
          {#each history as entry (entry.ts)}
            <div
              class="history-item"
              class:active={entry.ts === viewingHistoryTs}
              class:diff-pick={entry.ts === diffPickA}
            >
              <button class="history-main" onclick={() => viewHistoryEntry(entry)}>
                <span class="ts">{formatTs(entry.ts)}</span>
                <span class="hs" style="color: {statusColor(entry.response.status)}">
                  {entry.response.status}
                </span>
                <span class="el dim">{entry.response.elapsed_ms} ms</span>
              </button>
              {#if history.length > 1}
                <button
                  class="diff-pick-btn"
                  title="mark for diff"
                  onclick={() => pickHistoryForDiff(entry)}
                >⇄</button>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
      {#if dataRunRows && dataRunRows.length > 0}
        <div class="diff-table">
          <div class="diff-head dim">
            data run · {dataRunRows.filter((r) => r.error === null && r.status !== null && r.status >= 200 && r.status < 300 && (r.total === 0 || r.passed === r.total)).length}/{dataRunRows.length} green
          </div>
          {#each dataRunRows as row (row.rowIndex)}
            <div class="diff-row">
              <span class="env-name">#{row.rowIndex + 1}</span>
              <span class="path">{row.rowSummary}</span>
              {#if row.error}
                <span class="warn">{row.error.slice(0, 60)}</span>
              {:else}
                <span class="status" style="color: {statusColor(row.status ?? 0)}">{row.status}</span>
                <span class="dim">{row.elapsedMs}ms</span>
                {#if row.total > 0}
                  <span class:warn={row.passed !== row.total}>{row.passed}/{row.total}</span>
                {/if}
              {/if}
            </div>
          {/each}
          <div class="diff-foot">
            <button class="cookies-toggle" onclick={() => (dataRunRows = null)}>dismiss</button>
          </div>
        </div>
      {/if}
      {#if folderRunRows && folderRunRows.length > 0}
        <div class="diff-table">
          <div class="diff-head dim">
            folder run · {folderRunRows.filter((r) => r.error === null && r.status !== null && r.status >= 200 && r.status < 300 && (r.total === 0 || r.passed === r.total)).length}/{folderRunRows.length} green
          </div>
          {#each folderRunRows as row (row.name)}
            <div class="diff-row">
              <span class="env-name">{row.method}</span>
              <span class="path">{row.name}</span>
              {#if row.error}
                <span class="warn">error: {row.error.slice(0, 60)}</span>
              {:else}
                <span class="status" style="color: {statusColor(row.status ?? 0)}">{row.status}</span>
                <span class="dim">{row.elapsedMs}ms</span>
                {#if row.total > 0}
                  <span class:warn={row.passed !== row.total}>
                    {row.passed}/{row.total} tests
                  </span>
                {/if}
              {/if}
            </div>
          {/each}
          <div class="diff-foot">
            <button class="cookies-toggle" onclick={() => (folderRunRows = null)}>dismiss</button>
          </div>
        </div>
      {/if}
      {#if diffRows && diffRows.length > 0}
        <div class="diff-table">
          <div class="diff-head dim">env diff ({diffRows.length})</div>
          {#each diffRows as row (row.envName)}
            <div class="diff-row">
              <span class="env-name">{row.envName}</span>
              {#if row.error}
                <span class="warn">error: {row.error.slice(0, 60)}</span>
              {:else}
                <span class="status" style="color: {statusColor(row.status ?? 0)}">
                  {row.status} {row.statusText}
                </span>
                <span class="dim">{row.elapsedMs}ms</span>
                {#if row.bodyKeys.length > 0}
                  <span class="dim shape">{`{${row.bodyKeys.slice(0, 5).join(",")}}`}</span>
                {/if}
              {/if}
            </div>
          {/each}
          <div class="diff-foot">
            <button class="cookies-toggle" onclick={() => (diffRows = null)}>dismiss</button>
          </div>
        </div>
      {/if}
      {#if isRunning}
        <div class="hint dim">Running…</div>
      {:else if runError}
        {@const ex = explainNetworkError(runError)}
        <div class="hint error">
          {ex.headline}
          {#if ex.hint}
            <div class="error-hint">{ex.hint}</div>
          {/if}
        </div>
      {:else if response}
        {#if pmTests.length > 0}
          <div class="assertions">
            <div class="assertions-head dim">
              pm.test · {pmTests.filter((t) => t.pass).length}/{pmTests.length} pass
            </div>
            {#each pmTests as t, idx (idx)}
              <div class="assertion-row" class:pass={t.pass} class:fail={!t.pass}>
                <span class="dot">{t.pass ? "✓" : "✗"}</span>
                <span class="raw">{t.name}</span>
                {#if !t.pass && t.error}
                  <span class="reason">{t.error}</span>
                {/if}
              </div>
            {/each}
          </div>
        {/if}
        {#if scriptLogs.length > 0}
          <details class="script-logs">
            <summary>script logs ({scriptLogs.length})</summary>
            <pre class="logs">{scriptLogs.join("\n")}</pre>
          </details>
        {/if}
        {#if regression}
          <div class="regression">
            <span class="reg-label">⚠ regression vs previous run</span>
            {#each regression as issue}
              <span class="reg-issue">{issue}</span>
            {/each}
          </div>
        {/if}
        <div class="status-bar">
          <span class="status" style="color: {statusColor(response.status)}">
            {response.status} {response.status_text}
          </span>
          <span class="dim">{response.elapsed_ms} ms</span>
          {#if viewingHistoryTs && history.length > 0 && viewingHistoryTs !== history[0].ts}
            <span class="dim">· past run · {formatTs(viewingHistoryTs)}</span>
          {/if}
          {#if sparklineValues.length > 1}
            <svg class="sparkline" width="100" height="20" viewBox="0 0 100 20">
              <path d={sparklinePath(sparklineValues, 100, 20)} fill="none" stroke="var(--accent)" stroke-width="1.2" />
            </svg>
            <span class="dim" title="last {sparklineValues.length} runs">
              ⋯{sparklineValues.length}
            </span>
          {/if}
          {#if lastUnresolved.length > 0}
            <span class="warn" title="missing in active env">
              ⚠ unresolved: {lastUnresolved.join(", ")}
            </span>
          {:else if lastResolved.length > 0}
            <span class="dim">resolved: {lastResolved.join(", ")}</span>
          {/if}
        </div>
        {#if extractResults.length > 0}
          <div class="assertions">
            <div class="assertions-head dim">extracted</div>
            {#each extractResults as r (r.rule.raw)}
              <div class="assertion-row" class:pass={r.value !== null} class:fail={r.value === null}>
                <span class="dot">{r.value !== null ? "→" : "✗"}</span>
                <span class="raw"><code>{r.rule.varName}</code> = {r.rule.raw.split("=").slice(1).join("=").trim()}</span>
                {#if r.value !== null}
                  <span class="reason dim">{r.value.length > 40 ? r.value.slice(0, 40) + "…" : r.value}</span>
                {:else}
                  <span class="reason">not found</span>
                {/if}
              </div>
            {/each}
          </div>
        {/if}
        {#if assertionResults.length > 0}
          <div class="assertions">
            <div class="assertions-head dim">
              tests: {assertionResults.filter((r) => r.pass).length}/{assertionResults.length} pass
            </div>
            {#each assertionResults as r (r.raw)}
              <div class="assertion-row" class:pass={r.pass} class:fail={!r.pass}>
                <span class="dot">{r.pass ? "✓" : "✗"}</span>
                <span class="raw">{r.raw}</span>
                {#if !r.pass && r.reason}
                  <span class="reason">{r.reason}</span>
                {/if}
              </div>
            {/each}
          </div>
        {/if}
        {#if sentSpec}
          <details class="script-logs">
            <summary>Sent request</summary>
            <pre class="headers">{`${sentSpec.method} ${sentSpec.url}\n${sentSpec.headers.map(([k, v]) => `${k}: ${v}`).join("\n")}${sentSpec.body ? `\n\n${sentSpec.body}` : ""}`}</pre>
          </details>
        {/if}
        <details>
          <summary>Headers ({response.headers.length})</summary>
          <pre class="headers">{response.headers
              .map(([k, v]) => `${k}: ${v}`)
              .join("\n")}</pre>
        </details>
        {#if isHtmlResponse(response)}
          <div class="render-toggle">
            <button
              class="cookies-toggle"
              class:active-mock={renderHtml}
              onclick={() => (renderHtml = !renderHtml)}
            >{renderHtml ? "view source" : "render HTML"}</button>
          </div>
          {#if renderHtml}
            <iframe
              class="render-frame"
              sandbox="allow-same-origin"
              srcdoc={response.body}
              title="rendered response"
            ></iframe>
          {:else}
            <pre class="body">{prettyBody(response.body, response.headers)}</pre>
          {/if}
        {:else if tryJsonValue(response.body) !== undefined}
          <div class="render-toggle">
            {#if tryJsonArray(response.body)}
              <button
                class="cookies-toggle"
                class:active-mock={renderTable}
                onclick={() => { renderTable = !renderTable; renderTree = false; }}
              >{renderTable ? "view JSON" : `table (${tryJsonArray(response.body)?.length ?? 0} rows)`}</button>
            {/if}
            <button
              class="cookies-toggle"
              class:active-mock={renderTree}
              onclick={() => { renderTree = !renderTree; renderTable = false; }}
            >{renderTree ? "view JSON" : "tree (click to extract)"}</button>
          </div>
          {#if renderTree}
            <div class="json-tree">
              {@render jsonNode(tryJsonValue(response.body), [])}
            </div>
          {:else if renderTable && tryJsonArray(response.body)}
            {@const t = jsonTable(tryJsonArray(response.body) ?? [])}
            <div class="json-table-wrap">
              <table class="json-table">
                <thead>
                  <tr>
                    {#each t.columns as c}
                      <th>{c}</th>
                    {/each}
                  </tr>
                </thead>
                <tbody>
                  {#each t.rows as row, i (i)}
                    <tr>
                      {#each row as cell}
                        <td title={cell}>{cell.length > 80 ? cell.slice(0, 80) + "…" : cell}</td>
                      {/each}
                    </tr>
                  {/each}
                </tbody>
              </table>
            </div>
          {:else}
            <pre class="body">{prettyBody(response.body, response.headers)}</pre>
          {/if}
        {:else}
          <pre class="body">{prettyBody(response.body, response.headers)}</pre>
        {/if}
      {:else}
        <div class="hint dim">No response yet — hit Run.</div>
      {/if}
      {/if}
    </section>
  </div>
</main>

<style>
  main {
    display: grid;
    grid-template-rows: auto 1fr;
    height: 100%;
  }

  header {
    display: grid;
    grid-template-columns: auto 1fr auto;
    align-items: center;
    gap: 14px;
    padding: 10px 16px;
    border-bottom: 1px solid var(--border);
    background: linear-gradient(180deg, var(--bg-elev) 0%, var(--bg) 100%);
  }

  .brand {
    font-weight: 700;
    font-size: 14px;
    letter-spacing: 0.01em;
    background: linear-gradient(135deg, #a78bfa 0%, #8b5cf6 60%, #6d28d9 100%);
    -webkit-background-clip: text;
    background-clip: text;
    color: transparent;
    user-select: none;
  }

  :root.light .brand {
    background: linear-gradient(135deg, #6d28d9 0%, #5b21b6 100%);
    -webkit-background-clip: text;
    background-clip: text;
  }

  .name-line {
    display: flex;
    align-items: center;
    gap: 8px;
    font-family: var(--mono);
    font-size: 12px;
    overflow: hidden;
  }

  .actions {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  .actions .hint {
    font-size: 11px;
  }

  .method {
    font-weight: 600;
    padding: 2px 7px;
    border-radius: var(--radius-sm);
    font-size: 10px;
    letter-spacing: 0.04em;
    font-family: var(--mono);
  }

  .method-get    { background: rgba(125, 211, 252, 0.14); color: #7dd3fc; }
  .method-post   { background: rgba(134, 239, 172, 0.14); color: #86efac; }
  .method-put    { background: rgba(252, 211, 77, 0.14);  color: #fcd34d; }
  .method-patch  { background: rgba(253, 186, 116, 0.14); color: #fdba74; }
  .method-delete { background: rgba(252, 165, 165, 0.14); color: #fca5a5; }
  .method-head,
  .method-options { background: var(--bg-elev-2); color: var(--text-dim); }

  :root.light .method-get    { color: #0369a1; }
  :root.light .method-post   { color: #15803d; }
  :root.light .method-put    { color: #b45309; }
  :root.light .method-patch  { color: #c2410c; }
  :root.light .method-delete { color: #b91c1c; }

  .badge-graphql {
    background: linear-gradient(90deg, #e535ab, #b13c97);
    color: white;
    font-size: 10px;
    font-weight: 600;
    padding: 1px 6px;
    border-radius: 3px;
    text-transform: none;
  }

  .badge-aws {
    background: linear-gradient(90deg, #ff9900, #d4810a);
    color: black;
    font-size: 10px;
    font-weight: 600;
    padding: 1px 6px;
    border-radius: 3px;
    text-transform: lowercase;
  }

  .badge-digest {
    background: var(--bg-elev-2);
    color: var(--accent);
    border: 1px solid var(--accent-dim);
    font-size: 10px;
    font-weight: 600;
    padding: 1px 6px;
    border-radius: 3px;
    text-transform: lowercase;
  }

  .badge-script {
    background: linear-gradient(90deg, #f0db4f, #d4b821);
    color: black;
    font-size: 10px;
    font-weight: 700;
    padding: 1px 6px;
    border-radius: 3px;
  }

  .badge-monitor {
    background: var(--bg-elev-2);
    color: var(--text-dim);
    border: 1px solid var(--border);
    font-size: 10px;
    font-weight: 600;
    padding: 1px 6px;
    border-radius: 3px;
    cursor: pointer;
    text-transform: lowercase;
    font-family: var(--mono);
  }

  .badge-monitor.active {
    background: rgba(74, 222, 128, 0.15);
    color: var(--green);
    border-color: var(--green);
  }

  .dirty-dot {
    color: var(--yellow);
    font-size: 8px;
    line-height: 1;
    animation: pulse 1.4s ease-in-out infinite;
  }

  @keyframes pulse {
    0%, 100% { opacity: 0.5; }
    50% { opacity: 1; }
  }

  .path {
    color: var(--text);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .dim {
    color: var(--text-dim);
  }

  .layout {
    display: grid;
    grid-template-columns: var(--sidebar-width, 240px) 4px 1fr 1fr;
    grid-template-rows: 1fr;
    overflow: hidden;
  }

  .layout.vertical-split {
    grid-template-columns: var(--sidebar-width, 240px) 4px 1fr;
    grid-template-rows: 1fr 1fr;
  }

  .layout.vertical-split .sidebar { grid-row: 1 / span 2; }
  .layout.vertical-split .resize-handle { grid-row: 1 / span 2; }
  .layout.vertical-split .editor-pane {
    grid-column: 3;
    grid-row: 1;
    border-right: none;
    border-bottom: 1px solid var(--border);
  }
  .layout.vertical-split .response-pane {
    grid-column: 3;
    grid-row: 2;
  }

  .layout.resizing {
    cursor: col-resize;
    user-select: none;
  }

  .layout.sidebar-hidden {
    grid-template-columns: 0 0 1fr 1fr;
  }

  .layout.sidebar-hidden.vertical-split {
    grid-template-columns: 0 0 1fr;
  }

  .layout.sidebar-hidden .sidebar,
  .layout.sidebar-hidden .resize-handle {
    display: none;
  }

  .resize-handle {
    background: var(--border);
    cursor: col-resize;
    transition: background 0.2s;
  }

  .resize-handle:hover {
    background: var(--accent-dim);
  }

  .sidebar {
    display: grid;
    grid-template-rows: auto 1fr auto;
    border-right: 1px solid var(--border);
    background: var(--bg-elev);
    overflow: hidden;
  }

  .sidebar-head {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 4px;
    padding: 6px;
    border-bottom: 1px solid var(--border);
  }

  .search-row {
    padding: 4px 6px;
    border-bottom: 1px solid var(--border);
  }

  .search-input {
    width: 100%;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 3px;
    padding: 3px 6px;
    color: var(--text);
    font-size: 11px;
  }

  .search-input:focus {
    border-color: var(--accent-dim);
  }

  .head-actions {
    display: flex;
    gap: 2px;
  }

  .head-btn {
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 2px 8px;
    font-size: 12px;
    cursor: pointer;
  }

  .head-btn:hover {
    border-color: var(--accent-dim);
  }

  .new-btn {
    width: 100%;
    text-align: left;
    background: transparent;
    border: 1px dashed var(--border);
    color: var(--text-dim);
    padding: 5px 10px;
    font-weight: 500;
    transition: all 0.12s ease;
  }

  .new-btn:hover {
    color: var(--accent);
    border-color: var(--accent);
    border-style: solid;
    background: var(--accent-soft);
  }

  .sidebar-list {
    overflow-y: auto;
  }

  .item {
    display: grid;
    grid-template-columns: 52px 1fr auto;
    align-items: center;
    gap: 8px;
    width: calc(100% - 12px);
    margin: 1px 6px;
    text-align: left;
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    padding: 5px 8px;
    font-size: 12px;
    cursor: pointer;
    color: var(--text-dim);
    transition: background 0.1s ease, color 0.1s ease;
  }

  .item:hover {
    background: var(--bg-elev-2);
    color: var(--text);
  }

  .item.nested {
    padding-left: 20px;
  }

  .group-label {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 10px 4px;
    font-size: 10px;
    color: var(--text-dim);
    font-family: var(--mono);
    text-transform: lowercase;
    letter-spacing: 0.04em;
    border-top: 1px solid var(--border);
  }

  .group-label:first-child {
    border-top: none;
  }

  .group-label.drop-target {
    background: rgba(139, 92, 246, 0.15);
    color: var(--accent);
  }

  .group-label.root-target {
    color: var(--text-dim);
    font-style: italic;
  }

  .group-run {
    background: transparent;
    border: 1px solid var(--border);
    color: var(--text-dim);
    font-size: 10px;
    padding: 0 5px;
    border-radius: 3px;
    cursor: pointer;
    line-height: 1.4;
  }

  .group-run:hover {
    color: var(--green);
    border-color: var(--green);
  }

  .item.active {
    background: var(--accent-soft);
    color: var(--text);
    box-shadow: inset 2px 0 0 var(--accent);
  }

  .item .m {
    font-size: 10px;
    text-align: center;
    padding: 1px 4px;
    border-radius: 3px;
    font-family: var(--mono);
  }

  .item .n {
    font-family: var(--mono);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .item .del {
    color: var(--text-dim);
    padding: 0 3px;
    border-radius: 3px;
    visibility: hidden;
    cursor: pointer;
    font-size: 11px;
  }

  .item .entry-actions {
    display: flex;
    gap: 1px;
  }

  .item:hover .del {
    visibility: visible;
  }

  .item .del:hover {
    color: var(--red);
    background: rgba(248, 113, 113, 0.12);
  }

  .empty {
    padding: 32px 16px;
    font-size: 12px;
    text-align: center;
    line-height: 1.6;
  }

  .sidebar-foot {
    border-top: 1px solid var(--border);
    background: var(--bg-elev);
  }

  .env-row {
    display: flex;
    gap: 4px;
    padding: 6px 8px;
    align-items: center;
  }

  .env-select {
    flex: 1;
    background: var(--bg-elev-2);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: 3px;
    padding: 2px 4px;
    font-size: 11px;
    font-family: var(--mono);
  }

  .env-edit {
    background: transparent;
    border: 1px solid var(--border);
    color: var(--text-dim);
    padding: 1px 6px;
    font-size: 11px;
    border-radius: 3px;
  }

  .env-edit:hover,
  .env-edit[aria-pressed="true"] {
    color: var(--text);
    border-color: var(--accent-dim);
  }

  .env-editor {
    border-top: 1px solid var(--border);
    padding: 6px 8px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    max-height: 240px;
    overflow-y: auto;
  }

  .env-pair {
    display: grid;
    grid-template-columns: 1fr 1fr auto;
    gap: 3px;
    align-items: center;
  }

  .env-pair:has(.oauth-label) {
    grid-template-columns: 100px 1fr;
  }

  .env-input.read-only {
    background: var(--bg);
    color: var(--text-dim);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    border-style: dashed;
  }

  .scope-label {
    font-size: 9px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    margin: 4px 0 2px 0;
  }

  .env-input {
    font-family: var(--mono);
    font-size: 11px;
    padding: 2px 5px;
    min-width: 0;
  }

  .env-del {
    background: transparent;
    border: none;
    color: var(--text-dim);
    padding: 0 4px;
    cursor: pointer;
    font-size: 14px;
  }

  .env-del:hover {
    color: var(--red);
  }

  .env-add {
    align-self: flex-start;
    background: transparent;
    border: 1px dashed var(--border);
    color: var(--text-dim);
    font-size: 11px;
    padding: 2px 8px;
  }

  .env-add:hover {
    color: var(--text);
    border-color: var(--accent-dim);
  }

  .folder-line {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px 6px;
    font-size: 10px;
    border-top: 1px solid var(--border);
  }

  .folder-line > span {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .theme-toggle {
    background: transparent;
    border: 1px solid var(--border);
    color: var(--text-dim);
    padding: 0 6px;
    font-size: 11px;
    border-radius: 3px;
    cursor: pointer;
  }

  .theme-toggle:hover {
    color: var(--accent);
    border-color: var(--accent-dim);
  }

  .cookies-row {
    display: flex;
    gap: 4px;
    padding: 4px 8px;
    border-top: 1px solid var(--border);
    align-items: center;
  }

  .cookies-toggle {
    background: transparent;
    border: 1px solid var(--border);
    color: var(--text-dim);
    padding: 1px 8px;
    font-size: 11px;
    border-radius: 3px;
    cursor: pointer;
  }

  .cookies-toggle:hover {
    color: var(--text);
    border-color: var(--accent-dim);
  }

  .cookies-clear {
    background: transparent;
    border: none;
    color: var(--text-dim);
    font-size: 10px;
    cursor: pointer;
    padding: 1px 6px;
  }

  .cookies-clear:hover {
    color: var(--red);
  }

  .cookies-list {
    border-top: 1px solid var(--border);
    max-height: 160px;
    overflow-y: auto;
    background: var(--bg);
  }

  .cookie-row {
    display: grid;
    grid-template-columns: 1fr auto auto auto;
    gap: 6px;
    padding: 3px 8px;
    font-family: var(--mono);
    font-size: 10px;
    border-bottom: 1px solid var(--border);
    align-items: center;
  }

  .cookie-domain {
    color: var(--text-dim);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .cookie-name {
    color: #7dd3fc;
  }

  .cookie-value {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 80px;
  }

  .oauth-help {
    font-size: 10px;
    line-height: 1.4;
    margin-bottom: 4px;
  }

  .oauth-help code {
    background: rgba(139, 92, 246, 0.15);
    color: #c4b5fd;
    padding: 0 4px;
    border-radius: 3px;
    font-family: var(--mono);
  }

  .oauth-label {
    font-size: 10px;
    color: var(--text-dim);
    align-self: center;
    padding-right: 6px;
  }

  .oauth-error {
    font-size: 10px;
    color: var(--red);
    background: rgba(248, 113, 113, 0.06);
    padding: 4px 6px;
    border-radius: 3px;
    word-break: break-word;
  }

  .toast {
    position: fixed;
    bottom: 20px;
    left: 50%;
    transform: translateX(-50%);
    background: var(--bg-elev-2);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    padding: 10px 18px;
    font-size: 12px;
    color: var(--text);
    z-index: 200;
    box-shadow: var(--shadow-lg);
    animation: toast-in 0.18s cubic-bezier(0.16, 1, 0.3, 1);
  }

  @keyframes toast-in {
    from { opacity: 0; transform: translate(-50%, 8px); }
    to   { opacity: 1; transform: translate(-50%, 0); }
  }

  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    z-index: 100;
  }

  .modal {
    position: fixed;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: min(640px, 90vw);
    max-height: 80vh;
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    z-index: 101;
    display: flex;
    flex-direction: column;
    box-shadow: var(--shadow-lg);
    overflow: hidden;
    animation: modal-in 0.16s cubic-bezier(0.16, 1, 0.3, 1);
  }

  @keyframes modal-in {
    from { opacity: 0; transform: translate(-50%, -48%) scale(0.97); }
    to   { opacity: 1; transform: translate(-50%, -50%) scale(1); }
  }

  .backdrop {
    animation: backdrop-in 0.16s ease-out;
  }

  @keyframes backdrop-in {
    from { opacity: 0; }
    to   { opacity: 1; }
  }

  .modal-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    border-bottom: 1px solid var(--border);
    padding: 8px 12px;
  }

  .modal .title {
    display: flex;
    gap: 8px;
    align-items: center;
    font-family: var(--mono);
  }

  .modal .close {
    background: transparent;
    border: none;
    color: var(--text-dim);
    font-size: 18px;
    cursor: pointer;
    padding: 0 8px;
  }

  .schema-body {
    overflow-y: auto;
    padding: 12px;
  }

  .schema-section {
    margin-bottom: 12px;
  }

  .schema-section h4 {
    margin: 0 0 4px 0;
    font-size: 12px;
    color: var(--accent);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .schema-section ul {
    margin: 0;
    padding: 0 0 0 16px;
    font-family: var(--mono);
    font-size: 11px;
  }

  .schema-section ul.types {
    columns: 2;
  }

  .shortcut-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 12px;
  }

  .shortcut-table td {
    padding: 4px 8px;
    border-bottom: 1px solid var(--border);
  }

  .shortcut-table td:first-child {
    width: 40%;
  }

  kbd {
    display: inline-block;
    background: var(--bg-elev-2);
    border: 1px solid var(--border);
    border-radius: 3px;
    padding: 1px 6px;
    font-family: var(--mono);
    font-size: 11px;
    margin: 0 1px;
  }

  .shortcut-section {
    margin-top: 12px;
    font-size: 12px;
    color: var(--text-dim);
  }

  .shortcut-section strong {
    color: var(--accent);
  }

  .loadstat-grid {
    display: grid;
    grid-template-columns: 100px 1fr;
    gap: 4px 12px;
    font-family: var(--mono);
    font-size: 12px;
  }

  .loadstat-grid > span:nth-child(odd) {
    color: var(--text-dim);
  }

  .loadstat-grid > span:nth-child(even) {
    color: var(--text);
  }

  .sparkline {
    vertical-align: middle;
  }

  .history-item {
    display: grid;
    grid-template-columns: 1fr auto;
  }

  .history-main {
    display: grid;
    grid-template-columns: 1fr auto auto;
    gap: 10px;
    background: transparent;
    border: none;
    padding: 6px 12px;
    text-align: left;
    cursor: pointer;
    font-family: var(--mono);
    font-size: 11px;
    color: var(--text);
  }

  .history-main:hover {
    background: var(--bg-elev-2);
  }

  .diff-pick-btn {
    background: transparent;
    border: none;
    color: var(--text-dim);
    padding: 0 8px;
    cursor: pointer;
    font-family: var(--mono);
    font-size: 12px;
  }

  .diff-pick-btn:hover {
    color: var(--accent);
  }

  .history-item.diff-pick {
    box-shadow: inset 2px 0 0 var(--accent);
    background: rgba(139, 92, 246, 0.08);
  }

  .diff-hint {
    padding: 4px 12px;
    font-size: 10px;
    border-bottom: 1px solid var(--border);
  }

  .diff-modal {
    width: min(900px, 95vw);
    max-height: 90vh;
  }

  .diff-body {
    overflow-y: auto;
    padding: 12px;
  }

  .diff-cols {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
    margin-bottom: 12px;
  }

  .diff-col {
    border: 1px solid var(--border);
    border-radius: 4px;
    overflow: hidden;
    max-height: 300px;
    display: flex;
    flex-direction: column;
  }

  .diff-col-head {
    padding: 6px 10px;
    font-family: var(--mono);
    font-size: 11px;
    background: var(--bg-elev-2);
    border-bottom: 1px solid var(--border);
    display: flex;
    gap: 8px;
  }

  .diff-text {
    margin: 0;
    padding: 8px 10px;
    font-family: var(--mono);
    font-size: 11px;
    overflow: auto;
    background: var(--bg);
    flex: 1;
  }

  .diff-inline {
    border: 1px solid var(--border);
    border-radius: 4px;
    overflow: hidden;
    max-height: 400px;
    overflow-y: auto;
  }

  .diff-line {
    display: grid;
    grid-template-columns: 24px 1fr;
    font-family: var(--mono);
    font-size: 11px;
    padding: 1px 0;
  }

  .diff-add {
    background: rgba(74, 222, 128, 0.08);
  }

  .diff-add .diff-marker {
    color: var(--green);
  }

  .diff-del {
    background: rgba(248, 113, 113, 0.08);
  }

  .diff-del .diff-marker {
    color: var(--red);
  }

  .diff-marker {
    text-align: center;
    color: var(--text-dim);
    user-select: none;
  }

  .clipboard-banner {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 12px;
    background: rgba(74, 222, 128, 0.06);
    border-bottom: 1px solid rgba(74, 222, 128, 0.3);
    color: var(--text);
    font-size: 11px;
  }

  .clipboard-banner > span {
    flex: 1;
  }

  .clipboard-use {
    background: var(--green);
    color: black;
    border: none;
    padding: 2px 10px;
    border-radius: 3px;
    font-size: 11px;
    cursor: pointer;
    font-weight: 500;
  }

  .clipboard-dismiss {
    background: transparent;
    border: none;
    color: var(--text-dim);
    cursor: pointer;
    font-size: 14px;
    padding: 0 6px;
  }

  .active-mock {
    background: rgba(74, 222, 128, 0.15);
    color: var(--green);
    border-color: var(--green);
  }

  .render-toggle {
    padding: 4px 12px;
    border-bottom: 1px solid var(--border);
  }

  .render-frame {
    width: 100%;
    height: 100%;
    min-height: 400px;
    border: none;
    background: white;
  }

  .json-table-wrap {
    overflow: auto;
    max-height: 500px;
  }

  .json-table {
    width: 100%;
    border-collapse: collapse;
    font-family: var(--mono);
    font-size: 11px;
  }

  .json-table th {
    background: var(--bg-elev);
    color: var(--text-dim);
    text-align: left;
    padding: 6px 10px;
    border-bottom: 1px solid var(--border);
    position: sticky;
    top: 0;
    text-transform: lowercase;
    letter-spacing: 0.04em;
    font-weight: 500;
  }

  .json-table td {
    padding: 4px 10px;
    border-bottom: 1px solid var(--border);
    color: var(--text);
    max-width: 240px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .json-table tbody tr:hover {
    background: var(--bg-elev-2);
  }

  .json-tree {
    padding: 8px 12px;
    font-family: var(--mono);
    font-size: 11px;
    overflow: auto;
    max-height: 500px;
  }

  .tree-node {
    display: inline-block;
    width: 100%;
  }

  .tree-node summary {
    cursor: pointer;
    list-style: none;
    color: var(--text-dim);
  }

  .tree-node summary::-webkit-details-marker {
    display: none;
  }

  .tree-node summary::before {
    content: "▶ ";
    font-size: 9px;
    margin-right: 2px;
  }

  .tree-node[open] > summary::before {
    content: "▼ ";
  }

  .tree-children {
    padding-left: 14px;
    border-left: 1px solid var(--border);
    margin-left: 4px;
  }

  .kv {
    display: flex;
    gap: 6px;
    align-items: flex-start;
    padding: 1px 0;
  }

  .kv .k {
    color: #7dd3fc;
  }

  .leaf.string { color: #a3e635; }
  .leaf.number { color: #fb923c; }
  .leaf.bool { color: #f472b6; }
  .leaf.null { color: var(--text-dim); }

  .leaf.clickable {
    cursor: pointer;
    border-bottom: 1px dashed transparent;
  }

  .leaf.clickable:hover {
    border-bottom-color: var(--accent);
    background: rgba(139, 92, 246, 0.08);
  }

  .copy-path {
    background: transparent;
    border: none;
    color: var(--text-dim);
    cursor: pointer;
    font-size: 10px;
    margin-left: 4px;
    padding: 0 4px;
  }

  .copy-path:hover {
    color: var(--accent);
  }

  .diff-table {
    border-bottom: 1px solid var(--border);
    background: var(--bg-elev);
  }

  .diff-head {
    padding: 6px 12px;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .diff-row {
    display: flex;
    gap: 12px;
    padding: 4px 12px;
    align-items: center;
    font-family: var(--mono);
    font-size: 11px;
    border-top: 1px solid var(--border);
  }

  .diff-row .env-name {
    color: var(--accent);
    min-width: 100px;
    font-weight: 500;
  }

  .diff-row .shape {
    font-size: 10px;
    color: var(--text-dim);
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .diff-foot {
    padding: 4px 12px;
    text-align: right;
    border-top: 1px solid var(--border);
  }

  .warn {
    color: #facc15;
    font-size: 11px;
  }

  .vars-line {
    font-family: var(--mono);
    text-transform: none;
    letter-spacing: 0;
    font-size: 10px;
  }

  .params-panel {
    border-bottom: 1px solid var(--border);
    background: var(--bg-elev);
    padding: 6px 12px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .param-row {
    display: grid;
    grid-template-columns: 1fr 2fr auto;
    gap: 4px;
  }

  .regression {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    padding: 6px 12px;
    border-bottom: 1px solid var(--border);
    background: rgba(248, 113, 113, 0.08);
    color: var(--red);
    font-size: 11px;
  }

  .reg-label {
    font-weight: 600;
  }

  .reg-issue {
    background: rgba(248, 113, 113, 0.18);
    padding: 1px 6px;
    border-radius: 3px;
    font-family: var(--mono);
  }

  .assertions {
    border-bottom: 1px solid var(--border);
    background: var(--bg-elev);
  }

  .assertions-head {
    padding: 6px 12px;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .assertion-row {
    display: grid;
    grid-template-columns: 16px 1fr auto;
    gap: 8px;
    padding: 3px 12px;
    font-family: var(--mono);
    font-size: 11px;
  }

  .assertion-row.pass {
    color: var(--green);
  }

  .assertion-row.pass .raw {
    color: var(--text-dim);
  }

  .assertion-row.fail {
    color: var(--red);
  }

  .assertion-row .dot {
    text-align: center;
    font-weight: 600;
  }

  .assertion-row .reason {
    color: var(--red);
    opacity: 0.85;
  }

  .review-strip {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    border-bottom: 1px solid var(--border);
    background: rgba(250, 204, 21, 0.06);
    font-size: 11px;
  }

  .review-label {
    color: #facc15;
    font-weight: 500;
  }

  .review-chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: var(--bg-elev-2);
    border: 1px solid var(--border);
    padding: 2px 8px;
    border-radius: 12px;
    font-size: 11px;
    color: var(--text);
    cursor: pointer;
    font-family: inherit;
  }

  .review-chip code {
    background: rgba(139, 92, 246, 0.15);
    color: #c4b5fd;
    padding: 0 4px;
    border-radius: 3px;
    font-size: 10px;
  }

  .review-chip:hover {
    border-color: var(--accent-dim);
  }

  .review-all {
    background: var(--accent);
    color: white;
    border: 1px solid var(--accent);
    padding: 2px 10px;
    border-radius: 12px;
    font-size: 11px;
    cursor: pointer;
  }

  .review-all:hover {
    background: var(--accent-dim);
  }

  .editor-pane,
  .response-pane {
    display: grid;
    grid-template-rows: auto 1fr;
    overflow: hidden;
  }

  .editor-pane {
    border-right: 1px solid var(--border);
  }

  .pane-label {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 14px;
    color: var(--text-faint);
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    border-bottom: 1px solid var(--border);
    background: var(--bg-elev);
  }

  .history-toggle {
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 3px;
    color: var(--text-dim);
    font-size: 10px;
    padding: 1px 6px;
    text-transform: none;
    letter-spacing: 0;
  }

  .history-toggle:hover {
    color: var(--text);
    border-color: var(--accent-dim);
  }

  .history-list {
    max-height: 200px;
    overflow-y: auto;
    border-bottom: 1px solid var(--border);
    background: var(--bg-elev);
  }

  .history-item {
    display: grid;
    grid-template-columns: 1fr auto auto;
    gap: 10px;
    width: 100%;
    text-align: left;
    background: transparent;
    border: none;
    border-radius: 0;
    border-bottom: 1px solid var(--border);
    padding: 6px 12px;
    font-family: var(--mono);
    font-size: 11px;
    cursor: pointer;
    color: var(--text);
  }

  .history-item:hover {
    background: var(--bg-elev-2);
  }

  .history-item.active {
    background: var(--bg-elev-2);
    box-shadow: inset 2px 0 0 var(--accent);
  }

  .history-item .hs {
    font-weight: 600;
  }

  .editor-mount {
    overflow: hidden;
    background: var(--bg);
  }

  .hint {
    padding: 8px 12px;
    font-size: 12px;
    border-top: 1px solid var(--border);
  }

  .hint.error {
    color: var(--red);
    background: rgba(248, 113, 113, 0.06);
  }

  .error-hint {
    margin-top: 4px;
    font-size: 11px;
    color: var(--text-dim);
    font-style: italic;
  }

  .response-pane {
    overflow: auto;
  }

  .status-bar {
    display: flex;
    gap: 12px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
    font-family: var(--mono);
    font-size: 12px;
  }

  .status {
    font-weight: 600;
  }

  details {
    border-bottom: 1px solid var(--border);
  }

  summary {
    padding: 6px 12px;
    cursor: pointer;
    color: var(--text-dim);
    font-size: 12px;
    user-select: none;
  }

  summary:hover {
    color: var(--text);
  }

  pre {
    margin: 0;
    padding: 12px;
    font-family: var(--mono);
    font-size: 12px;
    white-space: pre-wrap;
    word-break: break-word;
    overflow-x: auto;
  }

  pre.headers {
    background: var(--bg-elev);
    color: var(--text-dim);
    border-top: 1px solid var(--border);
  }

  pre.body {
    background: var(--bg);
    overflow-y: auto;
  }

  .ws-pane {
    display: grid;
    grid-template-rows: 1fr auto;
    overflow: hidden;
  }

  .ws-messages {
    overflow-y: auto;
    padding: 6px 0;
  }

  .ws-msg {
    display: grid;
    grid-template-columns: auto auto 1fr;
    gap: 8px;
    padding: 4px 12px;
    font-family: var(--mono);
    font-size: 11px;
    border-bottom: 1px solid var(--border);
  }

  .ws-msg-out {
    background: rgba(139, 92, 246, 0.05);
  }

  .ws-msg .ts {
    font-size: 10px;
    align-self: center;
  }

  .ws-msg .dir {
    color: var(--accent);
    font-weight: 700;
    align-self: start;
  }

  .ws-msg-out .dir {
    color: var(--green);
  }

  .ws-text {
    margin: 0;
    padding: 0;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .ws-send {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 4px;
    padding: 6px;
    border-top: 1px solid var(--border);
    background: var(--bg-elev);
  }

  .ws-draft {
    font-family: var(--mono);
    font-size: 12px;
    min-height: 50px;
    resize: vertical;
  }

  .status-connected {
    color: var(--green);
  }

  .script-logs {
    border-bottom: 1px solid var(--border);
  }

  .script-logs summary {
    color: var(--accent);
    font-family: var(--mono);
    font-size: 11px;
  }

  pre.logs {
    background: var(--bg-elev);
    color: var(--text);
    border-top: 1px solid var(--border);
    font-size: 11px;
  }
</style>
