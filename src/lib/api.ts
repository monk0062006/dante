import { invoke } from "@tauri-apps/api/core";
import type { RequestSpec, ResponseData } from "./types";

export interface DigestCreds {
  username: string;
  password: string;
}

export interface ResponseDataForScript {
  status: number;
  status_text: string;
  headers: Array<[string, string]>;
  body: string;
  elapsed_ms: number;
}

export interface PmTestResult {
  name: string;
  pass: boolean;
  error?: string | null;
}

export interface ScriptOutcome {
  env: Array<[string, string]>;
  headers: Array<[string, string]>;
  logs: string[];
  tests: PmTestResult[];
  error: string | null;
}

export async function runScript(args: {
  script: string;
  env: Array<[string, string]>;
  method: string;
  url: string;
  headers: Array<[string, string]>;
  body: string | null;
  response: ResponseDataForScript | null;
  timeoutMs?: number;
}): Promise<ScriptOutcome> {
  return invoke<ScriptOutcome>("run_script", {
    args: {
      script: args.script,
      env: args.env,
      method: args.method,
      url: args.url,
      headers: args.headers,
      body: args.body,
      response: args.response,
      timeout_ms: args.timeoutMs ?? 5000,
    },
  });
}

export async function runRequest(
  spec: RequestSpec,
  aws: AwsParams | null = null,
  digest: DigestCreds | null = null,
): Promise<ResponseData> {
  return invoke<ResponseData>("run_request", { spec, aws, digest });
}

export interface Settings {
  project_folder: string | null;
  active_env?: string | null;
}

export interface EnvFile {
  name: string;
  path: string;
}

export type EnvPair = [string, string];

export async function listEnvs(folder: string): Promise<EnvFile[]> {
  return invoke<EnvFile[]>("list_envs", { folder });
}

export async function readEnv(path: string): Promise<EnvPair[]> {
  return invoke<EnvPair[]>("read_env", { path });
}

export async function writeEnv(path: string, pairs: EnvPair[]): Promise<void> {
  return invoke<void>("write_env", { path, pairs });
}

export async function createEnv(folder: string, name: string): Promise<string> {
  return invoke<string>("create_env", { folder, name });
}

export interface CookieView {
  domain: string;
  name: string;
  value: string;
  path: string;
}

export async function listCookies(): Promise<CookieView[]> {
  return invoke<CookieView[]>("list_cookies");
}

export async function clearCookies(): Promise<void> {
  return invoke<void>("clear_cookies");
}

export async function deleteCookie(domain: string, path: string, name: string): Promise<void> {
  return invoke<void>("delete_cookie", { domain, path, name });
}

export async function loadCookies(folder: string): Promise<number> {
  return invoke<number>("load_cookies", { folder });
}

export async function saveCookies(folder: string): Promise<void> {
  return invoke<void>("save_cookies", { folder });
}

export interface OAuthTokenResponse {
  access_token?: string;
  token_type?: string;
  expires_in?: number;
  scope?: string;
  [key: string]: unknown;
}

export async function fetchOauthToken(
  tokenUrl: string,
  clientId: string,
  clientSecret: string,
  scope: string | null,
): Promise<OAuthTokenResponse> {
  return invoke<OAuthTokenResponse>("fetch_oauth_token", {
    tokenUrl,
    clientId,
    clientSecret,
    scope,
  });
}

export interface AiExtract {
  var_name: string;
  source: string;
}

export interface AiReview {
  suggested_name: string;
  summary: string;
  tests: string[];
  extracts: AiExtract[];
  security_observations: string[];
}

export async function aiReviewRequest(
  apiKey: string,
  method: string,
  url: string,
  headers: Array<[string, string]>,
  body: string | null,
): Promise<AiReview> {
  return invoke<AiReview>("ai_review_request", {
    apiKey,
    method,
    url,
    headers,
    body,
  });
}

