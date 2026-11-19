//! Import OpenAPI 3.x specs and Postman collections into LucAPI.
//!
//! The output is a workspace-agnostic plan: collections, requests, environment
//! variables and human-readable warnings. Applying it is the app's job.

use serde_json::Value;

use crate::model::{ApiRequest, Auth, BodyMode, Folder, FormField, KV, Method};

#[derive(Debug)]
pub struct ImportPlan {
    pub format: String,
    pub title: String,
    pub folders: Vec<Folder>,
    pub variables: Vec<KV>,
    pub warnings: Vec<String>,
}

impl ImportPlan {
    pub fn request_count(&self) -> usize {
        self.folders.iter().map(|folder| folder.requests.len()).sum()
    }

    pub fn summary(&self) -> String {
        format!(
            "{} · {} request{} · {} collection{}",
            self.format,
            self.request_count(),
            if self.request_count() == 1 { "" } else { "s" },
            self.folders.len(),
            if self.folders.len() == 1 { "" } else { "s" }
        )
    }
}

/// Detects the document format and turns it into an import plan.
pub fn detect_and_parse(text: &str) -> Result<ImportPlan, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("Paste a spec or choose a file first.".to_string());
    }
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return from_value(&value);
    }
    if let Ok(value) = serde_yaml::from_str::<Value>(trimmed) {
        return from_value(&value);
    }
    Err("The document is not valid JSON or YAML.".to_string())
}

fn from_value(value: &Value) -> Result<ImportPlan, String> {
    if value.get("openapi").and_then(Value::as_str).is_some() {
        return openapi(value);
    }
    if value.get("swagger").is_some() {
        return Err(
            "Swagger 2.0 documents are not supported yet. Convert the spec to OpenAPI 3 first."
                .to_string(),
        );
    }
    if is_postman(value) {
        return postman(value);
    }
    Err("Unrecognized document. Expected an OpenAPI 3 spec or a Postman collection.".to_string())
}

fn is_postman(value: &Value) -> bool {
    if value.get("item").and_then(Value::as_array).is_none() {
        return false;
    }
    value
        .get("info")
        .and_then(|info| info.get("schema"))
        .and_then(Value::as_str)
        .map(|schema| schema.contains("postman"))
        .unwrap_or(value.get("info").is_some())
}

// ---------------------------------------------------------------------------
// OpenAPI
// ---------------------------------------------------------------------------

