use boa_engine::{Context, Source};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

const DANTE_SHIM: &str = r#"
(function() {
  const _logs = [];
  globalThis._dante_logs = _logs;

  globalThis.console = {
    log: (...args) => _logs.push(args.map(_fmt).join(" ")),
    info: (...args) => _logs.push(args.map(_fmt).join(" ")),
    warn: (...args) => _logs.push("[warn] " + args.map(_fmt).join(" ")),
    error: (...args) => _logs.push("[error] " + args.map(_fmt).join(" ")),
  };

  function _fmt(v) {
    if (v === null) return "null";
    if (v === undefined) return "undefined";
    if (typeof v === "string") return v;
    if (typeof v === "object") {
      try { return JSON.stringify(v); } catch (e) { return String(v); }
    }
    return String(v);
  }

  const env = globalThis._dante_env || {};
  const headers = globalThis._dante_headers || [];
  const response = globalThis._dante_response || null;

  globalThis.dante = {
    env: {
      get: (name) => (name in env ? env[name] : undefined),
      set: (name, value) => { env[name] = String(value); },
      has: (name) => name in env,
      delete: (name) => { delete env[name]; },
      all: () => ({ ...env }),
    },
    headers: {
      get: (name) => {
        const lower = name.toLowerCase();
        const found = headers.find(([k]) => k.toLowerCase() === lower);
        return found ? found[1] : undefined;
      },
      set: (name, value) => {
        const lower = name.toLowerCase();
        const idx = headers.findIndex(([k]) => k.toLowerCase() === lower);
        if (idx >= 0) headers[idx] = [name, String(value)];
        else headers.push([name, String(value)]);
      },
      delete: (name) => {
        const lower = name.toLowerCase();
        const idx = headers.findIndex(([k]) => k.toLowerCase() === lower);
        if (idx >= 0) headers.splice(idx, 1);
      },
      all: () => headers.map(([k, v]) => [k, v]),
    },
    method: globalThis._dante_method || "GET",
    url: globalThis._dante_url || "",
    body: globalThis._dante_body || null,
    response: response,
    crypto: {
      base64: (s) => {
        const bytes = new TextEncoder().encode(String(s));
        let bin = '';
        for (const b of bytes) bin += String.fromCharCode(b);
        return btoa ? btoa(bin) : _b64(bytes);
      },
    },
  };

  // Postman-style pm.* shim — minimal compatibility
  const _testResults = [];
  globalThis._dante_test_results = _testResults;
  globalThis.pm = {
    environment: {
      get: (k) => globalThis.dante.env.get(k),
      set: (k, v) => globalThis.dante.env.set(k, v),
      has: (k) => globalThis.dante.env.has(k),
      unset: (k) => globalThis.dante.env.delete(k),
    },
    variables: {
      get: (k) => globalThis.dante.env.get(k),
      set: (k, v) => globalThis.dante.env.set(k, v),
      has: (k) => globalThis.dante.env.has(k),
    },
    globals: {
      get: (k) => globalThis.dante.env.get(k),
      set: (k, v) => globalThis.dante.env.set(k, v),
    },
    request: {
      get url() { return globalThis.dante.url; },
      get method() { return globalThis.dante.method; },
      headers: {
        get: (k) => globalThis.dante.headers.get(k),
        add: (h) => globalThis.dante.headers.set(h.key, h.value),
        remove: (k) => globalThis.dante.headers.delete(k),
        upsert: (h) => globalThis.dante.headers.set(h.key, h.value),
      },
    },
    response: globalThis.dante.response ? {
      code: globalThis.dante.response.status,
      status: globalThis.dante.response.status_text,
      get responseTime() { return globalThis.dante.response.elapsed_ms; },
      headers: {
        get: (k) => {
          const lower = String(k).toLowerCase();
          const found = globalThis.dante.response.headers.find(([key]) => key.toLowerCase() === lower);
          return found ? found[1] : undefined;
        },
      },
      json: () => JSON.parse(globalThis.dante.response.body),
      text: () => globalThis.dante.response.body,
    } : undefined,
    test: (name, fn) => {
      try {
        fn();
        _testResults.push({ name, pass: true });
      } catch (err) {
        _testResults.push({ name, pass: false, error: String(err && err.message || err) });
      }
    },
    expect: (actual) => {
      const make = (negate) => ({
        to: {
          equal: (expected) => {
            const eq = actual === expected;
            if (negate ? eq : !eq) throw new Error(`expected ${JSON.stringify(actual)} ${negate ? "≠" : "=="} ${JSON.stringify(expected)}`);
            return undefined;
          },
          eql: (expected) => {
            const a = JSON.stringify(actual);
            const b = JSON.stringify(expected);
            if (negate ? a === b : a !== b) throw new Error(`expected ${a} ${negate ? "≠" : "=="} ${b}`);
            return undefined;
          },
          be: { ok: () => { if (negate ? !!actual : !actual) throw new Error("not ok"); } },
          have: {
            property: (k) => { const ok = actual && Object.prototype.hasOwnProperty.call(actual, k); if (negate ? ok : !ok) throw new Error(`missing property ${k}`); },
            status: (s) => { const ok = actual === s; if (negate ? ok : !ok) throw new Error(`expected status ${s}`); },
          },
          include: (sub) => {
            const ok = typeof actual === "string" ? actual.includes(sub) : Array.isArray(actual) ? actual.includes(sub) : false;
            if (negate ? ok : !ok) throw new Error(`does not include ${sub}`);
          },
          match: (re) => {
            const r = re instanceof RegExp ? re : new RegExp(String(re));
            const ok = r.test(String(actual));
            if (negate ? ok : !ok) throw new Error(`does not match ${r}`);
          },
        },
      });
      const obj = make(false);
      obj.not = make(true).to;
      return obj;
    },
  };

  function _b64(bytes) {
    const chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let out = "";
    let i = 0;
    while (i < bytes.length) {
      const b1 = bytes[i++] || 0;
      const b2 = bytes[i++] || 0;
      const b3 = bytes[i++] || 0;
      out += chars[b1 >> 2];
      out += chars[((b1 & 3) << 4) | (b2 >> 4)];
      out += i - 1 < bytes.length ? chars[((b2 & 15) << 2) | (b3 >> 6)] : "=";
      out += i < bytes.length ? chars[b3 & 63] : "=";
    }
    return out;
  }
})();
"#;

