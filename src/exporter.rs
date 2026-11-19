//! Export the workspace as an OpenAPI 3 document or a Postman collection.

use std::collections::HashMap;

use serde_json::{Map, Value, json};

use crate::model::{
    ApiRequest, Auth, BodyMode, Environment, FieldKind, Folder, KV, Method, ProjectData,
    expand_with,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    OpenApi,
    Postman,
}

impl Format {
    pub const ALL: [Format; 2] = [Format::OpenApi, Format::Postman];

    pub fn label(self) -> &'static str {
        match self {
            Format::OpenApi => "OpenAPI 3.0.3",
            Format::Postman => "Postman v2.1",
        }
    }

    pub fn file_name(self) -> &'static str {
        match self {
            Format::OpenApi => "lucapi-openapi.json",
            Format::Postman => "lucapi.postman_collection.json",
        }
    }
}

#[derive(Debug)]
pub struct ExportDoc {
    pub text: String,
    pub warnings: Vec<String>,
}

pub fn export(data: &ProjectData, format: Format) -> ExportDoc {
    match format {
        Format::OpenApi => openapi(data),
        Format::Postman => postman(data),
    }
}

fn active_environment(data: &ProjectData) -> Option<&Environment> {
    let id = data.active_env?;
    data.envs.iter().find(|env| env.id == id)
}

fn env_map(env: Option<&Environment>) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Some(env) = env {
        for var in env.vars.iter().filter(|var| var.enabled) {
            if !var.key.trim().is_empty() {
                map.insert(var.key.trim().to_string(), var.value.clone());
            }
        }
    }
    map
}

