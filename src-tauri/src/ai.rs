use serde::{Deserialize, Serialize};
use std::time::Duration;

const ANTHROPIC_ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const REVIEW_MODEL: &str = "claude-sonnet-4-6";

const SYSTEM_PROMPT: &str = r#"You are Dante's API review assistant. The user has just pasted a curl command into Dante (an HTTP client similar to Postman/Bruno but lighter and local-first). Your job is to look at the parsed request and propose improvements that the user can accept with one click.

Output a single JSON object matching the supplied schema. Be concise; every field counts toward what the user sees.

Field guidance:

- `suggested_name`: a short, human-friendly name for the saved request file. Use kebab-case, no extension. Prefer the resource and action: "users-list", "user-create", "auth-login", "stripe-charge-create". Avoid the word "request". 1-4 words.

- `summary`: one sentence (≤120 chars) describing what the endpoint probably does. Reason from the URL path, method, body, and headers. Example: "Lists active users in the workspace, paginated by limit/offset query params."

- `tests`: 3-5 assertion lines in Dante's test DSL. Available syntax:
  - `status == 200`, `status >= 200 && status < 300`
  - `elapsed < 1000`
  - `body.field exists`, `body.path.to.field == "value"`
  - `header content-type contains "json"`
  - Operators: `==`, `!=`, `<`, `>`, `<=`, `>=`, `exists`, `!exists`, `contains`, `!contains`, `matches`
  Pick assertions that match the *expected* shape of a successful response based on the request semantics. For a list endpoint expect an array or `body.data exists`. For an auth endpoint expect a token field. Always include at least a status assertion and a content-type assertion.

- `extracts`: variables to capture from the response into the user's env file (so subsequent requests can use them as `{{varName}}`). Source format: `body.path` (JSON path), `header X-Header-Name`, `cookie name`, or `status`. Suggest extracts only for endpoints that produce reusable values: login responses → `token`, create endpoints → the new resource id, list endpoints might extract `body.data[0].id` for the first item. Skip if nothing is reusable.

- `security_observations`: short notes about anything risky or surprising. Examples: "Bearer token is hardcoded — recommend extracting to env var", "API key in URL query param — prefer header", "Body contains user PII (email, phone) — confirm intent", "No User-Agent header — some APIs reject this", "Endpoint hits a localhost URL — confirm intentional", "Authorization missing on what looks like a private endpoint". Empty array is fine if nothing is notable. Do NOT include generic remarks like "consider HTTPS" — focus on signal that's specific to this exact request.

Worked example 1:

INPUT:
POST https://api.example.com/auth/login
Content-Type: application/json

{"username": "alice", "password": "hunter2"}

OUTPUT:
{
  "suggested_name": "auth-login",
  "summary": "Authenticates a user with username and password, likely returning an access token.",
  "tests": [
    "status == 200",
    "header content-type contains \"json\"",
    "body.token exists",
    "elapsed < 2000"
  ],
  "extracts": [
    {"var_name": "token", "source": "body.token"},
    {"var_name": "userId", "source": "body.user.id"}
  ],
  "security_observations": [
    "Body contains a literal password — recommend extracting to {{password}} env var"
  ]
}

Worked example 2:

INPUT:
GET https://api.stripe.com/v1/charges?limit=10
Authorization: Bearer sk_test_abc123

OUTPUT:
{
  "suggested_name": "charges-list",
  "summary": "Lists the most recent 10 charges from Stripe.",
  "tests": [
    "status == 200",
    "header content-type contains \"json\"",
    "body.data exists",
    "body.has_more exists",
    "elapsed < 3000"
  ],
  "extracts": [
    {"var_name": "firstChargeId", "source": "body.data[0].id"}
  ],
  "security_observations": [
    "Bearer token is hardcoded — recommend extracting to {{stripeKey}} env var",
    "Test mode key (sk_test_*) — confirm not running against production"
  ]
}

Worked example 3:

INPUT:
GET https://httpbin.org/get
Accept: application/json

OUTPUT:
{
  "suggested_name": "httpbin-get",
  "summary": "Echoes the request back as JSON; useful as a sanity-check endpoint.",
  "tests": [
    "status == 200",
    "header content-type contains \"json\"",
    "body.url exists",
    "body.headers.Accept == \"application/json\""
  ],
  "extracts": [],
  "security_observations": []
}

Now do the same for the request below. Return only the JSON object."#;

#[derive(Serialize)]
pub struct ExtractRule {
    pub var_name: String,
    pub source: String,
}