export async function aiReviewRequestOpenaiCompat(
  baseUrl: string,
  apiKey: string,
  model: string,
  supportsJsonMode: boolean,
  method: string,
  url: string,
  headers: Array<[string, string]>,
  body: string | null,
): Promise<AiReview> {
  return invoke<AiReview>("ai_review_request_openai_compat", {
    baseUrl,
    apiKey,
    model,
    supportsJsonMode,
    method,
    url,
    headers,
    body,
  });
}

export interface DeviceAuthInit {
  user_code: string;
  verification_uri: string;
  verification_uri_complete: string | null;
  device_code: string;
  expires_in: number;
  interval: number;
}

export async function oauthDeviceInit(
  deviceAuthorizationUrl: string,
  clientId: string,
  scope: string | null,
): Promise<DeviceAuthInit> {
  return invoke<DeviceAuthInit>("oauth_device_init", {
    deviceAuthorizationUrl,
    clientId,
    scope,
  });
}

export async function oauthDevicePoll(
  tokenUrl: string,
  clientId: string,
  clientSecret: string | null,
  deviceCode: string,
  intervalSec: number,
  expiresInSec: number,
): Promise<OAuthTokenResponse> {
  return invoke<OAuthTokenResponse>("oauth_device_poll", {
    tokenUrl,
    clientId,
    clientSecret,
    deviceCode,
    intervalSec,
    expiresInSec,
  });
}

export async function fetchOauthRefresh(
  tokenUrl: string,
  clientId: string,
  clientSecret: string | null,
  refreshToken: string,
  scope: string | null,
): Promise<OAuthTokenResponse> {
  return invoke<OAuthTokenResponse>("fetch_oauth_refresh", {
    tokenUrl,
    clientId,
    clientSecret,
    refreshToken,
    scope,
  });
}

export async function fetchOauthPassword(
  tokenUrl: string,
  clientId: string,
  clientSecret: string | null,
  username: string,
  password: string,
  scope: string | null,
): Promise<OAuthTokenResponse> {
  return invoke<OAuthTokenResponse>("fetch_oauth_password", {
    tokenUrl,
    clientId,
    clientSecret,
    username,
    password,
    scope,
  });
}

export async function wsConnect(url: string, id: string): Promise<void> {
  return invoke<void>("ws_connect", { url, id });
}

export async function wsSend(id: string, text: string): Promise<void> {
  return invoke<void>("ws_send", { id, text });
}

export async function wsClose(id: string): Promise<void> {
  return invoke<void>("ws_close", { id });
}

export async function fetchOauthAuthcode(
  authUrl: string,
  tokenUrl: string,
  clientId: string,
  clientSecret: string,
  scope: string | null,
  redirectPort: number,
): Promise<OAuthTokenResponse> {
  return invoke<OAuthTokenResponse>("fetch_oauth_authcode", {
    authUrl,
    tokenUrl,
    clientId,
    clientSecret,
    scope,
    redirectPort,
  });
}

export interface ImportResult {
  created: string[];
  skipped: string[];
}

export async function importOpenapi(folder: string, specPath: string): Promise<ImportResult> {
  return invoke<ImportResult>("import_openapi", { folder, specPath });
}

export async function exportMarkdown(folder: string): Promise<string> {
  return invoke<string>("export_markdown", { folder });
}

export async function importPostman(folder: string, specPath: string): Promise<ImportResult> {
  return invoke<ImportResult>("import_postman", { folder, specPath });
}

export async function importHar(folder: string, specPath: string): Promise<ImportResult> {
  return invoke<ImportResult>("import_har", { folder, specPath });
}

export async function importInsomnia(folder: string, specPath: string): Promise<ImportResult> {
  return invoke<ImportResult>("import_insomnia", { folder, specPath });
}

export async function exportOpenapi(folder: string): Promise<string> {
  return invoke<string>("export_openapi", { folder });
}

export async function exportPostman(folder: string): Promise<string> {
  return invoke<string>("export_postman", { folder });
}