fn all_requests(data: &ProjectData) -> Vec<(&Folder, &ApiRequest)> {
    data.folders
        .iter()
        .flat_map(|folder| {
            folder
                .requests
                .iter()
                .map(move |request| (folder, request))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// OpenAPI
// ---------------------------------------------------------------------------

fn openapi(data: &ProjectData) -> ExportDoc {
    let env = active_environment(data);
    let vars = env_map(env);
    let base = env
        .and_then(|env| {
            env.vars
                .iter()
                .find(|var| var.key == "baseUrl" && var.enabled)
        })
        .map(|var| expand_with(&var.value, &vars))
        .filter(|value| value.contains("://"))
        .map(|mut value| {
            while value.ends_with('/') {
                value.pop();
            }
            value
        });

    let requests = all_requests(data);
    let mut warnings: Vec<String> = Vec::new();
    if requests.is_empty() {
        warnings.push("The workspace has no requests to export.".to_string());
    }

    // Work out a server URL: the environment base URL when it is absolute,
    // otherwise the origin of the first absolute request URL.
    let server = base.clone().or_else(|| {
        requests.iter().find_map(|(_, request)| {
            let resolved = expand_with(&request.url, &vars);
            reqwest::Url::parse(&resolved)
                .ok()
                .map(|url| url.origin().ascii_serialization())
        })
    });
    let server = server.unwrap_or_else(|| {
        warnings.push(
            "No absolute base URL was found; `https://example.com` is used as the server."
                .to_string(),
        );
        "https://example.com".to_string()
    });
    let server_trim = server.trim_end_matches('/');

    let mut paths: Map<String, Value> = Map::new();
    let mut tags: Vec<String> = Vec::new();
    let mut schemes: Map<String, Value> = Map::new();
    let mut api_key_schemes: HashMap<String, String> = HashMap::new();
    let mut foreign_origins: Vec<String> = Vec::new();

    for (folder, request) in &requests {
        let resolved_auth = request.auth.resolve(&folder.auth);
        if !tags.contains(&folder.name) {
            tags.push(folder.name.clone());
        }
        let resolved = resolve_url(&request.url, &vars);
        let (mut path, mut query) = split_url(&resolved);
        if let Some(rest) = resolved.strip_prefix(server_trim) {
            let rest = rest.split('?').next().unwrap_or("");
            let rest = rest.split('#').next().unwrap_or("");
            path = if rest.is_empty() {
                "/".to_string()
            } else {
                rest.to_string()
            };
        }
        if !resolved.starts_with(server_trim) {
            if let Ok(url) = reqwest::Url::parse(&resolved) {
                let origin = url.origin().ascii_serialization();
                if !foreign_origins.contains(&origin) {
                    foreign_origins.push(origin);
                }
            }
        }
        let path = normalize_path(&path);
        let path_params = path_params(&path);
        for kv in request
            .params
            .iter()
            .filter(|kv| kv.enabled && !kv.key.trim().is_empty())
        {
            if !query.iter().any(|existing| existing.key == kv.key) {
                query.push(KV::pair(&kv.key, &kv.value));
            }
        }

        let mut parameters: Vec<Value> = Vec::new();
        for kv in &query {
            let mut parameter = Map::new();
            parameter.insert("name".into(), json!(kv.key));
            parameter.insert("in".into(), json!("query"));
            parameter.insert("required".into(), json!(false));
            parameter.insert("schema".into(), json!({ "type": "string" }));
            if !kv.value.is_empty() {
                parameter.insert("example".into(), json!(kv.value));
            }
            parameters.push(Value::Object(parameter));
        }
        for kv in request
            .headers
            .iter()
            .filter(|kv| kv.enabled && !kv.key.trim().is_empty())
        {
            if kv.key.eq_ignore_ascii_case("content-type")
                || kv.key.eq_ignore_ascii_case("authorization")
            {
                continue;
            }
            let mut parameter = Map::new();
            parameter.insert("name".into(), json!(kv.key));
            parameter.insert("in".into(), json!("header"));
            parameter.insert("required".into(), json!(false));
            parameter.insert("schema".into(), json!({ "type": "string" }));
            if !kv.value.is_empty() {
                parameter.insert("example".into(), json!(kv.value));
            }
            parameters.push(Value::Object(parameter));
        }
        for name in &path_params {
            let mut parameter = Map::new();
            parameter.insert("name".into(), json!(name));
            parameter.insert("in".into(), json!("path"));
            parameter.insert("required".into(), json!(true));
            parameter.insert("schema".into(), json!({ "type": "string" }));
            parameters.push(Value::Object(parameter));
        }

        let mut operation = Map::new();
        operation.insert("summary".into(), json!(request.name));
        if !request.description.trim().is_empty() {
            operation.insert("description".into(), json!(request.description));
        }
        operation.insert("tags".into(), json!([folder.name]));
        if !parameters.is_empty() {
            operation.insert("parameters".into(), Value::Array(parameters));
        }

        if let Some(body) = request_body(request) {
            operation.insert("requestBody".into(), body);
        }

        let mut response = Map::new();
        response.insert(
            "description".into(),
            json!(if request.example_body.trim().is_empty() {
                "Successful response"
            } else {
                "Example response"
            }),
        );
        if !request.example_body.trim().is_empty() {
            if let Ok(value) = serde_json::from_str::<Value>(&request.example_body) {
                response.insert(
                    "content".into(),
                    json!({ "application/json": { "example": value } }),
                );
            }
        }
        let status = if request.example_status == 0 {
            if request.method == Method::Post { 201 } else { 200 }
        } else {
            request.example_status
        };
        let mut responses = Map::new();
        responses.insert(status.to_string(), Value::Object(response));
        operation.insert("responses".into(), Value::Object(responses));

        if let Some(security) = security_for(&resolved_auth, &mut schemes, &mut api_key_schemes) {
            operation.insert("security".into(), security);
        }

        let method = request.method.as_str().to_ascii_lowercase();
        let entry = paths
            .entry(path.clone())
            .or_insert_with(|| Value::Object(Map::new()));
        if let Some(map) = entry.as_object_mut() {
            if map.contains_key(&method) {
                warnings.push(format!(
                    "Duplicate `{} {path}` was skipped.",
                    request.method.as_str()
                ));
                continue;
            }
            map.insert(method, Value::Object(operation));
        }
    }

    if !foreign_origins.is_empty() {
        warnings.push(format!(
            "Requests from {} other host(s) keep their path but share the declared server.",
            foreign_origins.len()
        ));
    }

    let mut components = Map::new();
    if !schemes.is_empty() {
        components.insert("securitySchemes".into(), Value::Object(schemes));
    }

    let mut root = Map::new();
    root.insert("openapi".into(), json!("3.0.3"));
    root.insert(
        "info".into(),
        json!({
            "title": data.name,
            "version": "1.0.0",
            "description": "Exported from LucAPI."
        }),
    );
    root.insert("servers".into(), json!([{ "url": server }]));
    if !tags.is_empty() {
        root.insert(
            "tags".into(),
            Value::Array(tags.iter().map(|name| json!({ "name": name })).collect()),
        );
    }
    root.insert("paths".into(), Value::Object(paths));
    if !components.is_empty() {
        root.insert("components".into(), Value::Object(components));
    }

    ExportDoc {
        text: serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_default(),
        warnings,
    }
}

fn resolve_url(url: &str, vars: &HashMap<String, String>) -> String {
    let resolved = expand_with(url.trim(), vars);
    if resolved.contains("://") {
        resolved
    } else {
        format!("http://{resolved}")
    }
}

fn split_url(url: &str) -> (String, Vec<KV>) {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        let (path, query) = match url.split_once('?') {
            Some((path, query)) => (path.to_string(), query),
            None => (url.to_string(), ""),
        };
        return (path, parse_query(query));
    };
    let path = parsed.path().to_string();
    let mut query = Vec::new();
    for (key, value) in parsed.query_pairs() {
        query.push(KV::pair(&key, &value));
    }
    (path, query)
}

fn parse_query(query: &str) -> Vec<KV> {
    query
        .split('&')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let (key, value) = part.split_once('=').unwrap_or((part, ""));
            KV::pair(key, value)
        })
        .collect()
}

