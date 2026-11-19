//! HTTP execution, assertion evaluation and the local mock server.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::StreamExt;

use crate::model::{
    ApiRequest, AssertKind, Assertion, Auth, BodyMode, Capture, CaptureSource, FieldKind, Id,
    Method, MockFailure, Settings, expand_with, now_secs,
};

#[derive(Clone, Debug)]
pub struct CachedToken {
    pub token: String,
    pub expires_at: u64,
}

pub type OAuthCache = Arc<Mutex<HashMap<String, CachedToken>>>;

pub fn new_oauth_cache() -> OAuthCache {
    Arc::new(Mutex::new(HashMap::new()))
}

#[derive(Clone, Debug, Default)]
pub struct Timing {
    pub dns_ms: Option<u64>,
    pub connect_ms: Option<u64>,
    pub tls_ms: Option<u64>,
    pub ttfb_ms: u64,
    pub download_ms: u64,
    pub total_ms: u64,
}

/// Responses larger than this are streamed up to the limit and marked as
/// truncated instead of exhausting memory.
const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct ResponseData {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub content_type: String,
    pub elapsed_ms: u64,
    pub size: usize,
    pub final_url: String,
    pub timings: Timing,
    pub truncated: bool,
}

#[derive(Clone, Debug)]
pub struct AssertionResult {
    pub label: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Clone, Debug)]
pub struct RequestOutcome {
    pub request_id: Id,
    pub request_name: String,
    pub folder: String,
    pub method: Method,
    pub url: String,
    pub at: u64,
    pub data: Result<ResponseData, String>,
    pub assertions: Vec<AssertionResult>,
}

impl RequestOutcome {
    pub fn status(&self) -> Option<u16> {
        self.data.as_ref().ok().map(|d| d.status)
    }

    pub fn passed(&self) -> usize {
        self.assertions.iter().filter(|a| a.passed).count()
    }

    pub fn failed(&self) -> usize {
        self.assertions.iter().filter(|a| !a.passed).count()
    }
}

fn method_of(method: Method) -> reqwest::Method {
    match method {
        Method::Get => reqwest::Method::GET,
        Method::Post => reqwest::Method::POST,
        Method::Put => reqwest::Method::PUT,
        Method::Patch => reqwest::Method::PATCH,
        Method::Delete => reqwest::Method::DELETE,
        Method::Head => reqwest::Method::HEAD,
        Method::Options => reqwest::Method::OPTIONS,
    }
}

pub fn resolved_url(req: &ApiRequest, env: &HashMap<String, String>) -> String {
    let raw = expand_with(req.url.trim(), env);
    if raw.is_empty() {
        return String::new();
    }
    if raw.contains("://") {
        raw
    } else {
        format!("http://{raw}")
    }
}

#[allow(dead_code)]
pub fn execute(
    req: &ApiRequest,
    env: &HashMap<String, String>,
    settings: &Settings,
) -> RequestOutcome {
    block_on(execute_async(req, env, settings, &new_oauth_cache()))
}

#[allow(dead_code)]
pub fn execute_with_cache(
    req: &ApiRequest,
    env: &HashMap<String, String>,
    settings: &Settings,
    cache: &OAuthCache,
) -> RequestOutcome {
    block_on(execute_async(req, env, settings, cache))
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
        .block_on(future)
}

pub async fn execute_async(
    req: &ApiRequest,
    env: &HashMap<String, String>,
    settings: &Settings,
    cache: &OAuthCache,
) -> RequestOutcome {
    let started = Instant::now();
    let mut outcome = RequestOutcome {
        request_id: req.id,
        request_name: req.name.clone(),
        folder: String::new(),
        method: req.method,
        url: resolved_url(req, env),
        at: now_secs(),
        data: Err("Request did not run".to_string()),
        assertions: Vec::new(),
    };
    let result = run_http_async(req, env, settings, cache).await;
    let elapsed = started.elapsed().as_millis() as u64;
    outcome.assertions = evaluate_assertions(req, &result, elapsed);
    outcome.data = result;
    outcome
}

fn build_blocking_client(settings: &Settings) -> Result<reqwest::blocking::Client, String> {
    let mut builder = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20));
    let proxy = settings.proxy.trim();
    if !proxy.is_empty() {
        builder = builder.proxy(
            reqwest::Proxy::all(proxy).map_err(|error| format!("Invalid proxy URL: {error}"))?,
        );
    }
    if settings.accept_invalid_certs {
        builder = builder.danger_accept_invalid_certs(true);
    }
    let ca_bundle = settings.ca_bundle.trim();
    if !ca_bundle.is_empty() {
        let pem = std::fs::read(ca_bundle)
            .map_err(|error| format!("Could not read the CA bundle: {error}"))?;
        for certificate in reqwest::Certificate::from_pem_bundle(&pem)
            .map_err(|error| format!("Invalid CA bundle: {error}"))?
        {
            builder = builder.add_root_certificate(certificate);
        }
    }
    builder
        .build()
        .map_err(|error| format!("Could not start the HTTP client: {error}"))
}