#[derive(Serialize)]
pub struct ReviewResult {
    pub suggested_name: String,
    pub summary: String,
    pub tests: Vec<String>,
    pub extracts: Vec<ExtractRule>,
    pub security_observations: Vec<String>,
}

#[derive(Deserialize)]
struct ReviewRaw {
    suggested_name: String,
    summary: String,
    tests: Vec<String>,
    extracts: Vec<ExtractRaw>,
    security_observations: Vec<String>,
}

#[derive(Deserialize)]
struct ExtractRaw {
    var_name: String,
    source: String,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContentBlock>,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum AnthropicContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(other)]
    Other,
}

pub async fn review_request(
    api_key: String,
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<String>,
) -> Result<ReviewResult, String> {
    let user_prompt = build_user_prompt(&method, &url, &headers, body.as_deref());

    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "suggested_name": {"type": "string"},
            "summary": {"type": "string"},
            "tests": {
                "type": "array",
                "items": {"type": "string"}
            },
            "extracts": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "var_name": {"type": "string"},
                        "source": {"type": "string"}
                    },
                    "required": ["var_name", "source"],
                    "additionalProperties": false
                }
            },
            "security_observations": {
                "type": "array",
                "items": {"type": "string"}
            }
        },
        "required": ["suggested_name", "summary", "tests", "extracts", "security_observations"],
        "additionalProperties": false
    });

    let request_body = serde_json::json!({
        "model": REVIEW_MODEL,
        "max_tokens": 2048,
        "system": [
            {
                "type": "text",
                "text": SYSTEM_PROMPT,
                "cache_control": {"type": "ephemeral"}
            }
        ],
        "messages": [
            {"role": "user", "content": user_prompt}
        ],
        "output_config": {
            "format": {
                "type": "json_schema",
                "schema": schema
            }
        }
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;

    let body_str = serde_json::to_string(&request_body)
        .map_err(|e| format!("serialize: {e}"))?;

    let resp = client
        .post(ANTHROPIC_ENDPOINT)
        .header("x-api-key", api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("content-type", "application/json")
        .body(body_str)
        .send()
        .await
        .map_err(|e| format!("anthropic request failed: {e}"))?;

    let status = resp.status();
    let body_text = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("anthropic {status}: {}", truncate(&body_text, 400)));
    }

    let parsed: AnthropicResponse =
        serde_json::from_str(&body_text).map_err(|e| format!("response not JSON: {e}"))?;

    let text = parsed
        .content
        .into_iter()
        .find_map(|b| match b {
            AnthropicContentBlock::Text { text } => Some(text),
            _ => None,
        })
        .ok_or_else(|| "anthropic response had no text block".to_string())?;

    let raw: ReviewRaw = serde_json::from_str(&text)
        .map_err(|e| format!("could not parse review JSON ({e}): {}", truncate(&text, 200)))?;

    Ok(ReviewResult {
        suggested_name: raw.suggested_name,
        summary: raw.summary,
        tests: raw.tests,
        extracts: raw
            .extracts
            .into_iter()
            .map(|e| ExtractRule {
                var_name: e.var_name,
                source: e.source,
            })
            .collect(),
        security_observations: raw.security_observations,
    })
}

fn build_user_prompt(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: Option<&str>,
) -> String {
    let mut s = String::new();
    s.push_str(method);
    s.push(' ');
    s.push_str(url);
    s.push('\n');
    for (k, v) in headers {
        let v_safe = if is_secret_header(k) {
            redact(v)
        } else {
            v.clone()
        };
        s.push_str(&format!("{k}: {v_safe}\n"));
    }
    if let Some(b) = body {
        s.push('\n');
        s.push_str(b);
    }
    s
}

fn is_secret_header(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "authorization"
        || lower == "cookie"
        || lower.contains("api-key")
        || lower.contains("apikey")
        || lower.contains("token")
        || lower.contains("secret")
}

fn redact(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.starts_with("{{") && trimmed.ends_with("}}") {
        return value.to_string();
    }
    if let Some(rest) = trimmed.strip_prefix("Bearer ") {
        return format!("Bearer {}", redact_token(rest));
    }
    if let Some(rest) = trimmed.strip_prefix("Basic ") {
        return format!("Basic {}", redact_token(rest));
    }
    redact_token(trimmed)
}

fn redact_token(t: &str) -> String {
    if t.len() <= 8 {
        return "<redacted>".to_string();
    }
    let head: String = t.chars().take(4).collect();
    let tail: String = t.chars().rev().take(2).collect::<String>().chars().rev().collect();
    format!("{head}…{tail} <redacted>")
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}…", &s[..n])
    }
}

