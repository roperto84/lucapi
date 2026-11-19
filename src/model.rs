//! Data model, demo workspace and on-disk persistence for LucAPI.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub type Id = u64;

static ID_SEQ: AtomicU64 = AtomicU64::new(1_000);

pub fn next_id() -> Id {
    ID_SEQ.fetch_add(1, Ordering::Relaxed)
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

impl Default for Method {
    fn default() -> Self {
        Method::Get
    }
}

impl Method {
    pub const ALL: [Method; 7] = [
        Method::Get,
        Method::Post,
        Method::Put,
        Method::Patch,
        Method::Delete,
        Method::Head,
        Method::Options,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
            Method::Head => "HEAD",
            Method::Options => "OPTIONS",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct KV {
    pub enabled: bool,
    pub key: String,
    pub value: String,
    #[serde(default)]
    pub desc: String,
    #[serde(default)]
    pub secret: bool,
}

impl KV {
    pub fn new() -> Self {
        Self {
            enabled: true,
            ..Default::default()
        }
    }

    pub fn pair(key: &str, value: &str) -> Self {
        Self {
            enabled: true,
            key: key.to_string(),
            value: value.to_string(),
            ..Default::default()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BodyMode {
    #[default]
    None,
    Json,
    Text,
    Form,
    Multipart,
    Binary,
}

impl BodyMode {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FieldKind {
    #[default]
    Text,
    File,
}

/// One entry of a multipart/form-data body. `value` holds the text or the
/// absolute path of the file, depending on `kind`.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct FormField {
    pub enabled: bool,
    pub key: String,
    pub value: String,
    pub kind: FieldKind,
    #[serde(default)]
    pub content_type: String,
}

impl FormField {
    pub fn text(key: &str, value: &str) -> Self {
        Self {
            enabled: true,
            key: key.to_string(),
            value: value.to_string(),
            kind: FieldKind::Text,
            content_type: String::new(),
        }
    }

    pub fn file(key: &str, path: &str) -> Self {
        Self {
            enabled: true,
            key: key.to_string(),
            value: path.to_string(),
            kind: FieldKind::File,
            content_type: String::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum OAuthGrant {
    #[default]
    ClientCredentials,
    Pkce,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub enum Auth {
    /// Use the auth configured on the collection.
    #[default]
    Inherit,
    None,
    Bearer {
        token: String,
    },
    Basic {
        user: String,
        pass: String,
    },
    ApiKey {
        header: String,
        value: String,
    },
    OAuth2 {
        grant: OAuthGrant,
        token_url: String,
        client_id: String,
        client_secret: String,
        scope: String,
        #[serde(default)]
        authorization_url: String,
    },
}

impl Auth {
    pub const KINDS: [&'static str; 7] = [
        "Inherit from collection",
        "No auth",
        "Bearer token",
        "Basic auth",
        "API key",
        "OAuth2 · client credentials",
        "OAuth2 · PKCE",
    ];

    pub fn kind_index(&self) -> usize {
        match self {
            Auth::Inherit => 0,
            Auth::None => 1,
            Auth::Bearer { .. } => 2,
            Auth::Basic { .. } => 3,
            Auth::ApiKey { .. } => 4,
            Auth::OAuth2 {
                grant: OAuthGrant::Pkce,
                ..
            } => 6,
            Auth::OAuth2 { .. } => 5,
        }
    }

    pub fn with_kind(index: usize) -> Self {
        match index {
            1 => Auth::None,
            2 => Auth::Bearer {
                token: String::new(),
            },
            3 => Auth::Basic {
                user: String::new(),
                pass: String::new(),
            },
            4 => Auth::ApiKey {
                header: "X-API-Key".to_string(),
                value: String::new(),
            },
            5 => Auth::OAuth2 {
                grant: OAuthGrant::ClientCredentials,
                token_url: String::new(),
                client_id: String::new(),
                client_secret: String::new(),
                scope: String::new(),
                authorization_url: String::new(),
            },
            6 => Auth::OAuth2 {
                grant: OAuthGrant::Pkce,
                token_url: String::new(),
                client_id: String::new(),
                client_secret: String::new(),
                scope: String::new(),
                authorization_url: String::new(),
            },
            _ => Auth::Inherit,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Auth::Inherit => "Inherit",
            Auth::None => "No auth",
            Auth::Bearer { .. } => "Bearer token",
            Auth::Basic { .. } => "Basic auth",
            Auth::ApiKey { .. } => "API key",
            Auth::OAuth2 {
                grant: OAuthGrant::Pkce,
                ..
            } => "OAuth2 PKCE",
            Auth::OAuth2 { .. } => "OAuth2 client credentials",
        }
    }

    /// Resolves `Inherit` against the collection auth.
    pub fn resolve(&self, collection: &Auth) -> Auth {
        match self {
            Auth::Inherit => match collection {
                Auth::Inherit => Auth::None,
                other => other.clone(),
            },
            other => other.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssertKind {
    StatusEquals,
    BodyContains,
    JsonFieldEquals,
    HeaderEquals,
    ResponseTimeUnder,
}

impl Default for AssertKind {
    fn default() -> Self {
        AssertKind::StatusEquals
    }
}

impl AssertKind {
    pub const ALL: [AssertKind; 5] = [
        AssertKind::StatusEquals,
        AssertKind::BodyContains,
        AssertKind::JsonFieldEquals,
        AssertKind::HeaderEquals,
        AssertKind::ResponseTimeUnder,
    ];

    pub fn label(self) -> &'static str {
        match self {
            AssertKind::StatusEquals => "Status code equals",
            AssertKind::BodyContains => "Body contains",
            AssertKind::JsonFieldEquals => "JSON field equals",
            AssertKind::HeaderEquals => "Header equals",
            AssertKind::ResponseTimeUnder => "Response time under",
        }
    }

    pub fn placeholder(self) -> (&'static str, &'static str) {
        match self {
            AssertKind::StatusEquals => ("", "200"),
            AssertKind::BodyContains => ("", "\"id\""),
            AssertKind::JsonFieldEquals => ("data.0.id", "1"),
            AssertKind::HeaderEquals => ("content-type", "application/json"),
            AssertKind::ResponseTimeUnder => ("", "500"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Assertion {
    pub enabled: bool,
    pub kind: AssertKind,
    #[serde(default)]
    pub target: String,
    pub expected: String,
}

impl Assertion {
    pub fn new(kind: AssertKind, target: &str, expected: &str) -> Self {
        Self {
            enabled: true,
            kind,
            target: target.to_string(),
            expected: expected.to_string(),
        }
    }

    pub fn label(&self) -> String {
        match self.kind {
            AssertKind::StatusEquals => format!("Status equals {}", self.expected),
            AssertKind::BodyContains => format!("Body contains {}", self.expected),
            AssertKind::JsonFieldEquals => {
                format!("{} equals {}", self.target, self.expected)
            }
            AssertKind::HeaderEquals => {
                format!("Header {} equals {}", self.target, self.expected)
            }
            AssertKind::ResponseTimeUnder => {
                format!("Response time under {} ms", self.expected)
            }
        }
    }
}

/// How a mock route can misbehave on purpose.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MockFailure {
    #[default]
    None,
    /// Never answers; the client hits its timeout.
    Timeout,
    /// Closes the socket without a response.
    Reset,
}

impl MockFailure {
    pub const ALL: [MockFailure; 3] =
        [MockFailure::None, MockFailure::Timeout, MockFailure::Reset];

    pub fn label(self) -> &'static str {
        match self {
            MockFailure::None => "Normal",
            MockFailure::Timeout => "Timeout",
            MockFailure::Reset => "Connection reset",
        }
    }

    pub fn index(self) -> usize {
        match self {
            MockFailure::None => 0,
            MockFailure::Timeout => 1,
            MockFailure::Reset => 2,
        }
    }

    pub fn from_index(index: usize) -> Self {
        MockFailure::ALL.get(index).copied().unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CaptureSource {
    #[default]
    JsonField,
    Header,
}

impl CaptureSource {
    pub fn label(self) -> &'static str {
        match self {
            CaptureSource::JsonField => "JSON field",
            CaptureSource::Header => "Header",
        }
    }

    pub fn placeholder(self) -> &'static str {
        match self {
            CaptureSource::JsonField => "data.token",
            CaptureSource::Header => "x-auth-token",
        }
    }
}

/// Extracts a value from a response after it is received and stores it in the
/// active environment, so later requests can use it as `{{variable}}`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Capture {
    pub enabled: bool,
    pub source: CaptureSource,
    pub path: String,
    pub variable: String,
    #[serde(default)]
    pub secret: bool,
}

impl Capture {
    pub fn new(source: CaptureSource, path: &str, variable: &str) -> Self {
        Self {
            enabled: true,
            source,
            path: path.to_string(),
            variable: variable.to_string(),
            secret: true,
        }
    }
}

impl Default for Capture {
    fn default() -> Self {
        Self {
            enabled: true,
            source: CaptureSource::JsonField,
            path: String::new(),
            variable: String::new(),
            secret: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ApiRequest {
    pub id: Id,
    pub name: String,
    pub method: Method,
    pub url: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub params: Vec<KV>,
    #[serde(default)]
    pub headers: Vec<KV>,
    #[serde(default)]
    pub body_mode: BodyMode,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub form: Vec<KV>,
    #[serde(default)]
    pub multipart: Vec<FormField>,
    #[serde(default)]
    pub binary_path: String,
    #[serde(default)]
    pub auth: Auth,
    #[serde(default)]
    pub assertions: Vec<Assertion>,
    #[serde(default)]
    pub captures: Vec<Capture>,
    #[serde(default)]
    pub mock: bool,
    /// Artificial delay served by the mock server, in milliseconds.
    #[serde(default)]
    pub mock_latency_ms: u64,
    /// Simulated failure served by the mock server.
    #[serde(default)]
    pub mock_failure: MockFailure,
    #[serde(default)]
    pub example_status: u16,
    #[serde(default)]
    pub example_body: String,
}

impl ApiRequest {
    pub fn new(name: &str, method: Method, url: &str) -> Self {
        Self {
            id: next_id(),
            name: name.to_string(),
            method,
            url: url.to_string(),
            example_status: if method == Method::Post { 201 } else { 200 },
            ..Default::default()
        }
    }

    pub fn filled_params(&self) -> usize {
        self.params
            .iter()
            .filter(|k| k.enabled && !k.key.trim().is_empty())
            .count()
    }

    pub fn filled_headers(&self) -> usize {
        self.headers
            .iter()
            .filter(|k| k.enabled && !k.key.trim().is_empty())
            .count()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Folder {
    pub id: Id,
    pub name: String,
    pub open: bool,
    #[serde(default)]
    pub auth: Auth,
    pub requests: Vec<ApiRequest>,
}

impl Folder {
    pub fn new(name: &str) -> Self {
        Self {
            id: next_id(),
            name: name.to_string(),
            open: true,
            auth: Auth::None,
            requests: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Environment {
    pub id: Id,
    pub name: String,
    pub vars: Vec<KV>,
}

impl Environment {
    pub fn new(name: &str) -> Self {
        Self {
            id: next_id(),
            name: name.to_string(),
            vars: vec![KV::pair("baseUrl", "http://localhost:8080/v1")],
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: Id,
    pub request_id: Option<Id>,
    pub method: Method,
    pub url: String,
    pub status: Option<u16>,
    pub ms: u64,
    pub size: u64,
    pub at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub dark: bool,
    pub timeout_ms: u64,
    pub follow_redirects: bool,
    pub line_numbers: bool,
    pub mock_port: u16,
    #[serde(default)]
    pub proxy: String,
    #[serde(default)]
    pub accept_invalid_certs: bool,
    #[serde(default)]
    pub ca_bundle: String,
    /// Measures DNS/TCP/TLS with a parallel probe connection.
    #[serde(default = "default_true")]
    pub measure_phases: bool,
    /// Public URL of the published documentation, used for share links.
    #[serde(default)]
    pub docs_base_url: String,
    /// Keeps secret values in the macOS Keychain instead of the workspace file.
    #[serde(default = "default_true")]
    pub keychain_secrets: bool,
}

fn default_true() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            dark: false,
            timeout_ms: 15_000,
            follow_redirects: true,
            line_numbers: true,
            mock_port: 4010,
            proxy: String::new(),
            accept_invalid_certs: false,
            ca_bundle: String::new(),
            measure_phases: true,
            docs_base_url: String::new(),
            keychain_secrets: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct SessionState {
    /// `None` means "no session saved yet", which is different from a session
    /// with every tab closed.
    #[serde(default)]
    pub open_tabs: Option<Vec<Id>>,
    #[serde(default)]
    pub active_tab: Option<Id>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectData {
    pub name: String,
    pub folders: Vec<Folder>,
    pub envs: Vec<Environment>,
    pub active_env: Option<Id>,
    pub history: Vec<HistoryEntry>,
    pub settings: Settings,
    #[serde(default)]
    pub session: SessionState,
}

impl ProjectData {
    pub fn find_request(&self, id: Id) -> Option<&ApiRequest> {
        self.folders
            .iter()
            .flat_map(|f| f.requests.iter())
            .find(|r| r.id == id)
    }

    pub fn find_request_mut(&mut self, id: Id) -> Option<&mut ApiRequest> {
        self.folders
            .iter_mut()
            .flat_map(|f| f.requests.iter_mut())
            .find(|r| r.id == id)
    }

    pub fn folder_of(&self, id: Id) -> Option<&Folder> {
        self.folders
            .iter()
            .find(|f| f.requests.iter().any(|r| r.id == id))
    }

    /// Auth actually applied to a request, resolving collection inheritance.
    pub fn resolved_auth(&self, id: Id) -> Option<Auth> {
        let request = self.find_request(id)?;
        let folder = self.folder_of(id)?;
        Some(request.auth.resolve(&folder.auth))
    }

    pub fn all_requests(&self) -> Vec<&ApiRequest> {
        self.folders
            .iter()
            .flat_map(|f| f.requests.iter())
            .collect()
    }

    pub fn total_requests(&self) -> usize {
        self.folders.iter().map(|f| f.requests.len()).sum()
    }

    pub fn active_env(&self) -> Option<&Environment> {
        self.active_env
            .and_then(|id| self.envs.iter().find(|e| e.id == id))
    }

    pub fn env_map(&self) -> HashMap<String, String> {
        let mut map = HashMap::new();
        if let Some(env) = self.active_env() {
            for var in &env.vars {
                if !var.enabled || var.key.trim().is_empty() {
                    continue;
                }
                // A value left as a Keychain reference (for example after a
                // forgotten item) is resolved here so expansion never emits it.
                let value = if crate::vault::is_reference(&var.value) {
                    crate::vault::resolve(&var.value).unwrap_or_else(|| var.value.clone())
                } else {
                    var.value.clone()
                };
                map.insert(var.key.trim().to_string(), value);
            }
        }
        map
    }

    pub fn expand(&self, text: &str) -> String {
        expand_with(text, &self.env_map())
    }

    pub fn add_history(&mut self, entry: HistoryEntry) {
        self.history.insert(0, entry);
        if self.history.len() > 120 {
            self.history.truncate(120);
        }
    }

    pub fn remove_folder(&mut self, id: Id) {
        self.folders.retain(|f| f.id != id);
    }

    pub fn remove_request(&mut self, id: Id) {
        for folder in &mut self.folders {
            folder.requests.retain(|r| r.id != id);
        }
    }
}

pub fn expand_with(text: &str, vars: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'{' {
            if let Some(end) = text[i + 2..].find("}}") {
                let key = text[i + 2..i + 2 + end].trim();
                if let Some(value) = vars.get(key) {
                    out.push_str(value);
                    i = i + 2 + end + 2;
                    continue;
                }
            }
        }
        let ch = text[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

pub fn storage_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".lucapi").join("workspace.json")
}

/// Reads a workspace from disk and resolves Keychain references.
pub fn load_from(path: &std::path::Path) -> Result<ProjectData, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let mut data: ProjectData = serde_json::from_str(&raw)
        .map_err(|error| format!("{} is not a LucAPI workspace: {error}", path.display()))?;
    open_after_load(&mut data);
    Ok(data)
}

impl ProjectData {
    /// Loads a workspace from an explicit path, resolving Keychain references.
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        load_from(path)
    }
}

pub fn load_or_seed() -> ProjectData {
    let path = storage_path();
    if let Ok(mut data) = load_from(&path) {
        if !data.folders.is_empty() {
            open_after_load(&mut data);
            return data;
        }
    }
    seed()
}

/// Accounts of every secret held by this workspace, so they can be sealed,
/// reopened or forgotten as a set.
#[derive(Default, Debug)]
pub struct SecretReport {
    pub sealed: usize,
    pub opened: usize,
    pub missing: Vec<String>,
    pub failed: Vec<String>,
}

fn secret_slots(data: &ProjectData) -> Vec<(String, String)> {
    let mut slots: Vec<(String, String)> = Vec::new();
    for env in &data.envs {
        for var in &env.vars {
            if var.secret && !var.value.trim().is_empty() {
                slots.push((
                    format!("env:{}:{}", env.id, var.key.trim()),
                    var.value.clone(),
                ));
            }
        }
    }
    for request in data.all_requests() {
        for kv in &request.headers {
            if kv.secret && !kv.value.trim().is_empty() {
                slots.push((
                    format!("req:{}:header:{}", request.id, kv.key.trim()),
                    kv.value.clone(),
                ));
            }
        }
        for kv in &request.params {
            if kv.secret && !kv.value.trim().is_empty() {
                slots.push((
                    format!("req:{}:param:{}", request.id, kv.key.trim()),
                    kv.value.clone(),
                ));
            }
        }
        match &request.auth {
            Auth::Bearer { token } if !token.trim().is_empty() => slots.push((
                format!("req:{}:bearer", request.id),
                token.clone(),
            )),
            Auth::Basic { pass, .. } if !pass.trim().is_empty() => slots.push((
                format!("req:{}:basic-pass", request.id),
                pass.clone(),
            )),
            Auth::ApiKey { value, .. } if !value.trim().is_empty() => slots.push((
                format!("req:{}:api-key", request.id),
                value.clone(),
            )),
            Auth::OAuth2 { client_secret, .. } if !client_secret.trim().is_empty() => slots.push((
                format!("req:{}:oauth-secret", request.id),
                client_secret.clone(),
            )),
            _ => {}
        }
    }
    slots
}

/// How many secret values this workspace holds.
pub fn secret_slot_count(data: &ProjectData) -> usize {
    secret_slots(data).len()
}

/// Every secret with whether it is already a Keychain reference.
pub fn secret_inventory(data: &ProjectData) -> Vec<(String, bool)> {
    secret_slots(data)
        .into_iter()
        .map(|(account, value)| (account, crate::vault::is_reference(&value)))
        .collect()
}

/// Replaces secret values with Keychain references in a copy meant for disk.
/// The in-memory workspace keeps plaintext so editing stays ordinary.
pub fn seal_for_storage(data: &ProjectData) -> (ProjectData, SecretReport) {
    let mut sealed = data.clone();
    let mut report = SecretReport::default();
    if !data.settings.keychain_secrets || !crate::vault::available() {
        return (sealed, report);
    }
    for (account, value) in secret_slots(data) {
        if crate::vault::is_reference(&value) {
            report.sealed += 1;
            continue;
        }
        match crate::vault::store(&account, &value) {
            Ok(()) => {
                report.sealed += 1;
                if let Some(slot) = find_slot_mut(&mut sealed, &account) {
                    *slot = crate::vault::reference(&account);
                }
            }
            Err(error) => report.failed.push(format!("{account}: {error}")),
        }
    }
    (sealed, report)
}

/// Resolves `keychain://` references after loading, so the app works with the
/// real values.
pub fn open_after_load(data: &mut ProjectData) -> SecretReport {
    let mut report = SecretReport::default();
    if !crate::vault::available() {
        return report;
    }
    let slots = secret_slots(data);
    for (account, value) in slots {
        if !crate::vault::is_reference(&value) {
            continue;
        }
        match crate::vault::load(&account) {
            Ok(Some(secret)) => {
                report.opened += 1;
                if let Some(slot) = find_slot_mut(data, &account) {
                    *slot = secret;
                }
            }
            Ok(None) => report.missing.push(account),
            Err(error) => report.failed.push(format!("{account}: {error}")),
        }
    }
    report
}

/// Removes every secret of this workspace from the Keychain.
pub fn forget_secrets(data: &mut ProjectData) -> SecretReport {
    let mut report = SecretReport::default();
    for (account, _) in secret_slots(data) {
        if crate::vault::delete(&account).is_ok() {
            report.sealed += 1;
        }
    }
    report
}

fn find_slot_mut<'a>(data: &'a mut ProjectData, account: &str) -> Option<&'a mut String> {
    let (env_id, key) = parse_env_account(account);
    if let Some(key) = key {
        let env = data.envs.iter_mut().find(|env| Some(env.id) == env_id)?;
        let var = env.vars.iter_mut().find(|var| var.key.trim() == key)?;
        return Some(&mut var.value);
    }
    let (request_id, field) = parse_request_account(account)?;
    let request = data.find_request_mut(request_id)?;
    match field {
        "bearer" => match &mut request.auth {
            Auth::Bearer { token } => Some(token),
            _ => None,
        },
        "basic-pass" => match &mut request.auth {
            Auth::Basic { pass, .. } => Some(pass),
            _ => None,
        },
        "api-key" => match &mut request.auth {
            Auth::ApiKey { value, .. } => Some(value),
            _ => None,
        },
        "oauth-secret" => match &mut request.auth {
            Auth::OAuth2 { client_secret, .. } => Some(client_secret),
            _ => None,
        },
        header if header.starts_with("header:") => {
            let key = &header[7..];
            request
                .headers
                .iter_mut()
                .find(|kv| kv.key.trim() == key)
                .map(|kv| &mut kv.value)
        }
        param if param.starts_with("param:") => {
            let key = &param[6..];
            request
                .params
                .iter_mut()
                .find(|kv| kv.key.trim() == key)
                .map(|kv| &mut kv.value)
        }
        _ => None,
    }
}

fn parse_env_account(account: &str) -> (Option<Id>, Option<&str>) {
    let Some(rest) = account.strip_prefix("env:") else {
        return (None, None);
    };
    match rest.split_once(':') {
        Some((id, key)) => (id.parse::<Id>().ok(), Some(key)),
        None => (None, None),
    }
}

fn parse_request_account(account: &str) -> Option<(Id, &str)> {
    let rest = account.strip_prefix("req:")?;
    let (id, field) = rest.split_once(':')?;
    Some((id.parse::<Id>().ok()?, field))
}

/// How many previous versions of the workspace are kept next to it.
const BACKUP_LIMIT: usize = 10;

pub fn save(data: &ProjectData) -> Result<(), String> {
    save_with_report(data).map(|_| ())
}

/// Saves and returns what happened to the secrets, so the UI can warn when the
/// Keychain refused a value (it then stays in the workspace, never lost).
pub fn save_with_report(data: &ProjectData) -> Result<SecretReport, String> {
    let path = storage_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let (stored, report) = seal_for_storage(data);
    let raw = serde_json::to_string_pretty(&stored).map_err(|e| e.to_string())?;
    if path.exists() {
        backup_workspace(&path);
    }
    write_atomic(&path, raw.as_bytes())?;
    Ok(report)
}

/// Writes through a temporary file in the same directory and renames it, so a
/// crash can never leave a half-written workspace behind.
pub fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let temporary = path.with_extension("json.tmp");
    {
        let mut file = std::fs::File::create(&temporary).map_err(|e| e.to_string())?;
        file.write_all(bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
    }
    std::fs::rename(&temporary, path).map_err(|e| e.to_string())
}

/// Copies the previous file into `backups/` before it is replaced.
pub fn backup_workspace(path: &std::path::Path) {
    let Some(parent) = path.parent() else {
        return;
    };
    let directory = parent.join("backups");
    if std::fs::create_dir_all(&directory).is_err() {
        return;
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let backup = directory.join(format!("workspace-{stamp}.json"));
    let _ = std::fs::copy(path, backup);
    prune_backups(&directory, BACKUP_LIMIT);
}

fn prune_backups(directory: &std::path::Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let mut files: Vec<std::path::PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().and_then(|extension| extension.to_str()) == Some("json")
        })
        .collect();
    if files.len() <= keep {
        return;
    }
    files.sort();
    let remove = files.len() - keep;
    for path in files.into_iter().take(remove) {
        let _ = std::fs::remove_file(path);
    }
}

pub fn seed() -> ProjectData {
    let mut users = Folder::new("User Service");
    users.requests.push(ApiRequest {
        name: "List users".into(),
        method: Method::Get,
        url: "{{baseUrl}}/users".into(),
        description: "Returns a paginated list of users in the workspace.".into(),
        params: vec![
            KV {
                desc: "Page number".into(),
                ..KV::pair("page", "1")
            },
            KV {
                desc: "Items per page".into(),
                ..KV::pair("per_page", "20")
            },
        ],
        headers: vec![KV::pair("Accept", "application/json")],
        assertions: vec![
            Assertion::new(AssertKind::StatusEquals, "", "200"),
            Assertion::new(AssertKind::JsonFieldEquals, "total", "128"),
        ],
        mock: true,
        mock_latency_ms: 0,
        mock_failure: MockFailure::None,
        example_status: 200,
        example_body: r#"{
  "data": [
    {
      "id": 1,
      "name": "Ada Lovelace",
      "email": "ada@example.com",
      "role": "admin",
      "created_at": "2026-02-14T09:21:03Z"
    },
    {
      "id": 2,
      "name": "Grace Hopper",
      "email": "grace@example.com",
      "role": "member",
      "created_at": "2026-03-02T14:07:55Z"
    }
  ],
  "page": 1,
  "per_page": 20,
  "total": 128
}"#
        .into(),
        ..ApiRequest::new("List users", Method::Get, "{{baseUrl}}/users")
    });

    users.requests.push(ApiRequest {
        name: "Get user".into(),
        method: Method::Get,
        url: "{{baseUrl}}/users/1".into(),
        description: "Fetches a single user by its numeric identifier.".into(),
        headers: vec![KV::pair("Accept", "application/json")],
        assertions: vec![Assertion::new(AssertKind::StatusEquals, "", "200")],
        example_status: 200,
        example_body: r#"{
  "id": 1,
  "name": "Ada Lovelace",
  "email": "ada@example.com",
  "role": "admin",
  "created_at": "2026-02-14T09:21:03Z"
}"#
        .into(),
        ..ApiRequest::new("Get user", Method::Get, "{{baseUrl}}/users/1")
    });

    users.requests.push(ApiRequest {
        name: "Create user".into(),
        method: Method::Post,
        url: "{{baseUrl}}/users".into(),
        description: "Creates a user and returns the stored record.".into(),
        headers: vec![KV::pair("Accept", "application/json")],
        body_mode: BodyMode::Json,
        body: r#"{
  "name": "Linus Torvalds",
  "email": "linus@example.com",
  "role": "member"
}"#
        .into(),
        auth: Auth::Bearer {
            token: "{{token}}".into(),
        },
        assertions: vec![
            Assertion::new(AssertKind::StatusEquals, "", "201"),
            Assertion::new(AssertKind::BodyContains, "", "\"id\""),
        ],
        mock: true,
        mock_latency_ms: 0,
        mock_failure: MockFailure::None,
        example_status: 201,
        example_body: r#"{
  "id": 129,
  "name": "Linus Torvalds",
  "email": "linus@example.com",
  "role": "member",
  "created_at": "2026-11-12T16:40:12Z"
}"#
        .into(),
        ..ApiRequest::new("Create user", Method::Post, "{{baseUrl}}/users")
    });

    users.requests.push(ApiRequest {
        name: "Update user".into(),
        method: Method::Put,
        url: "{{baseUrl}}/users/1".into(),
        description: "Replaces the mutable fields of an existing user.".into(),
        body_mode: BodyMode::Json,
        body: r#"{
  "name": "Ada Lovelace",
  "role": "admin"
}"#
        .into(),
        auth: Auth::Bearer {
            token: "{{token}}".into(),
        },
        example_status: 200,
        example_body: r#"{
  "id": 1,
  "name": "Ada Lovelace",
  "role": "admin",
  "updated_at": "2026-11-12T16:41:20Z"
}"#
        .into(),
        ..ApiRequest::new("Update user", Method::Put, "{{baseUrl}}/users/1")
    });

    users.requests.push(ApiRequest {
        name: "Delete user".into(),
        method: Method::Delete,
        url: "{{baseUrl}}/users/1".into(),
        description: "Removes a user permanently. Returns an empty body.".into(),
        auth: Auth::Bearer {
            token: "{{token}}".into(),
        },
        assertions: vec![Assertion::new(AssertKind::StatusEquals, "", "204")],
        example_status: 204,
        example_body: String::new(),
        ..ApiRequest::new("Delete user", Method::Delete, "{{baseUrl}}/users/1")
    });

    let mut auth = Folder::new("Auth");
    auth.requests.push(ApiRequest {
        name: "Create session".into(),
        method: Method::Post,
        url: "{{baseUrl}}/auth/login".into(),
        description: "Exchanges credentials for a short lived access token.".into(),
        body_mode: BodyMode::Json,
        body: r#"{
  "email": "ada@example.com",
  "password": "correct-horse-battery"
}"#
        .into(),
        assertions: vec![Assertion::new(AssertKind::StatusEquals, "", "200")],
        captures: vec![Capture::new(CaptureSource::JsonField, "access_token", "token")],
        mock: true,
        mock_latency_ms: 0,
        mock_failure: MockFailure::None,
        example_status: 200,
        example_body: r#"{
  "access_token": "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.demo",
  "token_type": "Bearer",
  "expires_in": 3600
}"#
        .into(),
        ..ApiRequest::new("Create session", Method::Post, "{{baseUrl}}/auth/login")
    });

    auth.requests.push(ApiRequest {
        name: "Refresh token".into(),
        method: Method::Post,
        url: "{{baseUrl}}/auth/refresh".into(),
        description: "Issues a fresh access token using the refresh cookie.".into(),
        auth: Auth::Bearer {
            token: "{{token}}".into(),
        },
        example_status: 200,
        example_body: r#"{
  "access_token": "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.refreshed",
  "expires_in": 3600
}"#
        .into(),
        ..ApiRequest::new("Refresh token", Method::Post, "{{baseUrl}}/auth/refresh")
    });

    let mut billing = Folder::new("Billing");
    billing.requests.push(ApiRequest {
        name: "List invoices".into(),
        method: Method::Get,
        url: "{{baseUrl}}/invoices".into(),
        description: "Lists invoices for the current workspace, newest first.".into(),
        params: vec![KV {
            desc: "Filter by invoice status".into(),
            ..KV::pair("status", "paid")
        }],
        auth: Auth::Bearer {
            token: "{{token}}".into(),
        },
        mock: true,
        mock_latency_ms: 0,
        mock_failure: MockFailure::None,
        example_status: 200,
        example_body: r#"{
  "data": [
    {
      "id": "inv_1024",
      "amount": 4200,
      "currency": "USD",
      "status": "paid",
      "issued_at": "2026-10-01T00:00:00Z"
    },
    {
      "id": "inv_1025",
      "amount": 1250,
      "currency": "USD",
      "status": "paid",
      "issued_at": "2026-11-01T00:00:00Z"
    }
  ],
  "total": 2
}"#
        .into(),
        ..ApiRequest::new("List invoices", Method::Get, "{{baseUrl}}/invoices")
    });

    let envs = vec![
        Environment {
            id: next_id(),
            name: "Local".into(),
            vars: vec![
                KV::pair("baseUrl", "http://localhost:8080/v1"),
                KV::pair("token", "local-dev-token"),
            ],
        },
        Environment {
            id: next_id(),
            name: "Staging".into(),
            vars: vec![
                KV::pair("baseUrl", "https://staging.example.com/v1"),
                KV {
                    secret: true,
                    ..KV::pair("token", "stg_9f2c41ab77d0")
                },
            ],
        },
        Environment {
            id: next_id(),
            name: "Production".into(),
            vars: vec![
                KV::pair("baseUrl", "https://api.example.com/v1"),
                KV {
                    secret: true,
                    ..KV::pair("token", "prod_51ce09ba33e8")
                },
            ],
        },
    ];

    let now = now_secs();
    let list_users_id = users.requests[0].id;
    let create_user_id = users.requests[2].id;
    let invoices_id = billing.requests[0].id;
    let history = vec![
        HistoryEntry {
            id: next_id(),
            request_id: Some(list_users_id),
            method: Method::Get,
            url: "https://api.example.com/v1/users?page=1&per_page=20".into(),
            status: Some(200),
            ms: 184,
            size: 842,
            at: now - 240,
        },
        HistoryEntry {
            id: next_id(),
            request_id: Some(create_user_id),
            method: Method::Post,
            url: "https://api.example.com/v1/users".into(),
            status: Some(201),
            ms: 263,
            size: 214,
            at: now - 1_500,
        },
        HistoryEntry {
            id: next_id(),
            request_id: Some(invoices_id),
            method: Method::Get,
            url: "https://api.example.com/v1/invoices?status=paid".into(),
            status: Some(200),
            ms: 96,
            size: 388,
            at: now - 7_200,
        },
    ];

    ProjectData {
        name: "LucAPI Demo".into(),
        folders: vec![users, auth, billing],
        active_env: Some(envs[2].id),
        envs,
        history,
        settings: Settings::default(),
        session: SessionState::default(),
    }
}

pub fn format_bytes(n: usize) -> String {
    if n < 1024 {
        format!("{n} B")
    } else if n < 1024 * 1024 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{:.1} MB", n as f64 / (1024.0 * 1024.0))
    }
}

pub fn format_ms(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms} ms")
    } else {
        format!("{:.2} s", ms as f64 / 1000.0)
    }
}

pub fn relative_time(at: u64) -> String {
    let now = now_secs();
    let delta = now.saturating_sub(at);
    if delta < 60 {
        "just now".to_string()
    } else if delta < 3600 {
        format!("{} min ago", delta / 60)
    } else if delta < 86_400 {
        format!("{} h ago", delta / 3600)
    } else {
        format!("{} d ago", delta / 86_400)
    }
}


#[cfg(test)]
mod secret_tests {
    use super::*;

    fn workspace() -> ProjectData {
        let mut folder = Folder::new("API");
        let mut request = ApiRequest::new("Login", Method::Post, "https://api.test/login");
        request.auth = Auth::Bearer {
            token: "bearer-abc-123".into(),
        };
        let mut header = KV::pair("X-Api-Key", "header-secret-999");
        header.secret = true;
        request.headers.push(header);
        folder.requests.push(request);
        let mut data: ProjectData = serde_json::from_str(
            "{\"name\":\"team\",\"folders\":[],\"envs\":[],\"active_env\":null,\"history\":[],\"settings\":{\"dark\":true,\"timeout_ms\":15000,\"follow_redirects\":true,\"line_numbers\":true,\"mock_port\":4010},\"session\":{}}",
        )
        .expect("workspace");
        data.folders = vec![folder];
        let mut env = Environment::new("Prod");
        env.vars.clear();
        let mut var = KV::pair("token", "env-secret-777");
        var.secret = true;
        env.vars.push(var);
        data.envs.push(env);
        data
    }

    #[test]
    fn secrets_are_replaced_by_references_on_disk() {
        let data = workspace();
        let (stored, report) = seal_for_storage(&data);
        let json = serde_json::to_string(&stored).unwrap();
        assert!(!json.contains("bearer-abc-123"));
        assert!(!json.contains("header-secret-999"));
        assert!(!json.contains("env-secret-777"));
        assert!(json.contains("keychain://req:"));
        assert!(json.contains("keychain://env:"));
        assert_eq!(report.sealed, 3);
        // The caller keeps plaintext in memory.
        assert_eq!(
            data.find_request(data.folders[0].requests[0].id)
                .unwrap()
                .headers[0]
                .value,
            "header-secret-999"
        );
    }

    #[test]
    fn references_are_reopened_after_loading() {
        let data = workspace();
        let (mut stored, _) = seal_for_storage(&data);
        let report = open_after_load(&mut stored);
        assert_eq!(report.opened, 3);
        assert!(report.missing.is_empty());
        let request = &stored.folders[0].requests[0];
        assert!(matches!(&request.auth, Auth::Bearer { token } if token == "bearer-abc-123"));
        assert_eq!(request.headers[0].value, "header-secret-999");
        assert_eq!(stored.envs[0].vars[0].value, "env-secret-777");
    }

    #[test]
    fn sealing_can_be_turned_off() {
        let mut data = workspace();
        data.settings.keychain_secrets = false;
        let (stored, report) = seal_for_storage(&data);
        assert_eq!(report.sealed, 0);
        let json = serde_json::to_string(&stored).unwrap();
        assert!(json.contains("bearer-abc-123"));
    }

    #[test]
    fn forgetting_removes_the_items_and_reports_them_missing() {
        let data = workspace();
        let (mut stored, _) = seal_for_storage(&data);
        let report = forget_secrets(&mut stored);
        assert_eq!(report.sealed, 3);
        let reopened = open_after_load(&mut stored);
        assert_eq!(reopened.opened, 0);
        assert_eq!(reopened.missing.len(), 3);
    }

    #[test]
    fn secret_count_only_looks_at_flagged_values() {
        let data = workspace();
        assert_eq!(secret_slot_count(&data), 3);
        let mut plain = workspace();
        plain.envs[0].vars.clear();
        plain.folders[0].requests[0].headers[0].secret = false;
        plain.folders[0].requests[0].auth = Auth::None;
        assert_eq!(secret_slot_count(&plain), 0);
    }
}

#[cfg(test)]
mod storage_tests {
    use super::*;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let directory = std::env::temp_dir().join(format!(
            "lucapi-{tag}-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn atomic_write_replaces_the_file() {
        let directory = temp_dir("atomic");
        let path = directory.join("workspace.json");
        write_atomic(&path, b"first").unwrap();
        write_atomic(&path, b"second").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        assert!(!directory.join("workspace.json.tmp").exists());
        std::fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn backups_keep_the_most_recent_versions() {
        let directory = temp_dir("backups");
        let path = directory.join("workspace.json");
        std::fs::write(&path, b"current").unwrap();
        std::fs::create_dir_all(directory.join("backups")).unwrap();
        for index in 0..BACKUP_LIMIT + 5 {
            std::fs::write(
                directory.join("backups").join(format!("workspace-{index:020}.json")),
                b"old",
            )
            .ok();
        }
        backup_workspace(&path);
        let count = std::fs::read_dir(directory.join("backups"))
            .unwrap()
            .count();
        assert_eq!(count, BACKUP_LIMIT);
        std::fs::remove_dir_all(directory).ok();
    }
}

#[cfg(test)]
mod auth_tests {
    use super::*;

    #[test]
    fn inheritance_resolves_against_the_collection() {
        let bearer = Auth::Bearer {
            token: "abc".into(),
        };
        assert!(matches!(
            Auth::Inherit.resolve(&bearer),
            Auth::Bearer { token } if token == "abc"
        ));
        assert!(matches!(Auth::None.resolve(&bearer), Auth::None));
        assert!(matches!(
            Auth::Inherit.resolve(&Auth::Inherit),
            Auth::None
        ));
    }
}