fn build_client(settings: &Settings) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_millis(settings.timeout_ms.clamp(500, 120_000)));
    if !settings.follow_redirects {
        builder = builder.redirect(reqwest::redirect::Policy::none());
    }
    let proxy = settings.proxy.trim();
    if !proxy.is_empty() {
        builder = builder.proxy(
            reqwest::Proxy::all(proxy).map_err(|error| format!("Invalid proxy URL: {error}"))?,
        );
    }
    if settings.accept_invalid_certs {
        builder = builder.danger_accept_invalid_certs(true);
    }
    let ca_bundle = settings.ca_bundle.trim();
    if !ca_bundle.is_empty() {
        let pem = std::fs::read(ca_bundle)
            .map_err(|error| format!("Could not read the CA bundle: {error}"))?;
        for certificate in reqwest::Certificate::from_pem_bundle(&pem)
            .map_err(|error| format!("Invalid CA bundle: {error}"))?
        {
            builder = builder.add_root_certificate(certificate);
        }
    }
    builder
        .build()
        .map_err(|error| format!("Could not start the HTTP client: {error}"))
}

async fn run_http_async(
    req: &ApiRequest,
    env: &HashMap<String, String>,
    settings: &Settings,
    cache: &OAuthCache,
) -> Result<ResponseData, String> {
    let raw = resolved_url(req, env);
    if raw.trim().is_empty() {
        return Err("The request URL is empty.".to_string());
    }
    let mut url = reqwest::Url::parse(&raw).map_err(|e| format!("Invalid request URL: {e}"))?;

    let pairs: Vec<(String, String)> = req
        .params
        .iter()
        .filter(|kv| kv.enabled && !kv.key.trim().is_empty())
        .map(|kv| (expand_with(kv.key.trim(), env), expand_with(&kv.value, env)))
        .collect();
    if !pairs.is_empty() {
        let mut query = url.query_pairs_mut();
        for (key, value) in pairs {
            query.append_pair(&key, &value);
        }
    }

    let client = build_client(settings)?;
    let mut request = client.request(method_of(req.method), url.clone());

    let custom_content_type = req.headers.iter().any(|kv| {
        kv.enabled
            && kv.key.eq_ignore_ascii_case("content-type")
            && !kv.value.trim().is_empty()
    });
    for kv in &req.headers {
        if kv.enabled && !kv.key.trim().is_empty() {
            if req.body_mode == BodyMode::Multipart && kv.key.eq_ignore_ascii_case("content-type") {
                continue;
            }
            request = request.header(expand_with(kv.key.trim(), env), expand_with(&kv.value, env));
        }
    }

    match &req.auth {
        Auth::None | Auth::Inherit => {}
        Auth::Bearer { token } => {
            let token = expand_with(token, env);
            if !token.trim().is_empty() {
                request = request.bearer_auth(token.trim());
            }
        }
        Auth::Basic { user, pass } => {
            request = request.basic_auth(expand_with(user, env), Some(expand_with(pass, env)));
        }
        Auth::ApiKey { header, value } => {
            let header = expand_with(header, env);
            if !header.trim().is_empty() {
                request = request.header(header.trim(), expand_with(value, env));
            }
        }
        Auth::OAuth2 { .. } => {
            let token = oauth_token(&req.auth, env, cache, settings).await?;
            request = request.bearer_auth(token);
        }
    }

    match req.body_mode {
        BodyMode::None => {}
        BodyMode::Json => {
            let body = expand_with(&req.body, env);
            if !body.trim().is_empty() {
                request = request.header("Content-Type", "application/json").body(body);
            }
        }
        BodyMode::Text => {
            let body = expand_with(&req.body, env);
            if !body.trim().is_empty() {
                request = request
                    .header("Content-Type", "text/plain; charset=utf-8")
                    .body(body);
            }
        }
        BodyMode::Form => {
            let form: Vec<(String, String)> = req
                .form
                .iter()
                .filter(|kv| kv.enabled && !kv.key.trim().is_empty())
                .map(|kv| (expand_with(kv.key.trim(), env), expand_with(&kv.value, env)))
                .collect();
            if !form.is_empty() {
                let mut encoded = String::new();
                for (k, v) in form {
                    if !encoded.is_empty() {
                        encoded.push('&');
                    }
                    encoded.push_str(&urlencode(&k));
                    encoded.push('=');
                    encoded.push_str(&urlencode(&v));
                }
                request = request
                    .header("Content-Type", "application/x-www-form-urlencoded")
                    .body(encoded);
            }
        }
        BodyMode::Multipart => {
            let (content_type, bytes) = build_multipart(req, env)?;
            request = request.header("Content-Type", content_type).body(bytes);
        }
        BodyMode::Binary => {
            let path = expand_with(&req.binary_path, env);
            if path.trim().is_empty() {
                return Err("Choose a file for the binary body.".to_string());
            }
            let bytes =
                std::fs::read(&path).map_err(|error| format!("Could not read `{path}`: {error}"))?;
            if !custom_content_type {
                request = request.header("Content-Type", guess_content_type(&path));
            }
            request = request.body(bytes);
        }
    }

    // The phase probe runs next to the real request so it does not add
    // latency; with a proxy the phases would describe the proxy, so skip it.
    let probe = if settings.measure_phases && settings.proxy.trim().is_empty() {
        match (
            url.host_str(),
            url.port_or_known_default(),
            url.scheme() == "https",
        ) {
            (Some(host), Some(port), https) => {
                let host = host.to_string();
                Some(tokio::spawn(async move {
                    crate::timing::probe(&host, port, https, Duration::from_secs(6)).await
                }))
            }
            _ => None,
        }
    } else {
        None
    };

    let start = Instant::now();
    let response = request.send().await.map_err(|e| human_error(&e))?;
    let ttfb = start.elapsed().as_millis() as u64;
    let status = response.status().as_u16();
    let final_url = response.url().to_string();
    let headers: Vec<(String, String)> = response
        .headers()
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_string(),
                String::from_utf8_lossy(v.as_bytes()).to_string(),
            )
        })
        .collect();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let mut bytes: Vec<u8> = Vec::new();
    let mut truncated = false;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| human_error(&e))?;
        if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES {
            let take = MAX_RESPONSE_BYTES - bytes.len();
            bytes.extend_from_slice(&chunk[..take]);
            truncated = true;
            break;
        }
        bytes.extend_from_slice(&chunk);
    }
    let total = start.elapsed().as_millis() as u64;
    let size = bytes.len();
    let body = String::from_utf8_lossy(&bytes).to_string();

    let mut timings = Timing {
        dns_ms: None,
        connect_ms: None,
        tls_ms: None,
        ttfb_ms: ttfb,
        download_ms: total.saturating_sub(ttfb),
        total_ms: total,
    };
    if let Some(handle) = probe {
        if let Ok(Ok(phases)) =
            tokio::time::timeout(Duration::from_millis(2_500), handle).await
        {
            timings.dns_ms = phases.dns_ms;
            timings.connect_ms = phases.connect_ms;
            timings.tls_ms = phases.tls_ms;
        }
    }

    Ok(ResponseData {
        status,
        headers,
        body,
        content_type,
        elapsed_ms: total,
        size,
        final_url,
        timings,
        truncated,
    })
}