#[derive(Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}

#[derive(Deserialize)]
struct OpenAiMessage {
    content: Option<String>,
}

pub async fn review_request_openai_compat(
    base_url: String,
    api_key: String,
    model: String,
    supports_json_mode: bool,
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<String>,
) -> Result<ReviewResult, String> {
    let user_prompt = build_user_prompt(&method, &url, &headers, body.as_deref());

    let mut request_body = serde_json::json!({
        "model": model,
        "max_tokens": 2048,
        "messages": [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": format!("{}\n\nReturn ONLY a JSON object — no markdown fences, no preamble.", user_prompt)}
        ]
    });
    if supports_json_mode {
        if let Some(obj) = request_body.as_object_mut() {
            obj.insert(
                "response_format".to_string(),
                serde_json::json!({"type": "json_object"}),
            );
        }
    }

    let endpoint = format!(
        "{}/chat/completions",
        base_url.trim_end_matches('/')
    );

    let body_str = serde_json::to_string(&request_body)
        .map_err(|e| format!("serialize: {e}"))?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client
        .post(&endpoint)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .body(body_str)
        .send()
        .await
        .map_err(|e| format!("AI provider request failed: {e}"))?;

    let status = resp.status();
    let body_text = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("provider {status}: {}", truncate(&body_text, 400)));
    }

    let parsed: OpenAiResponse = serde_json::from_str(&body_text)
        .map_err(|e| format!("response not OpenAI-shaped JSON: {e}"))?;

    let content = parsed
        .choices
        .into_iter()
        .next()
        .and_then(|c| c.message.content)
        .ok_or_else(|| "provider response had no message content".to_string())?;

    let stripped = strip_json_fence(&content);
    let raw: ReviewRaw = serde_json::from_str(stripped)
        .map_err(|e| format!("could not parse review JSON ({e}): {}", truncate(stripped, 200)))?;

    Ok(ReviewResult {
        suggested_name: raw.suggested_name,
        summary: raw.summary,
        tests: raw.tests,
        extracts: raw
            .extracts
            .into_iter()
            .map(|e| ExtractRule {
                var_name: e.var_name,
                source: e.source,
            })
            .collect(),
        security_observations: raw.security_observations,
    })
}