fn normalize_path(path: &str) -> String {
    let mut out = String::new();
    for segment in path.split('/').filter(|segment| !segment.is_empty()) {
        out.push('/');
        if let Some(name) = segment.strip_prefix(':') {
            out.push('{');
            out.push_str(name);
            out.push('}');
        } else {
            out.push_str(segment);
        }
    }
    if out.is_empty() {
        "/".to_string()
    } else {
        out
    }
}

fn path_params(path: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = path;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            break;
        };
        let name = &after[..end];
        if !name.is_empty() && !names.iter().any(|existing| existing == name) {
            names.push(name.to_string());
        }
        rest = &after[end + 1..];
    }
    names
}

fn request_body(request: &ApiRequest) -> Option<Value> {
    match request.body_mode {
        BodyMode::None => None,
        BodyMode::Json => {
            let content = match serde_json::from_str::<Value>(&request.body) {
                Ok(value) => json!({ "application/json": { "schema": { "type": "object" }, "example": value } }),
                Err(_) => json!({
                    "application/json": { "schema": { "type": "object" } }
                }),
            };
            Some(json!({ "required": true, "content": content }))
        }
        BodyMode::Text => Some(json!({
            "required": true,
            "content": { "text/plain": { "example": request.body } }
        })),
        BodyMode::Form => {
            let mut properties = Map::new();
            let mut example = Map::new();
            for field in request
                .form
                .iter()
                .filter(|field| field.enabled && !field.key.trim().is_empty())
            {
                properties.insert(field.key.clone(), json!({ "type": "string" }));
                example.insert(field.key.clone(), json!(field.value));
            }
            Some(json!({
                "required": true,
                "content": {
                    "application/x-www-form-urlencoded": {
                        "schema": {
                            "type": "object",
                            "properties": properties
                        },
                        "example": example
                    }
                }
            }))
        }
        BodyMode::Multipart => {
            let mut properties = Map::new();
            let mut example = Map::new();
            for field in request
                .multipart
                .iter()
                .filter(|field| field.enabled && !field.key.trim().is_empty())
            {
                if field.kind == FieldKind::File {
                    properties.insert(
                        field.key.clone(),
                        json!({ "type": "string", "format": "binary" }),
                    );
                } else {
                    properties.insert(field.key.clone(), json!({ "type": "string" }));
                    example.insert(field.key.clone(), json!(field.value));
                }
            }
            let mut media = Map::new();
            media.insert(
                "schema".into(),
                json!({ "type": "object", "properties": properties }),
            );
            if !example.is_empty() {
                media.insert("example".into(), Value::Object(example));
            }
            Some(json!({
                "required": true,
                "content": { "multipart/form-data": Value::Object(media) }
            }))
        }
        BodyMode::Binary => Some(json!({
            "required": true,
            "content": {
                "application/octet-stream": {
                    "schema": { "type": "string", "format": "binary" }
                }
            }
        })),
    }
}