fn build_multipart(
    req: &ApiRequest,
    env: &HashMap<String, String>,
) -> Result<(String, Vec<u8>), String> {
    let boundary = format!("LucAPIBoundary{:x}{:x}", now_secs(), req.id);
    let mut body: Vec<u8> = Vec::new();
    for field in req
        .multipart
        .iter()
        .filter(|field| field.enabled && !field.key.trim().is_empty())
    {
        let key = sanitize_part(&expand_with(field.key.trim(), env));
        let value = expand_with(&field.value, env);
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        match field.kind {
            FieldKind::File => {
                if value.trim().is_empty() {
                    return Err(format!("Choose a file for the `{key}` field."));
                }
                let bytes = std::fs::read(&value)
                    .map_err(|error| format!("Could not read `{value}`: {error}"))?;
                let filename = std::path::Path::new(&value)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("file");
                let filename = sanitize_part(filename);
                let content_type = if field.content_type.trim().is_empty() {
                    guess_content_type(&value).to_string()
                } else {
                    expand_with(&field.content_type, env)
                };
                body.extend_from_slice(
                    format!(
                        "Content-Disposition: form-data; name=\"{key}\"; filename=\"{filename}\"\r\n"
                    )
                    .as_bytes(),
                );
                body.extend_from_slice(format!("Content-Type: {content_type}\r\n\r\n").as_bytes());
                body.extend_from_slice(&bytes);
                body.extend_from_slice(b"\r\n");
            }
            FieldKind::Text => {
                body.extend_from_slice(
                    format!("Content-Disposition: form-data; name=\"{key}\"\r\n").as_bytes(),
                );
                if !field.content_type.trim().is_empty() {
                    let content_type = expand_with(&field.content_type, env);
                    body.extend_from_slice(format!("Content-Type: {content_type}\r\n").as_bytes());
                }
                body.extend_from_slice(b"\r\n");
                body.extend_from_slice(value.as_bytes());
                body.extend_from_slice(b"\r\n");
            }
        }
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    Ok((format!("multipart/form-data; boundary={boundary}"), body))
}

fn sanitize_part(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '"' | '\r' | '\n' => '_',
            other => other,
        })
        .collect()
}

fn guess_content_type(path: &str) -> &'static str {
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "json" => "application/json",
        "txt" | "log" => "text/plain",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "xml" => "application/xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "gz" => "application/gzip",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        _ => "application/octet-stream",
    }
}

fn urlencode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push('+'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

fn human_error(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "The request timed out.".to_string()
    } else if error.is_connect() {
        format!("Could not connect to the server: {error}")
    } else if error.is_decode() {
        format!("The server response could not be decoded: {error}")
    } else if error.is_redirect() {
        "The server tried to redirect the request.".to_string()
    } else {
        format!("Request failed: {error}")
    }
}

pub fn evaluate_assertions(
    req: &ApiRequest,
    data: &Result<ResponseData, String>,
    elapsed_ms: u64,
) -> Vec<AssertionResult> {
    let mut results = Vec::new();
    for assertion in req.assertions.iter().filter(|a| a.enabled) {
        let result = match data {
            Err(err) => AssertionResult {
                label: assertion.label(),
                passed: false,
                detail: err.clone(),
            },
            Ok(response) => evaluate_one(assertion, response, elapsed_ms),
        };
        results.push(result);
    }
    results
}

fn evaluate_one(
    assertion: &Assertion,
    response: &ResponseData,
    elapsed_ms: u64,
) -> AssertionResult {
    let label = assertion.label();
    let expected = assertion.expected.trim();
    match assertion.kind {
        AssertKind::StatusEquals => {
            let ok = expected
                .parse::<u16>()
                .map(|code| code == response.status)
                .unwrap_or(false);
            AssertionResult {
                label,
                passed: ok,
                detail: format!("Received {}", response.status),
            }
        }
        AssertKind::BodyContains => {
            let ok = !expected.is_empty() && response.body.contains(expected);
            AssertionResult {
                label,
                passed: ok,
                detail: if ok {
                    "Body contains the expected text".to_string()
                } else {
                    "Expected text was not found in the body".to_string()
                },
            }
        }
        AssertKind::JsonFieldEquals => {
            let (found, actual) = json_field(&response.body, assertion.target.trim());
            let ok = found && values_equal(&actual, expected);
            AssertionResult {
                label,
                passed: ok,
                detail: if found {
                    format!("Actual value: {actual}")
                } else {
                    "Field was not found in the JSON body".to_string()
                },
            }
        }
        AssertKind::HeaderEquals => {
            let target = assertion.target.trim();
            let actual = response
                .headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(target))
                .map(|(_, v)| v.clone());
            let ok = actual
                .as_deref()
                .map(|v| {
                    v.eq_ignore_ascii_case(expected) || v.trim().eq_ignore_ascii_case(expected)
                })
                .unwrap_or(false);
            AssertionResult {
                label,
                passed: ok,
                detail: actual
                    .map(|v| format!("Received: {v}"))
                    .unwrap_or_else(|| "Header is missing".to_string()),
            }
        }
        AssertKind::ResponseTimeUnder => {
            let limit = expected.parse::<u64>().unwrap_or(u64::MAX);
            let ok = elapsed_ms <= limit;
            AssertionResult {
                label,
                passed: ok,
                detail: format!("Took {} ms", elapsed_ms),
            }
        }
    }
}