fn openapi(root: &Value) -> Result<ImportPlan, String> {
    let version = root
        .get("openapi")
        .and_then(Value::as_str)
        .unwrap_or("3.0");
    let info = root.get("info").cloned().unwrap_or(Value::Null);
    let title = info
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("Imported API")
        .to_string();
    let base = server_base(root);
    let paths = root
        .get("paths")
        .and_then(Value::as_object)
        .ok_or_else(|| "The spec has no `paths` object.".to_string())?;

    let mut warnings: Vec<String> = Vec::new();
    match &base {
        None => warnings.push(
            "The spec declares no server URL, so request URLs are relative paths.".to_string(),
        ),
        Some(base) if !base.contains("://") => warnings.push(format!(
            "The server URL `{base}` is relative; requests will not be sendable until it is completed."
        )),
        _ => {}
    }
    let mut groups: Vec<(String, Vec<ApiRequest>)> = Vec::new();
    let mut path_params_seen: Vec<String> = Vec::new();

    for (path, item) in paths {
        let item = resolve(root, item);
        let shared = item
            .get("parameters")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        for method in Method::ALL {
            let key = method.as_str().to_ascii_lowercase();
            let Some(operation) = item.get(&key) else {
                continue;
            };
            if !operation.is_object() {
                continue;
            }
            let operation = resolve(root, operation);
            let mut request = ApiRequest::new("", method, "");
            request.name = operation
                .get("summary")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_string)
                .or_else(|| {
                    operation
                        .get("operationId")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_else(|| format!("{} {}", method.as_str(), path));
            request.description = operation
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            request.url = match &base {
                Some(base) => format!("{base}{path}"),
                None => path.clone(),
            };

            let mut params: Vec<Value> = shared.clone();
            if let Some(own) = operation.get("parameters").and_then(Value::as_array) {
                params.extend(own.iter().cloned());
            }
            for param in params {
                let param = resolve(root, &param);
                let location = param.get("in").and_then(Value::as_str).unwrap_or("");
                let name = param.get("name").and_then(Value::as_str).unwrap_or("");
                if name.is_empty() {
                    continue;
                }
                let value = param_value(root, param);
                let desc = param
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                match location {
                    "query" => request.params.push(KV {
                        enabled: true,
                        key: name.to_string(),
                        value,
                        desc,
                        secret: false,
                    }),
                    "header" => request.headers.push(KV {
                        enabled: true,
                        key: name.to_string(),
                        value,
                        desc,
                        secret: false,
                    }),
                    "path" => {
                        if !path_params_seen.contains(&name.to_string()) {
                            path_params_seen.push(name.to_string());
                        }
                    }
                    "cookie" => {
                        let warning = format!("Cookie parameter `{name}` was skipped.");
                        if !warnings.contains(&warning) {
                            warnings.push(warning);
                        }
                    }
                    _ => {}
                }
            }

            apply_openapi_body(root, operation, &mut request, &mut warnings);
            apply_openapi_auth(root, operation, &mut request, &mut warnings);
            apply_openapi_example(operation, &mut request);

            let tag = operation
                .get("tags")
                .and_then(Value::as_array)
                .and_then(|tags| tags.first())
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let group = if tag.is_empty() {
                title.clone()
            } else {
                tag
            };
            match groups.iter_mut().find(|(name, _)| *name == group) {
                Some((_, requests)) => requests.push(request),
                None => groups.push((group, vec![request])),
            }
        }
    }

    if groups.is_empty() {
        return Err("No operations were found in the spec.".to_string());
    }
    if !path_params_seen.is_empty() {
        warnings.push(format!(
            "Path parameters ({}) are kept as placeholders: replace them before sending.",
            path_params_seen.join(", ")
        ));
    }

    let folders = groups
        .into_iter()
        .map(|(name, requests)| Folder {
            id: crate::model::next_id(),
            name,
            open: true,
            auth: Auth::None,
            requests,
        })
        .collect();

    Ok(ImportPlan {
        format: format!("OpenAPI {version}"),
        title,
        folders,
        variables: Vec::new(),
        warnings,
    })
}

fn server_base(root: &Value) -> Option<String> {
    let server = root.get("servers")?.as_array()?.first()?;
    let mut url = server.get("url")?.as_str()?.trim().to_string();
    if let Some(vars) = server.get("variables").and_then(Value::as_object) {
        for (name, value) in vars {
            if let Some(default) = value.get("default").and_then(Value::as_str) {
                url = url.replace(&format!("{{{name}}}"), default);
            }
        }
    }
    while url.ends_with('/') {
        url.pop();
    }
    (!url.is_empty()).then_some(url)
}

fn param_value(root: &Value, param: &Value) -> String {
    if let Some(example) = param.get("example") {
        return scalar_text(example);
    }
    if let Some(schema) = param.get("schema") {
        let schema = resolve(root, schema);
        if let Some(example) = schema.get("example") {
            return scalar_text(example);
        }
        if let Some(default) = schema.get("default") {
            return scalar_text(default);
        }
        return scalar_text(&schema_sample(root, schema, 0, &mut Vec::new()));
    }
    String::new()
}

fn scalar_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn apply_openapi_body(
    root: &Value,
    operation: &Value,
    request: &mut ApiRequest,
    warnings: &mut Vec<String>,
) {
    let Some(body) = operation.get("requestBody") else {
        return;
    };
    let body = resolve(root, body);
    let Some(content) = body.get("content").and_then(Value::as_object) else {
        return;
    };
    let pick = [
        "application/json",
        "application/x-www-form-urlencoded",
        "multipart/form-data",
        "application/octet-stream",
        "text/plain",
    ]
        .iter()
        .find(|mime| content.contains_key(**mime))
        .copied()
        .or_else(|| content.keys().next().map(String::as_str));
    let Some(mime) = pick else {
        return;
    };
    let media = &content[mime];
    let example = media_example(root, media);
    match mime {
        "application/json" => {
            request.body_mode = BodyMode::Json;
            if let Some(example) = example {
                request.body = serde_json::to_string_pretty(&example).unwrap_or_default();
            } else if let Some(schema) = media.get("schema") {
                let sample = schema_sample(root, resolve(root, schema), 0, &mut Vec::new());
                request.body = serde_json::to_string_pretty(&sample).unwrap_or_default();
            }
        }
        "application/x-www-form-urlencoded" => {
            request.body_mode = BodyMode::Form;
            let object = example.and_then(|value| value.as_object().cloned()).or_else(|| {
                media
                    .get("schema")
                    .map(|schema| schema_sample(root, resolve(root, schema), 0, &mut Vec::new()))
                    .and_then(|sample| sample.as_object().cloned())
            });
            if let Some(object) = object {
                request.form = object
                    .into_iter()
                    .map(|(key, value)| KV::pair(&key, &scalar_text(&value)))
                    .collect();
            }
        }
        "text/plain" => {
            request.body_mode = BodyMode::Text;
            request.body = example.map(|value| scalar_text(&value)).unwrap_or_default();
        }
        "multipart/form-data" => {
            request.body_mode = BodyMode::Multipart;
            let example_object = example.as_ref().and_then(Value::as_object);
            let properties = media
                .get("schema")
                .map(|schema| resolve(root, schema))
                .and_then(|schema| schema.get("properties"))
                .and_then(Value::as_object);
            if let Some(properties) = properties {
                for (name, property) in properties {
                    let property = resolve(root, property);
                    let format = property
                        .get("format")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    if format == "binary" {
                        request.multipart.push(FormField::file(name, ""));
                        warnings.push(format!("Choose a file for the `{name}` field."));
                    } else {
                        let value = example_object
                            .and_then(|object| object.get(name))
                            .map(scalar_text)
                            .unwrap_or_else(|| {
                                scalar_text(&schema_sample(
                                    root,
                                    property,
                                    0,
                                    &mut Vec::new(),
                                ))
                            });
                        request.multipart.push(FormField::text(name, &value));
                    }
                }
            } else if let Some(object) = example_object {
                for (name, value) in object {
                    request
                        .multipart
                        .push(FormField::text(name, &scalar_text(value)));
                }
            }
        }
        "application/octet-stream" => {
            request.body_mode = BodyMode::Binary;
            warnings.push("Choose a file for the binary body.".to_string());
        }
        other => {
            warnings.push(format!(
                "Body type `{other}` in `{}` is not supported.",
                request.name
            ));
        }
    }
}

fn media_example(root: &Value, media: &Value) -> Option<Value> {
    if let Some(example) = media.get("example") {
        return Some(example.clone());
    }
    if let Some(examples) = media.get("examples").and_then(Value::as_object) {
        if let Some(first) = examples.values().next() {
            let first = resolve(root, first);
            if let Some(value) = first.get("value") {
                return Some(value.clone());
            }
        }
    }
    None
}

fn apply_openapi_auth(
    root: &Value,
    operation: &Value,
    request: &mut ApiRequest,
    warnings: &mut Vec<String>,
) {
    let security = operation
        .get("security")
        .or_else(|| root.get("security"))
        .and_then(Value::as_array);
    let Some(security) = security else {
        return;
    };
    let Some(requirement) = security.first().and_then(Value::as_object) else {
        return;
    };
    let Some((scheme_name, _)) = requirement.iter().next() else {
        return;
    };
    let Some(schemes) = root
        .get("components")
        .and_then(|components| components.get("securitySchemes"))
        .and_then(Value::as_object)
    else {
        warnings.push(format!("Security scheme `{scheme_name}` was not found."));
        return;
    };
    let Some(scheme) = schemes.get(scheme_name) else {
        warnings.push(format!("Security scheme `{scheme_name}` was not found."));
        return;
    };
    let scheme = resolve(root, scheme);
    let kind = scheme.get("type").and_then(Value::as_str).unwrap_or("");
    match kind {
        "http" => match scheme.get("scheme").and_then(Value::as_str) {
            Some("bearer") => request.auth = Auth::Bearer { token: "{{token}}".into() },
            Some("basic") => request.auth = Auth::Basic {
                user: "{{username}}".into(),
                pass: "{{password}}".into(),
            },
            other => warnings.push(format!(
                "HTTP auth scheme `{}` is not supported (request `{}`).",
                other.unwrap_or("unknown"),
                request.name
            )),
        },
        "apiKey" => {
            let name = scheme.get("name").and_then(Value::as_str).unwrap_or("X-API-Key");
            match scheme.get("in").and_then(Value::as_str) {
                Some("header") => {
                    request.auth = Auth::ApiKey {
                        header: name.to_string(),
                        value: "{{apiKey}}".into(),
                    }
                }
                Some(location) => warnings.push(format!(
                    "API key in `{location}` is not supported (request `{}`).",
                    request.name
                )),
                None => {}
            }
        }
        "oauth2" | "openIdConnect" => {
            request.auth = Auth::Bearer {
                token: "{{accessToken}}".into(),
            }
        }
        "" => {}
        other => warnings.push(format!(
            "Security type `{other}` is not supported (request `{}`).",
            request.name
        )),
    }
}

fn apply_openapi_example(operation: &Value, request: &mut ApiRequest) {
    let Some(responses) = operation.get("responses").and_then(Value::as_object) else {
        return;
    };
    let pick = ["200", "201", "202", "203", "204"]
        .iter()
        .find(|code| responses.contains_key(**code))
        .copied();
    let Some(code) = pick else {
        return;
    };
    let response = &responses[code];
    let Some(content) = response.get("content").and_then(Value::as_object) else {
        return;
    };
    let Some(media) = content.get("application/json") else {
        return;
    };
    let Some(example) = media.get("example") else {
        return;
    };
    request.example_status = code.parse::<u16>().unwrap_or(200);
    request.example_body = serde_json::to_string_pretty(example).unwrap_or_default();
}

fn resolve<'a>(root: &'a Value, node: &'a Value) -> &'a Value {
    let mut current = node;
    for _ in 0..8 {
        let Some(reference) = current.get("$ref").and_then(Value::as_str) else {
            break;
        };
        let Some(next) = pointer(root, reference) else {
            break;
        };
        current = next;
    }
    current
}

fn pointer<'a>(root: &'a Value, reference: &str) -> Option<&'a Value> {
    let path = reference.strip_prefix("#/")?;
    let mut current = root;
    for part in path.split('/') {
        let part = part.replace("~1", "/").replace("~0", "~");
        current = current.get(&part)?;
    }
    Some(current)
}