fn strip_json_fence(s: &str) -> &str {
    let trimmed = s.trim();
    if let Some(rest) = trimmed.strip_prefix("```json") {
        return rest.trim_start().trim_end_matches("```").trim();
    }
    if let Some(rest) = trimmed.strip_prefix("```") {
        return rest.trim_start().trim_end_matches("```").trim();
    }
    trimmed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_secret_header_catches_common_auth_headers() {
        assert!(is_secret_header("Authorization"));
        assert!(is_secret_header("authorization"));
        assert!(is_secret_header("Cookie"));
        assert!(is_secret_header("X-API-Key"));
        assert!(is_secret_header("X-ApiKey"));
        assert!(is_secret_header("X-Auth-Token"));
        assert!(is_secret_header("X-Secret-Header"));
        assert!(!is_secret_header("Content-Type"));
        assert!(!is_secret_header("Accept"));
        assert!(!is_secret_header("User-Agent"));
    }

    #[test]
    fn redact_preserves_template_vars() {
        // {{token}}-style references must not be redacted — they're already safe placeholders
        assert_eq!(redact("{{token}}"), "{{token}}");
        assert_eq!(redact("{{stripeKey}}"), "{{stripeKey}}");
    }

    #[test]
    fn redact_handles_bearer_tokens() {
        let out = redact("Bearer sk_test_abc123def456ghi789");
        assert!(out.starts_with("Bearer "));
        assert!(out.contains("<redacted>"));
        // Original token should NOT appear verbatim
        assert!(!out.contains("sk_test_abc123def456ghi789"));
    }

    #[test]
    fn redact_handles_basic_auth() {
        let out = redact("Basic YWxpY2U6aHVudGVyMjAyNg==");
        assert!(out.starts_with("Basic "));
        assert!(out.contains("<redacted>"));
        assert!(!out.contains("YWxpY2U6aHVudGVyMjAyNg"));
    }

    #[test]
    fn redact_token_short_values_fully_redacted() {
        // Short tokens have no head-tail preview — full redaction
        assert_eq!(redact_token("abc"), "<redacted>");
        assert_eq!(redact_token("12345678"), "<redacted>");
    }

    #[test]
    fn redact_token_long_values_show_head_tail() {
        // Long tokens show first 4 + last 2 chars, full token must NOT appear
        let out = redact_token("aaaabbbbccccdddd");
        assert!(out.contains("aaaa"));
        assert!(out.contains("dd"));
        assert!(out.contains("<redacted>"));
        assert!(!out.contains("aaaabbbbccccdddd"));
    }

    #[test]
    fn build_user_prompt_redacts_authorization() {
        // CRITICAL: this prevents leaking real API keys to the LLM provider
        let prompt = build_user_prompt(
            "GET",
            "https://api.example.com/x",
            &[
                ("Authorization".to_string(), "Bearer sk_live_real_secret_key_xyz".to_string()),
                ("Content-Type".to_string(), "application/json".to_string()),
            ],
            None,
        );
        assert!(!prompt.contains("sk_live_real_secret_key_xyz"), "prompt leaked secret: {prompt}");
        assert!(prompt.contains("<redacted>"));
        assert!(prompt.contains("Content-Type: application/json"));
    }

    #[test]
    fn build_user_prompt_redacts_api_key_headers() {
        let prompt = build_user_prompt(
            "POST",
            "https://api.openai.com/v1/chat",
            &[("X-API-Key".to_string(), "raw_secret_value_xxxxxxxxxx".to_string())],
            Some("{\"hello\":\"world\"}"),
        );
        assert!(!prompt.contains("raw_secret_value_xxxxxxxxxx"), "prompt leaked api key");
        assert!(prompt.contains("{\"hello\":\"world\"}"));
    }

    #[test]
    fn build_user_prompt_includes_method_and_url() {
        let prompt = build_user_prompt("PATCH", "https://api.example.com/users/42", &[], None);
        assert!(prompt.starts_with("PATCH https://api.example.com/users/42\n"));
    }

    #[test]
    fn strip_json_fence_handles_json_label() {
        let raw = "```json\n{\"a\":1}\n```";
        assert_eq!(strip_json_fence(raw), "{\"a\":1}");
    }

    #[test]
    fn strip_json_fence_handles_unlabeled_fence() {
        let raw = "```\n{\"a\":1}\n```";
        assert_eq!(strip_json_fence(raw), "{\"a\":1}");
    }

    #[test]
    fn strip_json_fence_passes_through_when_no_fence() {
        assert_eq!(strip_json_fence("{\"a\":1}"), "{\"a\":1}");
        assert_eq!(strip_json_fence("  {\"a\":1}  "), "{\"a\":1}");
    }

    #[test]
    fn truncate_caps_long_strings() {
        let long = "a".repeat(500);
        let out = truncate(&long, 100);
        // 100 chars from input + 1 ellipsis char = 101 chars; ellipsis is 3 bytes in UTF-8
        assert_eq!(out.chars().count(), 101);
        assert!(out.ends_with("…"));
    }

    #[test]
    fn truncate_passes_short_strings_through() {
        assert_eq!(truncate("hi", 100), "hi");
    }

    #[test]
    fn review_raw_parses_full_response_shape() {
        // The model MUST return this exact shape; verify our deserializer accepts it
        let json = r#"{
            "suggested_name": "users-list",
            "summary": "Lists users",
            "tests": ["status == 200", "body.data exists"],
            "extracts": [{"var_name": "firstId", "source": "body.data[0].id"}],
            "security_observations": ["Bearer token hardcoded"]
        }"#;
        let parsed: ReviewRaw = serde_json::from_str(json).expect("should parse");
        assert_eq!(parsed.suggested_name, "users-list");
        assert_eq!(parsed.tests.len(), 2);
        assert_eq!(parsed.extracts.len(), 1);
        assert_eq!(parsed.extracts[0].var_name, "firstId");
        assert_eq!(parsed.security_observations.len(), 1);
    }

    #[test]
    fn anthropic_response_picks_text_block_skipping_others() {
        // Anthropic responses can have multiple content blocks of varying types — verify
        // we extract the text block and ignore unknown types.
        let json = r#"{
            "content": [
                {"type": "thinking", "thinking": "..."},
                {"type": "text", "text": "actual response"}
            ]
        }"#;
        let parsed: AnthropicResponse = serde_json::from_str(json).expect("should parse");
        let text = parsed.content.into_iter().find_map(|b| match b {
            AnthropicContentBlock::Text { text } => Some(text),
            _ => None,
        });
        assert_eq!(text.as_deref(), Some("actual response"));
    }
}