fn security_for(
    auth: &Auth,
    schemes: &mut Map<String, Value>,
    api_keys: &mut HashMap<String, String>,
) -> Option<Value> {
    match auth {
        Auth::None | Auth::Inherit => None,
        Auth::OAuth2 { .. } => {
            schemes
                .entry("bearerAuth".to_string())
                .or_insert_with(|| json!({ "type": "http", "scheme": "bearer" }));
            Some(json!([{ "bearerAuth": [] }]))
        }
        Auth::Bearer { .. } => {
            schemes
                .entry("bearerAuth".to_string())
                .or_insert_with(|| json!({ "type": "http", "scheme": "bearer" }));
            Some(json!([{ "bearerAuth": [] }]))
        }
        Auth::Basic { .. } => {
            schemes
                .entry("basicAuth".to_string())
                .or_insert_with(|| json!({ "type": "http", "scheme": "basic" }));
            Some(json!([{ "basicAuth": [] }]))
        }
        Auth::ApiKey { header, .. } => {
            let name = api_keys
                .entry(header.clone())
                .or_insert_with(|| sanitize_scheme_name(header))
                .clone();
            schemes.entry(name.clone()).or_insert_with(|| {
                json!({ "type": "apiKey", "in": "header", "name": header })
            });
            let mut requirement = Map::new();
            requirement.insert(name, json!([]));
            Some(json!([Value::Object(requirement)]))
        }
    }
}

fn sanitize_scheme_name(header: &str) -> String {
    let mut out = String::new();
    for ch in header.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    let out = out.trim_matches('_').to_string();
    if out.is_empty() {
        "apiKeyAuth".to_string()
    } else {
        format!("{out}Auth")
    }
}

// ---------------------------------------------------------------------------
// Postman
// ---------------------------------------------------------------------------

fn postman(data: &ProjectData) -> ExportDoc {
    let env = active_environment(data);
    let mut warnings: Vec<String> = Vec::new();
    let mut variables: Vec<Value> = Vec::new();
    if let Some(env) = env {
        let mut secrets = 0;
        for var in env.vars.iter().filter(|var| var.enabled) {
            if var.secret {
                secrets += 1;
                continue;
            }
            variables.push(json!({ "key": var.key, "value": var.value }));
        }
        if secrets > 0 {
            warnings.push(format!(
                "{secrets} secret variable(s) were not exported. Add them in Postman if needed."
            ));
        }
    }

    let mut items: Vec<Value> = Vec::new();
    for folder in &data.folders {
        if folder.requests.is_empty() {
            continue;
        }
        let requests: Vec<Value> = folder
            .requests
            .iter()
            .map(|request| postman_item(request, &request.auth.resolve(&folder.auth)))
            .collect();
        items.push(json!({ "name": folder.name, "item": requests }));
    }
    if items.is_empty() {
        warnings.push("The workspace has no requests to export.".to_string());
    }

    let mut collection = Map::new();
    collection.insert(
        "info".into(),
        json!({
            "name": data.name,
            "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"
        }),
    );
    collection.insert("item".into(), Value::Array(items));
    if !variables.is_empty() {
        collection.insert("variable".into(), Value::Array(variables));
    }

    ExportDoc {
        text: serde_json::to_string_pretty(&Value::Object(collection)).unwrap_or_default(),
        warnings,
    }
}