fn json_field(body: &str, path: &str) -> (bool, String) {
    match extract_json(body, path) {
        Some(value) => (true, value),
        None => (false, String::new()),
    }
}

/// Reads a value out of a JSON body. Accepts both `data.0.email` and the
/// JSONPath-ish `$.data[0].email` written by the response tree.
pub fn extract_json(body: &str, path: &str) -> Option<String> {
    let path = path.trim();
    if path.is_empty() {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let mut current = &value;
    for part in normalize_path(path) {
        current = match current {
            serde_json::Value::Array(_) => current.get(part.parse::<usize>().ok()?)?,
            _ => current.get(part.as_str())?,
        };
    }
    Some(match current {
        serde_json::Value::String(text) => text.clone(),
        other => other.to_string(),
    })
}

fn normalize_path(path: &str) -> Vec<String> {
    let mut text = path.trim();
    if let Some(rest) = text.strip_prefix("$.") {
        text = rest;
    } else if text == "$" {
        return Vec::new();
    }
    let mut parts = Vec::new();
    let mut buffer = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '.' => {
                if !buffer.is_empty() {
                    parts.push(std::mem::take(&mut buffer));
                }
            }
            '[' => {
                if !buffer.is_empty() {
                    parts.push(std::mem::take(&mut buffer));
                }
                let mut inside = String::new();
                for inner in chars.by_ref() {
                    if inner == ']' {
                        break;
                    }
                    inside.push(inner);
                }
                let inside = inside.trim().trim_matches('"').trim_matches('\'');
                if !inside.is_empty() {
                    parts.push(inside.to_string());
                }
            }
            _ => buffer.push(ch),
        }
    }
    if !buffer.is_empty() {
        parts.push(buffer);
    }
    parts
}

/// Applies the enabled capture rules of a request to a live response.
/// Returns `(variable, value, secret)` for every successful extraction.
pub fn evaluate_captures(
    captures: &[Capture],
    data: &ResponseData,
) -> Vec<(String, String, bool)> {
    let mut out = Vec::new();
    for capture in captures.iter().filter(|capture| capture.enabled) {
        let variable = capture.variable.trim();
        if variable.is_empty() {
            continue;
        }
        let value = match capture.source {
            CaptureSource::JsonField => extract_json(&data.body, &capture.path),
            CaptureSource::Header => data
                .headers
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(capture.path.trim()))
                .map(|(_, value)| value.trim().to_string()),
        };
        if let Some(value) = value {
            if !value.is_empty() {
                out.push((variable.to_string(), value, capture.secret));
            }
        }
    }
    out
}

fn values_equal(actual: &str, expected: &str) -> bool {
    if actual == expected {
        return true;
    }
    if let (Ok(a), Ok(b)) = (actual.parse::<f64>(), expected.parse::<f64>()) {
        return (a - b).abs() < f64::EPSILON;
    }
    actual.trim_matches('"') == expected.trim_matches('"')
}

// ---------------------------------------------------------------------------
// OAuth2
// ---------------------------------------------------------------------------

pub fn oauth_key(auth: &Auth, env: &HashMap<String, String>) -> Option<String> {
    let Auth::OAuth2 {
        token_url,
        client_id,
        scope,
        ..
    } = auth
    else {
        return None;
    };
    Some(format!(
        "{}|{}|{}",
        expand_with(token_url.trim(), env),
        expand_with(client_id.trim(), env),
        expand_with(scope.trim(), env)
    ))
}

pub fn oauth_connected(
    auth: &Auth,
    env: &HashMap<String, String>,
    cache: &OAuthCache,
) -> bool {
    let Some(key) = oauth_key(auth, env) else {
        return false;
    };
    cache
        .lock()
        .ok()
        .and_then(|tokens| {
            tokens
                .get(&key)
                .map(|token| token.expires_at > now_secs())
        })
        .unwrap_or(false)
}