fn schema_sample(
    root: &Value,
    schema: &Value,
    depth: usize,
    seen: &mut Vec<String>,
) -> Value {
    use serde_json::{Map, json};
    if depth > 6 {
        return Value::Null;
    }
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        if seen.iter().any(|entry| entry == reference) {
            return Value::Null;
        }
        seen.push(reference.to_string());
        let resolved = pointer(root, reference).unwrap_or(schema);
        let sample = schema_sample(root, resolved, depth + 1, seen);
        seen.pop();
        return sample;
    }
    if let Some(example) = schema.get("example") {
        return example.clone();
    }
    if let Some(default) = schema.get("default") {
        return default.clone();
    }
    if let Some(first) = schema
        .get("enum")
        .and_then(Value::as_array)
        .and_then(|values| values.first())
    {
        return first.clone();
    }
    if let Some(parts) = schema.get("allOf").and_then(Value::as_array) {
        let mut merged = Map::new();
        for part in parts {
            let part = resolve(root, part);
            if let Some(sample) = schema_sample(root, part, depth + 1, seen).as_object() {
                for (key, value) in sample {
                    merged.insert(key.clone(), value.clone());
                }
            }
        }
        return Value::Object(merged);
    }
    for key in ["oneOf", "anyOf"] {
        if let Some(first) = schema
            .get(key)
            .and_then(Value::as_array)
            .and_then(|values| values.first())
        {
            return schema_sample(root, resolve(root, first), depth + 1, seen);
        }
    }
    let kind = schema
        .get("type")
        .and_then(Value::as_str)
        .or_else(|| {
            if schema.get("properties").is_some() {
                Some("object")
            } else if schema.get("items").is_some() {
                Some("array")
            } else {
                None
            }
        })
        .unwrap_or("string");
    match kind {
        "object" => {
            let mut map = Map::new();
            if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
                for (name, property) in properties {
                    let property = resolve(root, property);
                    if property
                        .get("readOnly")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                    {
                        continue;
                    }
                    map.insert(
                        name.clone(),
                        schema_sample(root, property, depth + 1, seen),
                    );
                }
            }
            Value::Object(map)
        }
        "array" => {
            let item = schema.get("items").map(|items| resolve(root, items));
            let sample = item
                .map(|item| schema_sample(root, item, depth + 1, seen))
                .unwrap_or_else(|| json!("string"));
            Value::Array(vec![sample])
        }
        "integer" | "number" => json!(0),
        "boolean" => json!(false),
        "null" => Value::Null,
        _ => {
            let format = schema.get("format").and_then(Value::as_str).unwrap_or("");
            json!(match format {
                "date-time" => "2024-01-01T00:00:00Z",
                "date" => "2024-01-01",
                "email" => "user@example.com",
                "uuid" => "00000000-0000-0000-0000-000000000000",
                "uri" | "url" => "https://example.com",
                "hostname" => "example.com",
                "ipv4" => "127.0.0.1",
                _ => "string",
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Postman
// ---------------------------------------------------------------------------

fn postman(root: &Value) -> Result<ImportPlan, String> {
    let info = root.get("info").cloned().unwrap_or(Value::Null);
    let title = info
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Imported collection")
        .to_string();
    let schema = info
        .get("schema")
        .and_then(Value::as_str)
        .unwrap_or("v2.1");
    let version = if schema.contains("v2.0") { "v2.0" } else { "v2.1" };

    let mut variables: Vec<KV> = Vec::new();
    if let Some(vars) = root.get("variable").and_then(Value::as_array) {
        for var in vars {
            let key = var.get("key").and_then(Value::as_str).unwrap_or("");
            if key.is_empty() {
                continue;
            }
            variables.push(KV::pair(
                key,
                &var
                    .get("value")
                    .map(scalar_text)
                    .unwrap_or_default(),
            ));
        }
    }

    let default_auth = root.get("auth");
    let mut warnings: Vec<String> = Vec::new();
    let mut groups: Vec<(String, Vec<ApiRequest>)> = Vec::new();
    if let Some(items) = root.get("item").and_then(Value::as_array) {
        walk_postman_items(
            items,
            "",
            &title,
            default_auth,
            &mut groups,
            &mut warnings,
        );
    }
    if groups.is_empty() {
        return Err("The collection has no requests.".to_string());
    }

    let values: std::collections::HashMap<String, String> = variables
        .iter()
        .filter(|variable| !variable.value.is_empty())
        .map(|variable| (variable.key.clone(), variable.value.clone()))
        .collect();
    let mut folders: Vec<Folder> = groups
        .into_iter()
        .map(|(name, requests)| Folder {
            id: crate::model::next_id(),
            name,
            open: true,
            auth: Auth::None,
            requests,
        })
        .collect();
    // Collection variables such as `baseUrl` are baked into request URLs so
    // imported requests work without touching the active environment.
    if !values.is_empty() {
        for folder in &mut folders {
            for request in &mut folder.requests {
                request.url = substitute(&request.url, &values);
            }
        }
    }

    Ok(ImportPlan {
        format: format!("Postman collection {version}"),
        title,
        folders,
        variables,
        warnings,
    })
}

fn walk_postman_items(
    items: &[Value],
    prefix: &str,
    root_name: &str,
    default_auth: Option<&Value>,
    groups: &mut Vec<(String, Vec<ApiRequest>)>,
    warnings: &mut Vec<String>,
) {
    for item in items {
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Untitled")
            .trim()
            .to_string();
        if let Some(children) = item.get("item").and_then(Value::as_array) {
            let folder = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix} / {name}")
            };
            walk_postman_items(children, &folder, root_name, default_auth, groups, warnings);
            continue;
        }
        let Some(request) = item.get("request") else {
            continue;
        };
        let group = if prefix.is_empty() {
            root_name.to_string()
        } else {
            prefix.to_string()
        };
        if let Some(request) = build_postman_request(&name, request, default_auth, warnings) {
            match groups.iter_mut().find(|(key, _)| *key == group) {
                Some((_, requests)) => requests.push(request),
                None => groups.push((group, vec![request])),
            }
        }
        if item.get("event").is_some() {
            let warning = format!("Scripts in `{name}` were not imported.");
            if !warnings.contains(&warning) {
                warnings.push(warning);
            }
        }
    }
}

fn build_postman_request(
    name: &str,
    request: &Value,
    default_auth: Option<&Value>,
    warnings: &mut Vec<String>,
) -> Option<ApiRequest> {
    if let Value::String(url) = request {
        return Some(ApiRequest::new(name, Method::Get, url));
    }
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .and_then(parse_method)
        .unwrap_or(Method::Get);
    let (url, params) = postman_url(request.get("url"), warnings);
    let mut api = ApiRequest::new(name, method, &url);
    api.params = params;
    api.description = request
        .get("description")
        .map(|description| match description {
            Value::String(text) => text.clone(),
            other => other
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        })
        .unwrap_or_default();

    if let Some(headers) = request.get("header").and_then(Value::as_array) {
        for header in headers {
            if header.get("disabled").and_then(Value::as_bool).unwrap_or(false) {
                continue;
            }
            let key = header.get("key").and_then(Value::as_str).unwrap_or("");
            if key.is_empty() {
                continue;
            }
            api.headers.push(KV::pair(
                key,
                &header
                    .get("value")
                    .map(scalar_text)
                    .unwrap_or_default(),
            ));
        }
    }

    if let Some(body) = request.get("body") {
        apply_postman_body(body, &mut api, warnings);
    }

    let auth = match request.get("auth") {
        None | Some(Value::Null) => default_auth,
        other => other,
    };
    if let Some(auth) = auth {
        apply_postman_auth(auth, &mut api, warnings);
    }
    Some(api)
}

fn postman_url(url: Option<&Value>, warnings: &mut Vec<String>) -> (String, Vec<KV>) {
    match url {
        Some(Value::String(raw)) => {
            let (path, params) = split_query(raw);
            (path, params)
        }
        Some(value) => {
            let raw = value
                .get("raw")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| {
                    let host = value
                        .get("host")
                        .and_then(Value::as_array)
                        .map(|parts| {
                            parts
                                .iter()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join(".")
                        })
                        .unwrap_or_default();
                    let path = value
                        .get("path")
                        .and_then(Value::as_array)
                        .map(|parts| {
                            parts
                                .iter()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join("/")
                        })
                        .unwrap_or_default();
                    let protocol = value
                        .get("protocol")
                        .and_then(Value::as_str)
                        .unwrap_or("https");
                    format!("{protocol}://{host}/{path}")
                });
            let mut params = Vec::new();
            if let Some(query) = value.get("query").and_then(Value::as_array) {
                for entry in query {
                    if entry
                        .get("disabled")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                    {
                        continue;
                    }
                    let key = entry.get("key").and_then(Value::as_str).unwrap_or("");
                    if key.is_empty() {
                        continue;
                    }
                    params.push(KV::pair(
                        key,
                        &entry
                            .get("value")
                            .map(scalar_text)
                            .unwrap_or_default(),
                    ));
                }
            }
            let (path, query_params) = split_query(&raw);
            if params.is_empty() {
                params = query_params;
            }
            if value.get("variable").is_some() {
                warnings.push(
                    "Postman path variables were kept as {{placeholders}}.".to_string(),
                );
            }
            (path, params)
        }
        None => (String::new(), Vec::new()),
    }
}