export async function monitorStart(requestPath: string, intervalSecs: number): Promise<void> {
  return invoke<void>("monitor_start", { requestPath, intervalSecs });
}

export async function monitorStop(requestPath: string): Promise<void> {
  return invoke<void>("monitor_stop", { requestPath });
}

export async function monitorList(): Promise<Array<[string, number]>> {
  return invoke<Array<[string, number]>>("monitor_list");
}

export async function monitorParseSchedule(content: string): Promise<number | null> {
  return invoke<number | null>("monitor_parse_schedule", { content });
}

export async function graphqlIntrospect(
  url: string,
  headers: Array<[string, string]>,
): Promise<unknown> {
  return invoke<unknown>("graphql_introspect", { url, headers });
}

export interface AwsParams {
  access_key: string;
  secret_key: string;
  region: string;
  service: string;
  session_token?: string | null;
}

export interface MockStatus {
  running: boolean;
  port: number | null;
}

export async function startMockServer(folder: string, port: number): Promise<MockStatus> {
  return invoke<MockStatus>("start_mock_server", { folder, port });
}

export async function stopMockServer(): Promise<MockStatus> {
  return invoke<MockStatus>("stop_mock_server");
}

export async function mockServerStatus(): Promise<MockStatus> {
  return invoke<MockStatus>("mock_server_status");
}


export interface RequestEntry {
  name: string;
  path: string;
  folder: string;
  method: string;
  url: string;
  description: string;
  modified_ms: number;
}

export async function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

export async function saveSettings(settings: Settings): Promise<void> {
  return invoke<void>("save_settings", { settings });
}

export async function defaultProjectFolder(): Promise<string> {
  return invoke<string>("default_project_folder");
}

export interface WorkspaceConfig {
  default_headers: Record<string, string>;
  base_url: string | null;
  timeout_secs: number | null;
}

export async function readWorkspaceConfig(folder: string): Promise<WorkspaceConfig> {
  return invoke<WorkspaceConfig>("read_workspace_config", { folder });
}

export async function writeWorkspaceConfig(folder: string, config: WorkspaceConfig): Promise<void> {
  return invoke<void>("write_workspace_config", { folder, config });
}

export async function saveRequest(
  folder: string,
  name: string,
  content: string,
  overwritePath: string | null,
  subfolder: string | null = null,
): Promise<string> {
  return invoke<string>("save_request", {
    folder,
    name,
    content,
    overwritePath,
    subfolder,
  });
}

export async function listRequests(folder: string): Promise<RequestEntry[]> {
  return invoke<RequestEntry[]>("list_requests", { folder });
}

export async function loadRequest(path: string): Promise<string> {
  return invoke<string>("load_request", { path });
}

export async function deleteRequest(path: string): Promise<void> {
  return invoke<void>("delete_request", { path });
}

export async function renameRequest(oldPath: string, newName: string): Promise<string> {
  return invoke<string>("rename_request", { oldPath, newName });
}

export async function duplicateRequest(path: string): Promise<string> {
  return invoke<string>("duplicate_request", { path });
}

export async function moveRequest(
  path: string,
  targetFolder: string,
  root: string,
): Promise<string> {
  return invoke<string>("move_request", { path, targetFolder, root });
}

export async function renameFolder(root: string, oldName: string, newName: string): Promise<string> {
  return invoke<string>("rename_folder", { root, oldName, newName });
}

export async function deleteFolder(root: string, folderName: string): Promise<void> {
  return invoke<void>("delete_folder", { root, folderName });
}

export interface HistoryEntry {
  ts: number;
  request: RequestSpec;
  response: ResponseData;
}

export async function appendHistory(
  requestPath: string,
  request: RequestSpec,
  response: ResponseData,
): Promise<number> {
  return invoke<number>("append_history", { requestPath, request, response });
}

export async function readHistory(
  requestPath: string,
  limit: number | null = null,
): Promise<HistoryEntry[]> {
  return invoke<HistoryEntry[]>("read_history", { requestPath, limit });
}