fn postman_item(request: &ApiRequest, auth: &Auth) -> Value {
    let mut url_parts: Vec<String> = Vec::new();
    for parameter in request
        .params
        .iter()
        .filter(|parameter| parameter.enabled && !parameter.key.trim().is_empty())
    {
        url_parts.push(format!(
            "{}={}",
            encode(&parameter.key),
            encode(&parameter.value)
        ));
    }
    let mut raw = request.url.trim().to_string();
    if !url_parts.is_empty() {
        raw.push(if raw.contains('?') { '&' } else { '?' });
        raw.push_str(&url_parts.join("&"));
    }
    let query: Vec<Value> = request
        .params
        .iter()
        .filter(|parameter| parameter.enabled && !parameter.key.trim().is_empty())
        .map(|parameter| {
            json!({
                "key": parameter.key,
                "value": parameter.value
            })
        })
        .collect();

    let mut headers: Vec<Value> = request
        .headers
        .iter()
        .filter(|header| header.enabled && !header.key.trim().is_empty())
        .map(|header| json!({ "key": header.key, "value": header.value, "type": "text" }))
        .collect();
    let has_content_type = request
        .headers
        .iter()
        .any(|header| header.enabled && header.key.eq_ignore_ascii_case("content-type"));
    if !has_content_type {
        let content_type = match request.body_mode {
            BodyMode::Json => Some("application/json"),
            BodyMode::Text => Some("text/plain"),
            BodyMode::Form => Some("application/x-www-form-urlencoded"),
            BodyMode::Multipart => None,
            BodyMode::Binary => Some("application/octet-stream"),
            BodyMode::None => None,
        };
        if let Some(content_type) = content_type {
            headers.push(json!({
                "key": "Content-Type",
                "value": content_type,
                "type": "text"
            }));
        }
    }

    let body = match request.body_mode {
        BodyMode::None => None,
        BodyMode::Json => Some(json!({
            "mode": "raw",
            "raw": request.body,
            "options": { "raw": { "language": "json" } }
        })),
        BodyMode::Text => Some(json!({
            "mode": "raw",
            "raw": request.body,
            "options": { "raw": { "language": "text" } }
        })),
        BodyMode::Form => Some(json!({
            "mode": "urlencoded",
            "urlencoded": request
                .form
                .iter()
                .filter(|field| field.enabled && !field.key.trim().is_empty())
                .map(|field| json!({
                    "key": field.key,
                    "value": field.value,
                    "type": "text"
                }))
                .collect::<Vec<_>>()
        })),
        BodyMode::Multipart => Some(json!({
            "mode": "formdata",
            "formdata": request
                .multipart
                .iter()
                .filter(|field| field.enabled && !field.key.trim().is_empty())
                .map(|field| match field.kind {
                    FieldKind::File => json!({
                        "key": field.key,
                        "type": "file",
                        "src": field.value
                    }),
                    FieldKind::Text => json!({
                        "key": field.key,
                        "value": field.value,
                        "type": "text"
                    }),
                })
                .collect::<Vec<_>>()
        })),
        BodyMode::Binary => Some(json!({
            "mode": "file",
            "file": { "src": request.binary_path }
        })),
    };

    let auth = match auth {
        Auth::None | Auth::Inherit => None,
        Auth::OAuth2 {
            grant,
            token_url,
            client_id,
            client_secret,
            scope,
            authorization_url,
        } => {
            let mut entries = vec![
                json!({ "key": "grant_type", "value": match grant {
                    crate::model::OAuthGrant::ClientCredentials => "client_credentials",
                    crate::model::OAuthGrant::Pkce => "authorization_code",
                }, "type": "string" }),
                json!({ "key": "accessTokenUrl", "value": token_url, "type": "string" }),
                json!({ "key": "clientId", "value": client_id, "type": "string" }),
                json!({ "key": "clientSecret", "value": client_secret, "type": "string" }),
                json!({ "key": "scope", "value": scope, "type": "string" }),
            ];
            if matches!(grant, crate::model::OAuthGrant::Pkce) {
                entries.push(json!({
                    "key": "authUrl",
                    "value": authorization_url,
                    "type": "string"
                }));
            }
            Some(json!({ "type": "oauth2", "oauth2": entries }))
        }
        Auth::Bearer { token } => Some(json!({
            "type": "bearer",
            "bearer": [{ "key": "token", "value": token, "type": "string" }]
        })),
        Auth::Basic { user, pass } => Some(json!({
            "type": "basic",
            "basic": [
                { "key": "username", "value": user, "type": "string" },
                { "key": "password", "value": pass, "type": "string" }
            ]
        })),
        Auth::ApiKey { header, value } => Some(json!({
            "type": "apikey",
            "apikey": [
                { "key": "key", "value": header, "type": "string" },
                { "key": "value", "value": value, "type": "string" },
                { "key": "in", "value": "header", "type": "string" }
            ]
        })),
    };

    let mut request_obj = Map::new();
    request_obj.insert("method".into(), json!(request.method.as_str()));
    request_obj.insert(
        "url".into(),
        json!({ "raw": raw, "query": query }),
    );
    if !headers.is_empty() {
        request_obj.insert("header".into(), Value::Array(headers));
    }
    if let Some(body) = body {
        request_obj.insert("body".into(), body);
    }
    if let Some(auth) = auth {
        request_obj.insert("auth".into(), auth);
    }
    if !request.description.trim().is_empty() {
        request_obj.insert("description".into(), json!(request.description));
    }

    let mut item = Map::new();
    item.insert("name".into(), json!(request.name));
    item.insert("request".into(), Value::Object(request_obj));
    if request.example_status != 0 || !request.example_body.trim().is_empty() {
        let status = if request.example_status == 0 {
            if request.method == Method::Post { 201 } else { 200 }
        } else {
            request.example_status
        };
        item.insert(
            "response".into(),
            json!([{
                "name": "Example",
                "status": "OK",
                "code": status,
                "_postman_previewlanguage": "json",
                "header": [{ "key": "Content-Type", "value": "application/json" }],
                "body": request.example_body
            }]),
        );
    }

    Value::Object(item)
}