fn substitute(text: &str, values: &std::collections::HashMap<String, String>) -> String {
    let mut out = text.to_string();
    for (key, value) in values {
        out = out.replace(&format!("{{{{{key}}}}}"), value);
    }
    out
}

fn split_query(raw: &str) -> (String, Vec<KV>) {
    let (path, query) = match raw.split_once('?') {
        Some((path, query)) => (path.to_string(), query),
        None => (raw.to_string(), ""),
    };
    let params = query
        .split('&')
        .filter_map(|part| {
            if part.is_empty() {
                return None;
            }
            let (key, value) = part.split_once('=').unwrap_or((part, ""));
            Some(KV::pair(&decode_query(key), &decode_query(value)))
        })
        .collect();
    (path, params)
}

fn decode_query(value: &str) -> String {
    let mut out = String::new();
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                match hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                    Some(byte) => {
                        out.push(byte as char);
                        i += 3;
                    }
                    None => {
                        out.push(bytes[i] as char);
                        i += 1;
                    }
                }
            }
            byte => {
                out.push(byte as char);
                i += 1;
            }
        }
    }
    out
}

fn apply_postman_body(body: &Value, api: &mut ApiRequest, warnings: &mut Vec<String>) {
    let mode = body.get("mode").and_then(Value::as_str).unwrap_or("");
    match mode {
        "raw" => {
            let raw = body.get("raw").and_then(Value::as_str).unwrap_or("");
            let language = body
                .get("options")
                .and_then(|options| options.get("raw"))
                .and_then(|raw| raw.get("language"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let content_type = api
                .headers
                .iter()
                .find(|header| header.key.eq_ignore_ascii_case("content-type"))
                .map(|header| header.value.to_ascii_lowercase())
                .unwrap_or_default();
            if language == "json"
                || content_type.contains("json")
                || serde_json::from_str::<Value>(raw).is_ok()
            {
                api.body_mode = BodyMode::Json;
                api.body = match serde_json::from_str::<Value>(raw) {
                    Ok(value) => serde_json::to_string_pretty(&value).unwrap_or(raw.to_string()),
                    Err(_) => raw.to_string(),
                };
            } else {
                api.body_mode = BodyMode::Text;
                api.body = raw.to_string();
            }
        }
        "urlencoded" => {
            api.body_mode = BodyMode::Form;
            if let Some(fields) = body.get("urlencoded").and_then(Value::as_array) {
                for field in fields {
                    if field.get("disabled").and_then(Value::as_bool).unwrap_or(false) {
                        continue;
                    }
                    let key = field.get("key").and_then(Value::as_str).unwrap_or("");
                    if key.is_empty() {
                        continue;
                    }
                    api.form.push(KV::pair(
                        key,
                        &field
                            .get("value")
                            .map(scalar_text)
                            .unwrap_or_default(),
                    ));
                }
            }
        }
        "formdata" => {
            api.body_mode = BodyMode::Multipart;
            if let Some(fields) = body.get("formdata").and_then(Value::as_array) {
                for field in fields {
                    if field
                        .get("disabled")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                    {
                        continue;
                    }
                    let key = field.get("key").and_then(Value::as_str).unwrap_or("");
                    if key.is_empty() {
                        continue;
                    }
                    if field.get("type").and_then(Value::as_str) == Some("file") {
                        let src = field
                            .get("src")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        if src.is_empty() {
                            warnings.push(format!(
                                "Choose a file for the `{key}` field in Postman."
                            ));
                        }
                        api.multipart.push(FormField::file(key, src));
                    } else {
                        api.multipart.push(FormField::text(
                            key,
                            &field
                                .get("value")
                                .map(scalar_text)
                                .unwrap_or_default(),
                        ));
                    }
                }
            }
        }
        "graphql" => {
            if let Some(graphql) = body.get("graphql") {
                let mut object = serde_json::Map::new();
                if let Some(query) = graphql.get("query") {
                    object.insert("query".into(), query.clone());
                }
                if let Some(variables) = graphql.get("variables") {
                    object.insert(
                        "variables".into(),
                        serde_json::from_str(variables.as_str().unwrap_or("{}"))
                            .unwrap_or(Value::Null),
                    );
                }
                api.body_mode = BodyMode::Json;
                api.body = serde_json::to_string_pretty(&Value::Object(object))
                    .unwrap_or_default();
            }
        }
        "file" => {
            api.body_mode = BodyMode::Binary;
            match body
                .get("file")
                .and_then(|file| file.get("src"))
                .and_then(Value::as_str)
            {
                Some(src) if !src.is_empty() => api.binary_path = src.to_string(),
                _ => warnings.push(
                    "The Postman collection sends a binary file; choose it before sending."
                        .to_string(),
                ),
            }
        }
        "" => {}
        other => warnings.push(format!("Body mode `{other}` is not supported.")),
    }
}

fn apply_postman_auth(auth: &Value, api: &mut ApiRequest, warnings: &mut Vec<String>) {
    let kind = auth.get("type").and_then(Value::as_str).unwrap_or("");
    let param = |name: &str| -> String {
        auth.get(kind)
            .and_then(Value::as_array)
            .and_then(|entries| {
                entries
                    .iter()
                    .find(|entry| entry.get("key").and_then(Value::as_str) == Some(name))
            })
            .and_then(|entry| entry.get("value"))
            .map(scalar_text)
            .unwrap_or_default()
    };
    match kind {
        "bearer" => {
            api.auth = Auth::Bearer {
                token: param("token"),
            }
        }
        "basic" => {
            api.auth = Auth::Basic {
                user: param("username"),
                pass: param("password"),
            }
        }
        "apikey" => {
            let name = param("key");
            let value = param("value");
            match param("in").as_str() {
                "query" => {
                    api.params.push(KV::pair(
                        if name.is_empty() { "api_key" } else { &name },
                        &value,
                    ));
                    warnings.push("An API key in the query string was added as a parameter.".into());
                }
                _ => {
                    api.auth = Auth::ApiKey {
                        header: if name.is_empty() { "X-API-Key".into() } else { name },
                        value,
                    }
                }
            }
        }
        "oauth2" | "oauth1" => {
            api.auth = Auth::Bearer {
                token: "{{accessToken}}".into(),
            };
            warnings.push(format!(
                "OAuth credentials in `{}` must be configured manually.",
                api.name
            ));
        }
        "noauth" | "" => {}
        other => warnings.push(format!("Auth type `{other}` is not supported.")),
    }
}

fn parse_method(raw: &str) -> Option<Method> {
    let upper = raw.trim().to_ascii_uppercase();
    Method::ALL
        .iter()
        .copied()
        .find(|method| method.as_str() == upper)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::FieldKind;

    #[test]
    fn imports_openapi_json_into_tagged_folders() {
        let spec = r##"{
            "openapi": "3.0.3",
            "info": { "title": "Pet Store" },
            "servers": [{ "url": "https://api.example.com/v1" }],
            "security": [{ "bearer": [] }],
            "components": {
                "securitySchemes": {
                    "bearer": { "type": "http", "scheme": "bearer" }
                },
                "schemas": {
                    "Pet": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string", "example": "Rex" },
                            "age": { "type": "integer" }
                        }
                    }
                }
            },
            "paths": {
                "/pets": {
                    "get": {
                        "tags": ["Pets"],
                        "summary": "List pets",
                        "parameters": [
                            { "in": "query", "name": "limit", "schema": { "type": "integer", "default": 20 } }
                        ],
                        "responses": {
                            "200": {
                                "content": {
                                    "application/json": { "example": [{ "name": "Rex" }] }
                                }
                            }
                        }
                    },
                    "post": {
                        "tags": ["Pets"],
                        "summary": "Create pet",
                        "requestBody": {
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/Pet" }
                                }
                            }
                        }
                    }
                }
            }
        }"##;
        let plan = detect_and_parse(spec).expect("import");
        assert_eq!(plan.format, "OpenAPI 3.0.3");
        assert_eq!(plan.request_count(), 2);
        assert_eq!(plan.folders.len(), 1);
        assert_eq!(plan.folders[0].name, "Pets");
        let get = &plan.folders[0].requests[0];
        assert_eq!(get.url, "https://api.example.com/v1/pets");
        assert_eq!(get.params[0].key, "limit");
        assert_eq!(get.params[0].value, "20");
        assert!(matches!(get.auth, Auth::Bearer { .. }));
        assert_eq!(get.example_status, 200);
        assert!(get.example_body.contains("Rex"));
        let post = &plan.folders[0].requests[1];
        assert_eq!(post.body_mode, BodyMode::Json);
        assert!(post.body.contains("\"name\": \"Rex\""));
    }

    #[test]
    fn imports_openapi_multipart_bodies() {
        let spec = r#"{
            "openapi": "3.0.3",
            "info": { "title": "Uploads" },
            "servers": [{ "url": "https://api.example.com" }],
            "paths": {
                "/avatars": {
                    "post": {
                        "summary": "Upload avatar",
                        "requestBody": {
                            "content": {
                                "multipart/form-data": {
                                    "schema": {
                                        "type": "object",
                                        "properties": {
                                            "title": { "type": "string", "example": "Portrait" },
                                            "avatar": { "type": "string", "format": "binary" }
                                        }
                                    }
                                }
                            }
                        },
                        "responses": { "200": { "description": "ok" } }
                    }
                }
            }
        }"#;
        let plan = detect_and_parse(spec).expect("import");
        let request = &plan.folders[0].requests[0];
        assert_eq!(request.body_mode, BodyMode::Multipart);
        assert_eq!(request.multipart.len(), 2);
        assert_eq!(request.multipart[0].value, "Portrait");
        assert_eq!(request.multipart[1].kind, FieldKind::File);
        assert!(plan.warnings.iter().any(|warning| warning.contains("avatar")));
    }

    #[test]
    fn imports_openapi_yaml() {
        let spec = r#"
openapi: 3.0.1
info:
  title: Tiny API
servers:
  - url: https://tiny.example.com
paths:
  /ping:
    get:
      summary: Ping
      parameters:
        - in: header
          name: X-Trace
          example: abc
      responses:
        "200":
          description: ok
"#;
        let plan = detect_and_parse(spec).expect("import yaml");
        assert_eq!(plan.request_count(), 1);
        let request = &plan.folders[0].requests[0];
        assert_eq!(request.url, "https://tiny.example.com/ping");
        assert_eq!(request.headers[0].key, "X-Trace");
        assert_eq!(request.headers[0].value, "abc");
    }

    #[test]
    fn swagger_two_is_reported() {
        let err = detect_and_parse(r#"{"swagger":"2.0","paths":{}}"#).unwrap_err();
        assert!(err.contains("Swagger 2.0"));
    }

    #[test]
    fn imports_a_postman_collection() {
        let collection = r#"{
            "info": {
                "name": "Shop",
                "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"
            },
            "variable": [{ "key": "baseUrl", "value": "https://shop.example.com" }],
            "auth": {
                "type": "bearer",
                "bearer": [{ "key": "token", "value": "{{token}}" }]
            },
            "item": [
                {
                    "name": "Orders",
                    "item": [
                        {
                            "name": "List orders",
                            "request": {
                                "method": "GET",
                                "url": {
                                    "raw": "{{baseUrl}}/orders?page=2&per_page=10",
                                    "query": [
                                        { "key": "page", "value": "2" },
                                        { "key": "per_page", "value": "10" }
                                    ]
                                },
                                "header": [{ "key": "Accept", "value": "application/json" }]
                            }
                        },
                        {
                            "name": "Create order",
                            "request": {
                                "method": "POST",
                                "url": "{{baseUrl}}/orders",
                                "header": [{ "key": "Content-Type", "value": "application/json" }],
                                "body": {
                                    "mode": "raw",
                                    "raw": "{\"item\":\"book\",\"qty\":2}",
                                    "options": { "raw": { "language": "json" } }
                                }
                            }
                        }
                    ]
                }
            ]
        }"#;
        let plan = detect_and_parse(collection).expect("import");
        assert_eq!(plan.request_count(), 2);
        assert_eq!(plan.variables.len(), 1);
        assert_eq!(plan.variables[0].key, "baseUrl");
        assert_eq!(plan.folders[0].name, "Orders");
        let list = &plan.folders[0].requests[0];
        assert_eq!(list.url, "https://shop.example.com/orders");
        assert_eq!(list.params.len(), 2);
        assert!(matches!(list.auth, Auth::Bearer { ref token } if token == "{{token}}"));
        let create = &plan.folders[0].requests[1];
        assert_eq!(create.body_mode, BodyMode::Json);
        assert!(create.body.contains("book"));
    }

    #[test]
    fn unknown_documents_are_rejected() {
        assert!(detect_and_parse("<html></html>").is_err());
        assert!(detect_and_parse("").is_err());
    }
}
