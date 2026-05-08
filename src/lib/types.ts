export type HttpMethod =
  | "GET"
  | "POST"
  | "PUT"
  | "PATCH"
  | "DELETE"
  | "HEAD"
  | "OPTIONS";

export interface RequestSpec {
  method: HttpMethod;
  url: string;
  headers: Array<[string, string]>;
  body: string | null;
}

export interface ResponseData {
  status: number;
  status_text: string;
  headers: Array<[string, string]>;
  body: string;
  elapsed_ms: number;
}

export interface SavedRequest {
  name: string;
  path: string;
  spec: RequestSpec;
  savedAt: string;
}