async fn oauth_token(
    auth: &Auth,
    env: &HashMap<String, String>,
    cache: &OAuthCache,
    settings: &Settings,
) -> Result<String, String> {
    let Auth::OAuth2 {
        grant,
        token_url,
        client_id,
        client_secret,
        scope,
        ..
    } = auth
    else {
        return Err("OAuth2: invalid auth configuration.".to_string());
    };
    let token_url = expand_with(token_url.trim(), env);
    if token_url.is_empty() {
        return Err("OAuth2: set the token URL in the Auth tab.".to_string());
    }
    let key = oauth_key(auth, env).ok_or("OAuth2: invalid configuration.")?;
    if let Some(token) = cached_token(&key, cache) {
        return Ok(token);
    }
    if matches!(grant, crate::model::OAuthGrant::Pkce) {
        return Err(
            "OAuth2: connect the account from the Auth tab before sending.".to_string(),
        );
    }
    let client = build_client(settings)?;
    let secret = expand_with(client_secret.trim(), env);
    let scope = expand_with(scope.trim(), env);
    let mut form = vec![
        ("grant_type".to_string(), "client_credentials".to_string()),
        (
            "client_id".to_string(),
            expand_with(client_id.trim(), env),
        ),
    ];
    if !secret.is_empty() {
        form.push(("client_secret".to_string(), secret));
    }
    if !scope.is_empty() {
        form.push(("scope".to_string(), scope));
    }
    let response = client
        .post(&token_url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(encode_form(&form))
        .send()
        .await
        .map_err(|error| format!("OAuth2 token request failed: {error}"))?;
    parse_token_response_async(response, &key, cache).await
}

pub fn exchange_pkce_code(
    auth: &Auth,
    env: &HashMap<String, String>,
    cache: &OAuthCache,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
    settings: &Settings,
) -> Result<(), String> {
    let Auth::OAuth2 {
        token_url,
        client_id,
        client_secret,
        ..
    } = auth
    else {
        return Err("OAuth2: invalid auth configuration.".to_string());
    };
    let token_url = expand_with(token_url.trim(), env);
    if token_url.is_empty() {
        return Err("OAuth2: set the token URL in the Auth tab.".to_string());
    }
    let key = oauth_key(auth, env).ok_or("OAuth2: invalid configuration.")?;
    let client = build_blocking_client(settings)?;
    let client_id = expand_with(client_id.trim(), env);
    let secret = expand_with(client_secret.trim(), env);
    let mut form = vec![
        ("grant_type".to_string(), "authorization_code".to_string()),
        ("code".to_string(), code.to_string()),
        ("redirect_uri".to_string(), redirect_uri.to_string()),
        ("client_id".to_string(), client_id),
        ("code_verifier".to_string(), verifier.to_string()),
    ];
    if !secret.is_empty() {
        form.push(("client_secret".to_string(), secret));
    }
    let response = client
        .post(&token_url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(encode_form(&form))
        .send()
        .map_err(|error| format!("OAuth2 token request failed: {error}"))?;
    parse_token_response(response, &key, cache).map(|_| ())
}

fn cached_token(key: &str, cache: &OAuthCache) -> Option<String> {
    cache.lock().ok().and_then(|tokens| {
        tokens
            .get(key)
            .filter(|token| token.expires_at > now_secs())
            .map(|token| token.token.clone())
    })
}

fn parse_token_response(
    response: reqwest::blocking::Response,
    key: &str,
    cache: &OAuthCache,
) -> Result<String, String> {
    let status = response.status();
    let body = response
        .text()
        .map_err(|error| format!("OAuth2: could not read the token response: {error}"))?;
    parse_token_json(status, &body, key, cache)
}

async fn parse_token_response_async(
    response: reqwest::Response,
    key: &str,
    cache: &OAuthCache,
) -> Result<String, String> {
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("OAuth2: could not read the token response: {error}"))?;
    parse_token_json(status, &body, key, cache)
}

fn parse_token_json(
    status: reqwest::StatusCode,
    body: &str,
    key: &str,
    cache: &OAuthCache,
) -> Result<String, String> {
    if !status.is_success() {
        return Err(format!(
            "OAuth2 token request failed ({status}): {}",
            truncate_text(body, 220)
        ));
    }
    let value: serde_json::Value = serde_json::from_str(body).map_err(|_| {
        format!(
            "OAuth2: unexpected token response: {}",
            truncate_text(body, 220)
        )
    })?;
    let token = value
        .get("access_token")
        .and_then(|value| value.as_str())
        .ok_or_else(|| "OAuth2: the token response has no access_token.".to_string())?;
    let expires_in = value
        .get("expires_in")
        .and_then(|value| value.as_u64())
        .unwrap_or(3600);
    let lifetime = expires_in.saturating_sub(30).max(30);
    if let Ok(mut tokens) = cache.lock() {
        tokens.insert(
            key.to_string(),
            CachedToken {
                token: token.to_string(),
                expires_at: now_secs() + lifetime,
            },
        );
    }
    Ok(token.to_string())
}

fn encode_form(form: &[(String, String)]) -> String {
    form.iter()
        .map(|(key, value)| format!("{}={}", urlencode(key), urlencode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn truncate_text(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    text.chars().take(limit).collect::<String>() + "…"
}

// ---------------------------------------------------------------------------
// Mock server
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct MockRoute {
    pub method: Method,
    pub path: String,
    pub status: u16,
    pub body: String,
    pub latency_ms: u64,
    pub failure: MockFailure,
}

#[derive(Clone, Debug)]
pub struct MockHit {
    pub method: String,
    pub path: String,
    pub status: u16,
    pub at: u64,
}

pub struct MockServer {
    pub running: bool,
    pub port: u16,
    pub error: Option<String>,
    stop: Arc<AtomicBool>,
    hits: Arc<Mutex<Vec<MockHit>>>,
}

impl Default for MockServer {
    fn default() -> Self {
        Self {
            running: false,
            port: 4010,
            error: None,
            stop: Arc::new(AtomicBool::new(true)),
            hits: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl MockServer {
    pub fn hits(&self) -> Vec<MockHit> {
        let mut hits = self.hits.lock().map(|h| h.clone()).unwrap_or_default();
        hits.reverse();
        hits
    }

    pub fn start(&mut self, port: u16, routes: Vec<MockRoute>) {
        self.stop();
        match TcpListener::bind(("127.0.0.1", port)) {
            Ok(listener) => {
                let stop = Arc::new(AtomicBool::new(false));
                let hits = Arc::new(Mutex::new(Vec::new()));
                self.stop = stop.clone();
                self.hits = hits.clone();
                self.port = port;
                self.running = true;
                self.error = None;
                if let Ok(mut log) = self.hits.lock() {
                    log.clear();
                }
                std::thread::spawn(move || serve_mock(listener, stop, routes, hits));
            }
            Err(err) => {
                self.running = false;
                self.error = Some(format!("Could not listen on port {port}: {err}"));
            }
        }
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.running = false;
    }

    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

fn serve_mock(
    listener: TcpListener,
    stop: Arc<AtomicBool>,
    routes: Vec<MockRoute>,
    hits: Arc<Mutex<Vec<MockHit>>>,
) {
    let _ = listener.set_nonblocking(true);
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => handle_mock(stream, &routes, &hits),
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

fn handle_mock(mut stream: TcpStream, routes: &[MockRoute], hits: &Arc<Mutex<Vec<MockHit>>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_millis(800)));
    let mut buffer = [0u8; 8192];
    let read = stream.read(&mut buffer).unwrap_or(0);
    if read == 0 {
        return;
    }
    let request = String::from_utf8_lossy(&buffer[..read]).to_string();
    let mut lines = request.lines();
    let first = lines.next().unwrap_or("");
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let raw_path = parts.next().unwrap_or("/").to_string();
    let path = raw_path.split('?').next().unwrap_or("/").to_string();

    if method.eq_ignore_ascii_case("OPTIONS") {
        let _ = write_response(&mut stream, 204, "No Content", "{}", "application/json");
        log_hit(hits, &method, &path, 204);
        return;
    }

    let matched = routes.iter().find(|route| {
        route.method.as_str().eq_ignore_ascii_case(&method) && path_matches(&route.path, &path)
    });

    // Per-request overrides let a test exercise failures without editing the
    // route: X-Mock-Status, X-Mock-Delay and X-Mock-Fail.
    let overrides = MockOverrides::parse(&request);

    match matched {
        Some(route) => {
            let status = overrides.status.unwrap_or(route.status);
            let latency = overrides.latency_ms.unwrap_or(route.latency_ms);
            let failure = overrides.failure.unwrap_or(route.failure);
            if latency > 0 {
                std::thread::sleep(Duration::from_millis(latency.min(30_000)));
            }
            match failure {
                MockFailure::Reset => {
                    log_hit(hits, &method, &path, 0);
                    return;
                }
                MockFailure::Timeout => {
                    // Hold the connection open without answering.
                    std::thread::sleep(Duration::from_secs(30));
                    log_hit(hits, &method, &path, 0);
                    return;
                }
                MockFailure::None => {}
            }
            let content_type = if route.body.trim_start().starts_with(['{', '[']) {
                "application/json"
            } else {
                "text/plain; charset=utf-8"
            };
            let _ = write_response(
                &mut stream,
                status,
                crate::widgets::reason_phrase(status),
                &route.body,
                content_type,
            );
            log_hit(hits, &method, &path, status);
        }
        None => {
            let body = format!(
                "{{\n  \"error\": \"No mock route matched\",\n  \"method\": \"{method}\",\n  \"path\": \"{path}\"\n}}"
            );
            let _ = write_response(&mut stream, 404, "Not Found", &body, "application/json");
            log_hit(hits, &method, &path, 404);
        }
    }
}

struct MockOverrides {
    status: Option<u16>,
    latency_ms: Option<u64>,
    failure: Option<MockFailure>,
}

impl MockOverrides {
    fn parse(request: &str) -> Self {
        let mut overrides = MockOverrides {
            status: None,
            latency_ms: None,
            failure: None,
        };
        for line in request.lines().skip(1) {
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim();
            match name.trim().to_ascii_lowercase().as_str() {
                "x-mock-status" => overrides.status = value.parse::<u16>().ok(),
                "x-mock-delay" => overrides.latency_ms = value.parse::<u64>().ok(),
                "x-mock-fail" => {
                    overrides.failure = match value.to_ascii_lowercase().as_str() {
                        "timeout" => Some(MockFailure::Timeout),
                        "reset" => Some(MockFailure::Reset),
                        "none" => Some(MockFailure::None),
                        _ => None,
                    }
                }
                _ => {}
            }
        }
        overrides
    }
}

fn path_matches(route: &str, actual: &str) -> bool {
    let route_parts: Vec<&str> = route.split('/').filter(|s| !s.is_empty()).collect();
    let actual_parts: Vec<&str> = actual.split('/').filter(|s| !s.is_empty()).collect();
    if route_parts.len() != actual_parts.len() {
        return false;
    }
    route_parts
        .iter()
        .zip(actual_parts.iter())
        .all(|(route_part, actual_part)| {
            route_part.starts_with(':') || route_part.starts_with('{') || route_part == actual_part
        })
}

fn log_hit(hits: &Arc<Mutex<Vec<MockHit>>>, method: &str, path: &str, status: u16) {
    if let Ok(mut log) = hits.lock() {
        log.push(MockHit {
            method: method.to_string(),
            path: path.to_string(),
            status,
            at: now_secs(),
        });
        if log.len() > 50 {
            log.remove(0);
        }
    }
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    body: &str,
    content_type: &str,
) -> std::io::Result<()> {
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Headers: *\r\nAccess-Control-Allow-Methods: GET,POST,PUT,PATCH,DELETE,OPTIONS\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AssertKind, Assertion};

    fn sample_response() -> ResponseData {
        ResponseData {
            timings: Timing::default(),
            truncated: false,
            status: 200,
            headers: vec![("content-type".into(), "application/json".into())],
            body: "{\"total\": 2, \"data\": [{\"id\": 1}]}".into(),
            content_type: "application/json".into(),
            elapsed_ms: 41,
            size: 37,
            final_url: "http://example.test/v1/users".into(),
        }
    }

    #[test]
    fn assertions_pass_on_matching_response() {
        let mut req = ApiRequest::new("t", Method::Get, "http://example.test/v1/users");
        req.assertions = vec![
            Assertion::new(AssertKind::StatusEquals, "", "200"),
            Assertion::new(AssertKind::JsonFieldEquals, "total", "2"),
            Assertion::new(AssertKind::JsonFieldEquals, "data.0.id", "1"),
            Assertion::new(AssertKind::BodyContains, "", "\"total\""),
            Assertion::new(AssertKind::HeaderEquals, "content-type", "application/json"),
            Assertion::new(AssertKind::ResponseTimeUnder, "", "500"),
        ];
        let results = evaluate_assertions(&req, &Ok(sample_response()), 41);
        assert_eq!(results.len(), 6);
        for result in results {
            assert!(result.passed, "{} failed: {}", result.label, result.detail);
        }
    }

    #[test]
    fn assertions_fail_on_mismatch() {
        let mut req = ApiRequest::new("t", Method::Get, "http://example.test");
        req.assertions = vec![Assertion::new(AssertKind::StatusEquals, "", "201")];
        let results = evaluate_assertions(&req, &Ok(sample_response()), 41);
        assert!(!results[0].passed);
    }

    #[test]
    fn env_placeholders_are_replaced() {
        let mut vars = HashMap::new();
        vars.insert(
            "baseUrl".to_string(),
            "http://localhost:9000/v1".to_string(),
        );
        assert_eq!(
            expand_with("{{ baseUrl }}/users?page=1", &vars),
            "http://localhost:9000/v1/users?page=1"
        );
        assert_eq!(expand_with("{{missing}}/x", &vars), "{{missing}}/x");
    }

    #[test]
    fn extract_json_accepts_dotted_and_jsonpath() {
        let body = r#"{"data":{"users":[{"email":"ada@example.com"}]},"total":1}"#;
        assert_eq!(
            extract_json(body, "data.users.0.email").as_deref(),
            Some("ada@example.com")
        );
        assert_eq!(
            extract_json(body, "$.data.users[0].email").as_deref(),
            Some("ada@example.com")
        );
        assert_eq!(extract_json(body, "total").as_deref(), Some("1"));
        assert_eq!(extract_json(body, "missing.path"), None);
    }

    #[test]
    fn captures_pull_json_fields_and_headers() {
        let data = ResponseData {
            timings: Timing::default(),
            truncated: false,
            status: 200,
            headers: vec![
                ("content-type".into(), "application/json".into()),
                ("x-session".into(), "abc-123".into()),
            ],
            body: r#"{"token":"secret-token","user":{"id":7}}"#.into(),
            content_type: "application/json".into(),
            elapsed_ms: 10,
            size: 48,
            final_url: "http://example.test/login".into(),
        };
        let captures = vec![
            Capture::new(CaptureSource::JsonField, "$.token", "token"),
            Capture::new(CaptureSource::JsonField, "user.id", "userId"),
            Capture::new(CaptureSource::Header, "X-Session", "session"),
            Capture::new(CaptureSource::JsonField, "missing", "nope"),
        ];
        let values = evaluate_captures(&captures, &data);
        assert_eq!(values.len(), 3);
        assert!(values.iter().any(|(name, value, _)| name == "token" && value == "secret-token"));
        assert!(values.iter().any(|(name, value, _)| name == "userId" && value == "7"));
        assert!(values.iter().any(|(name, value, _)| name == "session" && value == "abc-123"));
    }

    #[test]
    fn client_credentials_tokens_are_fetched_and_cached() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("free port");
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buffer = [0u8; 4096];
                let _ = stream.read(&mut buffer);
                let body = r#"{"access_token":"test-token","expires_in":3600}"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        let auth = Auth::OAuth2 {
            grant: crate::model::OAuthGrant::ClientCredentials,
            token_url: format!("http://127.0.0.1:{port}/token"),
            client_id: "client".into(),
            client_secret: "secret".into(),
            scope: "read".into(),
            authorization_url: String::new(),
        };
        let cache = new_oauth_cache();
        let env = HashMap::new();
        assert_eq!(block_on(oauth_token(&auth, &env, &cache, &Settings::default())).unwrap(), "test-token");
        assert!(oauth_connected(&auth, &env, &cache));
        server.join().unwrap();
        // The server is gone: a second call must come from the cache.
        assert_eq!(block_on(oauth_token(&auth, &env, &cache, &Settings::default())).unwrap(), "test-token");
    }

    #[test]
    fn pkce_without_a_token_asks_to_connect() {
        let auth = Auth::OAuth2 {
            grant: crate::model::OAuthGrant::Pkce,
            token_url: "https://provider.test/token".into(),
            client_id: "client".into(),
            client_secret: String::new(),
            scope: String::new(),
            authorization_url: "https://provider.test/authorize".into(),
        };
        let cache = new_oauth_cache();
        let error = block_on(oauth_token(&auth, &HashMap::new(), &cache, &Settings::default())).unwrap_err();
        assert!(error.to_lowercase().contains("connect"));
    }

    /// Mock tests bind real ports; serialise them so two tests can never pick
    /// the same free port at the same time.
    fn mock_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn free_port() -> u16 {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("free port");
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        port
    }

    fn raw_request(port: u16, path: &str, extra: &str) -> (String, String) {
        use std::net::TcpStream;
        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let request =
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n{extra}\r\n");
        stream.write_all(request.as_bytes()).unwrap();
        let mut buffer = Vec::new();
        let mut chunk = [0u8; 1024];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => buffer.extend_from_slice(&chunk[..read]),
                Err(_) => break,
            }
        }
        let text = String::from_utf8_lossy(&buffer).to_string();
        let (head, body) = text.split_once("\r\n\r\n").unwrap_or((text.as_str(), ""));
        (head.lines().next().unwrap_or("").to_string(), body.to_string())
    }

    #[test]
    fn mock_applies_latency_and_request_overrides() {
        let _guard = mock_lock();
        let port = free_port();
        let mut server = MockServer::default();
        server.start(
            port,
            vec![MockRoute {
                method: Method::Get,
                path: "/slow".into(),
                status: 200,
                body: "{\"ok\":true}".into(),
                latency_ms: 140,
                failure: MockFailure::None,
            }],
        );
        assert!(server.running, "{:?}", server.error);
        let started = Instant::now();
        let (status, body) = raw_request(port, "/slow", "");
        assert!(status.contains("200"), "{status}");
        assert!(body.contains("ok"));
        assert!(started.elapsed().as_millis() >= 120);

        // Header overrides change the status without touching the route.
        let (status, _) = raw_request(port, "/slow", "X-Mock-Status: 503\r\n");
        assert!(status.contains("503"), "{status}");
        server.stop();
    }

    #[test]
    fn mock_can_reset_the_connection() {
        let _guard = mock_lock();
        let port = free_port();
        let mut server = MockServer::default();
        server.start(
            port,
            vec![MockRoute {
                method: Method::Get,
                path: "/boom".into(),
                status: 200,
                body: "{}".into(),
                latency_ms: 0,
                failure: MockFailure::Reset,
            }],
        );
        assert!(server.running);
        let (status, body) = raw_request(port, "/boom", "");
        assert!(status.is_empty(), "unexpected status line: {status}");
        assert!(body.is_empty());
        server.stop();
    }

    #[test]
    fn mock_overrides_are_parsed_case_insensitively() {
        let request = "GET / HTTP/1.1\r\nHost: x\r\nX-Mock-Status: 429\r\nX-Mock-Delay: 250\r\nX-Mock-Fail: Timeout\r\n";
        let overrides = MockOverrides::parse(request);
        assert_eq!(overrides.status, Some(429));
        assert_eq!(overrides.latency_ms, Some(250));
        assert_eq!(overrides.failure, Some(MockFailure::Timeout));
        assert!(MockOverrides::parse("GET / HTTP/1.1\r\n").status.is_none());
    }

    #[test]
    fn async_requests_report_ttfb_and_total() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("free port");
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buffer = [0u8; 4096];
                let _ = stream.read(&mut buffer);
                std::thread::sleep(Duration::from_millis(120));
                let body = "hello streaming";
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        let request = ApiRequest::new(
            "timing",
            crate::model::Method::Get,
            &format!("http://127.0.0.1:{port}/"),
        );
        let settings = Settings {
            timeout_ms: 5_000,
            ..Settings::default()
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let outcome = runtime.block_on(execute_async(
            &request,
            &HashMap::new(),
            &settings,
            &new_oauth_cache(),
        ));
        server.join().unwrap();
        let data = outcome.data.expect("response");
        assert_eq!(data.body, "hello streaming");
        assert!(!data.truncated);
        assert!(data.timings.ttfb_ms >= 90, "ttfb was {}", data.timings.ttfb_ms);
        assert!(data.timings.total_ms >= data.timings.ttfb_ms);
        // 127.0.0.1 needs no DNS and is plain HTTP: only TCP is measurable.
        assert_eq!(data.timings.dns_ms, Some(0));
        assert!(data.timings.connect_ms.is_some());
        assert!(data.timings.tls_ms.is_none());
    }

    #[test]
    fn mock_server_serves_marked_routes() {
        let _guard = mock_lock();
        let probe = TcpListener::bind(("127.0.0.1", 0)).expect("free port");
        let port = probe.local_addr().unwrap().port();
        drop(probe);

        let mut server = MockServer::default();
        server.start(
            port,
            vec![MockRoute {
                method: Method::Get,
                path: "/v1/users".into(),
                status: 200,
                body: "{\"total\": 128}".into(),
                latency_ms: 0,
                failure: MockFailure::None,
            }],
        );
        assert!(server.running, "mock server failed to start");

        let req = ApiRequest::new(
            "t",
            Method::Get,
            &format!("http://127.0.0.1:{port}/v1/users?page=1"),
        );
        let outcome = execute(&req, &HashMap::new(), &Settings::default());
        match outcome.data {
            Ok(data) => {
                assert_eq!(data.status, 200);
                assert!(data.body.contains("128"));
            }
            Err(err) => panic!("request failed: {err}"),
        }

        let req_missing = ApiRequest::new(
            "t",
            Method::Get,
            &format!("http://127.0.0.1:{port}/v1/nope"),
        );
        let outcome = execute(&req_missing, &HashMap::new(), &Settings::default());
        assert_eq!(outcome.status(), Some(404));

        server.stop();
        assert!(!server.running);
    }

    #[test]
    fn query_params_and_form_encoding() {
        assert_eq!(urlencode("a b&c"), "a+b%26c");
    }
}