fn encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push_str("%20"),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::importer;

    #[test]
    fn openapi_export_is_valid_and_round_trips() {
        let data = crate::model::seed();
        let doc = export(&data, Format::OpenApi);
        let value: Value = serde_json::from_str(&doc.text).expect("valid JSON");
        assert_eq!(value["openapi"], "3.0.3");
        assert!(value["paths"].as_object().is_some());
        assert!(value["servers"][0]["url"].as_str().unwrap().contains("://"));

        let plan = importer::detect_and_parse(&doc.text).expect("re-import");
        assert_eq!(plan.request_count(), 8);
        assert_eq!(plan.folders.len(), 3);
        assert!(plan.warnings.iter().all(|warning| !warning.contains("server")));
    }

    #[test]
    fn openapi_export_keeps_parameters_and_bodies() {
        let data = crate::model::seed();
        let doc = export(&data, Format::OpenApi);
        let value: Value = serde_json::from_str(&doc.text).unwrap();
        let list = &value["paths"]["/users"]["get"];
        // The seed base URL carries /v1, so paths are relative to it.
        assert!(list.is_object(), "missing GET /users: {list}");
        assert!(list["parameters"].as_array().unwrap().iter().any(|param| {
            param["name"] == "page" && param["in"] == "query"
        }));
        let create = &value["paths"]["/users"]["post"];
        assert_eq!(create["requestBody"]["content"]["application/json"]["schema"]["type"], "object");
        assert_eq!(create["security"][0]["bearerAuth"].is_array(), true);
    }

    #[test]
    fn postman_export_round_trips() {
        let data = crate::model::seed();
        let doc = export(&data, Format::Postman);
        let value: Value = serde_json::from_str(&doc.text).unwrap();
        assert!(value["info"]["schema"].as_str().unwrap().contains("postman"));
        let plan = importer::detect_and_parse(&doc.text).expect("re-import");
        assert_eq!(plan.request_count(), 8);
        assert_eq!(plan.folders.len(), 3);
        assert!(plan
            .folders
            .iter()
            .any(|folder| folder.requests.iter().any(|request| request.method == Method::Post)));
    }

    #[test]
    fn postman_export_marks_secret_variables() {
        let mut data = crate::model::seed();
        let active = data.active_env;
        if let Some(env) = data.envs.iter_mut().find(|env| Some(env.id) == active) {
            env.vars.push(KV {
                enabled: true,
                key: "apiSecret".into(),
                value: "nope".into(),
                desc: String::new(),
                secret: true,
            });
        }
        let doc = export(&data, Format::Postman);
        assert!(doc.warnings.iter().any(|warning| warning.contains("secret")));
        assert!(!doc.text.contains("nope"));
    }
}