#[derive(Serialize, Deserialize, Clone)]
pub struct ResponseDataForScript {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub elapsed_ms: u64,
}

#[derive(Serialize, Deserialize)]
pub struct PmTestResult {
    pub name: String,
    pub pass: bool,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct ScriptOutcome {
    pub env: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub logs: Vec<String>,
    pub tests: Vec<PmTestResult>,
    pub error: Option<String>,
}

pub struct ScriptInput {
    pub script: String,
    pub env: Vec<(String, String)>,
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    pub response: Option<ResponseDataForScript>,
    pub timeout_ms: u64,
}

pub fn run_script(input: ScriptInput) -> ScriptOutcome {
    let mut ctx = Context::default();

    let env_json = serde_json::to_string(
        &input
            .env
            .iter()
            .cloned()
            .collect::<std::collections::HashMap<_, _>>(),
    )
    .unwrap_or_else(|_| "{}".to_string());

    let headers_json = serde_json::to_string(&input.headers).unwrap_or_else(|_| "[]".to_string());

    let response_json = match &input.response {
        Some(r) => serde_json::to_string(r).unwrap_or_else(|_| "null".to_string()),
        None => "null".to_string(),
    };

    // Inject globals
    let setup = format!(
        "globalThis._dante_env = {};\n\
         globalThis._dante_headers = {};\n\
         globalThis._dante_response = {};\n\
         globalThis._dante_method = {};\n\
         globalThis._dante_url = {};\n\
         globalThis._dante_body = {};\n",
        env_json,
        headers_json,
        response_json,
        serde_json::to_string(&input.method).unwrap(),
        serde_json::to_string(&input.url).unwrap(),
        match &input.body {
            Some(b) => serde_json::to_string(b).unwrap(),
            None => "null".to_string(),
        }
    );

    if let Err(e) = ctx.eval(Source::from_bytes(&setup)) {
        return ScriptOutcome {
            env: input.env.clone(),
            headers: input.headers.clone(),
            logs: vec![],
            tests: vec![],
            error: Some(format!("setup: {e}")),
        };
    }
    if let Err(e) = ctx.eval(Source::from_bytes(DANTE_SHIM)) {
        return ScriptOutcome {
            env: input.env.clone(),
            headers: input.headers.clone(),
            logs: vec![],
            tests: vec![],
            error: Some(format!("shim: {e}")),
        };
    }

    let started = Instant::now();
    let _timeout = Duration::from_millis(input.timeout_ms);
    // boa doesn't have a built-in script timeout; we run synchronously and trust users
    // to write small scripts. A future version can use Context::interrupt_handler.

    let user_result = ctx.eval(Source::from_bytes(&input.script));
    let elapsed = started.elapsed();

    let user_error = match user_result {
        Ok(_) => None,
        Err(e) => Some(format!("script: {e}")),
    };

    // Read back env
    let env_out = ctx
        .eval(Source::from_bytes(
            "JSON.stringify(globalThis._dante_env || {})",
        ))
        .ok()
        .and_then(|v| v.to_string(&mut ctx).ok())
        .map(|s| s.to_std_string_escaped())
        .unwrap_or_else(|| "{}".to_string());
    let env_map: std::collections::HashMap<String, String> =
        serde_json::from_str(&env_out).unwrap_or_default();
    let mut env_pairs: Vec<(String, String)> = env_map.into_iter().collect();
    env_pairs.sort_by(|a, b| a.0.cmp(&b.0));

    // Read back headers
    let headers_out = ctx
        .eval(Source::from_bytes(
            "JSON.stringify(globalThis._dante_headers || [])",
        ))
        .ok()
        .and_then(|v| v.to_string(&mut ctx).ok())
        .map(|s| s.to_std_string_escaped())
        .unwrap_or_else(|| "[]".to_string());
    let headers_pairs: Vec<(String, String)> =
        serde_json::from_str(&headers_out).unwrap_or_default();

    // Read back logs
    let logs_out = ctx
        .eval(Source::from_bytes(
            "JSON.stringify(globalThis._dante_logs || [])",
        ))
        .ok()
        .and_then(|v| v.to_string(&mut ctx).ok())
        .map(|s| s.to_std_string_escaped())
        .unwrap_or_else(|| "[]".to_string());
    let mut logs: Vec<String> = serde_json::from_str(&logs_out).unwrap_or_default();
    logs.push(format!("(script ran in {} ms)", elapsed.as_millis()));

    // Read pm.test results
    let tests_out = ctx
        .eval(Source::from_bytes(
            "JSON.stringify(globalThis._dante_test_results || [])",
        ))
        .ok()
        .and_then(|v| v.to_string(&mut ctx).ok())
        .map(|s| s.to_std_string_escaped())
        .unwrap_or_else(|| "[]".to_string());
    let tests: Vec<PmTestResult> = serde_json::from_str(&tests_out).unwrap_or_default();

    ScriptOutcome {
        env: env_pairs,
        headers: headers_pairs,
        logs,
        tests,
        error: user_error,
    }
}
